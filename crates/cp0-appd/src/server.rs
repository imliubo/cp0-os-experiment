use std::collections::BTreeSet;
use std::fmt;
use std::io::BufReader;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cp0_audio_protocol::AudioErrorCode as ServiceAudioErrorCode;
use cp0_camera_protocol::CameraErrorCode as ServiceCameraErrorCode;
use cp0_document_protocol::{DocumentErrorCode as ServiceDocumentErrorCode, send_frame_with_fd};
use cp0_gpio_protocol::GpioErrorCode as ServiceGpioErrorCode;
use cp0_manifest::Permission;
use cp0_network_protocol::NetworkErrorCode as ServiceNetworkErrorCode;

use crate::protocol::APPD_PROTOCOL_VERSION;
use crate::{
    AppManager, AppManagerError, AppSummary, AppdCommand, AppdRequest, AppdResponse, AudioClient,
    AudioClientError, BrokerCommand, BrokerErrorCode, BrokerProtocolError, BrokerRequest,
    BrokerResponse, CameraClient, CameraClientError, DocumentClient, DocumentClientError,
    DocumentCoordinator, DocumentPromptError, DocumentRequestResult, ErrorCode, GpioClient,
    GpioClientError, NetworkClient, NetworkClientError, NotificationQueue, PermissionChoice,
    PermissionCoordinator, PermissionPromptError, PermissionRequestResult, ResponseData,
    encode_broker_response, peer_credentials, read_broker_request, read_request,
    write_broker_response, write_response,
};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(3);
const BROKER_CLIENT_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub enum ServerError {
    Io(std::io::Error),
    Manager(AppManagerError),
    StatePoisoned,
}

impl fmt::Display for ServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "appd server I/O error: {error}"),
            Self::Manager(error) => write!(formatter, "appd manager error: {error}"),
            Self::StatePoisoned => formatter.write_str("appd state lock is poisoned"),
        }
    }
}

impl std::error::Error for ServerError {}

impl From<std::io::Error> for ServerError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct AppdServer {
    state: Arc<Mutex<ServerState>>,
    trusted_uids: BTreeSet<u32>,
    capabilities: CapabilityServices,
}

#[derive(Debug, Clone, Default)]
pub struct CapabilityServices {
    pub network: NetworkClient,
    pub documents: DocumentClient,
    pub audio: AudioClient,
    pub camera: CameraClient,
    pub gpio: GpioClient,
}

#[derive(Debug)]
struct ServerState {
    manager: AppManager,
    permissions: PermissionCoordinator,
    notifications: NotificationQueue,
    document_prompts: DocumentCoordinator,
}

impl AppdServer {
    pub fn new(
        manager: AppManager,
        permissions: PermissionCoordinator,
        trusted_uids: impl IntoIterator<Item = u32>,
    ) -> Self {
        Self::new_with_services(
            manager,
            permissions,
            trusted_uids,
            NetworkClient::default(),
            DocumentClient::default(),
        )
    }

    pub fn new_with_network(
        manager: AppManager,
        permissions: PermissionCoordinator,
        trusted_uids: impl IntoIterator<Item = u32>,
        network: NetworkClient,
    ) -> Self {
        Self::new_with_services(
            manager,
            permissions,
            trusted_uids,
            network,
            DocumentClient::default(),
        )
    }

    pub fn new_with_services(
        manager: AppManager,
        permissions: PermissionCoordinator,
        trusted_uids: impl IntoIterator<Item = u32>,
        network: NetworkClient,
        documents: DocumentClient,
    ) -> Self {
        Self::new_with_capability_services(
            manager,
            permissions,
            trusted_uids,
            CapabilityServices {
                network,
                documents,
                ..CapabilityServices::default()
            },
        )
    }

