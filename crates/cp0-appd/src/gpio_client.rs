use std::fmt;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cp0_gpio_protocol::{
    GpioErrorCode, GpioLine, GpioOutcome, GpioProtocolError, GpioRequest, read_response,
    write_request,
};

pub const DEFAULT_GPIO_SOCKET: &str = "/run/cardputerzero-gpiod/gpio.sock";
const GPIO_SERVICE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub enum GpioClientError {
    Io(std::io::Error),
    Protocol(GpioProtocolError),
    EmptyResponse,
    MismatchedRequestId,
    MismatchedOutcome,
    Service(GpioErrorCode),
}

impl fmt::Display for GpioClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "GPIO service I/O error: {error}"),
            Self::Protocol(error) => write!(formatter, "GPIO service protocol error: {error}"),
            Self::EmptyResponse => formatter.write_str("GPIO service returned no response"),
            Self::MismatchedRequestId => {
                formatter.write_str("GPIO service returned a mismatched request ID")
            }
            Self::MismatchedOutcome => {
                formatter.write_str("GPIO service returned a mismatched line or operation")
            }
            Self::Service(code) => write!(formatter, "GPIO service rejected request: {code:?}"),
        }
    }
}

impl std::error::Error for GpioClientError {}

impl From<std::io::Error> for GpioClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<GpioProtocolError> for GpioClientError {
    fn from(error: GpioProtocolError) -> Self {
        Self::Protocol(error)
    }
}

#[derive(Debug, Clone)]
pub struct GpioClient {
    socket_path: PathBuf,
}

impl Default for GpioClient {
    fn default() -> Self {
        Self::new(DEFAULT_GPIO_SOCKET)
    }
}

impl GpioClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub fn read(&self, request_id: u64, line: GpioLine) -> Result<bool, GpioClientError> {
        match self.exchange(&GpioRequest::read(request_id, line))? {
            GpioOutcome::Value {
                line: returned_line,
                value,
            } if returned_line == line => Ok(value),
            GpioOutcome::Error { code, .. } => Err(GpioClientError::Service(code)),
            _ => Err(GpioClientError::MismatchedOutcome),
        }
    }

    pub fn write(
        &self,
        request_id: u64,
        line: GpioLine,
        value: bool,
    ) -> Result<(), GpioClientError> {
        match self.exchange(&GpioRequest::write(request_id, line, value))? {
            GpioOutcome::Written {
                line: returned_line,
                value: returned_value,
            } if returned_line == line && returned_value == value => Ok(()),
            GpioOutcome::Error { code, .. } => Err(GpioClientError::Service(code)),
            _ => Err(GpioClientError::MismatchedOutcome),
        }
    }

    fn exchange(&self, request: &GpioRequest) -> Result<GpioOutcome, GpioClientError> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(GPIO_SERVICE_TIMEOUT))?;
        stream.set_write_timeout(Some(GPIO_SERVICE_TIMEOUT))?;
        write_request(&mut stream, request)?;
        let response = read_response(&mut BufReader::new(stream.try_clone()?))?
            .ok_or(GpioClientError::EmptyResponse)?;
        if response.request_id != request.request_id {
            return Err(GpioClientError::MismatchedRequestId);
        }
        Ok(response.outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use cp0_gpio_protocol::{GpioResponse, read_request, write_response};

    use super::*;

    #[test]
    fn correlates_operation_line_value_and_request_id() {
        let (mut client, mut service) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let request = read_request(&mut BufReader::new(service.try_clone().unwrap()))
                .unwrap()
                .unwrap();
            write_response(
                &mut service,
                &GpioResponse::written(request.request_id, GpioLine::GroveFunction, true),
            )
            .unwrap();
        });
        let request = GpioRequest::write(9, GpioLine::GroveFunction, true);
        let outcome = exchange_stream(&mut client, &request).unwrap();
        worker.join().unwrap();
        assert_eq!(
            outcome,
            GpioOutcome::Written {
                line: GpioLine::GroveFunction,
                value: true
            }
        );
    }

    fn exchange_stream(
        stream: &mut UnixStream,
        request: &GpioRequest,
    ) -> Result<GpioOutcome, GpioClientError> {
        write_request(&mut *stream, request)?;
        let response = read_response(&mut BufReader::new(stream.try_clone()?))?
            .ok_or(GpioClientError::EmptyResponse)?;
        if response.request_id != request.request_id {
            return Err(GpioClientError::MismatchedRequestId);
        }
        Ok(response.outcome)
    }
}
