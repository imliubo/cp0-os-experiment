use std::collections::{BTreeMap, BTreeSet};
#[cfg(target_os = "linux")]
use std::ffi::CString;
use std::fmt;
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::Write;
use std::io::{Read, Seek};
#[cfg(target_os = "linux")]
use std::os::fd::FromRawFd;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cp0_audio_protocol::{AudioErrorCode as ServiceAudioErrorCode, KEY_CLICK_FRAMES};
use cp0_camera_protocol::{CameraErrorCode as ServiceCameraErrorCode, decode_photo_payload};
use cp0_document_protocol::{DocumentErrorCode as ServiceDocumentErrorCode, send_frame_with_fd};
use cp0_gpio_protocol::GpioErrorCode as ServiceGpioErrorCode;
use cp0_manifest::Permission;
use cp0_network_protocol::NetworkErrorCode as ServiceNetworkErrorCode;
use cp0_radio_protocol::RadioErrorCode as ServiceRadioErrorCode;
use cp0_storage_protocol::StorageErrorCode as ServiceStorageErrorCode;
use cp0_store_protocol::StoreRuntimeMetricEvent;

use crate::photo_library::{
    PHOTO_FRAME_BYTES, PHOTO_LIBRARY_HEAD_KEY, PHOTO_LIBRARY_ID, PHOTO_LIBRARY_QUOTA_BYTES,
    PhotoImportError, import_app_photo, import_camera_photo, import_screenshot, load_legacy_photo,
    photo_blob_key, photo_is_active, remove_photo,
};
use crate::photo_view::{PhotoViewCache, PhotoViewError};
use crate::protocol::APPD_PROTOCOL_VERSION;
use crate::{
    AppManager, AppManagerError, AppPermissionDecision, AppPermissionState, AppSummary,
    AppdCommand, AppdRequest, AppdResponse, AudioClient, AudioClientError, Authorization,
    BrokerCommand, BrokerErrorCode, BrokerProtocolError, BrokerRequest, BrokerResponse,
    CameraClient, CameraClientError, CheckpointFailure, CheckpointStatus, DevicePolicyEngine,
    DocumentClient, DocumentClientError, DocumentCoordinator, DocumentPromptError,
    DocumentRequestResult, ErrorCode, EvictionCheckpoint, GpioClient, GpioClientError,
    InstallError, IntentQueue, MediaAction, MediaSessionBroker, MediaSessionError, NetworkClient,
    NetworkClientError, NotificationQueue, PackageInstaller, PermissionChoice,
    PermissionCoordinator, PermissionPromptError, PermissionRequestResult, PolicyError,
    RadioClient, RadioClientError, ResponseData, RuntimeBinding, StorageClient, StorageClientError,
    StoreMetricsClient, TaskError, TaskId, TaskRegistry, TaskState, TaskSummary, TrustPaths,
    TrustPolicy, encode_broker_response, peer_credentials, recv_broker_request_with_fd,
    recv_request_with_fd, write_broker_response, write_response,
};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(3);
const BROKER_CLIENT_TIMEOUT: Duration = Duration::from_millis(500);
const RUNTIME_MONITOR_RETRY: Duration = Duration::from_secs(5);

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
    store_installer_uids: BTreeSet<u32>,
    shell_uid: Option<u32>,
    capabilities: CapabilityServices,
    installer: PackageInstaller,
    store_metrics: StoreMetricsClient,
    lifecycle: Arc<Mutex<()>>,
    runtime: Arc<Mutex<RuntimeState>>,
    photo_library: Arc<Mutex<()>>,
    photo_view: Arc<Mutex<PhotoViewCache>>,
    runtime_sequence: Arc<AtomicU64>,
}

#[derive(Debug, Clone, Default)]
pub struct CapabilityServices {
    pub network: NetworkClient,
    pub documents: DocumentClient,
    pub audio: AudioClient,
    pub camera: CameraClient,
    pub gpio: GpioClient,
    pub radio: RadioClient,
    pub storage: StorageClient,
}