    pub fn new_with_capability_services(
        manager: AppManager,
        permissions: PermissionCoordinator,
        trusted_uids: impl IntoIterator<Item = u32>,
        capabilities: CapabilityServices,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(ServerState {
                manager,
                permissions,
                notifications: NotificationQueue::default(),
                document_prompts: DocumentCoordinator::default(),
            })),
            trusted_uids: trusted_uids.into_iter().collect(),
            capabilities,
        }
    }

    pub fn serve(self, listener: UnixListener) -> Result<(), ServerError> {
        loop {
            let (stream, _) = listener.accept()?;
            if let Err(error) = self.handle_connection(stream) {
                eprintln!("cp0-appd: rejected control connection: {error}");
            }
        }
    }

    pub fn serve_with_broker(
        self,
        control_listener: UnixListener,
        broker_listener: UnixListener,
    ) -> Result<(), ServerError> {
        let broker_server = self.clone();
        thread::Builder::new()
            .name("cp0-broker".into())
            .spawn(move || broker_server.serve_broker(broker_listener))
            .map_err(ServerError::Io)?;
        self.serve(control_listener)
    }

    fn serve_broker(self, listener: UnixListener) {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Err(error) = self.handle_broker_connection(stream) {
                        eprintln!("cp0-appd: rejected broker connection: {error}");
                    }
                }
                Err(error) => {
                    eprintln!("cp0-appd: broker accept failed: {error}");
                    thread::sleep(Duration::from_millis(250));
                }
            }
        }
    }

    fn handle_connection(&self, mut stream: UnixStream) -> Result<(), ServerError> {
        stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
        let credentials = peer_credentials(&stream)?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let request = match read_request(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(error) => {
                write_response(
                    &mut stream,
                    &AppdResponse::error(0, ErrorCode::InvalidRequest, error.to_string()),
                )
                .map_err(protocol_io)?;
                return Ok(());
            }
        };
        if !self.trusted_uids.contains(&credentials.uid) {
            write_response(
                &mut stream,
                &AppdResponse::error(
                    request.request_id,
                    ErrorCode::Unauthorized,
                    "peer UID is not authorized for application lifecycle control",
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }

        let response = self.dispatch(request);
        write_response(&mut stream, &response).map_err(protocol_io)?;
        Ok(())
    }

    fn dispatch(&self, request: AppdRequest) -> AppdResponse {
        let request_id = request.request_id;
        debug_assert_eq!(request.protocol_version, APPD_PROTOCOL_VERSION);
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application service state is unavailable",
                );
            }
        };
        let result: Result<ResponseData, CommandError> = match request.command {
            AppdCommand::Ping => Ok(ResponseData::Pong),
            AppdCommand::List { offset, limit } => {
                Self::list_apps(&state, offset, limit).map_err(CommandError::Manager)
            }
            AppdCommand::Start { app_id } => self
                .start_app(&state, &app_id)
                .map(|unit| ResponseData::Started { app_id, unit })
                .map_err(CommandError::Manager),
            AppdCommand::Stop { app_id } => match state.manager.stop(&app_id) {
                Ok(()) => {
                    state.permissions.clear_app_session(&app_id);
                    state.document_prompts.clear_app(&app_id);
                    Ok(ResponseData::Stopped { app_id })
                }
                Err(error) => Err(CommandError::Manager(error)),
            },
            AppdCommand::GetPermissionPrompt => Ok(ResponseData::PendingPermission {
                prompt: state.permissions.pending().cloned(),
            }),
            AppdCommand::ResolvePermission { prompt_id, choice } => {
                Self::resolve_permission(&mut state, prompt_id, choice)
            }
            AppdCommand::ResetPermission { app_id, permission } => {
                let result = state
                    .manager
                    .installed_manifest(&app_id)
                    .map_err(CommandError::Manager)
                    .and_then(|manifest| {
                        state
                            .permissions
                            .reset(&manifest, permission)
                            .map_err(CommandError::Permission)
                    });
                result.map(|()| ResponseData::PermissionReset { app_id, permission })
            }
            AppdCommand::TakeNotification => Ok(ResponseData::NextNotification {
                notification: state.notifications.take(),
            }),
            AppdCommand::GetDocumentPrompt => Ok(ResponseData::PendingDocument {
                prompt: state.document_prompts.pending().cloned(),
            }),
            AppdCommand::ResolveDocument {
                prompt_id,
                document_id,
            } => state
                .document_prompts
                .resolve(prompt_id, document_id.as_deref())
                .map(|(app_id, document_id)| ResponseData::DocumentResolved {
                    prompt_id,
                    app_id,
                    document_id,
                })
                .map_err(CommandError::Document),
        };
        match result {
            Ok(data) => AppdResponse::success(request_id, data),
            Err(error) => {
                eprintln!("cp0-appd: control request failed: {error}");
                command_error_response(request_id, &error)
            }
        }
    }

    fn start_app(&self, state: &ServerState, app_id: &str) -> Result<String, AppManagerError> {
        state.manager.start(app_id)
    }

    fn list_apps(
        state: &ServerState,
        offset: u16,
        limit: u8,
    ) -> Result<ResponseData, AppManagerError> {
        let installed_apps = state.manager.installed_apps();
        let mut apps = Vec::new();
        for installed in installed_apps
            .iter()
            .skip(usize::from(offset))
            .take(usize::from(limit))
        {
            let manifest = state.manager.installed_manifest(&installed.app_id)?;
            apps.push(AppSummary {
                running: state.manager.is_running(&installed.app_id)?,
                app_id: installed.app_id.clone(),
                name: manifest.name,
                version: installed.version.clone(),
                display: manifest.display,
            });
        }
        let consumed = usize::from(offset) + apps.len();
        let next_offset = (consumed < installed_apps.len()).then(|| {
            u16::try_from(consumed).expect("application registry is bounded below u16::MAX")
        });
        Ok(ResponseData::Applications { apps, next_offset })
    }

    fn resolve_permission(
        state: &mut ServerState,
        prompt_id: u64,
        choice: PermissionChoice,
    ) -> Result<ResponseData, CommandError> {
        let pending = state
            .permissions
            .pending()
            .cloned()
            .ok_or(PermissionPromptError::NoPendingPrompt)?;
        let manifest = state.manager.installed_manifest(&pending.app_id)?;
        let prompt = state.permissions.resolve(prompt_id, &manifest, choice)?;
        Ok(ResponseData::PermissionResolved {
            prompt_id: prompt.prompt_id,
            app_id: prompt.app_id,
            permission: prompt.permission,
            choice,
        })
    }

    fn handle_broker_connection(&self, mut stream: UnixStream) -> Result<(), ServerError> {
        stream.set_read_timeout(Some(BROKER_CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(BROKER_CLIENT_TIMEOUT))?;
        let credentials = peer_credentials(&stream)?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let request = match read_broker_request(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(error) => {
                write_broker_response(
                    &mut stream,
                    &BrokerResponse::error(0, BrokerErrorCode::InvalidRequest, error.to_string()),
                )
                .map_err(broker_io)?;
                return Ok(());
            }
        };
        let dispatch = match &request.command {
            BrokerCommand::OpenDocument => self.dispatch_document(credentials, request.request_id),
            BrokerCommand::CaptureCamera => self.dispatch_camera(credentials, request.request_id),
            _ => BrokerDispatch::response(self.dispatch_broker(credentials, request)),
        };
        if let Some(descriptor) = dispatch.descriptor {
            let frame = encode_broker_response(&dispatch.response).map_err(broker_io)?;
            send_frame_with_fd(&mut stream, &frame, descriptor.as_fd())
                .map_err(document_protocol_io)
        } else {
            write_broker_response(&mut stream, &dispatch.response).map_err(broker_io)
        }
    }

    fn dispatch_broker(
        &self,
        peer: crate::PeerCredentials,
        request: BrokerRequest,
    ) -> BrokerResponse {
        let request_id = request.request_id;
        match request.command {
            BrokerCommand::PostNotification { title, body } => {
                let app = match self.authorize_broker_caller(
                    peer,
                    request_id,
                    Permission::NotificationsPost,
                ) {
                    Ok(app) => app,
                    Err(response) => return response,
                };
                let mut state = match self.state.lock() {
                    Ok(state) => state,
                    Err(_) => {
                        return BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::Internal,
                            "application service state is unavailable",
                        );
                    }
                };
                match state
                    .notifications
                    .enqueue(&app.app_id, &app.app_name, title, body)
                {
                    Ok(notification_id) => BrokerResponse::success(request_id, notification_id),
                    Err(_) => BrokerResponse::error(
                        request_id,
                        BrokerErrorCode::ResourceExhausted,
                        "notification queue is full",
                    ),
                }
            }
            BrokerCommand::HttpGet { url } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::NetworkClient)
                {
                    return response;
                }
                match self.capabilities.network.http_get(request_id, &url) {
                    Ok(response) => BrokerResponse::http_response(
                        request_id,
                        response.status_code,
                        response.body_base64,
                    ),
                    Err(error) => {
                        eprintln!("cp0-appd: network service request failed: {error}");
                        network_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::OpenDocument => unreachable!("document requests carry descriptors"),
            BrokerCommand::PlayAudio { samples_base64 } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::AudioPlayback)
                {
                    return response;
                }
                let samples = match cp0_audio_protocol::decode_samples(&samples_base64) {
                    Ok(samples) => samples,
                    Err(_) => {
                        return BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::InvalidRequest,
                            "invalid bounded playback samples",
                        );
                    }
                };
                match self.capabilities.audio.play(request_id, &samples) {
                    Ok(frames) => BrokerResponse::audio_played(request_id, frames),
                    Err(error) => {
                        eprintln!("cp0-appd: audio playback request failed: {error}");
                        audio_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::CaptureAudio { frames } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::AudioCapture)
                {
                    return response;
                }
                match self.capabilities.audio.capture(request_id, frames) {
                    Ok(samples) => BrokerResponse::audio_captured(
                        request_id,
                        cp0_audio_protocol::encode_base64(&samples),
                    ),
                    Err(error) => {
                        eprintln!("cp0-appd: audio capture request failed: {error}");
                        audio_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::CaptureCamera => {
                unreachable!("camera requests carry descriptors")
            }
            BrokerCommand::ReadGpio { line } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::HardwareGpio)
                {
                    return response;
                }
                match self.capabilities.gpio.read(request_id, line) {
                    Ok(value) => BrokerResponse::gpio_value(request_id, line, value),
                    Err(error) => {
                        eprintln!("cp0-appd: GPIO read request failed: {error}");
                        gpio_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::WriteGpio { line, value } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::HardwareGpio)
                {
                    return response;
                }
                match self.capabilities.gpio.write(request_id, line, value) {
                    Ok(()) => BrokerResponse::gpio_written(request_id, line, value),
                    Err(error) => {
                        eprintln!("cp0-appd: GPIO write request failed: {error}");
                        gpio_error_response(request_id, &error)
                    }
                }
            }
        }
    }

    fn dispatch_camera(&self, peer: crate::PeerCredentials, request_id: u64) -> BrokerDispatch {
        if let Err(response) =
            self.authorize_broker_caller(peer, request_id, Permission::CameraCapture)
        {
            return BrokerDispatch::response(response);
        }
        match self.capabilities.camera.capture(request_id) {
            Ok(frame) => BrokerDispatch {
                response: BrokerResponse::camera_captured(request_id),
                descriptor: Some(frame.descriptor),
            },
            Err(error) => {
                eprintln!("cp0-appd: camera capture request failed: {error}");
                BrokerDispatch::response(camera_error_response(request_id, &error))
            }
        }
    }

    fn dispatch_document(&self, peer: crate::PeerCredentials, request_id: u64) -> BrokerDispatch {
        let app = match self.authorize_broker_caller(peer, request_id, Permission::DocumentsOpen) {
            Ok(app) => app,
            Err(response) => return BrokerDispatch::response(response),
        };

        let state_result = match self.state.lock() {
            Ok(mut state) => state.document_prompts.poll(&app.app_id),
            Err(_) => {
                return BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application service state is unavailable",
                ));
            }
        };
        match state_result {
            Ok(DocumentRequestResult::Selected(document_id)) => {
                match self.capabilities.documents.open(request_id, &document_id) {
                    Ok(opened) => BrokerDispatch {
                        response: BrokerResponse::document_opened(
                            request_id,
                            opened.summary.document_id,
                            opened.summary.size_bytes,
                        ),
                        descriptor: Some(opened.descriptor),
                    },
                    Err(error) => {
                        eprintln!("cp0-appd: document service open failed: {error}");
                        BrokerDispatch::response(document_error_response(request_id, &error))
                    }
                }
            }
            Ok(DocumentRequestResult::Cancelled) => {
                BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Denied,
                    "document selection was cancelled",
                ))
            }
            Ok(DocumentRequestResult::Pending(prompt)) => BrokerDispatch::response(
                BrokerResponse::document_selection_pending(request_id, prompt.prompt_id),
            ),
            Err(DocumentPromptError::Busy(_)) => BrokerDispatch::response(BrokerResponse::error(
                request_id,
                BrokerErrorCode::ResourceExhausted,
                "another document selection is pending",
            )),
            Err(error) => {
                eprintln!("cp0-appd: document selection state failed: {error}");
                BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "document selection state is unavailable",
                ))
            }
            Ok(DocumentRequestResult::NeedsDocuments) => {
                let documents = match self.capabilities.documents.list(request_id) {
                    Ok(documents) => documents,
                    Err(error) => {
                        eprintln!("cp0-appd: document service list failed: {error}");
                        return BrokerDispatch::response(document_error_response(
                            request_id, &error,
                        ));
                    }
                };
                let selection = match self.state.lock() {
                    Ok(mut state) => {
                        state
                            .document_prompts
                            .request(&app.app_id, &app.app_name, documents)
                    }
                    Err(_) => {
                        return BrokerDispatch::response(BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::Internal,
                            "application service state is unavailable",
                        ));
                    }
                };
                match selection {
                    Ok(DocumentRequestResult::Pending(prompt)) => BrokerDispatch::response(
                        BrokerResponse::document_selection_pending(request_id, prompt.prompt_id),
                    ),
                    Ok(DocumentRequestResult::Selected(document_id)) => {
                        match self.capabilities.documents.open(request_id, &document_id) {
                            Ok(opened) => BrokerDispatch {
                                response: BrokerResponse::document_opened(
                                    request_id,
                                    opened.summary.document_id,
                                    opened.summary.size_bytes,
                                ),
                                descriptor: Some(opened.descriptor),
                            },
                            Err(error) => BrokerDispatch::response(document_error_response(
                                request_id, &error,
                            )),
                        }
                    }
                    Ok(DocumentRequestResult::Cancelled) => {
                        BrokerDispatch::response(BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::Denied,
                            "document selection was cancelled",
                        ))
                    }
                    Ok(DocumentRequestResult::NeedsDocuments) => {
                        BrokerDispatch::response(BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::Internal,
                            "document selection could not be created",
                        ))
                    }
                    Err(DocumentPromptError::EmptyDocumentList) => {
                        BrokerDispatch::response(BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::Unavailable,
                            "no documents are available",
                        ))
                    }
                    Err(DocumentPromptError::Busy(prompt)) => BrokerDispatch::response(
                        BrokerResponse::document_selection_pending(request_id, prompt.prompt_id),
                    ),
                    Err(error) => {
                        eprintln!("cp0-appd: document prompt creation failed: {error}");
                        BrokerDispatch::response(BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::Internal,
                            "document selection could not be created",
                        ))
                    }
                }
            }
        }
    }

    fn authorize_broker_caller(
        &self,
        peer: crate::PeerCredentials,
        request_id: u64,
        permission: Permission,
    ) -> Result<AuthorizedApp, BrokerResponse> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                return Err(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application service state is unavailable",
                ));
            }
        };
        let Some(installed) = state.manager.installed_app_for_uid(peer.uid) else {
            return Err(BrokerResponse::error(
                request_id,
                BrokerErrorCode::Unauthorized,
                "peer UID is not an installed application identity",
            ));
        };
        let app_id = installed.app_id.clone();
        match state.manager.is_running(&installed.app_id) {
            Ok(true) => {}
            Ok(false) => {
                return Err(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Unauthorized,
                    "application is not running",
                ));
            }
            Err(error) => {
                eprintln!("cp0-appd: cannot verify broker caller state: {error}");
                return Err(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application state could not be verified",
                ));
            }
        }
        let unit = match state.manager.unit_for_app(&app_id) {
            Ok(unit) => unit,
            Err(error) => {
                eprintln!("cp0-appd: cannot derive broker caller unit: {error}");
                return Err(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application identity could not be verified",
                ));
            }
        };
        match process_is_in_unit(peer.pid, &unit) {
            Ok(true) => {}
            Ok(false) => {
                return Err(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Unauthorized,
                    "peer process is outside the application runtime cgroup",
                ));
            }
            Err(error) => {
                eprintln!("cp0-appd: cannot verify broker caller cgroup: {error}");
                return Err(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application process identity could not be verified",
                ));
            }
        }
        let manifest = match state.manager.installed_manifest(&app_id) {
            Ok(manifest) => manifest,
            Err(error) => {
                eprintln!("cp0-appd: cannot load broker caller manifest: {error}");
                return Err(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "installed application metadata is unavailable",
                ));
            }
        };
        match state.permissions.request(&manifest, permission) {
            Ok(PermissionRequestResult::Allow) => Ok(AuthorizedApp {
                app_id: manifest.id,
                app_name: manifest.name,
            }),
            Ok(PermissionRequestResult::Prompt(prompt)) => Err(BrokerResponse::permission_pending(
                request_id,
                prompt.prompt_id,
            )),
            Ok(PermissionRequestResult::Deny) => Err(BrokerResponse::error(
                request_id,
                BrokerErrorCode::Denied,
                "capability permission was denied",
            )),
            Ok(PermissionRequestResult::Undeclared) => Err(BrokerResponse::error(
                request_id,
                BrokerErrorCode::Undeclared,
                "application did not declare the requested capability",
            )),
            Err(PermissionPromptError::Busy(_)) => Err(BrokerResponse::error(
                request_id,
                BrokerErrorCode::ResourceExhausted,
                "another permission prompt is pending",
            )),
            Err(error) => {
                eprintln!("cp0-appd: capability permission request failed: {error}");
                Err(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "capability permission could not be evaluated",
                ))
            }
        }
    }
}

