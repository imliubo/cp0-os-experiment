use std::fmt;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

pub const GPIO_PROTOCOL_VERSION: u32 = 1;
pub const MAX_GPIO_FRAME_BYTES: usize = 2 * 1024;
pub const MAX_GPIO_ERROR_CHARS: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GpioLine {
    GroveFunction,
    ExternalUsbFunction,
    Grove5vPower,
    External5vPower,
}

impl GpioLine {
    pub const ALL: [Self; 4] = [
        Self::GroveFunction,
        Self::ExternalUsbFunction,
        Self::Grove5vPower,
        Self::External5vPower,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpioRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: GpioCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum GpioCommand {
    Read { line: GpioLine },
    Write { line: GpioLine, value: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpioResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: GpioOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum GpioOutcome {
    Value {
        line: GpioLine,
        value: bool,
    },
    Written {
        line: GpioLine,
        value: bool,
    },
    Error {
        code: GpioErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GpioErrorCode {
    InvalidRequest,
    Unauthorized,
    Unavailable,
    Device,
    Internal,
}

#[derive(Debug)]
pub enum GpioProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidErrorMessage,
}

impl fmt::Display for GpioProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "GPIO protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid GPIO protocol JSON: {error}"),
            Self::FrameTooLarge => write!(
                formatter,
                "GPIO protocol frame exceeds {MAX_GPIO_FRAME_BYTES} bytes"
            ),
            Self::UnterminatedFrame => {
                formatter.write_str("GPIO protocol frame is not newline terminated")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported GPIO protocol version {version}")
            }
            Self::InvalidErrorMessage => {
                formatter.write_str("GPIO error message is empty, too long or contains controls")
            }
        }
    }
}

impl std::error::Error for GpioProtocolError {}

impl From<io::Error> for GpioProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for GpioProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl GpioRequest {
    pub const fn read(request_id: u64, line: GpioLine) -> Self {
        Self {
            protocol_version: GPIO_PROTOCOL_VERSION,
            request_id,
            command: GpioCommand::Read { line },
        }
    }

    pub const fn write(request_id: u64, line: GpioLine, value: bool) -> Self {
        Self {
            protocol_version: GPIO_PROTOCOL_VERSION,
            request_id,
            command: GpioCommand::Write { line, value },
        }
    }

    pub fn validate(&self) -> Result<(), GpioProtocolError> {
        validate_version(self.protocol_version)
    }
}

impl GpioResponse {
    pub const fn value(request_id: u64, line: GpioLine, value: bool) -> Self {
        Self {
            protocol_version: GPIO_PROTOCOL_VERSION,
            request_id,
            outcome: GpioOutcome::Value { line, value },
        }
    }

    pub const fn written(request_id: u64, line: GpioLine, value: bool) -> Self {
        Self {
            protocol_version: GPIO_PROTOCOL_VERSION,
            request_id,
            outcome: GpioOutcome::Written { line, value },
        }
    }

    pub fn error(request_id: u64, code: GpioErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: GPIO_PROTOCOL_VERSION,
            request_id,
            outcome: GpioOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), GpioProtocolError> {
        validate_version(self.protocol_version)?;
        if let GpioOutcome::Error { message, .. } = &self.outcome
            && (message.is_empty()
                || message.chars().count() > MAX_GPIO_ERROR_CHARS
                || message.chars().any(char::is_control))
        {
            return Err(GpioProtocolError::InvalidErrorMessage);
        }
        Ok(())
    }
}

pub fn write_request(
    writer: &mut impl Write,
    request: &GpioRequest,
) -> Result<(), GpioProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn read_request(reader: &mut impl BufRead) -> Result<Option<GpioRequest>, GpioProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: GpioRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn write_response(
    writer: &mut impl Write,
    response: &GpioResponse,
) -> Result<(), GpioProtocolError> {
    response.validate()?;
    write_frame(writer, response)
}

pub fn read_response(reader: &mut impl BufRead) -> Result<Option<GpioResponse>, GpioProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: GpioResponse = serde_json::from_slice(&frame)?;
    response.validate()?;
    Ok(Some(response))
}

fn validate_version(version: u32) -> Result<(), GpioProtocolError> {
    if version == GPIO_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(GpioProtocolError::UnsupportedVersion(version))
    }
}

fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<(), GpioProtocolError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_GPIO_FRAME_BYTES {
        return Err(GpioProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, GpioProtocolError> {
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
        if frame.len() + consumed > MAX_GPIO_FRAME_BYTES {
            return Err(GpioProtocolError::FrameTooLarge);
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
        return Err(GpioProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn round_trips_only_fixed_logical_lines() {
        for (index, line) in GpioLine::ALL.into_iter().enumerate() {
            let request = GpioRequest::write(index as u64 + 1, line, index % 2 == 0);
            let mut encoded = Vec::new();
            write_request(&mut encoded, &request).unwrap();
            assert_eq!(
                read_request(&mut Cursor::new(encoded)).unwrap(),
                Some(request)
            );
        }
    }

    #[test]
    fn rejects_unknown_lines_fields_and_oversized_frames() {
        for frame in [
            b"{\"protocol_version\":1,\"request_id\":1,\"command\":{\"name\":\"read\",\"line\":\"gpio22\"}}\n".as_slice(),
            b"{\"protocol_version\":1,\"request_id\":1,\"command\":{\"name\":\"read\",\"line\":\"grove-function\",\"path\":\"/tmp/x\"}}\n".as_slice(),
        ] {
            assert!(read_request(&mut Cursor::new(frame)).is_err());
        }
        let mut oversized = vec![b'x'; MAX_GPIO_FRAME_BYTES];
        oversized.push(b'\n');
        assert!(matches!(
            read_request(&mut Cursor::new(oversized)),
            Err(GpioProtocolError::FrameTooLarge)
        ));
    }
}