#[derive(Debug)]
struct ServerState {
    manager: AppManager,
    tasks: TaskRegistry,
    permissions: PermissionCoordinator,
    notifications: NotificationQueue,
    document_prompts: DocumentCoordinator,
    intents: IntentQueue,
    media_sessions: MediaSessionBroker,
    policy: DevicePolicyEngine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeSession {
    token: u64,
    task_id: TaskId,
    app_id: String,
    version: String,
    explicit_stop: bool,
}

#[derive(Debug, Default)]
struct RuntimeState {
    sessions: BTreeMap<u64, RuntimeSession>,
    foreground: Option<u64>,
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
                tasks: TaskRegistry::default(),
                permissions,
                notifications: NotificationQueue::default(),
                document_prompts: DocumentCoordinator::default(),
                intents: IntentQueue::default(),
                media_sessions: MediaSessionBroker::default(),
                policy: DevicePolicyEngine::unmanaged(),
            })),
            trusted_uids: trusted_uids.into_iter().collect(),
            store_installer_uids: BTreeSet::new(),
            shell_uid: None,
            capabilities,
            store_metrics: StoreMetricsClient::default(),
            lifecycle: Arc::new(Mutex::new(())),
            runtime: Arc::new(Mutex::new(RuntimeState::default())),
            photo_library: Arc::new(Mutex::new(())),
            photo_view: Arc::new(Mutex::new(PhotoViewCache::default())),
            runtime_sequence: Arc::new(AtomicU64::new(1)),
            installer: PackageInstaller::new(
                crate::DEFAULT_APPS_ROOT,
                TrustPolicy::new(TrustPaths::default(), true),
                true,
            ),
        }
    }

    pub fn allow_store_installer(mut self, uid: u32) -> Self {
        self.trusted_uids.insert(uid);
        self.store_installer_uids.insert(uid);
        self
    }

    pub fn allow_shell(mut self, uid: u32) -> Self {
        self.trusted_uids.insert(uid);
        self.shell_uid = Some(uid);
        self
    }

    pub fn with_device_policy(self, policy: DevicePolicyEngine) -> Self {
        self.state
            .lock()
            .expect("new application server state cannot be poisoned")
            .policy = policy;
        self
    }

    pub fn with_store_metrics_client(mut self, client: StoreMetricsClient) -> Self {
        self.store_metrics = client;
        self
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
        let (request, descriptor) = match recv_request_with_fd(&stream) {
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
        let imports_screenshot = matches!(request.command, AppdCommand::ImportScreenshot);
        if imports_screenshot != descriptor.is_some() {
            write_response(
                &mut stream,
                &AppdResponse::error(
                    request.request_id,
                    ErrorCode::InvalidRequest,
                    if imports_screenshot {
                        "screenshot import requires exactly one frame descriptor"
                    } else {
                        "application command does not accept a descriptor"
                    },
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }
        if !control_command_authorized(
            credentials.uid,
            &request.command,
            &self.trusted_uids,
            &self.store_installer_uids,
            self.shell_uid,
        ) {
            write_response(
                &mut stream,
                &AppdResponse::error(
                    request.request_id,
                    ErrorCode::Unauthorized,
                    "peer UID is not authorized for this application command",
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }

        let response = if imports_screenshot {
            self.import_screenshot_control(
                request.request_id,
                descriptor.expect("screenshot descriptor was checked"),
            )
        } else {
            self.dispatch(request, credentials.uid)
        };
        write_response(&mut stream, &response).map_err(protocol_io)?;
        Ok(())
    }

    fn import_screenshot_control(&self, request_id: u64, descriptor: OwnedFd) -> AppdResponse {
        let frame = match read_photo_frame(descriptor) {
            Ok(frame) => frame,
            Err(error) => {
                eprintln!("cp0-appd: rejected screenshot descriptor: {error}");
                return AppdResponse::error(
                    request_id,
                    ErrorCode::InvalidRequest,
                    "screenshot frame descriptor does not satisfy the fixed contract",
                );
            }
        };
        let _transaction = match self.photo_library.lock() {
            Ok(transaction) => transaction,
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "photo library transaction state is unavailable",
                );
            }
        };
        let suggested_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |duration| {
                u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
            });
        match import_screenshot(&self.capabilities.storage, request_id, &frame, suggested_id) {
            Ok(photo_id) => {
                AppdResponse::success(request_id, ResponseData::ScreenshotImported { photo_id })
            }
            Err(error) => {
                eprintln!("cp0-appd: screenshot photo import failed: {error}");
                photo_import_error_response(request_id, &error)
            }
        }
    }

    fn dispatch(&self, request: AppdRequest, peer_uid: u32) -> AppdResponse {
        let request_id = request.request_id;
        debug_assert_eq!(request.protocol_version, APPD_PROTOCOL_VERSION);
        let command = match request.command {
            AppdCommand::Install { package_name } => {
                return self.install_package(request_id, &package_name);
            }
            command @ AppdCommand::StoreInstall { .. } => {
                return self.install_store_package(request_id, peer_uid, command);
            }
            AppdCommand::DispatchMediaAction { action } => {
                return self.dispatch_media_action(request_id, action);
            }
            AppdCommand::Stop { app_id } => {
                return self.stop_app_control(request_id, &app_id);
            }
            AppdCommand::Start { app_id } => {
                return self.start_app_control(request_id, &app_id);
            }
            AppdCommand::ActivateTask { task_id } => {
                return self.activate_task_control(request_id, TaskId(task_id));
            }
            AppdCommand::CloseTask { task_id } => {
                return self.close_task_control(request_id, TaskId(task_id));
            }
            AppdCommand::SetForegroundApp { app_id } => {
                return self.set_foreground_app_control(request_id, app_id.as_deref());
            }
            command => command,
        };
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
        let result: Result<ResponseData, CommandError> = match command {
            AppdCommand::Install { .. } => unreachable!("install returned before state lock"),
            AppdCommand::StoreInstall { .. } => {
                unreachable!("store install returned before state lock")
            }
            AppdCommand::DispatchMediaAction { .. } => {
                unreachable!("media dispatch returned before state lock")
            }
            AppdCommand::ImportScreenshot => {
                unreachable!("screenshot import returned before state lock")
            }
            AppdCommand::Ping => Ok(ResponseData::Pong),
            AppdCommand::List { offset, limit } => {
                self.list_apps(&state, request_id, offset, limit)
            }
            AppdCommand::ListTasks { offset, limit } => Self::list_tasks(&state, offset, limit),
            AppdCommand::StoreListInstalled { offset, limit } => {
                Self::list_store_apps(&state, offset, limit).map_err(CommandError::Manager)
            }
            AppdCommand::Start { .. } => unreachable!("start returned before state lock"),
            AppdCommand::Stop { .. } => unreachable!("stop returned before state lock"),
            AppdCommand::ActivateTask { .. } => {
                unreachable!("task activation returned before state lock")
            }
            AppdCommand::CloseTask { .. } => {
                unreachable!("task close returned before state lock")
            }
            AppdCommand::SetForegroundApp { .. } => {
                unreachable!("foreground synchronization returned before state lock")
            }
            AppdCommand::Uninstall { app_id } => {
                let has_task = task_blocks_package_change(&state.tasks, &app_id);
                let result = if crate::is_removable_app(&app_id) {
                    Ok(())
                } else {
                    Err(CommandError::Manager(AppManagerError::ProtectedBuiltin(
                        app_id.clone(),
                    )))
                }
                .and_then(|()| {
                    state
                        .manager
                        .is_running(&app_id)
                        .map_err(CommandError::Manager)
                })
                .and_then(|running| {
                    if running || has_task {
                        Err(CommandError::Manager(AppManagerError::AlreadyRunning(
                            app_id.clone(),
                        )))
                    } else {
                        Ok(())
                    }
                })
                .and_then(|()| {
                    state
                        .permissions
                        .reset_app(&app_id)
                        .map_err(CommandError::Permission)
                })
                .and_then(|()| {
                    state.document_prompts.clear_app(&app_id);
                    state
                        .manager
                        .uninstall(&app_id)
                        .map_err(CommandError::Manager)
                });
                result.map(|removed| {
                    state.media_sessions.clear_app(&app_id);
                    ResponseData::Uninstalled {
                        app_id: removed.app_id,
                        private_data_retained: true,
                        package_cleanup_pending: removed.package_cleanup_pending,
                    }
                })
            }
            AppdCommand::Rollback { app_id } => {
                if task_blocks_package_change(&state.tasks, &app_id) {
                    Err(CommandError::Manager(AppManagerError::AlreadyRunning(
                        app_id,
                    )))
                } else {
                    state
                        .manager
                        .rollback(&app_id)
                        .map(|installed| ResponseData::RolledBack {
                            app_id: installed.app_id,
                            version: installed.version,
                        })
                        .map_err(CommandError::Manager)
                }
            }
            AppdCommand::Logs { app_id, limit } => state
                .manager
                .logs(&app_id, limit)
                .map(|lines| ResponseData::Logs { app_id, lines })
                .map_err(CommandError::Manager),
            AppdCommand::GetPermissionPrompt => Ok(ResponseData::PendingPermission {
                prompt: state.permissions.pending().cloned(),
            }),
            AppdCommand::GetPermissions { app_id } => state
                .manager
                .installed_manifest(&app_id)
                .map_err(CommandError::Manager)
                .map(|manifest| {
                    let permissions = manifest
                        .permissions
                        .iter()
                        .map(|request| {
                            let decision = if state.policy.denies_permission(request.name) {
                                AppPermissionDecision::PolicyDenied
                            } else {
                                match state.permissions.authorization(&manifest, request.name) {
                                    Authorization::Allow => AppPermissionDecision::Allowed,
                                    Authorization::Deny => AppPermissionDecision::Denied,
                                    Authorization::Prompt => AppPermissionDecision::Ask,
                                    Authorization::Undeclared => unreachable!(
                                        "manifest permission is declared by construction"
                                    ),
                                }
                            };
                            AppPermissionState {
                                permission: request.name,
                                decision,
                            }
                        })
                        .collect();
                    ResponseData::ApplicationPermissions {
                        app_id,
                        permissions,
                    }
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
            AppdCommand::GetDeviceSettings => state
                .policy
                .settings()
                .map(|settings| ResponseData::DeviceSettings { settings })
                .map_err(CommandError::Policy),
            AppdCommand::SetDeviceMode { mode, enabled } => state
                .policy
                .set_mode(mode, enabled)
                .and_then(|()| state.policy.settings())
                .map(|settings| ResponseData::DeviceModeChanged { settings })
                .map_err(CommandError::Policy),
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
        drop(state);
        match result {
            Ok(data) => AppdResponse::success(request_id, data),
            Err(error) => {
                eprintln!("cp0-appd: control request failed: {error}");
                command_error_response(request_id, &error)
            }
        }
    }

    fn dispatch_media_action(&self, request_id: u64, action: MediaAction) -> AppdResponse {
        let runtime = match self.runtime.lock() {
            Ok(runtime) => runtime,
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application runtime state is unavailable",
                );
            }
        };
        let Some(session) = runtime
            .foreground
            .and_then(|token| runtime.sessions.get(&token))
        else {
            return AppdResponse::error(
                request_id,
                ErrorCode::Unavailable,
                "no foreground application is running",
            );
        };
        let runtime_token = session.token;
        let active_app_id = session.app_id.clone();

        // Keep runtime identity stable until the bounded queue mutation finishes.
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
        let result = state
            .media_sessions
            .dispatch(&active_app_id, runtime_token, action);
        drop(state);
        drop(runtime);
        match result {
            Ok(()) => AppdResponse::success(
                request_id,
                ResponseData::MediaActionDispatched {
                    app_id: active_app_id,
                    action,
                },
            ),
            Err(MediaSessionError::Unavailable | MediaSessionError::Unsupported) => {
                AppdResponse::error(
                    request_id,
                    ErrorCode::Unavailable,
                    "the foreground application has no matching media session action",
                )
            }
            Err(MediaSessionError::Full) => AppdResponse::error(
                request_id,
                ErrorCode::ResourceExhausted,
                "the foreground media action queue is full",
            ),
        }
    }

    fn stop_app_control(&self, request_id: u64, app_id: &str) -> AppdResponse {
        let _lifecycle = match self.lifecycle.lock() {
            Ok(lifecycle) => lifecycle,
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application lifecycle coordinator is unavailable",
                );
            }
        };
        let runtime_token = self.mark_explicit_stop(app_id);
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.cancel_explicit_stop(runtime_token);
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application service state is unavailable",
                );
            }
        };
        let task_id = state.tasks.task_for_app(app_id).map(|task| task.task_id);
        let running = state.manager.is_running(app_id);
        let result = running.and_then(|running| {
            if running {
                state.manager.stop(app_id)?;
            } else if task_id.is_none() {
                return Err(AppManagerError::NotRunning(app_id.into()));
            }
            if let Some(task_id) = task_id {
                state
                    .tasks
                    .close(task_id)
                    .map_err(|_| AppManagerError::NotRunning(app_id.into()))?;
            }
            state.permissions.clear_app_session(app_id);
            state.document_prompts.clear_app(app_id);
            state.media_sessions.clear_app(app_id);
            Ok(ResponseData::Stopped {
                app_id: app_id.into(),
            })
        });
        drop(state);
        match result {
            Ok(data) => {
                self.finish_explicit_stop(runtime_token);
                AppdResponse::success(request_id, data)
            }
            Err(error) => {
                self.cancel_explicit_stop(runtime_token);
                command_error_response(request_id, &CommandError::Manager(error))
            }
        }
    }

    fn activate_task_control(&self, request_id: u64, task_id: TaskId) -> AppdResponse {
        let _lifecycle = match self.lifecycle.lock() {
            Ok(lifecycle) => lifecycle,
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application lifecycle coordinator is unavailable",
                );
            }
        };
        let app_id = match self.state.lock() {
            Ok(state) => match state.tasks.task(task_id) {
                Some(task) => task.app_id.clone(),
                None => {
                    return AppdResponse::error(
                        request_id,
                        ErrorCode::NotFound,
                        "task was not found",
                    );
                }
            },
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application service state is unavailable",
                );
            }
        };
        match self.start_or_activate_app(&app_id) {
            Ok((task_id, token, unit, started)) => {
                if started {
                    let version = match self.state.lock() {
                        Ok(state) => state
                            .tasks
                            .task(task_id)
                            .expect("started task remains registered")
                            .version
                            .clone(),
                        Err(_) => {
                            return AppdResponse::error(
                                request_id,
                                ErrorCode::Internal,
                                "application service state is unavailable",
                            );
                        }
                    };
                    self.start_runtime_monitor(task_id, token, app_id.clone(), version, unit);
                } else {
                    self.set_foreground_runtime(token);
                }
                AppdResponse::success(
                    request_id,
                    ResponseData::TaskActivated {
                        task_id: task_id.0,
                        app_id,
                        runtime_generation: token,
                    },
                )
            }
            Err(error) => command_error_response(request_id, &error),
        }
    }

    fn close_task_control(&self, request_id: u64, task_id: TaskId) -> AppdResponse {
        let _lifecycle = match self.lifecycle.lock() {
            Ok(lifecycle) => lifecycle,
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application lifecycle coordinator is unavailable",
                );
            }
        };
        let task = match self.state.lock() {
            Ok(state) => state.tasks.task(task_id).cloned(),
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application service state is unavailable",
                );
            }
        };
        let Some(task) = task else {
            return AppdResponse::error(request_id, ErrorCode::NotFound, "task was not found");
        };
        let runtime_token = task.runtime().map(|runtime| runtime.token);
        if runtime_token.is_some() {
            self.mark_explicit_stop(&task.app_id);
        }
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.cancel_explicit_stop(runtime_token);
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application service state is unavailable",
                );
            }
        };
        if task.state.is_resident() {
            if let Err(error) = state.manager.stop(&task.app_id) {
                drop(state);
                self.cancel_explicit_stop(runtime_token);
                return command_error_response(request_id, &CommandError::Manager(error));
            }
        }
        if let Err(error) = state.tasks.close(task_id) {
            drop(state);
            self.cancel_explicit_stop(runtime_token);
            return command_error_response(request_id, &CommandError::Task(error));
        }
        state.permissions.clear_app_session(&task.app_id);
        state.document_prompts.clear_app(&task.app_id);
        state.media_sessions.clear_app(&task.app_id);
        drop(state);
        self.finish_explicit_stop(runtime_token);
        AppdResponse::success(
            request_id,
            ResponseData::TaskClosed {
                task_id: task_id.0,
                app_id: task.app_id,
            },
        )
    }

    fn install_package(&self, request_id: u64, package_name: &str) -> AppdResponse {
        let path = std::path::Path::new("/run/cardputerzero-appd").join(package_name);
        let prepared = match self.installer.install(path) {
            Ok(prepared) => prepared,
            Err(error) => return install_error_response(request_id, &error),
        };
        self.commit_prepared_install(request_id, prepared, false)
    }

    fn commit_prepared_install(
        &self,
        request_id: u64,
        prepared: crate::PreparedInstall,
        allow_running_idempotent_replay: bool,
    ) -> AppdResponse {
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
        let result = (|| -> Result<ResponseData, CommandError> {
            let already_registered = state
                .manager
                .registry()
                .account(&prepared.manifest.id)
                .is_some_and(|account| account.installed_version.is_some());
            let previous_version = state
                .manager
                .registry()
                .account(&prepared.manifest.id)
                .and_then(|account| account.installed_version.clone());
            let idempotent_replay = allow_running_idempotent_replay
                && previous_version.as_deref() == Some(prepared.manifest.version.as_str());
            if already_registered
                && !idempotent_replay
                && (task_blocks_package_change(&state.tasks, &prepared.manifest.id)
                    || state.manager.is_running(&prepared.manifest.id)?)
            {
                return Err(AppManagerError::AlreadyRunning(prepared.manifest.id.clone()).into());
            }
            let account = state.manager.prepare_account(&prepared.manifest.id)?;
            let (uid, gid) = crate::lookup_unix_account(&account.unix_user)?;
            if uid != account.unix_uid || gid != account.unix_uid {
                return Err(AppManagerError::InvalidHostIdentity(format!(
                    "{} must resolve to UID/GID {}/{}",
                    account.unix_user, account.unix_uid, account.unix_uid
                ))
                .into());
            }
            let installed = state.manager.mark_installed(&prepared.manifest)?;
            Ok(ResponseData::Installed {
                app_id: installed.app_id,
                version: installed.version,
                previous_version: if idempotent_replay {
                    None
                } else {
                    previous_version
                },
                trust: match prepared.trust {
                    crate::TrustDecision::Store => "store",
                    crate::TrustDecision::DeveloperMode => "developer-mode",
                }
                .into(),
            })
        })();
        match result {
            Ok(data) => AppdResponse::success(request_id, data),
            Err(error) => command_error_response(request_id, &error),
        }
    }

    fn install_store_package(
        &self,
        request_id: u64,
        peer_uid: u32,
        command: AppdCommand,
    ) -> AppdResponse {
        let AppdCommand::StoreInstall {
            package_name,
            app_id,
            version,
            package_sha256,
            package_bytes,
            automatic,
        } = command
        else {
            unreachable!("store installation requires store-install command")
        };
        let current_version = match self.state.lock() {
            Ok(state) => {
                let allowed = if automatic {
                    state.policy.allows_store_auto_update(&app_id)
                } else {
                    state.policy.allows_store_install(&app_id)
                };
                if !allowed {
                    return AppdResponse::error(
                        request_id,
                        ErrorCode::Unauthorized,
                        "store installation mode is blocked by device policy",
                    );
                }
                state
                    .manager
                    .installed_apps()
                    .into_iter()
                    .find(|installed| installed.app_id == app_id)
                    .map(|installed| installed.version)
            }
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application service state is unavailable",
                );
            }
        };
        if !store_version_is_acceptable(current_version.as_deref(), &version) {
            return AppdResponse::error(
                request_id,
                ErrorCode::Conflict,
                "store installation must increase or exactly replay the installed version",
            );
        }
        let Some(package_sha256) = cp0_store_protocol::decode_hex::<32>(&package_sha256) else {
            return AppdResponse::error(
                request_id,
                ErrorCode::InvalidRequest,
                "store package hash is invalid",
            );
        };
        let path = std::path::Path::new("/run/cardputerzero-appd/store").join(&package_name);
        let prepared = match self.installer.install_store(
            path,
            peer_uid,
            &app_id,
            &version,
            &package_sha256,
            package_bytes,
        ) {
            Ok(prepared) => prepared,
            Err(error) => return install_error_response(request_id, &error),
        };
        self.commit_prepared_install(request_id, prepared, true)
    }

    fn start_app_control(&self, request_id: u64, app_id: &str) -> AppdResponse {
        let _lifecycle = match self.lifecycle.lock() {
            Ok(lifecycle) => lifecycle,
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application lifecycle coordinator is unavailable",
                );
            }
        };
        match self.start_or_activate_app(app_id) {
            Ok((task_id, token, unit, started)) => {
                if started {
                    let (version, app_id) = match self.state.lock() {
                        Ok(state) => {
                            let task = state
                                .tasks
                                .task(task_id)
                                .expect("new runtime has a task record");
                            (task.version.clone(), task.app_id.clone())
                        }
                        Err(_) => {
                            return AppdResponse::error(
                                request_id,
                                ErrorCode::Internal,
                                "application service state is unavailable",
                            );
                        }
                    };
                    self.start_runtime_monitor(task_id, token, app_id, version, unit.clone());
                } else {
                    self.set_foreground_runtime(token);
                }
                AppdResponse::success(
                    request_id,
                    ResponseData::Started {
                        app_id: app_id.into(),
                        unit,
                    },
                )
            }
            Err(error) => command_error_response(request_id, &error),
        }
    }

    fn start_or_activate_app(
        &self,
        app_id: &str,
    ) -> Result<(TaskId, u64, String, bool), CommandError> {
        let (existing, victim, version) = {
            let state = self
                .state
                .lock()
                .map_err(|_| CommandError::Task(TaskError::InvalidCapacity))?;
            if !state.policy.allows_app(app_id) {
                return Err(CommandError::Restricted(
                    "application launch is blocked by device policy",
                ));
            }
            let version = state.manager.installed_manifest(app_id)?.version;
            let existing = state.tasks.task_for_app(app_id).cloned();
            let victim =
                (existing.is_none() && state.tasks.len() == state.tasks.capacity()).then(|| {
                    state
                        .tasks
                        .oldest_task()
                        .expect("a full task registry has an oldest task")
                        .clone()
                });
            (existing, victim, version)
        };

        if let Some(task) = existing.as_ref().filter(|task| task.state.is_resident()) {
            if task.state == TaskState::Frozen {
                self.state
                    .lock()
                    .map_err(|_| CommandError::Task(TaskError::TaskNotFound(task.task_id)))?
                    .manager
                    .thaw(app_id)?;
            }
            let mut state = self
                .state
                .lock()
                .map_err(|_| CommandError::Task(TaskError::TaskNotFound(task.task_id)))?;
            state.tasks.activate(task.task_id)?;
            let runtime = task.runtime().expect("resident task has a runtime binding");
            return Ok((task.task_id, runtime.token, runtime.unit.clone(), false));
        }

        if let Some(victim) = victim {
            let checkpoint = if victim.checkpoint.is_available() {
                victim.checkpoint.clone()
            } else {
                CheckpointStatus::Unavailable {
                    reason: CheckpointFailure::Unsupported,
                }
            };
            if victim.state.is_resident() {
                let stop_token = self.mark_explicit_stop(&victim.app_id);
                let stop_result = self
                    .state
                    .lock()
                    .map_err(|_| CommandError::Task(TaskError::TaskNotFound(victim.task_id)))?
                    .manager
                    .stop(&victim.app_id);
                if let Err(error) = stop_result {
                    self.cancel_explicit_stop(stop_token);
                    return Err(CommandError::Manager(error));
                }
                self.finish_explicit_stop(stop_token);
            }
            let mut state = self
                .state
                .lock()
                .map_err(|_| CommandError::Task(TaskError::TaskNotFound(victim.task_id)))?;
            state.tasks.evict_capacity_victim(EvictionCheckpoint {
                task_id: victim.task_id,
                status: checkpoint,
            })?;
            state.permissions.clear_app_session(&victim.app_id);
            state.document_prompts.clear_app(&victim.app_id);
            state.media_sessions.clear_app(&victim.app_id);
        }

        let token = self.runtime_sequence.fetch_add(1, Ordering::Relaxed);
        let mut state = self
            .state
            .lock()
            .map_err(|_| CommandError::Task(TaskError::InvalidCapacity))?;
        let unit = state.manager.start(app_id)?;
        let runtime = RuntimeBinding::new(token, unit.clone())?;
        let outcome = match state.tasks.launch(app_id, version, runtime, None) {
            Ok(outcome) => outcome,
            Err(error) => {
                let _ = state.manager.stop(app_id);
                return Err(CommandError::Task(error));
            }
        };
        state.media_sessions.clear();
        Ok((outcome.task_id, token, unit, true))
    }

    fn start_runtime_monitor(
        &self,
        task_id: TaskId,
        token: u64,
        app_id: String,
        version: String,
        unit: String,
    ) {
        let Ok(mut runtime) = self.runtime.lock() else {
            eprintln!("cp0-appd: runtime metrics state is unavailable");
            return;
        };
        runtime.sessions.insert(
            token,
            RuntimeSession {
                token,
                task_id,
                app_id: app_id.clone(),
                version: version.clone(),
                explicit_stop: false,
            },
        );
        runtime.foreground = Some(token);
        drop(runtime);

        let server = self.clone();
        if let Err(error) = thread::Builder::new()
            .name("cp0-app-runtime-watch".into())
            .spawn(move || server.monitor_runtime(token, app_id, version, unit))
        {
            eprintln!("cp0-appd: cannot start runtime monitor: {error}");
            if let Ok(mut runtime) = self.runtime.lock() {
                take_runtime_end(&mut runtime, token);
            }
        }
    }

    fn monitor_runtime(&self, token: u64, app_id: String, version: String, unit: String) {
        self.report_runtime_metric(token, &app_id, &version, StoreRuntimeMetricEvent::Launch);
        loop {
            if !self.runtime_is_current(token) {
                return;
            }
            match crate::lifecycle::wait_for_unit_stopped(&unit) {
                Ok(()) => break,
                Err(error) => {
                    eprintln!("cp0-appd: runtime monitor wait failed for {app_id}: {error}");
                    thread::sleep(RUNTIME_MONITOR_RETRY);
                }
            }
        }

        let crashed = match self.runtime.lock() {
            Ok(mut runtime) => match take_runtime_end(&mut runtime, token) {
                Some(crashed) => crashed,
                None => return,
            },
            Err(_) => return,
        };
        if let Ok(mut state) = self.state.lock() {
            if crashed {
                state.tasks.runtime_exited(token);
            }
            state.permissions.clear_app_session(&app_id);
            state.document_prompts.clear_app(&app_id);
            state.media_sessions.clear_app(&app_id);
        }
        if crashed {
            self.report_runtime_metric(token, &app_id, &version, StoreRuntimeMetricEvent::Crash);
        }
    }

    fn report_runtime_metric(
        &self,
        token: u64,
        app_id: &str,
        version: &str,
        event: StoreRuntimeMetricEvent,
    ) {
        if let Err(error) = self.store_metrics.record(token, app_id, version, event) {
            eprintln!("cp0-appd: optional runtime aggregate was not recorded: {error}");
        }
    }

    fn runtime_is_current(&self, token: u64) -> bool {
        self.runtime
            .lock()
            .is_ok_and(|runtime| runtime.sessions.contains_key(&token))
    }

    fn set_foreground_runtime(&self, token: u64) {
        if let Ok(mut runtime) = self.runtime.lock() {
            if runtime.sessions.contains_key(&token) {
                runtime.foreground = Some(token);
            }
        }
    }

    fn set_foreground_app_control(&self, request_id: u64, app_id: Option<&str>) -> AppdResponse {
        let _lifecycle = match self.lifecycle.lock() {
            Ok(lifecycle) => lifecycle,
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application lifecycle coordinator is unavailable",
                );
            }
        };
        let mut runtime = match self.runtime.lock() {
            Ok(runtime) => runtime,
            Err(_) => {
                return AppdResponse::error(
                    request_id,
                    ErrorCode::Internal,
                    "application runtime state is unavailable",
                );
            }
        };
        let Some(app_id) = app_id else {
            runtime.foreground = None;
            return AppdResponse::success(
                request_id,
                ResponseData::ForegroundAppChanged { app_id: None },
            );
        };
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
        let Some(task) = state.tasks.task_for_app(app_id) else {
            return AppdResponse::error(request_id, ErrorCode::NotFound, "task was not found");
        };
        let task_id = task.task_id;
        let task_is_foreground = task.state == TaskState::Foreground;
        let Some(binding) = task.runtime() else {
            return AppdResponse::error(
                request_id,
                ErrorCode::Unavailable,
                "task does not have a resident runtime",
            );
        };
        let token = binding.token;
        if !runtime
            .sessions
            .get(&token)
            .is_some_and(|session| session.app_id == app_id)
        {
            return AppdResponse::error(
                request_id,
                ErrorCode::Unavailable,
                "task runtime identity is unavailable",
            );
        }
        if !task_is_foreground {
            if let Err(error) = state.tasks.activate(task_id) {
                return command_error_response(request_id, &CommandError::Task(error));
            }
        }
        runtime.foreground = Some(token);
        AppdResponse::success(
            request_id,
            ResponseData::ForegroundAppChanged {
                app_id: Some(app_id.into()),
            },
        )
    }

    fn mark_explicit_stop(&self, app_id: &str) -> Option<u64> {
        let mut runtime = self.runtime.lock().ok()?;
        let session = runtime
            .sessions
            .values_mut()
            .find(|session| session.app_id == app_id)?;
        session.explicit_stop = true;
        Some(session.token)
    }

    fn cancel_explicit_stop(&self, token: Option<u64>) {
        let Some(token) = token else {
            return;
        };
        if let Ok(mut runtime) = self.runtime.lock() {
            if let Some(session) = runtime.sessions.get_mut(&token) {
                session.explicit_stop = false;
            }
        }
    }

    fn finish_explicit_stop(&self, token: Option<u64>) {
        let Some(token) = token else {
            return;
        };
        if let Ok(mut runtime) = self.runtime.lock() {
            take_runtime_end(&mut runtime, token);
        }
    }

    fn list_apps(
        &self,
        state: &ServerState,
        request_id: u64,
        offset: u16,
        limit: u8,
    ) -> Result<ResponseData, CommandError> {
        let installed_apps = state.manager.installed_apps();
        let mut apps = Vec::new();
        for (index, installed) in installed_apps
            .iter()
            .skip(usize::from(offset))
            .take(usize::from(limit))
            .enumerate()
        {
            let manifest = state
                .manager
                .installed_manifest(&installed.app_id)
                .map_err(CommandError::Manager)?;
            let package_bytes = state
                .manager
                .package_usage(&installed.app_id)
                .map_err(CommandError::Manager)?;
            let storage_request_id = request_id.wrapping_add(index as u64);
            let storage_quota_bytes =
                u64::from(manifest.resources.storage_mb) * cp0_storage_protocol::MIB;
            let data_bytes = self
                .capabilities
                .storage
                .usage(storage_request_id, &installed.app_id, storage_quota_bytes)
                .map_err(CommandError::Storage)?;
            apps.push(AppSummary {
                running: state.manager.has_resident_process(&installed.app_id)?,
                removable: crate::is_removable_app(&installed.app_id),
                app_id: installed.app_id.clone(),
                name: manifest.name,
                version: installed.version.clone(),
                display: manifest.display,
                installed_at_unix_seconds: installed.installed_at_unix_seconds,
                package_bytes,
                data_bytes,
                permissions: manifest
                    .permissions
                    .into_iter()
                    .map(|request| request.name)
                    .collect(),
            });
        }
        let consumed = usize::from(offset) + apps.len();
        let next_offset = (consumed < installed_apps.len()).then(|| {
            u16::try_from(consumed).expect("application registry is bounded below u16::MAX")
        });
        Ok(ResponseData::Applications { apps, next_offset })
    }

    fn list_tasks(
        state: &ServerState,
        offset: u8,
        limit: u8,
    ) -> Result<ResponseData, CommandError> {
        let ordered: Vec<_> = state.tasks.switcher_order().into_iter().cloned().collect();
        let mut tasks = Vec::new();
        for task in ordered
            .iter()
            .skip(usize::from(offset))
            .take(usize::from(limit))
        {
            let manifest = state.manager.installed_manifest(&task.app_id)?;
            let account_uid = state
                .manager
                .installed_apps()
                .into_iter()
                .find(|installed| installed.app_id == task.app_id)
                .ok_or_else(|| AppManagerError::NotInstalled(task.app_id.clone()))?
                .account_uid;
            tasks.push(TaskSummary {
                task_id: task.task_id.0,
                account_uid,
                app_id: task.app_id.clone(),
                name: manifest.name,
                version: task.version.clone(),
                display: manifest.display,
                state: task.state,
                created_sequence: task.created_sequence,
                last_activated_sequence: task.last_activated_sequence,
                checkpoint: task.checkpoint.clone(),
                runtime_generation: task.runtime().map(|runtime| runtime.token),
                thumbnail_generation: task.thumbnail_generation,
            });
        }
        let consumed = usize::from(offset).saturating_add(tasks.len());
        let next_offset = (consumed < ordered.len()).then_some(consumed as u8);
        Ok(ResponseData::Tasks { tasks, next_offset })
    }

    fn list_store_apps(
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
            let permissions = canonical_store_permissions(
                manifest
                    .permissions
                    .into_iter()
                    .map(|request| request.name)
                    .collect(),
            );
            apps.push(crate::StoreInstalledApp {
                app_id: installed.app_id.clone(),
                version: installed.version.clone(),
                permissions,
            });
        }
        let consumed = usize::from(offset) + apps.len();
        let next_offset = (consumed < installed_apps.len()).then(|| {
            u16::try_from(consumed).expect("application registry is bounded below u16::MAX")
        });
        Ok(ResponseData::StoreApplications { apps, next_offset })
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
        let (request, descriptor) = match recv_broker_request_with_fd(&stream) {
            Ok(request) => request,
            Err(error) => {
                write_broker_response(
                    &mut stream,
                    &BrokerResponse::error(0, BrokerErrorCode::InvalidRequest, error.to_string()),
                )
                .map_err(broker_io)?;
                return Ok(());
            }
        };
        let imports_photo = matches!(request.command, BrokerCommand::PhotoImportRgb565 { .. });
        if imports_photo != descriptor.is_some() {
            write_broker_response(
                &mut stream,
                &BrokerResponse::error(
                    request.request_id,
                    BrokerErrorCode::InvalidRequest,
                    if imports_photo {
                        "photo import requires exactly one sealed frame descriptor"
                    } else {
                        "broker command does not accept a descriptor"
                    },
                ),
            )
            .map_err(broker_io)?;
            return Ok(());
        }
        let dispatch = match &request.command {
            BrokerCommand::OpenDocument => self.dispatch_document(credentials, request.request_id),
            BrokerCommand::CaptureCamera => self.dispatch_camera(credentials, request.request_id),
            BrokerCommand::CapturePhoto => BrokerDispatch::response(
                self.dispatch_camera_photo(credentials, request.request_id),
            ),
            BrokerCommand::PhotoImportRgb565 { suggested_id } => {
                BrokerDispatch::response(self.dispatch_photo_import(
                    credentials,
                    request.request_id,
                    *suggested_id,
                    descriptor.expect("photo descriptor was checked"),
                ))
            }
            BrokerCommand::PhotoLoadRgb565 { photo_id } => {
                self.dispatch_photo_load(credentials, request.request_id, *photo_id)
            }
            BrokerCommand::PhotoLoadViewRgb565 {
                photo_id,
                zoom_level,
                pan_x,
                pan_y,
            } => self.dispatch_photo_view(
                credentials,
                request.request_id,
                *photo_id,
                *zoom_level,
                *pan_x,
                *pan_y,
            ),
            BrokerCommand::SendIntent {
                action,
                payload_base64,
            } => self.dispatch_send_intent(credentials, request.request_id, action, payload_base64),
            _ => BrokerDispatch::response(self.dispatch_broker(credentials, request)),
        };
        let write_result = if let Some(descriptor) = dispatch.descriptor.as_ref() {
            let frame = encode_broker_response(&dispatch.response).map_err(broker_io)?;
            send_frame_with_fd(&mut stream, &frame, descriptor.as_fd())
                .map_err(document_protocol_io)
        } else {
            write_broker_response(&mut stream, &dispatch.response).map_err(broker_io)
        };
        if let Err(error) = write_result {
            if let Some(transition) = &dispatch.transition {
                self.cancel_intent(transition.intent_id);
            }
            return Err(error);
        }
        if let Some(transition) = dispatch.transition {
            self.complete_intent_transition(transition);
        }
        Ok(())
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
            BrokerCommand::HttpGetRange {
                url,
                offset,
                length,
            } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::NetworkClient)
                {
                    return response;
                }
                match self
                    .capabilities
                    .network
                    .http_get_range(request_id, &url, offset, length)
                {
                    Ok(response) => BrokerResponse::http_response(
                        request_id,
                        response.status_code,
                        response.body_base64,
                    ),
                    Err(error) => {
                        eprintln!("cp0-appd: network range request failed: {error}");
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
            BrokerCommand::PlayAudioStereo48k { samples_base64 } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::AudioPlayback)
                {
                    return response;
                }
                let samples = match cp0_audio_protocol::decode_music_samples(&samples_base64) {
                    Ok(samples) => samples,
                    Err(_) => {
                        return BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::InvalidRequest,
                            "invalid bounded stereo playback samples",
                        );
                    }
                };
                match self
                    .capabilities
                    .audio
                    .play_stereo_48k(request_id, &samples)
                {
                    Ok(frames) => BrokerResponse::audio_played(request_id, frames),
                    Err(error) => {
                        eprintln!("cp0-appd: stereo audio playback request failed: {error}");
                        audio_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::PlayKeyClick => {
                if let Err(response) = self.with_current_media_caller(
                    peer,
                    request_id,
                    |_app_id, _runtime_token, _sessions| (),
                ) {
                    return response;
                }
                match self.capabilities.audio.play_key_click(request_id) {
                    Ok(()) => BrokerResponse::audio_played(request_id, KEY_CLICK_FRAMES),
                    Err(error) => {
                        eprintln!("cp0-appd: key click request failed: {error}");
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
            BrokerCommand::CapturePhoto => {
                unreachable!("camera photo requests use a system-owned transaction")
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
            BrokerCommand::SendLora { payload_base64 } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::RadioLora)
                {
                    return response;
                }
                let payload = match cp0_radio_protocol::decode_payload(&payload_base64) {
                    Ok(payload) => payload,
                    Err(_) => {
                        return BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::InvalidRequest,
                            "invalid bounded LoRa payload",
                        );
                    }
                };
                match self.capabilities.radio.send(request_id, &payload) {
                    Ok(bytes) => BrokerResponse::lora_sent(request_id, bytes),
                    Err(error) => {
                        eprintln!("cp0-appd: LoRa send request failed: {error}");
                        radio_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::ReceiveLora { timeout_ms } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::RadioLora)
                {
                    return response;
                }
                match self.capabilities.radio.receive(request_id, timeout_ms) {
                    Ok(Some(packet)) => BrokerResponse::lora_packet(
                        request_id,
                        &packet.payload,
                        packet.rssi_dbm,
                        packet.snr_quarter_db,
                    ),
                    Ok(None) => BrokerResponse::lora_no_packet(request_id),
                    Err(error) => {
                        eprintln!("cp0-appd: LoRa receive request failed: {error}");
                        radio_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::StoragePut { key, value_base64 } => {
                let app = match self.authenticate_broker_caller(peer, request_id) {
                    Ok(app) => app,
                    Err(response) => return response,
                };
                let value = match cp0_storage_protocol::decode_value(&value_base64) {
                    Ok(value) => value,
                    Err(_) => {
                        return BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::InvalidRequest,
                            "invalid bounded private storage value",
                        );
                    }
                };
                match self.capabilities.storage.put(
                    request_id,
                    &app.app_id,
                    app.storage_quota_bytes,
                    &key,
                    &value,
                ) {
                    Ok(used_bytes) => BrokerResponse::storage_stored(request_id, used_bytes),
                    Err(error) => {
                        eprintln!("cp0-appd: private storage put failed: {error}");
                        storage_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::StorageGet { key } => {
                let app = match self.authenticate_broker_caller(peer, request_id) {
                    Ok(app) => app,
                    Err(response) => return response,
                };
                match self.capabilities.storage.get(
                    request_id,
                    &app.app_id,
                    app.storage_quota_bytes,
                    &key,
                ) {
                    Ok(Some(value)) => BrokerResponse::storage_value(request_id, &value),
                    Ok(None) => BrokerResponse::storage_not_found(request_id),
                    Err(error) => {
                        eprintln!("cp0-appd: private storage get failed: {error}");
                        storage_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::StorageDelete { key } => {
                let app = match self.authenticate_broker_caller(peer, request_id) {
                    Ok(app) => app,
                    Err(response) => return response,
                };
                match self.capabilities.storage.delete(
                    request_id,
                    &app.app_id,
                    app.storage_quota_bytes,
                    &key,
                ) {
                    Ok(existed) => BrokerResponse::storage_deleted(request_id, existed),
                    Err(error) => {
                        eprintln!("cp0-appd: private storage delete failed: {error}");
                        storage_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::PhotoPut { .. } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::PhotosWrite)
                {
                    return response;
                }
                BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::InvalidRequest,
                    "legacy direct photo writes are disabled; use photo-import-rgb565",
                )
            }
            BrokerCommand::PhotoGet { key } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::PhotosRead)
                {
                    return response;
                }
                let _transaction = match self.lock_photo_library(request_id) {
                    Ok(transaction) => transaction,
                    Err(response) => return response,
                };
                let result = if let Some((photo_id, chunk)) = parse_photo_chunk_key(&key) {
                    match self.capabilities.storage.get_blob_chunk(
                        request_id,
                        PHOTO_LIBRARY_ID,
                        PHOTO_LIBRARY_QUOTA_BYTES,
                        &photo_blob_key(photo_id),
                        u32::try_from(chunk * cp0_storage_protocol::MAX_STORAGE_VALUE_BYTES)
                            .expect("photo chunk offset fits u32"),
                        photo_chunk_length(chunk) as u32,
                    ) {
                        Ok(Some(value)) => Ok(Some(value)),
                        Ok(None) => self.capabilities.storage.get(
                            request_id,
                            PHOTO_LIBRARY_ID,
                            PHOTO_LIBRARY_QUOTA_BYTES,
                            &key,
                        ),
                        Err(error) => Err(error),
                    }
                } else if valid_photo_metadata_key(&key) {
                    self.capabilities.storage.get(
                        request_id,
                        PHOTO_LIBRARY_ID,
                        PHOTO_LIBRARY_QUOTA_BYTES,
                        &key,
                    )
                } else {
                    return BrokerResponse::error(
                        request_id,
                        BrokerErrorCode::InvalidRequest,
                        "photo library key is outside the versioned format",
                    );
                };
                match result {
                    Ok(Some(value)) => BrokerResponse::storage_value(request_id, &value),
                    Ok(None) => BrokerResponse::storage_not_found(request_id),
                    Err(error) => {
                        eprintln!("cp0-appd: photo library get failed: {error}");
                        storage_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::PhotoIndexGet => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::PhotosWrite)
                {
                    return response;
                }
                let _transaction = match self.lock_photo_library(request_id) {
                    Ok(transaction) => transaction,
                    Err(response) => return response,
                };
                match self.capabilities.storage.get(
                    request_id,
                    PHOTO_LIBRARY_ID,
                    PHOTO_LIBRARY_QUOTA_BYTES,
                    PHOTO_LIBRARY_HEAD_KEY,
                ) {
                    Ok(Some(value)) => BrokerResponse::storage_value(request_id, &value),
                    Ok(None) => BrokerResponse::storage_not_found(request_id),
                    Err(error) => {
                        eprintln!("cp0-appd: photo library index read failed: {error}");
                        storage_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::PhotoDelete { .. } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::PhotosWrite)
                {
                    return response;
                }
                BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::InvalidRequest,
                    "legacy direct photo deletion is disabled; use photo-remove",
                )
            }
            BrokerCommand::PhotoRemove { photo_id } => {
                if let Err(response) =
                    self.authorize_broker_caller(peer, request_id, Permission::PhotosWrite)
                {
                    return response;
                }
                let _transaction = match self.lock_photo_library(request_id) {
                    Ok(transaction) => transaction,
                    Err(response) => return response,
                };
                match remove_photo(&self.capabilities.storage, request_id, photo_id) {
                    Ok(existed) => BrokerResponse::storage_deleted(request_id, existed),
                    Err(error) => {
                        eprintln!("cp0-appd: photo library removal failed: {error}");
                        photo_broker_error_response(request_id, &error)
                    }
                }
            }
            BrokerCommand::PhotoImportRgb565 { .. } => {
                unreachable!("photo imports carry descriptors")
            }
            BrokerCommand::PhotoLoadRgb565 { .. } => {
                unreachable!("photo loads carry descriptors")
            }
            BrokerCommand::PhotoLoadViewRgb565 { .. } => {
                unreachable!("photo view loads carry descriptors")
            }
            BrokerCommand::SendIntent { .. } => {
                unreachable!("intent send requires an acknowledgement-bound transition")
            }
            BrokerCommand::TakeIntent => {
                let app = match self.authenticate_broker_caller(peer, request_id) {
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
                match state.intents.take(&app.app_id) {
                    Some(intent) => {
                        BrokerResponse::intent_message(request_id, intent.action, &intent.payload)
                    }
                    None => BrokerResponse::intent_empty(request_id),
                }
            }
            BrokerCommand::UpdateMediaSession {
                state: playback_state,
                supported_actions,
            } => {
                if let Err(response) = self.with_current_media_caller(
                    peer,
                    request_id,
                    |app_id, runtime_token, sessions| {
                        sessions.update(app_id, runtime_token, playback_state, supported_actions);
                    },
                ) {
                    return response;
                }
                BrokerResponse::media_session_updated(request_id, playback_state, supported_actions)
            }
            BrokerCommand::TakeMediaAction => {
                let action = match self.with_current_media_caller(
                    peer,
                    request_id,
                    |app_id, runtime_token, sessions| sessions.take(app_id, runtime_token),
                ) {
                    Ok(action) => action,
                    Err(response) => return response,
                };
                match action {
                    Some(action) => BrokerResponse::media_action(request_id, action),
                    None => BrokerResponse::media_action_empty(request_id),
                }
            }
        }
    }

    fn lock_photo_library(
        &self,
        request_id: u64,
    ) -> Result<std::sync::MutexGuard<'_, ()>, BrokerResponse> {
        self.photo_library.lock().map_err(|_| {
            BrokerResponse::error(
                request_id,
                BrokerErrorCode::Internal,
                "photo library transaction state is unavailable",
            )
        })
    }

    fn dispatch_photo_import(
        &self,
        peer: crate::PeerCredentials,
        request_id: u64,
        suggested_id: u64,
        descriptor: OwnedFd,
    ) -> BrokerResponse {
        if let Err(response) =
            self.authorize_broker_caller(peer, request_id, Permission::PhotosWrite)
        {
            return response;
        }
        let frame = match read_photo_frame(descriptor) {
            Ok(frame) => frame,
            Err(error) => {
                eprintln!("cp0-appd: rejected application photo descriptor: {error}");
                return BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::InvalidRequest,
                    "photo frame descriptor does not satisfy the fixed contract",
                );
            }
        };
        let _transaction = match self.lock_photo_library(request_id) {
            Ok(transaction) => transaction,
            Err(response) => return response,
        };
        match import_app_photo(&self.capabilities.storage, request_id, &frame, suggested_id) {
            Ok(photo_id) => BrokerResponse::photo_imported(request_id, photo_id),
            Err(error) => {
                eprintln!("cp0-appd: application photo import failed: {error}");
                photo_broker_error_response(request_id, &error)
            }
        }
    }

    fn dispatch_photo_load(
        &self,
        peer: crate::PeerCredentials,
        request_id: u64,
        photo_id: u64,
    ) -> BrokerDispatch {
        if let Err(response) =
            self.authorize_broker_caller(peer, request_id, Permission::PhotosRead)
        {
            return BrokerDispatch::response(response);
        }
        let _transaction = match self.lock_photo_library(request_id) {
            Ok(transaction) => transaction,
            Err(response) => return BrokerDispatch::response(response),
        };
        match photo_is_active(&self.capabilities.storage, request_id, photo_id) {
            Ok(true) => {}
            Ok(false) => {
                return BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::NotFound,
                    "photo is not present in the shared library",
                ));
            }
            Err(error) => {
                eprintln!("cp0-appd: photo index lookup failed: {error}");
                return BrokerDispatch::response(photo_broker_error_response(request_id, &error));
            }
        }
        let descriptor = match self.capabilities.storage.open_blob(
            request_id,
            PHOTO_LIBRARY_ID,
            PHOTO_LIBRARY_QUOTA_BYTES,
            &photo_blob_key(photo_id),
            PHOTO_FRAME_BYTES as u32,
        ) {
            Ok(Some(descriptor)) => descriptor,
            Ok(None) => match load_legacy_photo(&self.capabilities.storage, request_id, photo_id) {
                Ok(Some(frame)) => match sealed_photo_descriptor(&frame) {
                    Ok(descriptor) => descriptor,
                    Err(error) => {
                        eprintln!("cp0-appd: cannot seal legacy photo frame: {error}");
                        return BrokerDispatch::response(BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::Internal,
                            "photo descriptor could not be created",
                        ));
                    }
                },
                Ok(None) => {
                    return BrokerDispatch::response(BrokerResponse::error(
                        request_id,
                        BrokerErrorCode::NotFound,
                        "photo frame is unavailable",
                    ));
                }
                Err(error) => {
                    eprintln!("cp0-appd: legacy photo load failed: {error}");
                    return BrokerDispatch::response(photo_broker_error_response(
                        request_id, &error,
                    ));
                }
            },
            Err(error) => {
                eprintln!("cp0-appd: photo blob open failed: {error}");
                return BrokerDispatch::response(storage_error_response(request_id, &error));
            }
        };
        BrokerDispatch {
            response: BrokerResponse::photo_loaded(request_id, photo_id),
            descriptor: Some(descriptor),
            transition: None,
        }
    }

    fn dispatch_photo_view(
        &self,
        peer: crate::PeerCredentials,
        request_id: u64,
        photo_id: u64,
        zoom_level: u8,
        pan_x: i16,
        pan_y: i16,
    ) -> BrokerDispatch {
        if let Err(response) =
            self.authorize_broker_caller(peer, request_id, Permission::PhotosRead)
        {
            return BrokerDispatch::response(response);
        }
        let transaction = match self.lock_photo_library(request_id) {
            Ok(transaction) => transaction,
            Err(response) => return BrokerDispatch::response(response),
        };
        match photo_is_active(&self.capabilities.storage, request_id, photo_id) {
            Ok(true) => {}
            Ok(false) => {
                return BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::NotFound,
                    "photo is not present in the shared library",
                ));
            }
            Err(error) => {
                eprintln!("cp0-appd: photo view index lookup failed: {error}");
                return BrokerDispatch::response(photo_broker_error_response(request_id, &error));
            }
        }
        let mut cache = match self.photo_view.lock() {
            Ok(cache) => cache,
            Err(_) => {
                return BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "photo view cache is unavailable",
                ));
            }
        };
        let frame = match cache.render(
            &self.capabilities.storage,
            request_id,
            photo_id,
            zoom_level,
            pan_x,
            pan_y,
        ) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                drop(cache);
                drop(transaction);
                return self.dispatch_photo_load(peer, request_id, photo_id);
            }
            Err(error) => {
                eprintln!("cp0-appd: original photo view render failed: {error}");
                return BrokerDispatch::response(photo_view_error_response(request_id, &error));
            }
        };
        let descriptor = match sealed_photo_descriptor(&frame) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                eprintln!("cp0-appd: cannot seal rendered photo view: {error}");
                return BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "photo view descriptor could not be created",
                ));
            }
        };
        BrokerDispatch {
            response: BrokerResponse::photo_loaded(request_id, photo_id),
            descriptor: Some(descriptor),
            transition: None,
        }
    }

    fn dispatch_send_intent(
        &self,
        peer: crate::PeerCredentials,
        request_id: u64,
        action: &str,
        payload_base64: &str,
    ) -> BrokerDispatch {
        let app = match self.authenticate_broker_caller(peer, request_id) {
            Ok(app) => app,
            Err(response) => return BrokerDispatch::response(response),
        };
        let payload = match cp0_network_protocol::decode_base64(payload_base64) {
            Ok(payload) if payload.len() <= crate::MAX_INTENT_PAYLOAD_BYTES => payload,
            _ => {
                return BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::InvalidRequest,
                    "invalid bounded intent payload",
                ));
            }
        };
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                return BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application service state is unavailable",
                ));
            }
        };
        let mut manifests = Vec::new();
        for installed in state.manager.installed_apps() {
            match state.manager.installed_manifest(&installed.app_id) {
                Ok(manifest) => manifests.push(manifest),
                Err(error) => {
                    eprintln!("cp0-appd: cannot resolve intent receiver: {error}");
                    return BrokerDispatch::response(BrokerResponse::error(
                        request_id,
                        BrokerErrorCode::Internal,
                        "installed application metadata is unavailable",
                    ));
                }
            }
        }
        let target_app_id = match select_intent_target(action, &manifests) {
            Ok(target) => target,
            Err(IntentTargetError::NotFound) => {
                return BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::NotFound,
                    "no installed application handles the intent action",
                ));
            }
            Err(IntentTargetError::Ambiguous) => {
                return BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Ambiguous,
                    "multiple installed applications handle the intent action",
                ));
            }
        };
        let intent_id =
            match state
                .intents
                .enqueue(&app.app_id, &target_app_id, action.into(), payload)
            {
                Ok(intent_id) => intent_id,
                Err(error) => {
                    eprintln!("cp0-appd: cannot enqueue intent: {error}");
                    return BrokerDispatch::response(BrokerResponse::error(
                        request_id,
                        BrokerErrorCode::ResourceExhausted,
                        "intent queue is full",
                    ));
                }
            };
        BrokerDispatch {
            response: BrokerResponse::intent_accepted(request_id, intent_id),
            descriptor: None,
            transition: Some(IntentTransition {
                intent_id,
                sender_app_id: app.app_id,
                target_app_id,
            }),
        }
    }

    fn cancel_intent(&self, intent_id: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.intents.cancel(intent_id);
        }
    }

    fn complete_intent_transition(&self, transition: IntentTransition) {
        let response = self.start_app_control(transition.intent_id, &transition.target_app_id);
        if matches!(response.outcome, crate::ResponseOutcome::Error { .. }) {
            eprintln!(
                "cp0-appd: cannot activate intent receiver {} for sender {}",
                transition.target_app_id, transition.sender_app_id
            );
        }
    }

    fn dispatch_camera(&self, peer: crate::PeerCredentials, request_id: u64) -> BrokerDispatch {
        let app = match self.authorize_broker_caller(peer, request_id, Permission::CameraCapture) {
            Ok(app) => app,
            Err(response) => return BrokerDispatch::response(response),
        };
        let foreground = match self.runtime.lock() {
            Ok(runtime) => runtime_app_is_foreground(&runtime, &app.app_id),
            Err(_) => {
                return BrokerDispatch::response(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application runtime state is unavailable",
                ));
            }
        };
        if !foreground {
            return BrokerDispatch::response(BrokerResponse::error(
                request_id,
                BrokerErrorCode::Unavailable,
                "camera access requires the current foreground runtime",
            ));
        }
        match self.capabilities.camera.capture(request_id) {
            Ok(frame) => BrokerDispatch {
                response: BrokerResponse::camera_captured(request_id),
                descriptor: Some(frame.descriptor),
                transition: None,
            },
            Err(error) => {
                eprintln!("cp0-appd: camera capture request failed: {error}");
                BrokerDispatch::response(camera_error_response(request_id, &error))
            }
        }
    }

    fn dispatch_camera_photo(
        &self,
        peer: crate::PeerCredentials,
        request_id: u64,
    ) -> BrokerResponse {
        let app = match self.authorize_broker_caller(peer, request_id, Permission::CameraCapture) {
            Ok(app) => app,
            Err(response) => return response,
        };
        if let Err(response) =
            self.authorize_broker_caller(peer, request_id, Permission::PhotosWrite)
        {
            return response;
        }
        let foreground = match self.runtime.lock() {
            Ok(runtime) => runtime_app_is_foreground(&runtime, &app.app_id),
            Err(_) => {
                return BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application runtime state is unavailable",
                );
            }
        };
        if !foreground {
            return BrokerResponse::error(
                request_id,
                BrokerErrorCode::Unavailable,
                "camera access requires the current foreground runtime",
            );
        }
        let captured = match self.capabilities.camera.capture_photo(request_id) {
            Ok(captured) => captured,
            Err(error) => {
                eprintln!("cp0-appd: camera photo request failed: {error}");
                return camera_error_response(request_id, &error);
            }
        };
        let payload = match read_camera_photo(captured.descriptor, captured.jpeg_size_bytes) {
            Ok(payload) => payload,
            Err(error) => {
                eprintln!("cp0-appd: rejected camera photo payload: {error}");
                return BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "camera returned an invalid photo payload",
                );
            }
        };
        let (thumbnail, jpeg) = match decode_photo_payload(&payload) {
            Ok(photo) => photo,
            Err(error) => {
                eprintln!("cp0-appd: camera photo payload validation failed: {error}");
                return BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "camera returned an invalid photo payload",
                );
            }
        };
        let captured_milliseconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let _transaction = match self.lock_photo_library(request_id) {
            Ok(transaction) => transaction,
            Err(response) => return response,
        };
        match import_camera_photo(
            &self.capabilities.storage,
            request_id,
            thumbnail,
            jpeg,
            captured_milliseconds,
        ) {
            Ok(photo_id) => BrokerResponse::photo_imported(request_id, photo_id),
            Err(error) => {
                eprintln!("cp0-appd: camera photo library import failed: {error}");
                photo_broker_error_response(request_id, &error)
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
                        transition: None,
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
                                transition: None,
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

    fn authenticate_broker_caller(
        &self,
        peer: crate::PeerCredentials,
        request_id: u64,
    ) -> Result<AuthorizedApp, BrokerResponse> {
        let state = match self.state.lock() {
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
        Ok(AuthorizedApp {
            app_id: manifest.id,
            app_name: manifest.name,
            storage_quota_bytes: u64::from(manifest.resources.storage_mb)
                * cp0_storage_protocol::MIB,
        })
    }

    fn authorize_broker_caller(
        &self,
        peer: crate::PeerCredentials,
        request_id: u64,
        permission: Permission,
    ) -> Result<AuthorizedApp, BrokerResponse> {
        let app = self.authenticate_broker_caller(peer, request_id)?;
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
        let manifest = match state.manager.installed_manifest(&app.app_id) {
            Ok(manifest) => manifest,
            Err(error) => {
                eprintln!("cp0-appd: cannot reload broker caller manifest: {error}");
                return Err(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "installed application metadata is unavailable",
                ));
            }
        };
        if state.policy.denies_permission(permission) {
            return Err(BrokerResponse::error(
                request_id,
                BrokerErrorCode::Denied,
                "capability is blocked by device policy",
            ));
        }
        match state.permissions.request(&manifest, permission) {
            Ok(PermissionRequestResult::Allow) => Ok(AuthorizedApp {
                app_id: manifest.id,
                app_name: manifest.name,
                storage_quota_bytes: app.storage_quota_bytes,
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

    fn with_current_media_caller<T>(
        &self,
        peer: crate::PeerCredentials,
        request_id: u64,
        operation: impl FnOnce(&str, u64, &mut MediaSessionBroker) -> T,
    ) -> Result<T, BrokerResponse> {
        let app = self.authenticate_broker_caller(peer, request_id)?;
        let runtime = match self.runtime.lock() {
            Ok(runtime) => runtime,
            Err(_) => {
                return Err(BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application runtime state is unavailable",
                ));
            }
        };
        let Some(session) = runtime
            .foreground
            .and_then(|token| runtime.sessions.get(&token))
            .filter(|session| session.app_id == app.app_id)
        else {
            return Err(BrokerResponse::error(
                request_id,
                BrokerErrorCode::Unavailable,
                "application is not the current foreground runtime",
            ));
        };
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

        // Runtime must be locked before state so lifecycle and broker paths agree.
        Ok(operation(
            &app.app_id,
            session.token,
            &mut state.media_sessions,
        ))
    }
}

fn control_command_authorized(
    uid: u32,
    command: &AppdCommand,
    trusted_uids: &BTreeSet<u32>,
    store_installer_uids: &BTreeSet<u32>,
    shell_uid: Option<u32>,
) -> bool {
    if matches!(
        command,
        AppdCommand::ImportScreenshot | AppdCommand::SetForegroundApp { .. }
    ) {
        return shell_uid == Some(uid);
    }
    if store_installer_uids.contains(&uid) {
        return matches!(
            command,
            AppdCommand::StoreInstall { .. } | AppdCommand::StoreListInstalled { .. }
        );
    }
    if !trusted_uids.contains(&uid) {
        return false;
    }
    match command {
        AppdCommand::Install { .. } | AppdCommand::Rollback { .. } | AppdCommand::Logs { .. } => {
            uid == 0
        }
        AppdCommand::StoreInstall { .. } | AppdCommand::StoreListInstalled { .. } => false,
        _ => true,
    }
}

fn read_photo_frame(descriptor: OwnedFd) -> std::io::Result<Vec<u8>> {
    let mut status: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(descriptor.as_raw_fd(), &raw mut status) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if status.st_mode & libc::S_IFMT != libc::S_IFREG
        || status.st_size != PHOTO_FRAME_BYTES as libc::off_t
        || !valid_photo_descriptor_access(flags)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "descriptor is not an exact read-only screenshot frame",
        ));
    }
    validate_screenshot_seals(&descriptor)?;

    let mut file = File::from(descriptor);
    file.seek(std::io::SeekFrom::Start(0))?;
    let mut frame = vec![0_u8; PHOTO_FRAME_BYTES];
    file.read_exact(&mut frame)?;
    Ok(frame)
}

fn read_camera_photo(descriptor: OwnedFd, jpeg_size_bytes: u32) -> std::io::Result<Vec<u8>> {
    let jpeg_size = jpeg_size_bytes as usize;
    if jpeg_size == 0 || jpeg_size > cp0_camera_protocol::MAX_CAMERA_JPEG_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "camera JPEG size is outside the fixed contract",
        ));
    }
    let expected = cp0_camera_protocol::CAMERA_PHOTO_HEADER_BYTES
        + cp0_camera_protocol::CAMERA_FRAME_BYTES
        + jpeg_size;
    let mut file = File::from(descriptor);
    let metadata = file.metadata()?;
    if metadata.len() != expected as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "camera photo descriptor has an unexpected size",
        ));
    }
    let mut payload = Vec::with_capacity(expected);
    file.read_to_end(&mut payload)?;
    if payload.len() != expected {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "camera photo descriptor was truncated",
        ));
    }
    Ok(payload)
}