#[derive(Debug)]
struct AuthorizedApp {
    app_id: String,
    app_name: String,
}

#[derive(Debug)]
struct BrokerDispatch {
    response: BrokerResponse,
    descriptor: Option<OwnedFd>,
}

impl BrokerDispatch {
    fn response(response: BrokerResponse) -> Self {
        Self {
            response,
            descriptor: None,
        }
    }
}

#[derive(Debug)]
enum CommandError {
    Manager(AppManagerError),
    Permission(PermissionPromptError),
    Document(DocumentPromptError),
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manager(error) => write!(formatter, "{error}"),
            Self::Permission(error) => write!(formatter, "{error}"),
            Self::Document(error) => write!(formatter, "{error}"),
        }
    }
}

impl From<AppManagerError> for CommandError {
    fn from(error: AppManagerError) -> Self {
        Self::Manager(error)
    }
}

impl From<PermissionPromptError> for CommandError {
    fn from(error: PermissionPromptError) -> Self {
        Self::Permission(error)
    }
}

fn command_error_response(request_id: u64, error: &CommandError) -> AppdResponse {
    match error {
        CommandError::Manager(error) => manager_error_response(request_id, error),
        CommandError::Permission(PermissionPromptError::NoPendingPrompt)
        | CommandError::Permission(PermissionPromptError::StalePrompt) => AppdResponse::error(
            request_id,
            ErrorCode::NotFound,
            "permission prompt is missing or stale",
        ),
        CommandError::Permission(PermissionPromptError::Busy(_)) => AppdResponse::error(
            request_id,
            ErrorCode::ResourceExhausted,
            "another permission prompt is already pending",
        ),
        CommandError::Permission(PermissionPromptError::Permission(_)) => AppdResponse::error(
            request_id,
            ErrorCode::Internal,
            "permission decision could not be saved",
        ),
        CommandError::Document(DocumentPromptError::NoPendingPrompt)
        | CommandError::Document(DocumentPromptError::StalePrompt) => AppdResponse::error(
            request_id,
            ErrorCode::NotFound,
            "document prompt is missing or stale",
        ),
        CommandError::Document(DocumentPromptError::InvalidSelection) => AppdResponse::error(
            request_id,
            ErrorCode::InvalidRequest,
            "selected document is not in the trusted prompt",
        ),
        CommandError::Document(DocumentPromptError::Busy(_)) => AppdResponse::error(
            request_id,
            ErrorCode::ResourceExhausted,
            "another document prompt is already pending",
        ),
        CommandError::Document(DocumentPromptError::EmptyDocumentList) => AppdResponse::error(
            request_id,
            ErrorCode::NotFound,
            "no documents are available",
        ),
    }
}

