use std::fmt;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

pub const DISPLAY_PROTOCOL_VERSION: u32 = 1;
pub const DISPLAY_BRIGHTNESS_MIN_PERCENT: u8 = 5;
pub const DISPLAY_BRIGHTNESS_MAX_PERCENT: u8 = 100;
pub const DISPLAY_BRIGHTNESS_STEP_PERCENT: u8 = 10;
pub const MAX_DISPLAY_FRAME_BYTES: usize = 2 * 1024;
pub const MAX_DISPLAY_ERROR_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DisplayDirection {
    Decrease,
    Increase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: DisplayCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DisplayCommand {
    GetState {},
    SetBrightness { percent: u8 },
    AdjustBrightness { direction: DisplayDirection },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayState {
    pub available: bool,
    pub brightness_percent: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: DisplayOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DisplayOutcome {
    State {
        state: DisplayState,
    },
    Error {
        code: DisplayErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DisplayErrorCode {
    InvalidRequest,
    Unauthorized,
    Unavailable,
    Device,
    Internal,
}

#[derive(Debug)]
pub enum DisplayProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidBrightness(u8),
    InvalidState,
    InvalidErrorMessage,
}

impl fmt::Display for DisplayProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "display protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid display protocol JSON: {error}"),
            Self::FrameTooLarge => write!(
                formatter,
                "display protocol frame exceeds {MAX_DISPLAY_FRAME_BYTES} bytes"
            ),
            Self::UnterminatedFrame => {
                formatter.write_str("display protocol frame is not newline terminated")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported display protocol version {version}")
            }
            Self::InvalidBrightness(percent) => {
                write!(formatter, "invalid display brightness percentage {percent}")
            }
            Self::InvalidState => formatter.write_str("display state is inconsistent"),
            Self::InvalidErrorMessage => {
                formatter.write_str("display error message is empty, too long or contains controls")
            }
        }
    }
}

impl std::error::Error for DisplayProtocolError {}

impl From<io::Error> for DisplayProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DisplayProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl DisplayRequest {
    pub const fn get_state(request_id: u64) -> Self {
        Self {
            protocol_version: DISPLAY_PROTOCOL_VERSION,
            request_id,
            command: DisplayCommand::GetState {},
        }
    }

    pub const fn set_brightness(request_id: u64, percent: u8) -> Self {
        Self {
            protocol_version: DISPLAY_PROTOCOL_VERSION,
            request_id,
            command: DisplayCommand::SetBrightness { percent },
        }
    }

    pub const fn adjust_brightness(request_id: u64, direction: DisplayDirection) -> Self {
        Self {
            protocol_version: DISPLAY_PROTOCOL_VERSION,
            request_id,
            command: DisplayCommand::AdjustBrightness { direction },
        }
    }

    pub fn validate(&self) -> Result<(), DisplayProtocolError> {
        validate_version(self.protocol_version)?;
        if let DisplayCommand::SetBrightness { percent } = self.command {
            validate_requested_brightness(percent)?;
        }
        Ok(())
    }
}

impl DisplayState {
    pub const fn unavailable() -> Self {
        Self {
            available: false,
            brightness_percent: None,
        }
    }

    pub const fn available(brightness_percent: u8) -> Self {
        Self {
            available: true,
            brightness_percent: Some(brightness_percent),
        }
    }

    pub fn validate(&self) -> Result<(), DisplayProtocolError> {
        match (self.available, self.brightness_percent) {
            (false, None) => Ok(()),
            (true, Some(percent)) if percent <= DISPLAY_BRIGHTNESS_MAX_PERCENT => Ok(()),
            _ => Err(DisplayProtocolError::InvalidState),
        }
    }
}

impl DisplayResponse {
    pub const fn state(request_id: u64, state: DisplayState) -> Self {
        Self {
            protocol_version: DISPLAY_PROTOCOL_VERSION,
            request_id,
            outcome: DisplayOutcome::State { state },
        }
    }

    pub fn error(request_id: u64, code: DisplayErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: DISPLAY_PROTOCOL_VERSION,
            request_id,
            outcome: DisplayOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), DisplayProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.outcome {
            DisplayOutcome::State { state } => state.validate(),
            DisplayOutcome::Error { message, .. } => {
                if message.is_empty()
                    || message.chars().count() > MAX_DISPLAY_ERROR_CHARS
                    || message.chars().any(char::is_control)
                {
                    Err(DisplayProtocolError::InvalidErrorMessage)
                } else {
                    Ok(())
                }
            }
        }
    }
}

