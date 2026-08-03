use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, BufReader};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use cp0_power_protocol::{
    PowerAction, PowerCommand, PowerErrorCode, PowerOutcome, PowerProtocolError, PowerRequest,
    PowerResponse, read_request, write_response,
};

pub const DEFAULT_SYSTEMCTL_PATH: &str = "/usr/bin/systemctl";
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerBackendError {
    Unavailable,
    Operation,
}

impl fmt::Display for PowerBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("system power control is unavailable"),
            Self::Operation => formatter.write_str("system power operation failed"),
        }
    }
}

impl std::error::Error for PowerBackendError {}

pub trait PowerBackend {
    fn execute(&self, action: PowerAction) -> Result<(), PowerBackendError>;
}

#[derive(Debug, Clone)]
pub struct SystemdPowerBackend {
    systemctl_path: PathBuf,
}

impl Default for SystemdPowerBackend {
    fn default() -> Self {
        Self::new(DEFAULT_SYSTEMCTL_PATH)
    }
}

impl SystemdPowerBackend {
    pub fn new(systemctl_path: impl Into<PathBuf>) -> Self {
        Self {
            systemctl_path: systemctl_path.into(),
        }
    }
}

impl PowerBackend for SystemdPowerBackend {
    fn execute(&self, action: PowerAction) -> Result<(), PowerBackendError> {
        let status = Command::new(&self.systemctl_path)
            .args(systemd_arguments(action))
            .status()
            .map_err(map_spawn_error)?;
        if status.success() {
            Ok(())
        } else {
            Err(PowerBackendError::Operation)
        }
    }
}

fn systemd_arguments(action: PowerAction) -> [&'static str; 2] {
    match action {
        PowerAction::Restart => ["--no-block", "reboot"],
        PowerAction::PowerOff => ["--no-block", "poweroff"],
    }
}

fn map_spawn_error(error: io::Error) -> PowerBackendError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => PowerBackendError::Unavailable,
        _ => PowerBackendError::Operation,
    }
}

#[derive(Debug)]
pub struct PowerServer<B> {
    backend: B,
    trusted_uids: BTreeSet<u32>,
}

impl<B: PowerBackend> PowerServer<B> {
    pub fn new(backend: B, trusted_uids: impl IntoIterator<Item = u32>) -> Self {
        Self {
            backend,
            trusted_uids: trusted_uids.into_iter().collect(),
        }
    }

    pub fn serve(&self, listener: UnixListener) -> io::Result<()> {
        loop {
            let (stream, _) = listener.accept()?;
            if let Err(error) = self.handle_connection(stream) {
                eprintln!("cp0-powerd: rejected connection: {error}");
            }
        }
    }

    fn handle_connection(&self, mut stream: UnixStream) -> io::Result<()> {
        stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
        let uid = peer_uid(&stream)?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let request = match read_request(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(error) => {
                write_response(
                    &mut stream,
                    &PowerResponse::error(
                        0,
                        PowerErrorCode::InvalidRequest,
                        "invalid power service request",
                    ),
                )
                .map_err(protocol_io)?;
                eprintln!("cp0-powerd: invalid request: {error}");
                return Ok(());
            }
        };
        if !self.trusted_uids.contains(&uid) {
            write_response(
                &mut stream,
                &PowerResponse::error(
                    request.request_id,
                    PowerErrorCode::Unauthorized,
                    "peer UID is not authorized for power control",
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }
        let response = self.dispatch(request);
        if let PowerOutcome::Accepted { action } = response.outcome {
            eprintln!("cp0-powerd: audit uid={uid} action={action:?}");
        }
        write_response(&mut stream, &response).map_err(protocol_io)
    }

    pub fn dispatch(&self, request: PowerRequest) -> PowerResponse {
        let request_id = request.request_id;
        let action = match request.command {
            PowerCommand::Restart {} => PowerAction::Restart,
            PowerCommand::PowerOff {} => PowerAction::PowerOff,
        };
        match self.backend.execute(action) {
            Ok(()) => PowerResponse::accepted(request_id, action),
            Err(error) => backend_error_response(request_id, error),
        }
    }
}

fn backend_error_response(request_id: u64, error: PowerBackendError) -> PowerResponse {
    match error {
        PowerBackendError::Unavailable => PowerResponse::error(
            request_id,
            PowerErrorCode::Unavailable,
            "system power control is unavailable",
        ),
        PowerBackendError::Operation => PowerResponse::error(
            request_id,
            PowerErrorCode::Operation,
            "system power operation failed",
        ),
    }
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SO_PEERCRED returned an unexpected size",
        ));
    }
    Ok(credentials.uid)
}

#[cfg(not(target_os = "linux"))]
fn peer_uid(_stream: &UnixStream) -> io::Result<u32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "peer credentials are only implemented for the Linux target",
    ))
}

fn protocol_io(error: PowerProtocolError) -> io::Error {
    match error {
        PowerProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    #[derive(Debug, Default)]
    struct MockPower {
        actions: RefCell<Vec<PowerAction>>,
        fail: Cell<Option<PowerBackendError>>,
    }

    impl PowerBackend for MockPower {
        fn execute(&self, action: PowerAction) -> Result<(), PowerBackendError> {
            self.actions.borrow_mut().push(action);
            self.fail.get().map_or(Ok(()), Err)
        }
    }

    #[test]
    fn dispatches_only_fixed_power_actions() {
        let server = PowerServer::new(MockPower::default(), [1000]);
        let restart = server.dispatch(PowerRequest::restart(10));
        let power_off = server.dispatch(PowerRequest::power_off(11));
        assert_eq!(
            restart.outcome,
            PowerOutcome::Accepted {
                action: PowerAction::Restart
            }
        );
        assert_eq!(
            power_off.outcome,
            PowerOutcome::Accepted {
                action: PowerAction::PowerOff
            }
        );
        assert_eq!(
            *server.backend.actions.borrow(),
            [PowerAction::Restart, PowerAction::PowerOff]
        );
    }

    #[test]
    fn maps_backend_failure_without_accepting_the_action() {
        let backend = MockPower::default();
        backend.fail.set(Some(PowerBackendError::Operation));
        let response = PowerServer::new(backend, [1000]).dispatch(PowerRequest::restart(12));
        assert!(matches!(
            response.outcome,
            PowerOutcome::Error {
                code: PowerErrorCode::Operation,
                ..
            }
        ));
    }

    #[test]
    fn systemd_arguments_are_closed_and_nonblocking() {
        assert_eq!(
            systemd_arguments(PowerAction::Restart),
            ["--no-block", "reboot"]
        );
        assert_eq!(
            systemd_arguments(PowerAction::PowerOff),
            ["--no-block", "poweroff"]
        );
    }
}
