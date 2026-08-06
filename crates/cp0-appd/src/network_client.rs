use std::fmt;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cp0_network_protocol::{
    NetworkErrorCode, NetworkOutcome, NetworkProtocolError, NetworkRequest, read_response,
    write_request,
};

pub const DEFAULT_NETWORK_SOCKET: &str = "/run/cardputerzero-networkd/network.sock";
const NETWORK_SERVICE_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkHttpResponse {
    pub status_code: u16,
    pub body_base64: String,
}

#[derive(Debug)]
pub enum NetworkClientError {
    Io(std::io::Error),
    Protocol(NetworkProtocolError),
    MismatchedRequestId,
    Service(NetworkErrorCode),
    EmptyResponse,
}

impl fmt::Display for NetworkClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "network service I/O failed: {error}"),
            Self::Protocol(error) => write!(formatter, "network service protocol failed: {error}"),
            Self::MismatchedRequestId => {
                formatter.write_str("network service returned a mismatched request ID")
            }
            Self::Service(code) => write!(formatter, "network service returned {code:?}"),
            Self::EmptyResponse => formatter.write_str("network service closed without a response"),
        }
    }
}

impl std::error::Error for NetworkClientError {}

impl From<std::io::Error> for NetworkClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<NetworkProtocolError> for NetworkClientError {
    fn from(error: NetworkProtocolError) -> Self {
        Self::Protocol(error)
    }
}

#[derive(Debug, Clone)]
pub struct NetworkClient {
    socket_path: PathBuf,
}

impl Default for NetworkClient {
    fn default() -> Self {
        Self::new(DEFAULT_NETWORK_SOCKET)
    }
}

impl NetworkClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub fn http_get(
        &self,
        request_id: u64,
        url: &str,
    ) -> Result<NetworkHttpResponse, NetworkClientError> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(NETWORK_SERVICE_TIMEOUT))?;
        stream.set_write_timeout(Some(NETWORK_SERVICE_TIMEOUT))?;
        Self::exchange(&mut stream, &NetworkRequest::http_get(request_id, url))
    }

    pub fn http_get_range(
        &self,
        request_id: u64,
        url: &str,
        offset: u64,
        length: u16,
    ) -> Result<NetworkHttpResponse, NetworkClientError> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(NETWORK_SERVICE_TIMEOUT))?;
        stream.set_write_timeout(Some(NETWORK_SERVICE_TIMEOUT))?;
        Self::exchange(
            &mut stream,
            &NetworkRequest::http_get_range(request_id, url, offset, length),
        )
    }

    fn exchange(
        stream: &mut UnixStream,
        request: &NetworkRequest,
    ) -> Result<NetworkHttpResponse, NetworkClientError> {
        write_request(&mut *stream, request)?;
        let response = read_response(&mut BufReader::new(stream.try_clone()?))?
            .ok_or(NetworkClientError::EmptyResponse)?;
        if response.request_id != request.request_id {
            return Err(NetworkClientError::MismatchedRequestId);
        }
        match response.outcome {
            NetworkOutcome::Ok {
                status_code,
                body_base64,
            } => Ok(NetworkHttpResponse {
                status_code,
                body_base64,
            }),
            NetworkOutcome::Error { code, .. } => Err(NetworkClientError::Service(code)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use cp0_network_protocol::{NetworkResponse, read_request, write_response};

    use super::*;

    #[test]
    fn exchanges_a_strict_bounded_request() {
        let (mut client, mut service) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let request = read_request(&mut BufReader::new(service.try_clone().unwrap()))
                .unwrap()
                .unwrap();
            assert_eq!(request.request_id, 9);
            write_response(&mut service, &NetworkResponse::success(9, 200, b"ok")).unwrap();
        });
        let response = NetworkClient::exchange(
            &mut client,
            &NetworkRequest::http_get(9, "https://example.com"),
        )
        .unwrap();
        worker.join().unwrap();
        assert_eq!(response.status_code, 200);
        assert_eq!(
            cp0_network_protocol::decode_base64(&response.body_base64).unwrap(),
            b"ok"
        );
    }
}