fn manager_error_response(request_id: u64, error: &AppManagerError) -> AppdResponse {
    let (code, message) = match error {
        AppManagerError::NotInstalled(app_id) => (
            ErrorCode::NotFound,
            format!("application {app_id} is not installed"),
        ),
        AppManagerError::AlreadyRunning(app_id) => (
            ErrorCode::AlreadyRunning,
            format!("application {app_id} is already running"),
        ),
        AppManagerError::ForegroundBusy(app_id) => (
            ErrorCode::ResourceExhausted,
            format!("application {app_id} already owns the runtime slot"),
        ),
        AppManagerError::NotRunning(app_id) => (
            ErrorCode::NotRunning,
            format!("application {app_id} is not running"),
        ),
        AppManagerError::Registry(crate::RegistryError::Exhausted) => (
            ErrorCode::ResourceExhausted,
            "application identity range is exhausted".into(),
        ),
        _ => (
            ErrorCode::Internal,
            "application lifecycle operation failed".into(),
        ),
    };
    AppdResponse::error(request_id, code, message)
}

fn network_error_response(request_id: u64, error: &NetworkClientError) -> BrokerResponse {
    let (code, message) = match error {
        NetworkClientError::Service(ServiceNetworkErrorCode::InvalidRequest) => {
            (BrokerErrorCode::InvalidRequest, "invalid or non-HTTPS URL")
        }
        NetworkClientError::Service(ServiceNetworkErrorCode::BlockedAddress) => (
            BrokerErrorCode::BlockedAddress,
            "destination resolved to a non-public address",
        ),
        NetworkClientError::Service(ServiceNetworkErrorCode::Unavailable) => (
            BrokerErrorCode::UpstreamUnavailable,
            "HTTPS destination is unavailable",
        ),
        NetworkClientError::Service(ServiceNetworkErrorCode::Timeout) => {
            (BrokerErrorCode::Timeout, "HTTPS request timed out")
        }
        NetworkClientError::Service(ServiceNetworkErrorCode::Tls) => (
            BrokerErrorCode::Tls,
            "HTTPS certificate or TLS validation failed",
        ),
        NetworkClientError::Service(ServiceNetworkErrorCode::TooManyRedirects) => (
            BrokerErrorCode::TooManyRedirects,
            "HTTPS redirect limit was exceeded",
        ),
        NetworkClientError::Service(ServiceNetworkErrorCode::ResponseTooLarge) => (
            BrokerErrorCode::ResponseTooLarge,
            "HTTPS response body exceeds 2048 bytes",
        ),
        NetworkClientError::Io(_) | NetworkClientError::EmptyResponse => (
            BrokerErrorCode::UpstreamUnavailable,
            "network service is unavailable",
        ),
        NetworkClientError::Protocol(_)
        | NetworkClientError::MismatchedRequestId
        | NetworkClientError::Service(ServiceNetworkErrorCode::Unauthorized)
        | NetworkClientError::Service(ServiceNetworkErrorCode::Internal) => (
            BrokerErrorCode::Internal,
            "network service returned an invalid response",
        ),
    };
    BrokerResponse::error(request_id, code, message)
}

