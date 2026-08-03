use std::fmt;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

pub const POWER_PROTOCOL_VERSION: u32 = 1;
pub const MAX_POWER_FRAME_BYTES: usize = 1024;
pub const MAX_POWER_ERROR_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PowerAction {
    Restart,
    PowerOff,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: PowerCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PowerCommand {
    Restart {},
    PowerOff {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: PowerOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PowerOutcome {
    Accepted {
        action: PowerAction,
    },
    Error {
        code: PowerErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PowerErrorCode {
    InvalidRequest,
    Unauthorized,
    Unavailable,
    Operation,
}

#[derive(Debug)]
pub enum PowerProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidErrorMessage,
}

impl fmt::Display for PowerProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "power protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid power protocol JSON: {error}"),
            Self::FrameTooLarge => {
                write!(
                    formatter,
                    "power frame exceeds {MAX_POWER_FRAME_BYTES} bytes"
                )
            }
            Self::UnterminatedFrame => formatter.write_str("power frame is not terminated"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported power protocol version {version}")
            }
            Self::InvalidErrorMessage => formatter.write_str("invalid power error message"),
        }
    }
}

impl std::error::Error for PowerProtocolError {}

impl From<io::Error> for PowerProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for PowerProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl PowerRequest {
    pub const fn restart(request_id: u64) -> Self {
        Self {
            protocol_version: POWER_PROTOCOL_VERSION,
            request_id,
            command: PowerCommand::Restart {},
        }
    }

    pub const fn power_off(request_id: u64) -> Self {
        Self {
            protocol_version: POWER_PROTOCOL_VERSION,
            request_id,
            command: PowerCommand::PowerOff {},
        }
    }

    pub fn validate(&self) -> Result<(), PowerProtocolError> {
        validate_version(self.protocol_version)
    }
}

impl PowerResponse {
    pub const fn accepted(request_id: u64, action: PowerAction) -> Self {
        Self {
            protocol_version: POWER_PROTOCOL_VERSION,
            request_id,
            outcome: PowerOutcome::Accepted { action },
        }
    }

    pub fn error(request_id: u64, code: PowerErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: POWER_PROTOCOL_VERSION,
            request_id,
            outcome: PowerOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), PowerProtocolError> {
        validate_version(self.protocol_version)?;
        if let PowerOutcome::Error { message, .. } = &self.outcome {
            if message.is_empty()
                || message.chars().count() > MAX_POWER_ERROR_CHARS
                || message.chars().any(char::is_control)
            {
                return Err(PowerProtocolError::InvalidErrorMessage);
            }
        }
        Ok(())
    }
}

pub fn read_request(reader: &mut impl BufRead) -> Result<Option<PowerRequest>, PowerProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: PowerRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn write_request(
    writer: &mut impl Write,
    request: &PowerRequest,
) -> Result<(), PowerProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<PowerResponse>, PowerProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: PowerResponse = serde_json::from_slice(&frame)?;
    response.validate()?;
    Ok(Some(response))
}

pub fn write_response(
    writer: &mut impl Write,
    response: &PowerResponse,
) -> Result<(), PowerProtocolError> {
    response.validate()?;
    write_frame(writer, response)
}

fn validate_version(version: u32) -> Result<(), PowerProtocolError> {
    if version == POWER_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(PowerProtocolError::UnsupportedVersion(version))
    }
}

fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<(), PowerProtocolError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_POWER_FRAME_BYTES {
        return Err(PowerProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, PowerProtocolError> {
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
        if frame.len() + consumed > MAX_POWER_FRAME_BYTES {
            return Err(PowerProtocolError::FrameTooLarge);
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
        return Err(PowerProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use super::*;

    #[test]
    fn round_trips_only_fixed_power_actions() {
        for request in [PowerRequest::restart(7), PowerRequest::power_off(8)] {
            let mut encoded = Vec::new();
            write_request(&mut encoded, &request).unwrap();
            assert_eq!(
                read_request(&mut BufReader::new(Cursor::new(encoded))).unwrap(),
                Some(request)
            );
        }
    }

    #[test]
    fn rejects_unknown_fields_versions_and_unbounded_frames() {
        let unknown = b"{\"protocol_version\":1,\"request_id\":1,\"command\":{\"name\":\"restart\",\"extra\":true}}\n";
        assert!(read_request(&mut BufReader::new(&unknown[..])).is_err());

        let wrong_version =
            b"{\"protocol_version\":2,\"request_id\":1,\"command\":{\"name\":\"restart\"}}\n";
        assert!(matches!(
            read_request(&mut BufReader::new(&wrong_version[..])),
            Err(PowerProtocolError::UnsupportedVersion(2))
        ));

        let oversized = vec![b'x'; MAX_POWER_FRAME_BYTES + 1];
        assert!(matches!(
            read_request(&mut BufReader::new(Cursor::new(oversized))),
            Err(PowerProtocolError::FrameTooLarge)
        ));
    }

    #[test]
    fn validates_error_messages() {
        assert!(
            PowerResponse::error(1, PowerErrorCode::Operation, "")
                .validate()
                .is_err()
        );
        let response = PowerResponse::accepted(2, PowerAction::PowerOff);
        let mut encoded = Vec::new();
        write_response(&mut encoded, &response).unwrap();
        assert_eq!(
            read_response(&mut BufReader::new(Cursor::new(encoded))).unwrap(),
            Some(response)
        );
    }
}
