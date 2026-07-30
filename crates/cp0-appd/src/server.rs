use std::collections::BTreeSet;
use std::fmt;
use std::io::BufReader;
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Duration;

use crate::protocol::APPD_PROTOCOL_VERSION;
use crate::{
    AppManager, AppManagerError, AppSummary, AppdCommand, AppdRequest, AppdResponse, ErrorCode,
    ResponseData, peer_credentials, read_request, write_response,
};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub enum ServerError {
    Io(std::io::Error),
    Manager(AppManagerError),
}

impl fmt::Display for ServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "appd server I/O error: {error}"),
            Self::Manager(error) => write!(formatter, "appd manager error: {error}"),
        }
    }
}

impl std::error::Error for ServerError {}

impl From<std::io::Error> for ServerError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug)]
pub struct AppdServer {
    manager: AppManager,
    trusted_uids: BTreeSet<u32>,
}

impl AppdServer {
    pub fn new(manager: AppManager, trusted_uids: impl IntoIterator<Item = u32>) -> Self {
        Self {
            manager,
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
        let result = match request.command {
            AppdCommand::Ping => Ok(ResponseData::Pong),
            AppdCommand::List { offset, limit } => self.list_apps(offset, limit),
            AppdCommand::Start { app_id } => self
                .manager
                .start(&app_id)
                .map(|unit| ResponseData::Started { app_id, unit }),
            AppdCommand::Stop { app_id } => self
                .manager
                .stop(&app_id)
                .map(|()| ResponseData::Stopped { app_id }),
        };
        match result {
            Ok(data) => AppdResponse::success(request_id, data),
            Err(error) => {
                eprintln!("cp0-appd: lifecycle request failed: {error}");
                manager_error_response(request_id, &error)
            }
        }
    }

    fn list_apps(&self, offset: u16, limit: u8) -> Result<ResponseData, AppManagerError> {
        let installed_apps = self.manager.installed_apps();
        let mut apps = Vec::new();
        for installed in installed_apps
            .iter()
            .skip(usize::from(offset))
            .take(usize::from(limit))
        {
            apps.push(AppSummary {
                running: self.manager.is_running(&installed.app_id)?,
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
}