#[cfg(target_os = "linux")]
fn sealed_photo_descriptor(frame: &[u8]) -> std::io::Result<OwnedFd> {
    if frame.len() != PHOTO_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "photo frame has an invalid size",
        ));
    }
    let name = c"cp0-appd-photo";
    let raw =
        unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if raw < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut file = unsafe { File::from_raw_fd(raw) };
    file.write_all(frame)?;
    file.flush()?;
    let seals = libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let path = CString::new(format!("/proc/self/fd/{}", file.as_raw_fd()))
        .map_err(|_| std::io::Error::other("invalid photo descriptor path"))?;
    let read_only = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if read_only < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(read_only) })
}

#[cfg(not(target_os = "linux"))]
fn sealed_photo_descriptor(_frame: &[u8]) -> std::io::Result<OwnedFd> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "sealed photo descriptors require Linux",
    ))
}

#[cfg(target_os = "linux")]
fn valid_photo_descriptor_access(flags: libc::c_int) -> bool {
    matches!(flags & libc::O_ACCMODE, libc::O_RDONLY | libc::O_RDWR)
}

#[cfg(not(target_os = "linux"))]
fn valid_photo_descriptor_access(flags: libc::c_int) -> bool {
    flags & libc::O_ACCMODE == libc::O_RDONLY
}

