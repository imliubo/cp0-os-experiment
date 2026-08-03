use std::fmt;
use std::io::{self, BufRead, Write};

use cp0_appd::AppdResponse;
use serde::{Deserialize, Serialize};

pub const DEVELOPER_PROTOCOL_VERSION: u32 = 1;
pub const MAX_DEVELOPER_FRAME_BYTES: usize = 4096;
pub const MAX_HOST_LABEL_BYTES: usize = 32;
pub const MAX_SSH_PUBLIC_KEY_BYTES: usize = 512;
pub const MAX_PAIRED_HOSTS: usize = 8;
pub const MAX_PAIRING_WINDOW_SECONDS: u16 = 600;
pub const DEFAULT_DEVELOPER_SOCKET: &str = "/run/cardputerzero-devd/control.sock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeveloperRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: DeveloperCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DeveloperCommand {
    Pair {
        host_label: String,
        ssh_public_key: String,
        developer_public_key: String,
    },
    Install {
        package_bytes: u64,
        package_sha256: String,
    },
    Logs {
        app_id: String,
        limit: u16,
    },
    Start {
        app_id: String,
    },
    Stop {
        app_id: String,
    },
    Uninstall {
        app_id: String,
    },
    OpenPairing {
        duration_seconds: u16,
    },
    ListPaired,
    Unpair {
        host_fingerprint: String,
    },
    UnpairAll,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedHostSummary {
    pub label: String,
    pub ssh_fingerprint: String,
    pub developer_key_id: String,
    pub paired_at_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeveloperResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: DeveloperOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DeveloperOutcome {
    Paired {
        host_fingerprint: String,
        developer_key_id: String,
    },
    Appd {
        response: AppdResponse,
    },
    Device {
        developer_mode: bool,
        paired_hosts: u8,
    },
    PairingWindow {
        remaining_seconds: u16,
    },
    PairedHosts {
        pairing_remaining_seconds: Option<u16>,
        hosts: Vec<PairedHostSummary>,
    },
    Unpaired {
        removed: u8,
        paired_hosts: u8,
    },
    Error {
        code: DeveloperErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeveloperErrorCode {
    InvalidRequest,
    Unauthorized,
    DeveloperModeOff,
    PairingClosed,
    UnpairedKey,
    NotFound,
    Conflict,
    Unavailable,
    Internal,
}

impl DeveloperResponse {
    pub fn error(request_id: u64, code: DeveloperErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: DEVELOPER_PROTOCOL_VERSION,
            request_id,
            outcome: DeveloperOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }
}

#[derive(Debug)]
pub enum DeveloperProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidValue(&'static str),
}

impl fmt::Display for DeveloperProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "developer protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid developer JSON: {error}"),
            Self::FrameTooLarge => formatter.write_str("developer frame exceeds 4096 bytes"),
            Self::UnterminatedFrame => formatter.write_str("developer frame is not terminated"),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "unsupported developer protocol version {version}"
                )
            }
            Self::InvalidValue(field) => write!(formatter, "invalid developer {field}"),
        }
    }
}

impl std::error::Error for DeveloperProtocolError {}

impl From<io::Error> for DeveloperProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DeveloperProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl DeveloperRequest {
    pub fn validate(&self) -> Result<(), DeveloperProtocolError> {
        if self.protocol_version != DEVELOPER_PROTOCOL_VERSION {
            return Err(DeveloperProtocolError::UnsupportedVersion(
                self.protocol_version,
            ));
        }
        match &self.command {
            DeveloperCommand::Pair {
                host_label,
                ssh_public_key,
                developer_public_key,
            } => {
                if host_label.is_empty()
                    || host_label.len() > MAX_HOST_LABEL_BYTES
                    || !host_label.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                    })
                {
                    return Err(DeveloperProtocolError::InvalidValue("host label"));
                }
                if ssh_public_key.is_empty()
                    || ssh_public_key.len() > MAX_SSH_PUBLIC_KEY_BYTES
                    || ssh_public_key.contains(['\r', '\n', '\0'])
                {
                    return Err(DeveloperProtocolError::InvalidValue("SSH public key"));
                }
                decode_hex_32(developer_public_key)
                    .map(|_| ())
                    .map_err(|_| DeveloperProtocolError::InvalidValue("public key"))
            }
            DeveloperCommand::Install {
                package_bytes,
                package_sha256,
            } => {
                if *package_bytes == 0
                    || *package_bytes > (cp0_package::MAX_PAYLOAD_BYTES + 4096) as u64
                {
                    return Err(DeveloperProtocolError::InvalidValue("package size"));
                }
                decode_hex_32(package_sha256)
                    .map(|_| ())
                    .map_err(|_| DeveloperProtocolError::InvalidValue("package digest"))
            }
            DeveloperCommand::Logs { app_id, limit } => {
                validate_app_id(app_id)?;
                if !(1..=cp0_appd::MAX_LOG_LINES).contains(limit) {
                    return Err(DeveloperProtocolError::InvalidValue("log limit"));
                }
                Ok(())
            }
            DeveloperCommand::Start { app_id }
            | DeveloperCommand::Stop { app_id }
            | DeveloperCommand::Uninstall { app_id } => validate_app_id(app_id),
            DeveloperCommand::OpenPairing { duration_seconds } => {
                if !(60..=MAX_PAIRING_WINDOW_SECONDS).contains(duration_seconds) {
                    return Err(DeveloperProtocolError::InvalidValue(
                        "pairing window duration",
                    ));
                }
                Ok(())
            }
            DeveloperCommand::Unpair { host_fingerprint } => {
                validate_ssh_fingerprint(host_fingerprint)
            }
            DeveloperCommand::ListPaired
            | DeveloperCommand::UnpairAll
            | DeveloperCommand::Status => Ok(()),
        }
    }
}