fn document_error_response(request_id: u64, error: &DocumentClientError) -> BrokerResponse {
    let (code, message) = match error {
        DocumentClientError::Service(ServiceDocumentErrorCode::InvalidRequest) => (
            BrokerErrorCode::InvalidRequest,
            "selected document is invalid",
        ),
        DocumentClientError::Service(ServiceDocumentErrorCode::NotFound) => (
            BrokerErrorCode::Unavailable,
            "selected document is no longer available",
        ),
        DocumentClientError::Service(ServiceDocumentErrorCode::ResourceExhausted) => (
            BrokerErrorCode::ResourceExhausted,
            "document service resource limit was reached",
        ),
        DocumentClientError::Io(_) | DocumentClientError::EmptyResponse => (
            BrokerErrorCode::Unavailable,
            "document service is unavailable",
        ),
        DocumentClientError::Protocol(_)
        | DocumentClientError::MismatchedRequestId
        | DocumentClientError::MissingDescriptor
        | DocumentClientError::UnexpectedDescriptor
        | DocumentClientError::MismatchedDocument
        | DocumentClientError::Service(ServiceDocumentErrorCode::Unauthorized)
        | DocumentClientError::Service(ServiceDocumentErrorCode::Internal) => (
            BrokerErrorCode::Internal,
            "document service returned an invalid response",
        ),
    };
    BrokerResponse::error(request_id, code, message)
}

