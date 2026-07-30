use std::fmt;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

pub const NETWORK_PROTOCOL_VERSION: u32 = 1;
pub const MAX_NETWORK_FRAME_BYTES: usize = 4 * 1024;
pub const MAX_NETWORK_URL_BYTES: usize = 1024;
pub const MAX_NETWORK_BODY_BYTES: usize = 2 * 1024;
pub const MAX_NETWORK_ERROR_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: NetworkCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum NetworkCommand {
    HttpGet { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: NetworkOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum NetworkOutcome {
    Ok {
        status_code: u16,
        body_base64: String,
    },
    Error {
        code: NetworkErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkErrorCode {
    InvalidRequest,
    Unauthorized,
    BlockedAddress,
    Unavailable,
    Timeout,
    Tls,
    TooManyRedirects,
    ResponseTooLarge,
    Internal,
}

#[derive(Debug)]
pub enum NetworkProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidUrl,
    InvalidStatusCode,
    InvalidBodyEncoding,
    InvalidErrorMessage,
}

impl fmt::Display for NetworkProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "network protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid network protocol JSON: {error}"),
            Self::FrameTooLarge => write!(
                formatter,
                "network protocol frame exceeds {MAX_NETWORK_FRAME_BYTES} bytes"
            ),
            Self::UnterminatedFrame => {
                formatter.write_str("network protocol frame is not newline terminated")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported network protocol version {version}")
            }
            Self::InvalidUrl => formatter
                .write_str("network URL must be a bounded HTTPS URL without control characters"),
            Self::InvalidStatusCode => formatter.write_str("invalid HTTP response status code"),
            Self::InvalidBodyEncoding => formatter.write_str(
                "network response body is not canonical base64 or exceeds the body limit",
            ),
            Self::InvalidErrorMessage => {
                formatter.write_str("network error message is empty, too long or contains controls")
            }
        }
    }
}

impl std::error::Error for NetworkProtocolError {}

impl From<io::Error> for NetworkProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for NetworkProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl NetworkRequest {
    pub fn http_get(request_id: u64, url: impl Into<String>) -> Self {
        Self {
            protocol_version: NETWORK_PROTOCOL_VERSION,
            request_id,
            command: NetworkCommand::HttpGet { url: url.into() },
        }
    }

    pub fn validate(&self) -> Result<(), NetworkProtocolError> {
        if self.protocol_version != NETWORK_PROTOCOL_VERSION {
            return Err(NetworkProtocolError::UnsupportedVersion(
                self.protocol_version,
            ));
        }
        match &self.command {
            NetworkCommand::HttpGet { url } if !is_valid_https_url(url) => {
                Err(NetworkProtocolError::InvalidUrl)
            }
            _ => Ok(()),
        }
    }
}

impl NetworkResponse {
    pub fn success(request_id: u64, status_code: u16, body: &[u8]) -> Self {
        debug_assert!((100..=599).contains(&status_code));
        debug_assert!(body.len() <= MAX_NETWORK_BODY_BYTES);
        Self {
            protocol_version: NETWORK_PROTOCOL_VERSION,
            request_id,
            outcome: NetworkOutcome::Ok {
                status_code,
                body_base64: encode_base64(body),
            },
        }
    }

    pub fn error(request_id: u64, code: NetworkErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: NETWORK_PROTOCOL_VERSION,
            request_id,
            outcome: NetworkOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), NetworkProtocolError> {
        if self.protocol_version != NETWORK_PROTOCOL_VERSION {
            return Err(NetworkProtocolError::UnsupportedVersion(
                self.protocol_version,
            ));
        }
        match &self.outcome {
            NetworkOutcome::Ok {
                status_code,
                body_base64,
            } => {
                if !(100..=599).contains(status_code) {
                    return Err(NetworkProtocolError::InvalidStatusCode);
                }
                decode_base64(body_base64).map(|_| ())
            }
            NetworkOutcome::Error { message, .. }
                if message.is_empty()
                    || message.chars().count() > MAX_NETWORK_ERROR_CHARS
                    || message.chars().any(char::is_control) =>
            {
                Err(NetworkProtocolError::InvalidErrorMessage)
            }
            NetworkOutcome::Error { .. } => Ok(()),
        }
    }
}

pub fn is_valid_https_url(url: &str) -> bool {
    url.starts_with("https://")
        && url.len() <= MAX_NETWORK_URL_BYTES
        && url.len() > "https://".len()
        && !url.chars().any(char::is_control)
}

