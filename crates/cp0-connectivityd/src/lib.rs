use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, BufReader};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use cp0_connectivity_protocol::{
    ConnectivityCommand, ConnectivityErrorCode, ConnectivityOutcome, ConnectivityProtocolError,
    ConnectivityRequest, ConnectivityResponse, ConnectivityState, read_request, write_response,
};

pub const DEFAULT_NMCLI_PATH: &str = "/usr/bin/nmcli";
pub const DEFAULT_WIFI_INTERFACE_PATH: &str = "/sys/class/net/wlan0";
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectivityBackendError {
    Unavailable,
    Operation,
}

impl fmt::Display for ConnectivityBackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("NetworkManager is unavailable"),
            Self::Operation => formatter.write_str("NetworkManager operation failed"),
        }
    }
}

impl std::error::Error for ConnectivityBackendError {}

pub trait ConnectivityBackend {
    fn state(&self) -> Result<ConnectivityState, ConnectivityBackendError>;
    fn set_wifi_enabled(
        &self,
        enabled: bool,
    ) -> Result<ConnectivityState, ConnectivityBackendError>;
    fn set_airplane_mode(
        &self,
        enabled: bool,
    ) -> Result<ConnectivityState, ConnectivityBackendError>;
}

#[derive(Debug, Clone)]
pub struct NmcliConnectivityBackend {
    nmcli_path: PathBuf,
    wifi_interface_path: PathBuf,
}

impl Default for NmcliConnectivityBackend {
    fn default() -> Self {
        Self::new(DEFAULT_NMCLI_PATH, DEFAULT_WIFI_INTERFACE_PATH)
    }
}

impl NmcliConnectivityBackend {
    pub fn new(nmcli_path: impl Into<PathBuf>, wifi_interface_path: impl Into<PathBuf>) -> Self {
        Self {
            nmcli_path: nmcli_path.into(),
            wifi_interface_path: wifi_interface_path.into(),
        }
    }

    fn radio_state(&self) -> Result<(bool, bool), ConnectivityBackendError> {
        let output = Command::new(&self.nmcli_path)
            .args(["-t", "-f", "WIFI,WWAN", "general"])
            .output()
            .map_err(map_spawn_error)?;
        if !output.status.success() {
            return Err(ConnectivityBackendError::Unavailable);
        }
        parse_radio_state(&output.stdout)
    }

    fn set_radio(&self, radio: &str, enabled: bool) -> Result<(), ConnectivityBackendError> {
        let state = if enabled { "on" } else { "off" };
        let status = Command::new(&self.nmcli_path)
            .args(["radio", radio, state])
            .status()
            .map_err(map_spawn_error)?;
        if status.success() {
            Ok(())
        } else {
            Err(ConnectivityBackendError::Operation)
        }
    }

    fn observed_state(&self) -> Result<ConnectivityState, ConnectivityBackendError> {
        let (wifi_radio_enabled, wwan_radio_enabled) = self.radio_state()?;
        let wifi_available = self.wifi_interface_path.is_dir();
        let airplane_mode = !wifi_radio_enabled && !wwan_radio_enabled;
        Ok(ConnectivityState {
            available: true,
            wifi_available,
            wifi_enabled: wifi_available && wifi_radio_enabled,
            airplane_mode,
        })
    }
}

impl ConnectivityBackend for NmcliConnectivityBackend {
    fn state(&self) -> Result<ConnectivityState, ConnectivityBackendError> {
        self.observed_state()
    }

    fn set_wifi_enabled(
        &self,
        enabled: bool,
    ) -> Result<ConnectivityState, ConnectivityBackendError> {
        if !self.wifi_interface_path.is_dir() {
            return Err(ConnectivityBackendError::Unavailable);
        }
        self.set_radio("wifi", enabled)?;
        self.observed_state()
    }

    fn set_airplane_mode(
        &self,
        enabled: bool,
    ) -> Result<ConnectivityState, ConnectivityBackendError> {
        self.set_radio("all", !enabled)?;
        self.observed_state()
    }
}

fn map_spawn_error(error: io::Error) -> ConnectivityBackendError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
            ConnectivityBackendError::Unavailable
        }
        _ => ConnectivityBackendError::Operation,
    }
}

fn parse_radio_state(output: &[u8]) -> Result<(bool, bool), ConnectivityBackendError> {
    let text = std::str::from_utf8(output).map_err(|_| ConnectivityBackendError::Operation)?;
    let line = text.trim_end_matches(['\r', '\n']);
    if line.contains(['\r', '\n']) {
        return Err(ConnectivityBackendError::Operation);
    }
    let (wifi, wwan) = line
        .split_once(':')
        .ok_or(ConnectivityBackendError::Operation)?;
    if wwan.contains(':') {
        return Err(ConnectivityBackendError::Operation);
    }
    Ok((parse_radio_value(wifi)?, parse_radio_value(wwan)?))
}

fn parse_radio_value(value: &str) -> Result<bool, ConnectivityBackendError> {
    match value {
        "enabled" => Ok(true),
        "disabled" => Ok(false),
        _ => Err(ConnectivityBackendError::Operation),
    }
}

#[derive(Debug)]
pub struct ConnectivityServer<B> {
    backend: B,
    trusted_uids: BTreeSet<u32>,
}