fn audio_error_response(request_id: u64, error: &AudioClientError) -> BrokerResponse {
    let (code, message) = match error {
        AudioClientError::Service(ServiceAudioErrorCode::InvalidRequest) => {
            (BrokerErrorCode::InvalidRequest, "audio request is invalid")
        }
        AudioClientError::Service(ServiceAudioErrorCode::Busy) => {
            (BrokerErrorCode::ResourceExhausted, "audio device is busy")
        }
        AudioClientError::Service(ServiceAudioErrorCode::Unavailable)
        | AudioClientError::Service(ServiceAudioErrorCode::Device)
        | AudioClientError::Io(_)
        | AudioClientError::EmptyResponse => {
            (BrokerErrorCode::Unavailable, "audio device is unavailable")
        }
        AudioClientError::Protocol(_)
        | AudioClientError::MismatchedRequestId
        | AudioClientError::MismatchedFrameCount
        | AudioClientError::Service(ServiceAudioErrorCode::Unauthorized)
        | AudioClientError::Service(ServiceAudioErrorCode::Internal) => (
            BrokerErrorCode::Internal,
            "audio service returned an invalid response",
        ),
    };
    BrokerResponse::error(request_id, code, message)
}

fn camera_error_response(request_id: u64, error: &CameraClientError) -> BrokerResponse {
    let (code, message) = match error {
        CameraClientError::Service(ServiceCameraErrorCode::InvalidRequest) => {
            (BrokerErrorCode::InvalidRequest, "camera request is invalid")
        }
        CameraClientError::Service(ServiceCameraErrorCode::Busy) => {
            (BrokerErrorCode::ResourceExhausted, "camera is busy")
        }
        CameraClientError::Service(ServiceCameraErrorCode::Unavailable)
        | CameraClientError::Service(ServiceCameraErrorCode::CaptureFailed)
        | CameraClientError::Io(_)
        | CameraClientError::EmptyResponse => {
            (BrokerErrorCode::Unavailable, "camera is unavailable")
        }
        CameraClientError::Protocol(_)
        | CameraClientError::MismatchedRequestId
        | CameraClientError::MissingDescriptor
        | CameraClientError::UnexpectedDescriptor
        | CameraClientError::InvalidDescriptor
        | CameraClientError::Service(ServiceCameraErrorCode::Unauthorized)
        | CameraClientError::Service(ServiceCameraErrorCode::Internal) => (
            BrokerErrorCode::Internal,
            "camera service returned an invalid response",
        ),
    };
    BrokerResponse::error(request_id, code, message)
}