#[cfg(target_os = "linux")]
fn validate_screenshot_seals(descriptor: &OwnedFd) -> std::io::Result<()> {
    let seals = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GET_SEALS) };
    let required = libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
    if seals < 0 || seals & required != required {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "screenshot descriptor is not fully sealed",
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
fn validate_screenshot_seals(_descriptor: &OwnedFd) -> std::io::Result<()> {
    Ok(())
}

fn parse_photo_chunk_key(key: &str) -> Option<(u64, usize)> {
    let bytes = key.as_bytes();
    if bytes.len() != 21
        || bytes[0] != b'p'
        || bytes[17] != b'.'
        || bytes[18] != b'c'
        || !bytes[19].is_ascii_digit()
        || !bytes[20].is_ascii_digit()
    {
        return None;
    }
    let mut photo_id = 0_u64;
    for byte in &bytes[1..17] {
        let digit = match byte {
            b'0'..=b'9' => u64::from(*byte - b'0'),
            b'a'..=b'f' => u64::from(*byte - b'a' + 10),
            _ => return None,
        };
        photo_id = photo_id.checked_mul(16)?.checked_add(digit)?;
    }
    let chunk = usize::from(bytes[19] - b'0') * 10 + usize::from(bytes[20] - b'0');
    (photo_id != 0 && chunk < photo_chunk_count()).then_some((photo_id, chunk))
}

fn valid_photo_metadata_key(key: &str) -> bool {
    if matches!(key, "head.v2" | "index.v1") {
        return true;
    }
    let Some(suffix) = key.strip_prefix("index.v2.") else {
        return false;
    };
    suffix.len() == 8
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn photo_chunk_count() -> usize {
    PHOTO_FRAME_BYTES.div_ceil(cp0_storage_protocol::MAX_STORAGE_VALUE_BYTES)
}

fn photo_chunk_length(chunk: usize) -> usize {
    let offset = chunk * cp0_storage_protocol::MAX_STORAGE_VALUE_BYTES;
    (PHOTO_FRAME_BYTES - offset).min(cp0_storage_protocol::MAX_STORAGE_VALUE_BYTES)
}

fn photo_import_error_response(request_id: u64, error: &PhotoImportError) -> AppdResponse {
    let code = match error {
        PhotoImportError::ResourceExhausted => ErrorCode::ResourceExhausted,
        PhotoImportError::Storage(StorageClientError::Service(
            ServiceStorageErrorCode::QuotaExceeded,
        )) => ErrorCode::ResourceExhausted,
        PhotoImportError::Storage(StorageClientError::Service(
            ServiceStorageErrorCode::Unavailable,
        ))
        | PhotoImportError::Storage(StorageClientError::Io(_))
        | PhotoImportError::Storage(StorageClientError::EmptyResponse) => ErrorCode::Unavailable,
        PhotoImportError::InvalidFrame
        | PhotoImportError::InvalidIndex
        | PhotoImportError::Storage(_) => ErrorCode::Internal,
    };
    AppdResponse::error(
        request_id,
        code,
        "screenshot could not be imported into Gallery",
    )
}

fn photo_broker_error_response(request_id: u64, error: &PhotoImportError) -> BrokerResponse {
    let code = match error {
        PhotoImportError::ResourceExhausted => BrokerErrorCode::ResourceExhausted,
        PhotoImportError::Storage(StorageClientError::Service(
            ServiceStorageErrorCode::QuotaExceeded,
        )) => BrokerErrorCode::ResourceExhausted,
        PhotoImportError::Storage(StorageClientError::Service(
            ServiceStorageErrorCode::Unavailable,
        ))
        | PhotoImportError::Storage(StorageClientError::Io(_))
        | PhotoImportError::Storage(StorageClientError::EmptyResponse) => {
            BrokerErrorCode::Unavailable
        }
        PhotoImportError::InvalidFrame
        | PhotoImportError::InvalidIndex
        | PhotoImportError::Storage(_) => BrokerErrorCode::Internal,
    };
    BrokerResponse::error(request_id, code, "photo library transaction failed")
}

fn photo_view_error_response(request_id: u64, error: &PhotoViewError) -> BrokerResponse {
    match error {
        PhotoViewError::Library(error) => photo_broker_error_response(request_id, error),
        PhotoViewError::Storage(error) => storage_error_response(request_id, error),
        PhotoViewError::MissingOriginal => BrokerResponse::error(
            request_id,
            BrokerErrorCode::NotFound,
            "photo original is unavailable",
        ),
        PhotoViewError::InvalidJpeg => BrokerResponse::error(
            request_id,
            BrokerErrorCode::Internal,
            "photo original could not be decoded",
        ),
    }
}

#[derive(Debug)]
struct AuthorizedApp {
    app_id: String,
    app_name: String,
    storage_quota_bytes: u64,
}

#[derive(Debug)]
struct BrokerDispatch {
    response: BrokerResponse,
    descriptor: Option<OwnedFd>,
    transition: Option<IntentTransition>,
}

impl BrokerDispatch {
    fn response(response: BrokerResponse) -> Self {
        Self {
            response,
            descriptor: None,
            transition: None,
        }
    }
}

#[derive(Debug)]
struct IntentTransition {
    intent_id: u64,
    sender_app_id: String,
    target_app_id: String,
}

fn canonical_store_permissions(
    mut permissions: Vec<cp0_manifest::Permission>,
) -> Vec<cp0_manifest::Permission> {
    permissions.sort_unstable();
    permissions
}

fn task_blocks_package_change(tasks: &TaskRegistry, app_id: &str) -> bool {
    tasks.task_for_app(app_id).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntentTargetError {
    NotFound,
    Ambiguous,
}

fn select_intent_target(
    action: &str,
    manifests: &[cp0_manifest::AppManifest],
) -> Result<String, IntentTargetError> {
    let mut matches = manifests
        .iter()
        .filter(|manifest| manifest.intents.iter().any(|candidate| candidate == action));
    let first = matches.next().ok_or(IntentTargetError::NotFound)?;
    if matches.next().is_some() {
        return Err(IntentTargetError::Ambiguous);
    }
    Ok(first.id.clone())
}

#[derive(Debug)]
enum CommandError {
    Manager(AppManagerError),
    Task(TaskError),
    Storage(StorageClientError),
    Permission(PermissionPromptError),
    Document(DocumentPromptError),
    Policy(PolicyError),
    Restricted(&'static str),
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manager(error) => write!(formatter, "{error}"),
            Self::Task(error) => write!(formatter, "{error}"),
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::Permission(error) => write!(formatter, "{error}"),
            Self::Document(error) => write!(formatter, "{error}"),
            Self::Policy(error) => write!(formatter, "{error}"),
            Self::Restricted(error) => formatter.write_str(error),
        }
    }
}

impl From<AppManagerError> for CommandError {
    fn from(error: AppManagerError) -> Self {
        Self::Manager(error)
    }
}

impl From<TaskError> for CommandError {
    fn from(error: TaskError) -> Self {
        Self::Task(error)
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
        CommandError::Task(TaskError::TaskNotFound(_)) => {
            AppdResponse::error(request_id, ErrorCode::NotFound, "task was not found")
        }
        CommandError::Task(TaskError::TaskNotResident(_)) => AppdResponse::error(
            request_id,
            ErrorCode::Unavailable,
            "task runtime must be restored before activation",
        ),
        CommandError::Task(TaskError::AppAlreadyResident(_)) => AppdResponse::error(
            request_id,
            ErrorCode::AlreadyRunning,
            "application already has a resident task",
        ),
        CommandError::Task(_) => AppdResponse::error(
            request_id,
            ErrorCode::Conflict,
            "task lifecycle transition could not be committed",
        ),
        CommandError::Storage(_) => AppdResponse::error(
            request_id,
            ErrorCode::Unavailable,
            "private storage usage is unavailable",
        ),
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
        CommandError::Policy(PolicyError::Locked(_)) | CommandError::Restricted(_) => {
            AppdResponse::error(
                request_id,
                ErrorCode::Unauthorized,
                "operation is blocked by device policy",
            )
        }
        CommandError::Policy(_) => AppdResponse::error(
            request_id,
            ErrorCode::Internal,
            "device policy state could not be updated",
        ),
    }
}

fn manager_error_response(request_id: u64, error: &AppManagerError) -> AppdResponse {
    let (code, message) = match error {
        AppManagerError::NotInstalled(app_id) => (
            ErrorCode::NotFound,
            format!("application {app_id} is not installed"),
        ),
        AppManagerError::ProtectedBuiltin(app_id) => (
            ErrorCode::Conflict,
            format!("built-in application {app_id} cannot be uninstalled"),
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
        AppManagerError::NoRollback(app_id) => (
            ErrorCode::NotFound,
            format!("application {app_id} has no rollback version"),
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

fn install_error_response(request_id: u64, error: &InstallError) -> AppdResponse {
    let (code, message) = match error {
        InstallError::Untrusted(_) => (ErrorCode::Untrusted, "package trust verification failed"),
        InstallError::AlreadyInstalled(_) => (
            ErrorCode::Conflict,
            "the same application version already has different content",
        ),
        InstallError::Invalid(_) | InstallError::Package(_) | InstallError::Manifest(_) => {
            (ErrorCode::InvalidRequest, "application package is invalid")
        }
        InstallError::Io(_) => (ErrorCode::Internal, "application installation failed"),
    };
    AppdResponse::error(request_id, code, message)
}

fn store_version_is_upgrade(current: Option<&str>, candidate: &str) -> bool {
    let Ok(candidate) = semver::Version::parse(candidate) else {
        return false;
    };
    let Some(current) = current else {
        return true;
    };
    semver::Version::parse(current).is_ok_and(|current| candidate > current)
}

fn store_version_is_acceptable(current: Option<&str>, candidate: &str) -> bool {
    semver::Version::parse(candidate).is_ok()
        && (current == Some(candidate) || store_version_is_upgrade(current, candidate))
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

fn radio_error_response(request_id: u64, error: &RadioClientError) -> BrokerResponse {
    let (code, message) = match error {
        RadioClientError::Service(ServiceRadioErrorCode::InvalidRequest) => {
            (BrokerErrorCode::InvalidRequest, "LoRa request is invalid")
        }
        RadioClientError::Service(ServiceRadioErrorCode::Busy)
        | RadioClientError::Service(ServiceRadioErrorCode::RateLimited) => (
            BrokerErrorCode::ResourceExhausted,
            "LoRa radio is busy or transmission is rate limited",
        ),
        RadioClientError::Service(ServiceRadioErrorCode::Disabled)
        | RadioClientError::Service(ServiceRadioErrorCode::Unavailable)
        | RadioClientError::Service(ServiceRadioErrorCode::Device)
        | RadioClientError::Io(_)
        | RadioClientError::EmptyResponse => {
            (BrokerErrorCode::Unavailable, "LoRa radio is unavailable")
        }
        RadioClientError::Protocol(_)
        | RadioClientError::MismatchedRequestId
        | RadioClientError::MismatchedOutcome
        | RadioClientError::Service(ServiceRadioErrorCode::Unauthorized)
        | RadioClientError::Service(ServiceRadioErrorCode::Internal) => (
            BrokerErrorCode::Internal,
            "radio service returned an invalid response",
        ),
    };
    BrokerResponse::error(request_id, code, message)
}

fn storage_error_response(request_id: u64, error: &StorageClientError) -> BrokerResponse {
    let (code, message) = match error {
        StorageClientError::Service(ServiceStorageErrorCode::InvalidRequest) => (
            BrokerErrorCode::InvalidRequest,
            "private storage request is invalid",
        ),
        StorageClientError::Service(ServiceStorageErrorCode::QuotaExceeded) => (
            BrokerErrorCode::ResourceExhausted,
            "private storage quota was exceeded",
        ),
        StorageClientError::Service(ServiceStorageErrorCode::Unavailable)
        | StorageClientError::Io(_)
        | StorageClientError::EmptyResponse => (
            BrokerErrorCode::Unavailable,
            "private storage service is unavailable",
        ),
        StorageClientError::Protocol(_)
        | StorageClientError::MismatchedRequestId
        | StorageClientError::MismatchedOutcome
        | StorageClientError::MissingDescriptor
        | StorageClientError::UnexpectedDescriptor
        | StorageClientError::InvalidDescriptor
        | StorageClientError::Service(ServiceStorageErrorCode::Unauthorized)
        | StorageClientError::Service(ServiceStorageErrorCode::Internal) => (
            BrokerErrorCode::Internal,
            "private storage service returned an invalid response",
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

fn take_runtime_end(runtime: &mut RuntimeState, token: u64) -> Option<bool> {
    let session = runtime.sessions.remove(&token)?;
    let crashed = !session.explicit_stop;
    if runtime.foreground == Some(token) {
        runtime.foreground = None;
    }
    Some(crashed)
}

fn runtime_app_is_foreground(runtime: &RuntimeState, app_id: &str) -> bool {
    runtime
        .foreground
        .and_then(|token| runtime.sessions.get(&token))
        .is_some_and(|session| session.app_id == app_id)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::fd::OwnedFd;

    #[cfg(target_os = "linux")]
    use std::io::BufReader;
    #[cfg(target_os = "linux")]
    use std::os::fd::AsFd;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(target_os = "linux")]
    use std::sync::atomic::{AtomicU64, Ordering};
    #[cfg(target_os = "linux")]
    use std::thread;

    use super::*;
    #[cfg(target_os = "linux")]
    use crate::{
        AppRegistry, ManagerPaths, PermissionEngine, PermissionStore, read_response, write_request,
    };

    #[cfg(target_os = "linux")]
    static PHOTO_E2E_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    #[cfg(target_os = "linux")]
    fn valid_screenshot_descriptor(frame: &[u8]) -> OwnedFd {
        use std::ffi::CString;
        use std::io::Write;
        use std::os::fd::FromRawFd;

        let name = c"cp0-appd-screenshot-test";
        let raw = unsafe {
            libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING)
        };
        assert!(raw >= 0);
        let mut file = unsafe { File::from_raw_fd(raw) };
        file.write_all(frame).unwrap();
        let seals =
            libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
        assert_eq!(
            unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) },
            0
        );
        let path = CString::new(format!("/proc/self/fd/{}", file.as_raw_fd())).unwrap();
        let read_only = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
        assert!(read_only >= 0);
        unsafe { OwnedFd::from_raw_fd(read_only) }
    }

    #[cfg(not(target_os = "linux"))]
    fn valid_screenshot_descriptor(frame: &[u8]) -> OwnedFd {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/appd-screenshot-frame.rgb565");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, frame).unwrap();
        File::open(path).unwrap().into()
    }

    #[test]
    fn screenshot_descriptor_is_exact_read_only_and_sealed_on_linux() {
        let frame = vec![0x5a; PHOTO_FRAME_BYTES];
        assert_eq!(
            read_photo_frame(valid_screenshot_descriptor(&frame)).unwrap(),
            frame
        );

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/appd-screenshot-invalid.rgb565");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![0_u8; PHOTO_FRAME_BYTES]).unwrap();
        let writable: OwnedFd = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap()
            .into();
        assert!(read_photo_frame(writable).is_err());

        fs::write(&path, vec![0_u8; PHOTO_FRAME_BYTES - 1]).unwrap();
        let wrong_size: OwnedFd = File::open(path).unwrap().into();
        assert!(read_photo_frame(wrong_size).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn shell_screenshot_reaches_real_storage_as_gallery_v2_data() {
        use cp0_storage_protocol::MAX_STORAGE_VALUE_BYTES;
        use cp0_storaged::{StorageServer, StorageService};

        let sequence = PHOTO_E2E_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let test_root = std::path::PathBuf::from(format!(
            "target/test-tmp/photo-e2e-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&test_root).unwrap();
        fs::set_permissions(&test_root, fs::Permissions::from_mode(0o700)).unwrap();
        let socket_path = test_root.join("storage.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let uid = unsafe { libc::geteuid() };
        let storage_service = StorageService::new(&test_root, uid);
        storage_service.initialize().unwrap();
        let storage_server = StorageServer::new(storage_service, [uid]);
        thread::spawn(move || storage_server.serve(listener).unwrap());

        let storage = StorageClient::new(&socket_path);
        let manager =
            AppManager::from_registry(ManagerPaths::default(), AppRegistry::default()).unwrap();
        let permission_engine = PermissionEngine::new(
            test_root.join("permissions.json"),
            PermissionStore::default(),
        )
        .unwrap();
        let appd = AppdServer::new_with_capability_services(
            manager,
            PermissionCoordinator::new(permission_engine),
            [uid],
            CapabilityServices {
                storage: storage.clone(),
                ..CapabilityServices::default()
            },
        )
        .allow_shell(uid);

        let frame: Vec<u8> = (0..PHOTO_FRAME_BYTES)
            .map(|offset| ((offset * 31 + 17) & 0xff) as u8)
            .collect();
        let descriptor = valid_screenshot_descriptor(&frame);
        let (mut shell, appd_stream) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || appd.handle_connection(appd_stream).unwrap());
        let request = AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 91,
            command: AppdCommand::ImportScreenshot,
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &request).unwrap();
        cp0_camera_protocol::send_frame_with_fd(&mut shell, &encoded, descriptor.as_fd()).unwrap();
        let response = read_response(&mut BufReader::new(shell.try_clone().unwrap()))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
        assert!(matches!(
            response.outcome,
            crate::ResponseOutcome::Ok {
                data: ResponseData::ScreenshotImported { photo_id: 1 }
            }
        ));

        let head = storage
            .get(
                92,
                PHOTO_LIBRARY_ID,
                PHOTO_LIBRARY_QUOTA_BYTES,
                PHOTO_LIBRARY_HEAD_KEY,
            )
            .unwrap()
            .unwrap();
        assert_eq!(u64::from_le_bytes(head[8..16].try_into().unwrap()), 1);
        assert_eq!(u64::from_le_bytes(head[16..24].try_into().unwrap()), 1);
        assert_eq!(u64::from_le_bytes(head[24..32].try_into().unwrap()), 1);

        let page = storage
            .get(
                93,
                PHOTO_LIBRARY_ID,
                PHOTO_LIBRARY_QUOTA_BYTES,
                "index.v2.00000000",
            )
            .unwrap()
            .unwrap();
        assert_eq!(u64::from_le_bytes(page[16..24].try_into().unwrap()), 1);

        let mut loaded = Vec::with_capacity(PHOTO_FRAME_BYTES);
        while loaded.len() < PHOTO_FRAME_BYTES {
            let length = (PHOTO_FRAME_BYTES - loaded.len()).min(MAX_STORAGE_VALUE_BYTES);
            let chunk = storage
                .get_blob_chunk(
                    94 + loaded.len() as u64,
                    PHOTO_LIBRARY_ID,
                    PHOTO_LIBRARY_QUOTA_BYTES,
                    "p0000000000000001.rgb565",
                    loaded.len() as u32,
                    length as u32,
                )
                .unwrap()
                .unwrap();
            assert_eq!(chunk.len(), length);
            loaded.extend_from_slice(&chunk);
        }
        assert_eq!(loaded, frame);
    }

    #[test]
    fn photo_storage_keys_are_canonical_and_bounded() {
        assert_eq!(
            parse_photo_chunk_key("p000000000000002a.c13"),
            Some((42, 13))
        );
        for key in [
            "p0000000000000000.c00",
            "p000000000000002A.c00",
            "p000000000000002a.c14",
            "p000000000000002a.c1",
            "other",
        ] {
            assert_eq!(parse_photo_chunk_key(key), None, "accepted {key}");
        }
        for key in ["head.v2", "index.v1", "index.v2.00000000"] {
            assert!(valid_photo_metadata_key(key));
        }
        for key in ["head.v3", "index.v2.0", "index.v2.0000000A", "arbitrary"] {
            assert!(!valid_photo_metadata_key(key), "accepted {key}");
        }
        assert_eq!(photo_chunk_length(13), 2304);
        assert_eq!(photo_blob_key(42), "p000000000000002a.rgb565");
    }

    #[test]
    fn store_installation_accepts_exact_replay_and_strict_upgrade() {
        assert!(store_version_is_upgrade(None, "1.0.0"));
        assert!(store_version_is_upgrade(Some("1.0.0"), "1.0.1"));
        assert!(store_version_is_upgrade(Some("1.0.0-beta.1"), "1.0.0"));
        assert!(!store_version_is_upgrade(Some("1.0.0"), "1.0.0"));
        assert!(!store_version_is_upgrade(Some("2.0.0"), "1.9.9"));
        assert!(!store_version_is_upgrade(Some("1.0.0"), "invalid"));
        assert!(store_version_is_acceptable(Some("1.0.0"), "1.0.0"));
        assert!(!store_version_is_acceptable(Some("2.0.0"), "1.9.9"));
        assert!(!store_version_is_acceptable(Some("invalid"), "invalid"));
    }

    #[test]
    fn maps_public_lifecycle_errors_without_host_details() {
        let response = manager_error_response(
            9,
            &AppManagerError::NotInstalled("dev.cardputerzero.missing".into()),
        );
        let encoded = serde_json::to_string(&response).unwrap();
        assert!(encoded.contains("not-found"));
        assert!(encoded.contains("dev.cardputerzero.missing"));

        let protected = manager_error_response(
            11,
            &AppManagerError::ProtectedBuiltin("dev.cardputerzero.gallery".into()),
        );
        let encoded = serde_json::to_string(&protected).unwrap();
        assert!(encoded.contains("conflict"));
        assert!(encoded.contains("cannot be uninstalled"));

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

    #[test]
    fn counts_only_unexpected_runtime_disappearance_as_a_crash() {
        let session = |explicit_stop| RuntimeSession {
            token: 7,
            task_id: TaskId(3),
            app_id: "dev.cardputerzero.test".into(),
            version: "1.2.3".into(),
            explicit_stop,
        };
        let mut unexpected = RuntimeState::default();
        unexpected.sessions.insert(7, session(false));
        unexpected.foreground = Some(7);
        assert_eq!(take_runtime_end(&mut unexpected, 7), Some(true));
        assert!(unexpected.sessions.is_empty());
        assert_eq!(unexpected.foreground, None);

        let mut explicit = RuntimeState::default();
        explicit.sessions.insert(7, session(true));
        assert_eq!(take_runtime_end(&mut explicit, 7), Some(false));
        assert!(explicit.sessions.is_empty());

        let mut newer = RuntimeState::default();
        newer.sessions.insert(7, session(false));
        assert_eq!(take_runtime_end(&mut newer, 6), None);
        assert!(newer.sessions.contains_key(&7));
    }

    #[test]
    fn camera_access_tracks_only_the_current_foreground_runtime() {
        let session = |token, app_id: &str| RuntimeSession {
            token,
            task_id: TaskId(token),
            app_id: app_id.into(),
            version: "1.0.0".into(),
            explicit_stop: false,
        };
        let mut runtime = RuntimeState::default();
        runtime
            .sessions
            .insert(7, session(7, "dev.cardputerzero.camera"));
        runtime
            .sessions
            .insert(8, session(8, "dev.cardputerzero.background"));

        assert!(!runtime_app_is_foreground(
            &runtime,
            "dev.cardputerzero.camera"
        ));
        runtime.foreground = Some(7);
        assert!(runtime_app_is_foreground(
            &runtime,
            "dev.cardputerzero.camera"
        ));
        assert!(!runtime_app_is_foreground(
            &runtime,
            "dev.cardputerzero.background"
        ));
        runtime.foreground = Some(9);
        assert!(!runtime_app_is_foreground(
            &runtime,
            "dev.cardputerzero.camera"
        ));
    }

    #[test]
    fn checkpointed_task_still_blocks_package_changes() {
        let mut tasks = TaskRegistry::new(2).unwrap();
        let first = tasks
            .launch(
                "dev.cardputerzero.first",
                "1.0.0",
                RuntimeBinding::new(1, "cardputerzero-app-20000.service").unwrap(),
                None,
            )
            .unwrap();
        tasks
            .launch(
                "dev.cardputerzero.second",
                "1.0.0",
                RuntimeBinding::new(2, "cardputerzero-app-20001.service").unwrap(),
                None,
            )
            .unwrap();
        tasks
            .checkpoint(
                first.task_id,
                CheckpointStatus::Available {
                    schema_version: 1,
                    bytes: 32,
                },
            )
            .unwrap();

        assert!(task_blocks_package_change(
            &tasks,
            "dev.cardputerzero.first"
        ));
        tasks.close(first.task_id).unwrap();
        assert!(!task_blocks_package_change(
            &tasks,
            "dev.cardputerzero.first"
        ));
    }

    #[test]
    fn intent_routes_only_to_one_explicit_receiver() {
        let action = "dev.cardputerzero.documents.open";
        let mut first = crate::tests::manifest();
        first.intents = vec![action.into()];
        let mut second = first.clone();
        second.id = "dev.cardputerzero.second".into();

        assert_eq!(
            select_intent_target(action, &[first.clone()]),
            Ok(first.id.clone())
        );
        assert_eq!(
            select_intent_target(action, &[first.clone(), second]),
            Err(IntentTargetError::Ambiguous)
        );
        assert_eq!(
            select_intent_target("dev.cardputerzero.missing", &[first]),
            Err(IntentTargetError::NotFound)
        );
    }

    #[test]
    fn store_uid_has_only_install_and_minimal_installed_snapshot_commands() {
        let root = 0;
        let shell = 100;
        let store = 101;
        let trusted = BTreeSet::from([root, shell, store]);
        let stores = BTreeSet::from([store]);
        let authorized =
            |uid, command| control_command_authorized(uid, command, &trusted, &stores, Some(shell));
        let normal = AppdCommand::List {
            offset: 0,
            limit: 1,
        };
        let root_install = AppdCommand::Install {
            package_name: "incoming-test.capp".into(),
        };
        let store_install = AppdCommand::StoreInstall {
            package_name: "store-test.capp".into(),
            app_id: "dev.cardputerzero.example".into(),
            version: "1.0.0".into(),
            package_sha256: "11".repeat(32),
            package_bytes: 4096,
            automatic: false,
        };
        let store_list = AppdCommand::StoreListInstalled {
            offset: 0,
            limit: 8,
        };

        assert!(authorized(root, &root_install));
        assert!(!authorized(shell, &root_install));
        assert!(!authorized(store, &root_install));
        assert!(authorized(store, &store_install));
        assert!(!authorized(shell, &store_install));
        assert!(authorized(shell, &normal));
        assert!(!authorized(store, &normal));
        assert!(authorized(store, &store_list));
        assert!(!authorized(shell, &store_list));
        assert!(!authorized(
            store,
            &AppdCommand::SetDeviceMode {
                mode: crate::DeviceMode::Developer,
                enabled: true,
            }
        ));
        assert!(!authorized(999, &normal));

        let import = AppdCommand::ImportScreenshot;
        assert!(authorized(shell, &import));
        assert!(!authorized(root, &import));
        assert!(!authorized(store, &import));
        assert!(!authorized(999, &import));

        let foreground = AppdCommand::SetForegroundApp {
            app_id: Some("dev.cardputerzero.example".into()),
        };
        assert!(authorized(shell, &foreground));
        assert!(!authorized(root, &foreground));
        assert!(!authorized(store, &foreground));
        assert!(!authorized(999, &foreground));
    }

    #[test]
    fn store_snapshot_canonicalizes_manifest_permission_order() {
        assert_eq!(
            canonical_store_permissions(vec![
                cp0_manifest::Permission::NotificationsPost,
                cp0_manifest::Permission::NetworkClient,
                cp0_manifest::Permission::CameraCapture,
            ]),
            vec![
                cp0_manifest::Permission::NetworkClient,
                cp0_manifest::Permission::CameraCapture,
                cp0_manifest::Permission::NotificationsPost,
            ]
        );
    }
}
