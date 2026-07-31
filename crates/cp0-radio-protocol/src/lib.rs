use std::fmt;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

pub const RADIO_PROTOCOL_VERSION: u32 = 1;
pub const MAX_LORA_PAYLOAD_BYTES: usize = 64;
pub const MAX_LORA_RECEIVE_TIMEOUT_MS: u16 = 1000;
pub const MAX_RADIO_FRAME_BYTES: usize = 2 * 1024;
pub const MAX_RADIO_ERROR_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: RadioCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RadioCommand {
    SendLora { payload_base64: String },
    ReceiveLora { timeout_ms: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadioResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: RadioOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RadioOutcome {
    LoraSent {
        bytes: u8,
    },
    LoraPacket {
        payload_base64: String,
        rssi_dbm: i16,
        snr_quarter_db: i8,
    },
    LoraNoPacket,
    Error {
        code: RadioErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RadioErrorCode {
    InvalidRequest,
    Unauthorized,
    Disabled,
    Busy,
    RateLimited,
    Unavailable,
    Device,
    Internal,
}

#[derive(Debug)]
pub enum RadioProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidPayload,
    InvalidTimeout,
    InvalidMetadata,
    InvalidErrorMessage,
}

impl fmt::Display for RadioProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "radio protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid radio protocol JSON: {error}"),
            Self::FrameTooLarge => write!(
                formatter,
                "radio protocol frame exceeds {MAX_RADIO_FRAME_BYTES} bytes"
            ),
            Self::UnterminatedFrame => {
                formatter.write_str("radio protocol frame is not newline terminated")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported radio protocol version {version}")
            }
            Self::InvalidPayload => {
                formatter.write_str("LoRa payload is not bounded canonical base64")
            }
            Self::InvalidTimeout => {
                formatter.write_str("LoRa receive timeout is outside the supported range")
            }
            Self::InvalidMetadata => {
                formatter.write_str("LoRa response metadata is outside the supported range")
            }
            Self::InvalidErrorMessage => {
                formatter.write_str("radio error message is empty, too long or contains controls")
            }
        }
    }
}

impl std::error::Error for RadioProtocolError {}

impl From<io::Error> for RadioProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RadioProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl RadioRequest {
    pub fn send_lora(request_id: u64, payload: &[u8]) -> Self {
        Self {
            protocol_version: RADIO_PROTOCOL_VERSION,
            request_id,
            command: RadioCommand::SendLora {
                payload_base64: encode_base64(payload),
            },
        }
    }

    pub const fn receive_lora(request_id: u64, timeout_ms: u16) -> Self {
        Self {
            protocol_version: RADIO_PROTOCOL_VERSION,
            request_id,
            command: RadioCommand::ReceiveLora { timeout_ms },
        }
    }

    pub fn validate(&self) -> Result<(), RadioProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.command {
            RadioCommand::SendLora { payload_base64 } => decode_payload(payload_base64).map(|_| ()),
            RadioCommand::ReceiveLora { timeout_ms }
                if *timeout_ms == 0 || *timeout_ms > MAX_LORA_RECEIVE_TIMEOUT_MS =>
            {
                Err(RadioProtocolError::InvalidTimeout)
            }
            RadioCommand::ReceiveLora { .. } => Ok(()),
        }
    }
}

impl RadioResponse {
    pub const fn lora_sent(request_id: u64, bytes: u8) -> Self {
        Self {
            protocol_version: RADIO_PROTOCOL_VERSION,
            request_id,
            outcome: RadioOutcome::LoraSent { bytes },
        }
    }

    pub fn lora_packet(request_id: u64, payload: &[u8], rssi_dbm: i16, snr_quarter_db: i8) -> Self {
        Self {
            protocol_version: RADIO_PROTOCOL_VERSION,
            request_id,
            outcome: RadioOutcome::LoraPacket {
                payload_base64: encode_base64(payload),
                rssi_dbm,
                snr_quarter_db,
            },
        }
    }

    pub const fn no_lora_packet(request_id: u64) -> Self {
        Self {
            protocol_version: RADIO_PROTOCOL_VERSION,
            request_id,
            outcome: RadioOutcome::LoraNoPacket,
        }
    }

    pub fn error(request_id: u64, code: RadioErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: RADIO_PROTOCOL_VERSION,
            request_id,
            outcome: RadioOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), RadioProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.outcome {
            RadioOutcome::LoraSent { bytes }
                if *bytes == 0 || usize::from(*bytes) > MAX_LORA_PAYLOAD_BYTES =>
            {
                Err(RadioProtocolError::InvalidPayload)
            }
            RadioOutcome::LoraPacket {
                payload_base64,
                rssi_dbm,
                ..
            } => {
                decode_payload(payload_base64)?;
                if !(-200..=50).contains(rssi_dbm) {
                    return Err(RadioProtocolError::InvalidMetadata);
                }
                Ok(())
            }
            RadioOutcome::Error { message, .. }
                if message.is_empty()
                    || message.chars().count() > MAX_RADIO_ERROR_CHARS
                    || message.chars().any(char::is_control) =>
            {
                Err(RadioProtocolError::InvalidErrorMessage)
            }
            _ => Ok(()),
        }
    }
}