fn gpio_error_response(request_id: u64, error: &GpioClientError) -> BrokerResponse {
    let (code, message) = match error {
        GpioClientError::Service(ServiceGpioErrorCode::InvalidRequest) => {
            (BrokerErrorCode::InvalidRequest, "GPIO request is invalid")
        }
        GpioClientError::Service(ServiceGpioErrorCode::Unavailable)
        | GpioClientError::Service(ServiceGpioErrorCode::Device)
        | GpioClientError::Io(_)
        | GpioClientError::EmptyResponse => {
            (BrokerErrorCode::Unavailable, "GPIO service is unavailable")
        }
        GpioClientError::Protocol(_)
        | GpioClientError::MismatchedRequestId
        | GpioClientError::MismatchedOutcome
        | GpioClientError::Service(ServiceGpioErrorCode::Unauthorized)
        | GpioClientError::Service(ServiceGpioErrorCode::Internal) => (
            BrokerErrorCode::Internal,
            "GPIO service returned an invalid response",
        ),
    };
    BrokerResponse::error(request_id, code, message)
}

fn protocol_io(error: crate::ProtocolError) -> ServerError {
    match error {
        crate::ProtocolError::Io(error) => ServerError::Io(error),
        other => ServerError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            other.to_string(),
        )),
    }
}

