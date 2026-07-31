use std::fmt;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cp0_radio_protocol::{
    RadioErrorCode, RadioOutcome, RadioProtocolError, RadioRequest, read_response, write_request,
};

pub const DEFAULT_RADIO_SOCKET: &str = "/run/cardputerzero-radiod/radio.sock";
const RADIO_SERVICE_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedRadioPacket {
    pub payload: Vec<u8>,
    pub rssi_dbm: i16,
    pub snr_quarter_db: i8,
}

#[derive(Debug)]
pub enum RadioClientError {
    Io(std::io::Error),
    Protocol(RadioProtocolError),
    EmptyResponse,
    MismatchedRequestId,
    MismatchedOutcome,
    Service(RadioErrorCode),
}

impl fmt::Display for RadioClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "radio service I/O error: {error}"),
            Self::Protocol(error) => write!(formatter, "radio service protocol error: {error}"),
            Self::EmptyResponse => formatter.write_str("radio service returned no response"),
            Self::MismatchedRequestId => {
                formatter.write_str("radio service returned a mismatched request ID")
            }
            Self::MismatchedOutcome => {
                formatter.write_str("radio service returned a mismatched operation")
            }
            Self::Service(code) => write!(formatter, "radio service rejected request: {code:?}"),
        }
    }
}

impl std::error::Error for RadioClientError {}

impl From<std::io::Error> for RadioClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<RadioProtocolError> for RadioClientError {
    fn from(error: RadioProtocolError) -> Self {
        Self::Protocol(error)
    }
}

#[derive(Debug, Clone)]
pub struct RadioClient {
    socket_path: PathBuf,
}

impl Default for RadioClient {
    fn default() -> Self {
        Self::new(DEFAULT_RADIO_SOCKET)
    }
}

impl RadioClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub fn send(&self, request_id: u64, payload: &[u8]) -> Result<u8, RadioClientError> {
        match self.exchange(&RadioRequest::send_lora(request_id, payload))? {
            RadioOutcome::LoraSent { bytes } if usize::from(bytes) == payload.len() => Ok(bytes),
            RadioOutcome::Error { code, .. } => Err(RadioClientError::Service(code)),
            _ => Err(RadioClientError::MismatchedOutcome),
        }
    }

    pub fn receive(
        &self,
        request_id: u64,
        timeout_ms: u16,
    ) -> Result<Option<ReceivedRadioPacket>, RadioClientError> {
        match self.exchange(&RadioRequest::receive_lora(request_id, timeout_ms))? {
            RadioOutcome::LoraPacket {
                payload_base64,
                rssi_dbm,
                snr_quarter_db,
            } => Ok(Some(ReceivedRadioPacket {
                payload: cp0_radio_protocol::decode_payload(&payload_base64)?,
                rssi_dbm,
                snr_quarter_db,
            })),
            RadioOutcome::LoraNoPacket => Ok(None),
            RadioOutcome::Error { code, .. } => Err(RadioClientError::Service(code)),
            _ => Err(RadioClientError::MismatchedOutcome),
        }
    }

    fn exchange(&self, request: &RadioRequest) -> Result<RadioOutcome, RadioClientError> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(RADIO_SERVICE_TIMEOUT))?;
        stream.set_write_timeout(Some(RADIO_SERVICE_TIMEOUT))?;
        write_request(&mut stream, request)?;
        let response = read_response(&mut BufReader::new(stream.try_clone()?))?
            .ok_or(RadioClientError::EmptyResponse)?;
        if response.request_id != request.request_id {
            return Err(RadioClientError::MismatchedRequestId);
        }
        Ok(response.outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use cp0_radio_protocol::{RadioResponse, read_request, write_response};

    use super::*;

    #[test]
    fn correlates_packet_metadata_and_request_id() {
        let (mut client, mut service) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let request = read_request(&mut BufReader::new(service.try_clone().unwrap()))
                .unwrap()
                .unwrap();
            write_response(
                &mut service,
                &RadioResponse::lora_packet(request.request_id, b"hello", -92, -5),
            )
            .unwrap();
        });
        write_request(&mut client, &RadioRequest::receive_lora(9, 100)).unwrap();
        let response = read_response(&mut BufReader::new(client.try_clone().unwrap()))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
        assert_eq!(response.request_id, 9);
        assert!(matches!(
            response.outcome,
            RadioOutcome::LoraPacket {
                rssi_dbm: -92,
                snr_quarter_db: -5,
                ..
            }
        ));
    }
}