pub fn write_request(
    writer: &mut impl Write,
    request: &DisplayRequest,
) -> Result<(), DisplayProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn read_request(
    reader: &mut impl BufRead,
) -> Result<Option<DisplayRequest>, DisplayProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: DisplayRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn write_response(
    writer: &mut impl Write,
    response: &DisplayResponse,
) -> Result<(), DisplayProtocolError> {
    response.validate()?;
    write_frame(writer, response)
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<DisplayResponse>, DisplayProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: DisplayResponse = serde_json::from_slice(&frame)?;
    response.validate()?;
    Ok(Some(response))
}

fn validate_version(version: u32) -> Result<(), DisplayProtocolError> {
    if version == DISPLAY_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(DisplayProtocolError::UnsupportedVersion(version))
    }
}

fn validate_requested_brightness(percent: u8) -> Result<(), DisplayProtocolError> {
    if (DISPLAY_BRIGHTNESS_MIN_PERCENT..=DISPLAY_BRIGHTNESS_MAX_PERCENT).contains(&percent) {
        Ok(())
    } else {
        Err(DisplayProtocolError::InvalidBrightness(percent))
    }
}

fn write_frame(
    writer: &mut impl Write,
    value: &impl Serialize,
) -> Result<(), DisplayProtocolError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_DISPLAY_FRAME_BYTES {
        return Err(DisplayProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, DisplayProtocolError> {
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
        if frame.len() + consumed > MAX_DISPLAY_FRAME_BYTES {
            return Err(DisplayProtocolError::FrameTooLarge);
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
        return Err(DisplayProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use super::*;

    #[test]
    fn round_trips_bounded_requests_and_states() {
        let requests = [
            DisplayRequest::get_state(1),
            DisplayRequest::set_brightness(2, 75),
            DisplayRequest::adjust_brightness(3, DisplayDirection::Decrease),
        ];
        for request in requests {
            let mut encoded = Vec::new();
            write_request(&mut encoded, &request).unwrap();
            let decoded = read_request(&mut BufReader::new(Cursor::new(encoded)))
                .unwrap()
                .unwrap();
            assert_eq!(decoded, request);
        }

        let response = DisplayResponse::state(4, DisplayState::available(80));
        let mut encoded = Vec::new();
        write_response(&mut encoded, &response).unwrap();
        assert_eq!(
            read_response(&mut BufReader::new(Cursor::new(encoded)))
                .unwrap()
                .unwrap(),
            response
        );
    }

    #[test]
    fn rejects_unsafe_brightness_and_inconsistent_state() {
        assert!(DisplayRequest::set_brightness(1, 0).validate().is_err());
        assert!(DisplayRequest::set_brightness(1, 4).validate().is_err());
        assert!(DisplayRequest::set_brightness(1, 101).validate().is_err());
        assert!(DisplayRequest::set_brightness(1, 5).validate().is_ok());
        assert!(DisplayRequest::set_brightness(1, 100).validate().is_ok());
        assert!(
            DisplayState {
                available: false,
                brightness_percent: Some(50),
            }
            .validate()
            .is_err()
        );
        assert!(DisplayState::available(101).validate().is_err());
    }

    #[test]
    fn rejects_unknown_fields_versions_and_unterminated_frames() {
        let unknown = b"{\"protocol_version\":1,\"request_id\":1,\"command\":{\"name\":\"get-state\",\"extra\":true}}\n";
        assert!(
            read_request(&mut BufReader::new(Cursor::new(unknown)))
                .unwrap_err()
                .to_string()
                .contains("invalid display protocol JSON")
        );
        let version =
            b"{\"protocol_version\":2,\"request_id\":1,\"command\":{\"name\":\"get-state\"}}\n";
        assert!(matches!(
            read_request(&mut BufReader::new(Cursor::new(version))),
            Err(DisplayProtocolError::UnsupportedVersion(2))
        ));
        let unterminated = b"{\"protocol_version\":1}";
        assert!(matches!(
            read_request(&mut BufReader::new(Cursor::new(unterminated))),
            Err(DisplayProtocolError::UnterminatedFrame)
        ));
    }
}
