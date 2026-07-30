use std::collections::BTreeSet;
use std::fmt;
use std::io::BufReader;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cp0_manifest::Permission;

use crate::protocol::APPD_PROTOCOL_VERSION;
use crate::{
    AppManager, AppManagerError, AppSummary, AppdCommand, AppdRequest, AppdResponse, BrokerCommand,
    BrokerErrorCode, BrokerProtocolError, BrokerRequest, BrokerResponse, ErrorCode,
    NotificationQueue, PermissionChoice, PermissionCoordinator, PermissionPromptError,
    PermissionRequestResult, ResponseData, peer_credentials, read_broker_request, read_request,
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
}

#[derive(Debug)]
struct ServerState {
    manager: AppManager,
    permissions: PermissionCoordinator,
    notifications: NotificationQueue,
}

impl AppdServer {
    pub fn new(
        manager: AppManager,
        permissions: PermissionCoordinator,
        trusted_uids: impl IntoIterator<Item = u32>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(ServerState {
                manager,
                permissions,
                notifications: NotificationQueue::default(),
            })),
            trusted_uids: trusted_uids.into_iter().collect(),
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
            apps.push(AppSummary {
                running: state.manager.is_running(&installed.app_id)?,
                app_id: installed.app_id.clone(),
                version: installed.version.clone(),
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
        let response = self.dispatch_broker(credentials, request);
        write_broker_response(&mut stream, &response).map_err(broker_io)
    }

    fn dispatch_broker(
        &self,
        peer: crate::PeerCredentials,
        request: BrokerRequest,
    ) -> BrokerResponse {
        let request_id = request.request_id;
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
        let Some(installed) = state.manager.installed_app_for_uid(peer.uid) else {
            return BrokerResponse::error(
                request_id,
                BrokerErrorCode::Unauthorized,
                "peer UID is not an installed application identity",
            );
        };
        match state.manager.is_running(&installed.app_id) {
            Ok(true) => {}
            Ok(false) => {
                return BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Unauthorized,
                    "application is not running",
                );
            }
            Err(error) => {
                eprintln!("cp0-appd: cannot verify broker caller state: {error}");
                return BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application state could not be verified",
                );
            }
        }
        let unit = match state.manager.unit_for_app(&installed.app_id) {
            Ok(unit) => unit,
            Err(error) => {
                eprintln!("cp0-appd: cannot derive broker caller unit: {error}");
                return BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application identity could not be verified",
                );
            }
        };
        match process_is_in_unit(peer.pid, &unit) {
            Ok(true) => {}
            Ok(false) => {
                return BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Unauthorized,
                    "peer process is outside the application runtime cgroup",
                );
            }
            Err(error) => {
                eprintln!("cp0-appd: cannot verify broker caller cgroup: {error}");
                return BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "application process identity could not be verified",
                );
            }
        }
        let manifest = match state.manager.installed_manifest(&installed.app_id) {
            Ok(manifest) => manifest,
            Err(error) => {
                eprintln!("cp0-appd: cannot load broker caller manifest: {error}");
                return BrokerResponse::error(
                    request_id,
                    BrokerErrorCode::Internal,
                    "installed application metadata is unavailable",
                );
            }
        };
        match request.command {
            BrokerCommand::PostNotification { title, body } => {
                match state
                    .permissions
                    .request(&manifest, Permission::NotificationsPost)
                {
                    Ok(PermissionRequestResult::Allow) => {
                        match state
                            .notifications
                            .enqueue(&manifest.id, &manifest.name, title, body)
                        {
                            Ok(notification_id) => {
                                BrokerResponse::success(request_id, notification_id)
                            }
                            Err(_) => BrokerResponse::error(
                                request_id,
                                BrokerErrorCode::ResourceExhausted,
                                "notification queue is full",
                            ),
                        }
                    }
                    Ok(PermissionRequestResult::Prompt(prompt)) => {
                        BrokerResponse::permission_pending(request_id, prompt.prompt_id)
                    }
                    Ok(PermissionRequestResult::Deny) => BrokerResponse::error(
                        request_id,
                        BrokerErrorCode::Denied,
                        "notification permission was denied",
                    ),
                    Ok(PermissionRequestResult::Undeclared) => BrokerResponse::error(
                        request_id,
                        BrokerErrorCode::Undeclared,
                        "application did not declare notification permission",
                    ),
                    Err(PermissionPromptError::Busy(_)) => BrokerResponse::error(
                        request_id,
                        BrokerErrorCode::ResourceExhausted,
                        "another permission prompt is pending",
                    ),
                    Err(error) => {
                        eprintln!("cp0-appd: notification permission request failed: {error}");
                        BrokerResponse::error(
                            request_id,
                            BrokerErrorCode::Internal,
                            "notification permission could not be evaluated",
                        )
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
enum CommandError {
    Manager(AppManagerError),
    Permission(PermissionPromptError),
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manager(error) => write!(formatter, "{error}"),
            Self::Permission(error) => write!(formatter, "{error}"),
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