fn validate_ssh_fingerprint(value: &str) -> Result<(), DeveloperProtocolError> {
    let Some(encoded) = value.strip_prefix("SHA256:") else {
        return Err(DeveloperProtocolError::InvalidValue("SSH fingerprint"));
    };
    if encoded.len() == 43
        && encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
    {
        Ok(())
    } else {
        Err(DeveloperProtocolError::InvalidValue("SSH fingerprint"))
    }
}

fn validate_app_id(app_id: &str) -> Result<(), DeveloperProtocolError> {
    if cp0_manifest::is_valid_app_id(app_id) {
        Ok(())
    } else {
        Err(DeveloperProtocolError::InvalidValue("application ID"))
    }
}

pub fn read_request(
    reader: &mut impl BufRead,
) -> Result<Option<DeveloperRequest>, DeveloperProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: DeveloperRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<DeveloperResponse>, DeveloperProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: DeveloperResponse = serde_json::from_slice(&frame)?;
    if response.protocol_version != DEVELOPER_PROTOCOL_VERSION {
        return Err(DeveloperProtocolError::UnsupportedVersion(
            response.protocol_version,
        ));
    }
    Ok(Some(response))
}

pub fn write_request(
    writer: &mut impl Write,
    request: &DeveloperRequest,
) -> Result<(), DeveloperProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn write_response(
    writer: &mut impl Write,
    response: &DeveloperResponse,
) -> Result<(), DeveloperProtocolError> {
    write_frame(writer, response)
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, DeveloperProtocolError> {
    let mut frame = Vec::new();
    let mut terminated = false;
    while frame.len() < MAX_DEVELOPER_FRAME_BYTES {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let remaining = MAX_DEVELOPER_FRAME_BYTES - frame.len();
        let consumed = available.len().min(remaining);
        let slice = &available[..consumed];
        if let Some(index) = slice.iter().position(|byte| *byte == b'\n') {
            frame.extend_from_slice(&slice[..=index]);
            reader.consume(index + 1);
            terminated = true;
            break;
        }
        frame.extend_from_slice(slice);
        reader.consume(consumed);
    }
    if frame.is_empty() {
        return Ok(None);
    }
    if !terminated {
        return Err(if frame.len() == MAX_DEVELOPER_FRAME_BYTES {
            DeveloperProtocolError::FrameTooLarge
        } else {
            DeveloperProtocolError::UnterminatedFrame
        });
    }
    frame.pop();
    Ok(Some(frame))
}

fn write_frame(
    writer: &mut impl Write,
    value: &impl Serialize,
) -> Result<(), DeveloperProtocolError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_DEVELOPER_FRAME_BYTES {
        return Err(DeveloperProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}

pub fn decode_hex_32(value: &str) -> Result<[u8; 32], ()> {
    if value.len() != 64 {
        return Err(());
    }
    let mut output = [0_u8; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        let high = hex_nibble(value.as_bytes()[index * 2])?;
        let low = hex_nibble(value.as_bytes()[index * 2 + 1])?;
        *byte = high << 4 | low;
    }
    Ok(output)
}

fn hex_nibble(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use super::*;

    #[test]
    fn round_trips_bounded_request() {
        let request = DeveloperRequest {
            protocol_version: DEVELOPER_PROTOCOL_VERSION,
            request_id: 7,
            command: DeveloperCommand::Status,
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &request).unwrap();
        assert_eq!(
            read_request(&mut BufReader::new(Cursor::new(encoded))).unwrap(),
            Some(request)
        );
    }

    #[test]
    fn rejects_unbounded_and_noncanonical_values() {
        let request = DeveloperRequest {
            protocol_version: DEVELOPER_PROTOCOL_VERSION,
            request_id: 1,
            command: DeveloperCommand::Pair {
                host_label: "bad label".into(),
                ssh_public_key: "ssh-ed25519 bad".into(),
                developer_public_key: "00".repeat(32),
            },
        };
        assert!(request.validate().is_err());
        assert!(decode_hex_32(&"AA".repeat(32)).is_err());
        let too_long = DeveloperRequest {
            protocol_version: DEVELOPER_PROTOCOL_VERSION,
            request_id: 2,
            command: DeveloperCommand::OpenPairing {
                duration_seconds: MAX_PAIRING_WINDOW_SECONDS + 1,
            },
        };
        assert!(too_long.validate().is_err());
        let bad_fingerprint = DeveloperRequest {
            protocol_version: DEVELOPER_PROTOCOL_VERSION,
            request_id: 3,
            command: DeveloperCommand::Unpair {
                host_fingerprint: "SHA256:bad".into(),
            },
        };
        assert!(bad_fingerprint.validate().is_err());
    }
}
