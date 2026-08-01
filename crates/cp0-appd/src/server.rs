use std::collections::BTreeSet;
use std::fmt;
use std::io::BufReader;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cp0_audio_protocol::AudioErrorCode as ServiceAudioErrorCode;
use cp0_camera_protocol::CameraErrorCode as ServiceCameraErrorCode;
use cp0_document_protocol::{DocumentErrorCode as ServiceDocumentErrorCode, send_frame_with_fd};
use cp0_gpio_protocol::GpioErrorCode as ServiceGpioErrorCode;
use cp0_manifest::Permission;
use cp0_network_protocol::NetworkErrorCode as ServiceNetworkErrorCode;
use cp0_radio_protocol::RadioErrorCode as ServiceRadioErrorCode;
use cp0_storage_protocol::StorageErrorCode as ServiceStorageErrorCode;
use cp0_store_protocol::StoreRuntimeMetricEvent;

use crate::protocol::APPD_PROTOCOL_VERSION;
use crate::{
    AppManager, AppManagerError, AppSummary, AppdCommand, AppdRequest, AppdResponse, AudioClient,
    AudioClientError, BrokerCommand, BrokerErrorCode, BrokerProtocolError, BrokerRequest,
    BrokerResponse, CameraClient, CameraClientError, DevicePolicyEngine, DocumentClient,
    DocumentClientError, DocumentCoordinator, DocumentPromptError, DocumentRequestResult,
    ErrorCode, GpioClient, GpioClientError, InstallError, IntentQueue, MediaAction,
    MediaSessionBroker, MediaSessionError, NetworkClient, NetworkClientError, NotificationQueue,
    PackageInstaller, PermissionChoice, PermissionCoordinator, PermissionPromptError,
    PermissionRequestResult, PolicyError, RadioClient, RadioClientError, ResponseData,
    StorageClient, StorageClientError, StoreMetricsClient, TrustPaths, TrustPolicy,
    encode_broker_response, peer_credentials, read_broker_request, read_request,
    write_broker_response, write_response,
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
    capabilities: CapabilityServices,
    installer: PackageInstaller,
    store_metrics: StoreMetricsClient,
    runtime: Arc<Mutex<Option<RuntimeSession>>>,
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
    app_id: String,
    version: String,
    explicit_stop: bool,
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
                intents: IntentQueue::default(),
                media_sessions: MediaSessionBroker::default(),
                policy: DevicePolicyEngine::unmanaged(),
            })),
            trusted_uids: trusted_uids.into_iter().collect(),
            store_installer_uids: BTreeSet::new(),
            capabilities,
            store_metrics: StoreMetricsClient::default(),
            runtime: Arc::new(Mutex::new(None)),
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
        if !control_command_authorized(
            credentials.uid,
            &request.command,
            &self.trusted_uids,
            &self.store_installer_uids,
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

        let response = self.dispatch(request, credentials.uid);
        write_response(&mut stream, &response).map_err(protocol_io)?;
        Ok(())
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
        let mut started_runtime = None;
        let result: Result<ResponseData, CommandError> = match command {
            AppdCommand::Install { .. } => unreachable!("install returned before state lock"),
            AppdCommand::StoreInstall { .. } => {
                unreachable!("store install returned before state lock")
            }
            AppdCommand::DispatchMediaAction { .. } => {
                unreachable!("media dispatch returned before state lock")
            }
            AppdCommand::Ping => Ok(ResponseData::Pong),
            AppdCommand::List { offset, limit } => {
                Self::list_apps(&state, offset, limit).map_err(CommandError::Manager)
            }
            AppdCommand::StoreListInstalled { offset, limit } => {
                Self::list_store_apps(&state, offset, limit).map_err(CommandError::Manager)
            }
            AppdCommand::Start { app_id } => match self.start_app(&state, &app_id) {
                Ok((unit, version)) => {
                    state.media_sessions.clear();
                    started_runtime = Some((app_id.clone(), version, unit.clone()));
                    Ok(ResponseData::Started { app_id, unit })
                }
                Err(error) => Err(error),
            },
            AppdCommand::Stop { .. } => unreachable!("stop returned before state lock"),
            AppdCommand::Uninstall { app_id } => {
                let result = state
                    .manager
                    .is_running(&app_id)
                    .map_err(CommandError::Manager)
                    .and_then(|running| {
                        if running {
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
            AppdCommand::Rollback { app_id } => state
                .manager
                .rollback(&app_id)
                .map(|installed| ResponseData::RolledBack {
                    app_id: installed.app_id,
                    version: installed.version,
                })
                .map_err(CommandError::Manager),
            AppdCommand::Logs { app_id, limit } => state
                .manager
                .logs(&app_id, limit)
                .map(|lines| ResponseData::Logs { app_id, lines })
                .map_err(CommandError::Manager),
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
        if let Some((app_id, version, unit)) = started_runtime {
            self.start_runtime_monitor(app_id, version, unit);
        }
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
        let Some(session) = runtime.as_ref() else {
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
        let result = state.manager.stop(app_id).map(|()| {
            state.permissions.clear_app_session(app_id);
            state.document_prompts.clear_app(app_id);
            state.media_sessions.clear_app(app_id);
            ResponseData::Stopped {
                app_id: app_id.into(),
            }
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
                && state.manager.is_running(&prepared.manifest.id)?
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

    fn start_app(
        &self,
        state: &ServerState,
        app_id: &str,
    ) -> Result<(String, String), CommandError> {
        if !state.policy.allows_app(app_id) {
            return Err(CommandError::Restricted(
                "application launch is blocked by device policy",
            ));
        }
        let version = state
            .manager
            .installed_manifest(app_id)
            .map_err(CommandError::Manager)?
            .version;
        state
            .manager
            .start(app_id)
            .map(|unit| (unit, version))
            .map_err(CommandError::Manager)
    }

    fn start_runtime_monitor(&self, app_id: String, version: String, unit: String) {
        let token = self.runtime_sequence.fetch_add(1, Ordering::Relaxed);
        let Ok(mut runtime) = self.runtime.lock() else {
            eprintln!("cp0-appd: runtime metrics state is unavailable");
            return;
        };
        *runtime = Some(RuntimeSession {
            token,
            app_id: app_id.clone(),
            version: version.clone(),
            explicit_stop: false,
        });
        drop(runtime);

        let server = self.clone();
        if let Err(error) = thread::Builder::new()
            .name("cp0-app-runtime-watch".into())
            .spawn(move || server.monitor_runtime(token, app_id, version, unit))
        {
            eprintln!("cp0-appd: cannot start runtime monitor: {error}");
            if let Ok(mut runtime) = self.runtime.lock() {
                if runtime
                    .as_ref()
                    .is_some_and(|session| session.token == token)
                {
                    *runtime = None;
                }
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
        self.runtime.lock().is_ok_and(|runtime| {
            runtime
                .as_ref()
                .is_some_and(|session| session.token == token)
        })
    }

    fn mark_explicit_stop(&self, app_id: &str) -> Option<u64> {
        let mut runtime = self.runtime.lock().ok()?;
        let session = runtime
            .as_mut()
            .filter(|session| session.app_id == app_id)?;
        session.explicit_stop = true;
        Some(session.token)
    }

    fn cancel_explicit_stop(&self, token: Option<u64>) {
        let Some(token) = token else {
            return;
        };
        if let Ok(mut runtime) = self.runtime.lock() {
            if let Some(session) = runtime.as_mut().filter(|session| session.token == token) {
                session.explicit_stop = false;
            }
        }
    }

    fn finish_explicit_stop(&self, token: Option<u64>) {
        let Some(token) = token else {
            return;
        };
        if let Ok(mut runtime) = self.runtime.lock() {
            if runtime
                .as_ref()
                .is_some_and(|session| session.token == token)
            {
                *runtime = None;
            }
        }
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
            let usage = state.manager.app_usage(&installed.app_id)?;
            apps.push(AppSummary {
                running: state.manager.is_running(&installed.app_id)?,
                app_id: installed.app_id.clone(),
                name: manifest.name,
                version: installed.version.clone(),
                display: manifest.display,
                installed_at_unix_seconds: installed.installed_at_unix_seconds,
                package_bytes: usage.package_bytes,
                data_bytes: usage.data_bytes,
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
        let stop_token = self.mark_explicit_stop(&transition.sender_app_id);
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                self.cancel_explicit_stop(stop_token);
                eprintln!("cp0-appd: cannot switch applications after accepted intent");
                return;
            }
        };
        match state.manager.is_running(&transition.sender_app_id) {
            Ok(true) => {
                if let Err(error) = state.manager.stop(&transition.sender_app_id) {
                    drop(state);
                    self.cancel_explicit_stop(stop_token);
                    eprintln!("cp0-appd: cannot stop intent sender: {error}");
                    return;
                }
            }
            Ok(false) => {}
            Err(error) => {
                drop(state);
                self.cancel_explicit_stop(stop_token);
                eprintln!("cp0-appd: cannot verify intent sender state: {error}");
                return;
            }
        }
        state
            .permissions
            .clear_app_session(&transition.sender_app_id);
        state.document_prompts.clear_app(&transition.sender_app_id);
        state.media_sessions.clear();
        let version = match state.manager.installed_manifest(&transition.target_app_id) {
            Ok(manifest) => manifest.version,
            Err(error) => {
                drop(state);
                self.finish_explicit_stop(stop_token);
                eprintln!("cp0-appd: cannot inspect accepted intent receiver: {error}");
                return;
            }
        };
        let start_result = state.manager.start(&transition.target_app_id);
        drop(state);
        self.finish_explicit_stop(stop_token);
        match start_result {
            Ok(unit) => {
                self.start_runtime_monitor(transition.target_app_id, version, unit);
            }
            Err(error) => {
                eprintln!("cp0-appd: cannot start accepted intent receiver: {error}");
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
                transition: None,
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
            .as_ref()
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
) -> bool {
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
    Permission(PermissionPromptError),
    Document(DocumentPromptError),
    Policy(PolicyError),
    Restricted(&'static str),
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manager(error) => write!(formatter, "{error}"),
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

fn take_runtime_end(runtime: &mut Option<RuntimeSession>, token: u64) -> Option<bool> {
    let session = runtime.as_ref().filter(|session| session.token == token)?;
    let crashed = !session.explicit_stop;
    *runtime = None;
    Some(crashed)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            app_id: "dev.cardputerzero.test".into(),
            version: "1.2.3".into(),
            explicit_stop,
        };
        let mut unexpected = Some(session(false));
        assert_eq!(take_runtime_end(&mut unexpected, 7), Some(true));
        assert!(unexpected.is_none());

        let mut explicit = Some(session(true));
        assert_eq!(take_runtime_end(&mut explicit, 7), Some(false));
        assert!(explicit.is_none());

        let mut newer = Some(session(false));
        assert_eq!(take_runtime_end(&mut newer, 6), None);
        assert!(newer.is_some());
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

        assert!(control_command_authorized(
            root,
            &root_install,
            &trusted,
            &stores
        ));
        assert!(!control_command_authorized(
            shell,
            &root_install,
            &trusted,
            &stores
        ));
        assert!(!control_command_authorized(
            store,
            &root_install,
            &trusted,
            &stores
        ));
        assert!(control_command_authorized(
            store,
            &store_install,
            &trusted,
            &stores
        ));
        assert!(!control_command_authorized(
            shell,
            &store_install,
            &trusted,
            &stores
        ));
        assert!(control_command_authorized(
            shell, &normal, &trusted, &stores
        ));
        assert!(!control_command_authorized(
            store, &normal, &trusted, &stores
        ));
        assert!(control_command_authorized(
            store,
            &store_list,
            &trusted,
            &stores
        ));
        assert!(!control_command_authorized(
            shell,
            &store_list,
            &trusted,
            &stores
        ));
        assert!(!control_command_authorized(
            store,
            &AppdCommand::SetDeviceMode {
                mode: crate::DeviceMode::Developer,
                enabled: true,
            },
            &trusted,
            &stores
        ));
        assert!(!control_command_authorized(999, &normal, &trusted, &stores));
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
