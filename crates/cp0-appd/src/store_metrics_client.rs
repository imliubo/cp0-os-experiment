use std::fmt;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cp0_store_protocol::{
    STORE_PROTOCOL_VERSION, StoreCommand, StoreErrorCode, StoreOutcome, StoreProtocolError,
    StoreRequest, StoreResponseData, StoreRuntimeMetricEvent, read_response, write_request,
};

pub const DEFAULT_STORE_SOCKET: &str = "/run/cardputerzero-store/control.sock";
const STORE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub enum StoreMetricsClientError {
    Io(std::io::Error),
    Protocol(StoreProtocolError),
    MismatchedRequestId,
    Service(StoreErrorCode),
    UnexpectedResponse,
    EmptyResponse,
}

impl fmt::Display for StoreMetricsClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "store metrics I/O failed: {error}"),
            Self::Protocol(error) => write!(formatter, "store metrics protocol failed: {error}"),
            Self::MismatchedRequestId => {
                formatter.write_str("store metrics returned a mismatched request ID")
            }
            Self::Service(code) => write!(formatter, "store metrics returned {code:?}"),
            Self::UnexpectedResponse => {
                formatter.write_str("store metrics returned an unexpected response")
            }
            Self::EmptyResponse => formatter.write_str("store metrics closed without a response"),
        }
    }
}

impl std::error::Error for StoreMetricsClientError {}

impl From<std::io::Error> for StoreMetricsClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<StoreProtocolError> for StoreMetricsClientError {
    fn from(error: StoreProtocolError) -> Self {
        Self::Protocol(error)
    }
}

#[derive(Debug, Clone)]
pub struct StoreMetricsClient {
    socket_path: PathBuf,
}

impl Default for StoreMetricsClient {
    fn default() -> Self {
        Self::new(DEFAULT_STORE_SOCKET)
    }
}

impl StoreMetricsClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub fn record(
        &self,
        request_id: u64,
        app_id: &str,
        version: &str,
        event: StoreRuntimeMetricEvent,
    ) -> Result<(), StoreMetricsClientError> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(STORE_TIMEOUT))?;
        stream.set_write_timeout(Some(STORE_TIMEOUT))?;
        Self::exchange(&mut stream, request_id, app_id, version, event)
    }

    fn exchange(
        stream: &mut UnixStream,
        request_id: u64,
        app_id: &str,
        version: &str,
        event: StoreRuntimeMetricEvent,
    ) -> Result<(), StoreMetricsClientError> {
        write_request(
            &mut *stream,
            &StoreRequest {
                protocol_version: STORE_PROTOCOL_VERSION,
                request_id,
                command: StoreCommand::RecordRuntimeMetric {
                    app_id: app_id.into(),
                    version: version.into(),
                    event,
                },
            },
        )?;
        let response = read_response(&mut BufReader::new(stream.try_clone()?))?
            .ok_or(StoreMetricsClientError::EmptyResponse)?;
        if response.request_id != request_id {
            return Err(StoreMetricsClientError::MismatchedRequestId);
        }
        match response.outcome {
            StoreOutcome::Ok {
                data: StoreResponseData::MetricRecorded,
            } => Ok(()),
            StoreOutcome::Ok { .. } => Err(StoreMetricsClientError::UnexpectedResponse),
            StoreOutcome::Error { code, .. } => Err(StoreMetricsClientError::Service(code)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use cp0_store_protocol::{StoreResponse, read_request, write_response};

    use super::*;

    #[test]
    fn exchanges_a_strict_runtime_metric_request() {
        let (mut client, mut service) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let request = read_request(&mut BufReader::new(service.try_clone().unwrap()))
                .unwrap()
                .unwrap();
            assert_eq!(request.request_id, 19);
            assert!(matches!(
                request.command,
                StoreCommand::RecordRuntimeMetric {
                    app_id,
                    version,
                    event: StoreRuntimeMetricEvent::Crash,
                } if app_id == "dev.cardputerzero.test" && version == "1.2.3"
            ));
            write_response(
                &mut service,
                &StoreResponse::success(19, StoreResponseData::MetricRecorded),
            )
            .unwrap();
        });
        StoreMetricsClient::exchange(
            &mut client,
            19,
            "dev.cardputerzero.test",
            "1.2.3",
            StoreRuntimeMetricEvent::Crash,
        )
        .unwrap();
        worker.join().unwrap();
    }
}