impl<B: ConnectivityBackend> ConnectivityServer<B> {
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
                eprintln!("cp0-connectivityd: rejected connection: {error}");
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
                    &ConnectivityResponse::error(
                        0,
                        ConnectivityErrorCode::InvalidRequest,
                        "invalid connectivity service request",
                    ),
                )
                .map_err(protocol_io)?;
                eprintln!("cp0-connectivityd: invalid request: {error}");
                return Ok(());
            }
        };
        if !self.trusted_uids.contains(&uid) {
            write_response(
                &mut stream,
                &ConnectivityResponse::error(
                    request.request_id,
                    ConnectivityErrorCode::Unauthorized,
                    "peer UID is not authorized for connectivity control",
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }
        let mutating = !matches!(request.command, ConnectivityCommand::GetState {});
        let response = self.dispatch(request);
        if mutating && let ConnectivityOutcome::State { state } = response.outcome {
            eprintln!(
                "cp0-connectivityd: audit uid={uid} wifi_enabled={} airplane_mode={}",
                state.wifi_enabled, state.airplane_mode
            );
        }
        write_response(&mut stream, &response).map_err(protocol_io)
    }

    pub fn dispatch(&self, request: ConnectivityRequest) -> ConnectivityResponse {
        let request_id = request.request_id;
        let result = match request.command {
            ConnectivityCommand::GetState {} => self.backend.state(),
            ConnectivityCommand::SetWifiEnabled { enabled } => {
                self.backend.set_wifi_enabled(enabled)
            }
            ConnectivityCommand::SetAirplaneMode { enabled } => {
                self.backend.set_airplane_mode(enabled)
            }
        };
        match result {
            Ok(state) => ConnectivityResponse::state(request_id, state),
            Err(error) => backend_error_response(request_id, error),
        }
    }
}

fn backend_error_response(
    request_id: u64,
    error: ConnectivityBackendError,
) -> ConnectivityResponse {
    match error {
        ConnectivityBackendError::Unavailable => ConnectivityResponse::error(
            request_id,
            ConnectivityErrorCode::Unavailable,
            "connectivity control is unavailable",
        ),
        ConnectivityBackendError::Operation => ConnectivityResponse::error(
            request_id,
            ConnectivityErrorCode::Operation,
            "connectivity operation failed",
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

fn protocol_io(error: ConnectivityProtocolError) -> io::Error {
    match error {
        ConnectivityProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[derive(Debug)]
    struct MockConnectivity {
        state: Cell<ConnectivityState>,
        unavailable: bool,
    }

    impl MockConnectivity {
        fn available() -> Self {
            Self {
                state: Cell::new(ConnectivityState {
                    available: true,
                    wifi_available: true,
                    wifi_enabled: true,
                    airplane_mode: false,
                }),
                unavailable: false,
            }
        }
    }

    impl ConnectivityBackend for MockConnectivity {
        fn state(&self) -> Result<ConnectivityState, ConnectivityBackendError> {
            if self.unavailable {
                Err(ConnectivityBackendError::Unavailable)
            } else {
                Ok(self.state.get())
            }
        }

        fn set_wifi_enabled(
            &self,
            enabled: bool,
        ) -> Result<ConnectivityState, ConnectivityBackendError> {
            let mut state = self.state()?;
            state.wifi_enabled = enabled;
            if enabled {
                state.airplane_mode = false;
            }
            self.state.set(state);
            Ok(state)
        }

        fn set_airplane_mode(
            &self,
            enabled: bool,
        ) -> Result<ConnectivityState, ConnectivityBackendError> {
            let mut state = self.state()?;
            state.airplane_mode = enabled;
            state.wifi_enabled = !enabled;
            self.state.set(state);
            Ok(state)
        }
    }

    fn state(response: ConnectivityResponse) -> ConnectivityState {
        match response.outcome {
            ConnectivityOutcome::State { state } => state,
            outcome => panic!("expected state response, got {outcome:?}"),
        }
    }

    #[test]
    fn strictly_parses_nmcli_radio_state() {
        assert_eq!(parse_radio_state(b"enabled:disabled\n"), Ok((true, false)));
        assert_eq!(
            parse_radio_state(b"disabled:disabled\n"),
            Ok((false, false))
        );
        assert!(parse_radio_state(b"enabled:unknown\n").is_err());
        assert!(parse_radio_state(b"enabled:enabled:extra\n").is_err());
        assert!(parse_radio_state(b"enabled:enabled\nextra\n").is_err());
    }

    #[test]
    fn dispatches_state_and_radio_changes_through_mock_backend() {
        let server = ConnectivityServer::new(MockConnectivity::available(), [0]);
        assert!(state(server.dispatch(ConnectivityRequest::get_state(1))).wifi_enabled);
        let airplane = state(server.dispatch(ConnectivityRequest::set_airplane_mode(2, true)));
        assert!(airplane.airplane_mode);
        assert!(!airplane.wifi_enabled);
        let wifi = state(server.dispatch(ConnectivityRequest::set_wifi_enabled(3, true)));
        assert!(wifi.wifi_enabled);
        assert!(!wifi.airplane_mode);
    }

    #[test]
    fn maps_backend_unavailability_to_protocol_error() {
        let server = ConnectivityServer::new(
            MockConnectivity {
                state: Cell::new(ConnectivityState::unavailable()),
                unavailable: true,
            },
            [0],
        );
        assert!(matches!(
            server.dispatch(ConnectivityRequest::get_state(1)).outcome,
            ConnectivityOutcome::Error {
                code: ConnectivityErrorCode::Unavailable,
                ..
            }
        ));
    }
}