pub fn write_request(
    writer: &mut impl Write,
    request: &NetworkRequest,
) -> Result<(), NetworkProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn read_request(
    reader: &mut impl BufRead,
) -> Result<Option<NetworkRequest>, NetworkProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: NetworkRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn write_response(
    writer: &mut impl Write,
    response: &NetworkResponse,
) -> Result<(), NetworkProtocolError> {
    response.validate()?;
    write_frame(writer, response)
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<NetworkResponse>, NetworkProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: NetworkResponse = serde_json::from_slice(&frame)?;
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
        if chunk.len() > 1 {
            output.push(ALPHABET[usize::from((second & 0x0f) << 2 | third >> 6)] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[usize::from(third & 0x3f)] as char);
        } else {
            output.push('=');
        }
    }
    output
}

pub fn decode_base64(input: &str) -> Result<Vec<u8>, NetworkProtocolError> {
    let encoded = input.as_bytes();
    if encoded.len() % 4 != 0 || encoded.len() > MAX_NETWORK_BODY_BYTES.div_ceil(3) * 4 {
        return Err(NetworkProtocolError::InvalidBodyEncoding);
    }
    let mut output = Vec::with_capacity(encoded.len() / 4 * 3);
    for (index, chunk) in encoded.chunks_exact(4).enumerate() {
        let last = index + 1 == encoded.len() / 4;
        let a = decode_digit(chunk[0])?;
        let b = decode_digit(chunk[1])?;
        let c = if chunk[2] == b'=' {
            if !last || chunk[3] != b'=' || b & 0x0f != 0 {
                return Err(NetworkProtocolError::InvalidBodyEncoding);
            }
            None
        } else {
            Some(decode_digit(chunk[2])?)
        };
        let d = if chunk[3] == b'=' {
            if !last || c.is_some_and(|value| value & 0x03 != 0) {
                return Err(NetworkProtocolError::InvalidBodyEncoding);
            }
            None
        } else {
            if c.is_none() {
                return Err(NetworkProtocolError::InvalidBodyEncoding);
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
    if output.len() > MAX_NETWORK_BODY_BYTES {
        return Err(NetworkProtocolError::InvalidBodyEncoding);
    }
    Ok(output)
}

fn decode_digit(byte: u8) -> Result<u8, NetworkProtocolError> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(NetworkProtocolError::InvalidBodyEncoding),
    }
}

fn write_frame(
    writer: &mut impl Write,
    value: &impl Serialize,
) -> Result<(), NetworkProtocolError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_NETWORK_FRAME_BYTES {
        return Err(NetworkProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, NetworkProtocolError> {
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
        if frame.len() + consumed > MAX_NETWORK_FRAME_BYTES {
            return Err(NetworkProtocolError::FrameTooLarge);
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
        return Err(NetworkProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn round_trips_bounded_request_and_binary_response() {
        let request = NetworkRequest::http_get(7, "https://example.com/data?q=1");
        let mut request_frame = Vec::new();
        write_request(&mut request_frame, &request).unwrap();
        assert_eq!(
            read_request(&mut Cursor::new(request_frame)).unwrap(),
            Some(request)
        );

        let body = [0x00, 0x01, 0xfe, 0xff, b'x'];
        let response = NetworkResponse::success(7, 206, &body);
        let mut response_frame = Vec::new();
        write_response(&mut response_frame, &response).unwrap();
        let decoded = read_response(&mut Cursor::new(response_frame))
            .unwrap()
            .unwrap();
        let NetworkOutcome::Ok { body_base64, .. } = decoded.outcome else {
            panic!("expected successful response");
        };
        assert_eq!(decode_base64(&body_base64).unwrap(), body);
    }

    #[test]
    fn rejects_non_https_control_characters_and_oversized_urls() {
        for url in [
            "http://example.com",
            "https://",
            "https://example.com/line\nbreak",
        ] {
            assert!(!is_valid_https_url(url));
        }
        assert!(!is_valid_https_url(&format!(
            "https://example.com/{}",
            "x".repeat(MAX_NETWORK_URL_BYTES)
        )));
    }

    #[test]
    fn rejects_noncanonical_or_oversized_base64() {
        for encoded in ["A", "====", "AB==", "AAB="] {
            assert!(decode_base64(encoded).is_err(), "accepted {encoded}");
        }
        let oversized = encode_base64(&vec![0; MAX_NETWORK_BODY_BYTES + 1]);
        assert!(decode_base64(&oversized).is_err());
    }

    #[test]
    fn enforces_frame_and_response_bounds() {
        let mut oversized = vec![b'x'; MAX_NETWORK_FRAME_BYTES];
        oversized.push(b'\n');
        assert!(matches!(
            read_request(&mut Cursor::new(oversized)),
            Err(NetworkProtocolError::FrameTooLarge)
        ));

        let invalid = NetworkResponse {
            protocol_version: NETWORK_PROTOCOL_VERSION,
            request_id: 1,
            outcome: NetworkOutcome::Ok {
                status_code: 99,
                body_base64: String::new(),
            },
        };
        assert!(matches!(
            invalid.validate(),
            Err(NetworkProtocolError::InvalidStatusCode)
        ));
    }
}
