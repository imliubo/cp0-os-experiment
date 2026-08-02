use std::fmt;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

pub const CONNECTIVITY_PROTOCOL_VERSION: u32 = 1;
pub const MAX_CONNECTIVITY_FRAME_BYTES: usize = 2048;
pub const MAX_CONNECTIVITY_ERROR_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectivityRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: ConnectivityCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ConnectivityCommand {
    GetState {},
    SetWifiEnabled { enabled: bool },
    SetAirplaneMode { enabled: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectivityState {
    pub available: bool,
    pub wifi_available: bool,
    pub wifi_enabled: bool,
    pub airplane_mode: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectivityResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: ConnectivityOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ConnectivityOutcome {
    State {
        state: ConnectivityState,
    },
    Error {
        code: ConnectivityErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectivityErrorCode {
    InvalidRequest,
    Unauthorized,
    Unavailable,
    Operation,
    Internal,
}

#[derive(Debug)]
pub enum ConnectivityProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidState,
    InvalidErrorMessage,
}

impl fmt::Display for ConnectivityProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "connectivity protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid connectivity JSON: {error}"),
            Self::FrameTooLarge => formatter.write_str("connectivity frame exceeds 2048 bytes"),
            Self::UnterminatedFrame => formatter.write_str("connectivity frame is not terminated"),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "unsupported connectivity protocol version {version}"
                )
            }
            Self::InvalidState => formatter.write_str("connectivity state is inconsistent"),
            Self::InvalidErrorMessage => formatter.write_str("invalid connectivity error message"),
        }
    }
}

impl std::error::Error for ConnectivityProtocolError {}

impl From<io::Error> for ConnectivityProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ConnectivityProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl ConnectivityRequest {
    pub const fn get_state(request_id: u64) -> Self {
        Self {
            protocol_version: CONNECTIVITY_PROTOCOL_VERSION,
            request_id,
            command: ConnectivityCommand::GetState {},
        }
    }

    pub const fn set_wifi_enabled(request_id: u64, enabled: bool) -> Self {
        Self {
            protocol_version: CONNECTIVITY_PROTOCOL_VERSION,
            request_id,
            command: ConnectivityCommand::SetWifiEnabled { enabled },
        }
    }

    pub const fn set_airplane_mode(request_id: u64, enabled: bool) -> Self {
        Self {
            protocol_version: CONNECTIVITY_PROTOCOL_VERSION,
            request_id,
            command: ConnectivityCommand::SetAirplaneMode { enabled },
        }
    }

    pub fn validate(&self) -> Result<(), ConnectivityProtocolError> {
        validate_version(self.protocol_version)
    }
}

impl ConnectivityState {
    pub const fn unavailable() -> Self {
        Self {
            available: false,
            wifi_available: false,
            wifi_enabled: false,
            airplane_mode: false,
        }
    }

    pub fn validate(&self) -> Result<(), ConnectivityProtocolError> {
        if (!self.available && (self.wifi_available || self.wifi_enabled || self.airplane_mode))
            || (!self.wifi_available && self.wifi_enabled)
            || (self.airplane_mode && self.wifi_enabled)
        {
            Err(ConnectivityProtocolError::InvalidState)
        } else {
            Ok(())
        }
    }
}

impl ConnectivityResponse {
    pub const fn state(request_id: u64, state: ConnectivityState) -> Self {
        Self {
            protocol_version: CONNECTIVITY_PROTOCOL_VERSION,
            request_id,
            outcome: ConnectivityOutcome::State { state },
        }
    }

    pub fn error(request_id: u64, code: ConnectivityErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: CONNECTIVITY_PROTOCOL_VERSION,
            request_id,
            outcome: ConnectivityOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), ConnectivityProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.outcome {
            ConnectivityOutcome::State { state } => state.validate(),
            ConnectivityOutcome::Error { message, .. } => {
                if message.is_empty()
                    || message.chars().count() > MAX_CONNECTIVITY_ERROR_CHARS
                    || message.chars().any(char::is_control)
                {
                    Err(ConnectivityProtocolError::InvalidErrorMessage)
                } else {
                    Ok(())
                }
            }
        }
    }
}

pub fn read_request(
    reader: &mut impl BufRead,
) -> Result<Option<ConnectivityRequest>, ConnectivityProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: ConnectivityRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn write_request(
    writer: &mut impl Write,
    request: &ConnectivityRequest,
) -> Result<(), ConnectivityProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<ConnectivityResponse>, ConnectivityProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: ConnectivityResponse = serde_json::from_slice(&frame)?;
    response.validate()?;
    Ok(Some(response))
}

pub fn write_response(
    writer: &mut impl Write,
    response: &ConnectivityResponse,
) -> Result<(), ConnectivityProtocolError> {
    response.validate()?;
    write_frame(writer, response)
}

fn validate_version(version: u32) -> Result<(), ConnectivityProtocolError> {
    if version == CONNECTIVITY_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ConnectivityProtocolError::UnsupportedVersion(version))
    }
}

fn write_frame(
    writer: &mut impl Write,
    value: &impl Serialize,
) -> Result<(), ConnectivityProtocolError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_CONNECTIVITY_FRAME_BYTES {
        return Err(ConnectivityProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, ConnectivityProtocolError> {
    let mut frame = Vec::with_capacity(128);
    let mut terminated = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let consumed = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if frame.len() + consumed > MAX_CONNECTIVITY_FRAME_BYTES {
            return Err(ConnectivityProtocolError::FrameTooLarge);
        }
        terminated = available[consumed - 1] == b'\n';
        frame.extend_from_slice(&available[..consumed]);
        reader.consume(consumed);
        if terminated {
            break;
        }
    }
    if frame.is_empty() {
        return Ok(None);
    }
    if !terminated {
        return Err(ConnectivityProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use super::*;

    #[test]
    fn round_trips_state_and_toggle_requests() {
        for request in [
            ConnectivityRequest::get_state(1),
            ConnectivityRequest::set_wifi_enabled(2, true),
            ConnectivityRequest::set_airplane_mode(3, false),
        ] {
            let mut encoded = Vec::new();
            write_request(&mut encoded, &request).unwrap();
            assert_eq!(
                read_request(&mut BufReader::new(Cursor::new(encoded))).unwrap(),
                Some(request)
            );
        }
    }

    #[test]
    fn rejects_inconsistent_state() {
        let state = ConnectivityState {
            available: true,
            wifi_available: true,
            wifi_enabled: true,
            airplane_mode: true,
        };
        assert!(matches!(
            state.validate(),
            Err(ConnectivityProtocolError::InvalidState)
        ));
    }

    #[test]
    fn rejects_unknown_fields() {
        let document = b"{\"protocol_version\":1,\"request_id\":1,\"command\":{\"name\":\"get-state\",\"extra\":true}}\n";
        assert!(read_request(&mut BufReader::new(&document[..])).is_err());
    }
}