pub fn write_request(
    writer: &mut impl Write,
    request: &RadioRequest,
) -> Result<(), RadioProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn read_request(reader: &mut impl BufRead) -> Result<Option<RadioRequest>, RadioProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: RadioRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn write_response(
    writer: &mut impl Write,
    response: &RadioResponse,
) -> Result<(), RadioProtocolError> {
    response.validate()?;
    write_frame(writer, response)
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<RadioResponse>, RadioProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: RadioResponse = serde_json::from_slice(&frame)?;
    response.validate()?;
    Ok(Some(response))
}

pub fn encode_base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[usize::from(first >> 2)] as char);
        output.push(ALPHABET[usize::from((first & 0x03) << 4 | second >> 4)] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[usize::from((second & 0x0f) << 2 | third >> 6)] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[usize::from(third & 0x3f)] as char
        } else {
            '='
        });
    }
    output
}

pub fn decode_payload(input: &str) -> Result<Vec<u8>, RadioProtocolError> {
    let encoded = input.as_bytes();
    if encoded.is_empty()
        || encoded.len() % 4 != 0
        || encoded.len() > MAX_LORA_PAYLOAD_BYTES.div_ceil(3) * 4
    {
        return Err(RadioProtocolError::InvalidPayload);
    }
    let mut output = Vec::with_capacity(encoded.len() / 4 * 3);
    for (index, chunk) in encoded.chunks_exact(4).enumerate() {
        let last = index + 1 == encoded.len() / 4;
        let a = decode_digit(chunk[0])?;
        let b = decode_digit(chunk[1])?;
        let c = if chunk[2] == b'=' {
            if !last || chunk[3] != b'=' || b & 0x0f != 0 {
                return Err(RadioProtocolError::InvalidPayload);
            }
            None
        } else {
            Some(decode_digit(chunk[2])?)
        };
        let d = if chunk[3] == b'=' {
            if !last || c.is_some_and(|value| value & 0x03 != 0) {
                return Err(RadioProtocolError::InvalidPayload);
            }
            None
        } else {
            if c.is_none() {
                return Err(RadioProtocolError::InvalidPayload);
            }
            Some(decode_digit(chunk[3])?)
        };
        output.push(a << 2 | b >> 4);
        if let Some(c) = c {
            output.push(b << 4 | c >> 2);
            if let Some(d) = d {
                output.push(c << 6 | d);
            }
        }
    }
    if output.is_empty() || output.len() > MAX_LORA_PAYLOAD_BYTES {
        return Err(RadioProtocolError::InvalidPayload);
    }
    Ok(output)
}

fn decode_digit(byte: u8) -> Result<u8, RadioProtocolError> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(RadioProtocolError::InvalidPayload),
    }
}

fn validate_version(version: u32) -> Result<(), RadioProtocolError> {
    if version == RADIO_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(RadioProtocolError::UnsupportedVersion(version))
    }
}

fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<(), RadioProtocolError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_RADIO_FRAME_BYTES {
        return Err(RadioProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, RadioProtocolError> {
    let mut frame = Vec::with_capacity(256);
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
        if frame.len() + consumed > MAX_RADIO_FRAME_BYTES {
            return Err(RadioProtocolError::FrameTooLarge);
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
        return Err(RadioProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn round_trips_maximum_packet_and_metadata() {
        let payload = vec![0xa5; MAX_LORA_PAYLOAD_BYTES];
        let request = RadioRequest::send_lora(7, &payload);
        let mut frame = Vec::new();
        write_request(&mut frame, &request).unwrap();
        assert_eq!(
            read_request(&mut Cursor::new(frame)).unwrap(),
            Some(request)
        );

        let response = RadioResponse::lora_packet(7, &payload, -91, -7);
        let mut frame = Vec::new();
        write_response(&mut frame, &response).unwrap();
        assert_eq!(
            read_response(&mut Cursor::new(frame)).unwrap(),
            Some(response)
        );
    }

    #[test]
    fn rejects_noncanonical_empty_and_oversized_payloads() {
        for encoded in ["", "A", "====", "AB==", "AAB=", "AA=A"] {
            assert!(decode_payload(encoded).is_err(), "accepted {encoded}");
        }
        assert_eq!(decode_payload("AA==").unwrap(), [0]);
        assert!(decode_payload(&encode_base64(&[0; MAX_LORA_PAYLOAD_BYTES + 1])).is_err());
    }

    #[test]
    fn rejects_invalid_timeout_metadata_and_frame_size() {
        for timeout in [0, MAX_LORA_RECEIVE_TIMEOUT_MS + 1] {
            assert!(RadioRequest::receive_lora(1, timeout).validate().is_err());
        }
        assert!(
            RadioResponse::lora_packet(1, b"x", -201, 0)
                .validate()
                .is_err()
        );
        let mut oversized = vec![b'x'; MAX_RADIO_FRAME_BYTES];
        oversized.push(b'\n');
        assert!(matches!(
            read_request(&mut Cursor::new(oversized)),
            Err(RadioProtocolError::FrameTooLarge)
        ));
    }
}