fn broker_io(error: BrokerProtocolError) -> ServerError {
    match error {
        BrokerProtocolError::Io(error) => ServerError::Io(error),
        other => ServerError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            other.to_string(),
        )),
    }
}

fn document_protocol_io(error: cp0_document_protocol::DocumentProtocolError) -> ServerError {
    match error {
        cp0_document_protocol::DocumentProtocolError::Io(error) => ServerError::Io(error),
        other => ServerError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            other.to_string(),
        )),
    }
}

fn process_is_in_unit(pid: u32, unit: &str) -> std::io::Result<bool> {
    let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup"))?;
    Ok(cgroup_contains_unit(&cgroup, unit))
}

fn cgroup_contains_unit(cgroup: &str, unit: &str) -> bool {
    cgroup.lines().any(|line| {
        let mut fields = line.splitn(3, ':');
        fields.next().is_some()
            && fields.next().is_some()
            && fields.next().is_some_and(|path| {
                std::path::Path::new(path)
                    .components()
                    .any(|component| component.as_os_str() == unit)
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_public_lifecycle_errors_without_host_details() {
        let response = manager_error_response(
            9,
            &AppManagerError::NotInstalled("dev.cardputerzero.missing".into()),
        );
        let encoded = serde_json::to_string(&response).unwrap();
        assert!(encoded.contains("not-found"));
        assert!(encoded.contains("dev.cardputerzero.missing"));

        let internal = manager_error_response(
            10,
            &AppManagerError::InvalidHostIdentity("/secret/host/path".into()),
        );
        let encoded = serde_json::to_string(&internal).unwrap();
        assert!(encoded.contains("internal"));
        assert!(!encoded.contains("/secret/host/path"));
    }

    #[test]
    fn matches_exact_systemd_unit_in_cgroup_path() {
        assert!(cgroup_contains_unit(
            "0::/system.slice/cardputerzero-app-20000.service\n",
            "cardputerzero-app-20000.service"
        ));
        assert!(!cgroup_contains_unit(
            "0::/system.slice/cardputerzero-app-20000.service-escape\n",
            "cardputerzero-app-20000.service"
        ));
        assert!(!cgroup_contains_unit(
            "malformed\n",
            "cardputerzero-app-20000.service"
        ));
    }
}
