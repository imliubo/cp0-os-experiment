use std::fmt;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

pub const USB_MEDIA_PROTOCOL_VERSION: u32 = 1;
pub const MAX_USB_MEDIA_FRAME_BYTES: usize = 4096;
pub const MAX_USB_MEDIA_ERROR_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UsbMediaState {
    Off,
    Preparing,
    Connected,
    Importing,
    Complete,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsbMediaStatus {
    pub state: UsbMediaState,
    pub exported_photos: u32,
    pub imported_music: u32,
    pub rejected_music: u32,
    pub capacity_bytes: u64,
}

impl Default for UsbMediaStatus {
    fn default() -> Self {
        Self {
            state: UsbMediaState::Off,
            exported_photos: 0,
            imported_music: 0,
            rejected_music: 0,
            capacity_bytes: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsbMediaRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: UsbMediaCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum UsbMediaCommand {
    GetStatus {},
    Start {},
    Stop {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsbMediaResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: UsbMediaOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum UsbMediaOutcome {
    State {
        state: UsbMediaStatus,
    },
    Error {
        code: UsbMediaErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UsbMediaErrorCode {
    InvalidRequest,
    Unauthorized,
    InvalidState,
    Unavailable,
    Storage,
    Filesystem,
    Gadget,
    Internal,
}

#[derive(Debug)]
pub enum UsbMediaProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidStatus,
    InvalidError,
}

impl fmt::Display for UsbMediaProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "USB media protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid USB media JSON: {error}"),
            Self::FrameTooLarge => write!(
                formatter,
                "USB media frame exceeds {MAX_USB_MEDIA_FRAME_BYTES} bytes"
            ),
            Self::UnterminatedFrame => {
                formatter.write_str("USB media frame is not newline terminated")
            }
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "unsupported USB media protocol version {version}"
                )
            }
            Self::InvalidStatus => formatter.write_str("invalid USB media status"),
            Self::InvalidError => formatter.write_str("invalid USB media error response"),
        }
    }
}

impl std::error::Error for UsbMediaProtocolError {}

impl From<io::Error> for UsbMediaProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for UsbMediaProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl UsbMediaRequest {
    pub const fn new(request_id: u64, command: UsbMediaCommand) -> Self {
        Self {
            protocol_version: USB_MEDIA_PROTOCOL_VERSION,
            request_id,
            command,
        }
    }

    pub fn validate(&self) -> Result<(), UsbMediaProtocolError> {
        validate_version(self.protocol_version)
    }
}

impl UsbMediaStatus {
    pub fn validate(&self) -> bool {
        self.capacity_bytes > 0
            || (self.state == UsbMediaState::Off
                && self.exported_photos == 0
                && self.imported_music == 0
                && self.rejected_music == 0)
    }
}

impl UsbMediaResponse {
    pub fn state(request_id: u64, state: UsbMediaStatus) -> Self {
        Self {
            protocol_version: USB_MEDIA_PROTOCOL_VERSION,
            request_id,
            outcome: UsbMediaOutcome::State { state },
        }
    }

    pub fn error(request_id: u64, code: UsbMediaErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: USB_MEDIA_PROTOCOL_VERSION,
            request_id,
            outcome: UsbMediaOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), UsbMediaProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.outcome {
            UsbMediaOutcome::State { state } if state.validate() => Ok(()),
            UsbMediaOutcome::State { .. } => Err(UsbMediaProtocolError::InvalidStatus),
            UsbMediaOutcome::Error { message, .. }
                if !message.is_empty()
                    && message.chars().count() <= MAX_USB_MEDIA_ERROR_CHARS
                    && !message.chars().any(char::is_control) =>
            {
                Ok(())
            }
            UsbMediaOutcome::Error { .. } => Err(UsbMediaProtocolError::InvalidError),
        }
    }
}

pub fn read_request(
    reader: &mut impl BufRead,
) -> Result<Option<UsbMediaRequest>, UsbMediaProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: UsbMediaRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<UsbMediaResponse>, UsbMediaProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: UsbMediaResponse = serde_json::from_slice(&frame)?;
    response.validate()?;
    Ok(Some(response))
}

pub fn write_request(
    writer: &mut impl Write,
    request: &UsbMediaRequest,
) -> Result<(), UsbMediaProtocolError> {
    request.validate()?;
    write_value(writer, request)
}

pub fn write_response(
    writer: &mut impl Write,
    response: &UsbMediaResponse,
) -> Result<(), UsbMediaProtocolError> {
    response.validate()?;
    write_value(writer, response)
}

fn validate_version(version: u32) -> Result<(), UsbMediaProtocolError> {
    if version == USB_MEDIA_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(UsbMediaProtocolError::UnsupportedVersion(version))
    }
}

fn write_value(
    writer: &mut impl Write,
    value: &impl Serialize,
) -> Result<(), UsbMediaProtocolError> {
    let mut frame = serde_json::to_vec(value)?;
    if frame.len() + 1 > MAX_USB_MEDIA_FRAME_BYTES {
        return Err(UsbMediaProtocolError::FrameTooLarge);
    }
    frame.push(b'\n');
    writer.write_all(&frame)?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, UsbMediaProtocolError> {
    let mut frame = Vec::with_capacity(256);
    let read = reader.read_until(b'\n', &mut frame)?;
    if read == 0 {
        return Ok(None);
    }
    if frame.len() > MAX_USB_MEDIA_FRAME_BYTES {
        return Err(UsbMediaProtocolError::FrameTooLarge);
    }
    if frame.last() != Some(&b'\n') {
        return Err(UsbMediaProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trips_path_free_commands() {
        for command in [
            UsbMediaCommand::GetStatus {},
            UsbMediaCommand::Start {},
            UsbMediaCommand::Stop {},
        ] {
            let request = UsbMediaRequest::new(7, command);
            let mut encoded = Vec::new();
            write_request(&mut encoded, &request).unwrap();
            let decoded = read_request(&mut Cursor::new(encoded)).unwrap().unwrap();
            assert_eq!(decoded, request);
        }
    }

    #[test]
    fn rejects_unknown_fields_that_could_smuggle_a_backing_path() {
        let frame = b"{\"protocol_version\":1,\"request_id\":1,\"command\":{\"name\":\"start\",\"path\":\"/dev/mmcblk0p3\"}}\n";
        assert!(read_request(&mut Cursor::new(frame)).is_err());
    }

    #[test]
    fn validates_bounded_status_and_errors() {
        let status = UsbMediaStatus {
            state: UsbMediaState::Connected,
            exported_photos: 12,
            imported_music: 0,
            rejected_music: 0,
            capacity_bytes: 512 * 1024 * 1024,
        };
        assert!(UsbMediaResponse::state(1, status).validate().is_ok());
        assert!(
            UsbMediaResponse::error(1, UsbMediaErrorCode::Internal, "x".repeat(161))
                .validate()
                .is_err()
        );
    }
}
