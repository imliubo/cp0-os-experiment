use std::fmt;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

pub const STORAGE_PROTOCOL_VERSION: u32 = 2;
pub const MAX_STORAGE_KEY_BYTES: usize = 64;
pub const MAX_STORAGE_VALUE_BYTES: usize = 8 * 1024;
pub const MAX_STORAGE_BLOB_BYTES: usize = 128 * 1024;
pub const MAX_STORAGE_FRAME_BYTES: usize = 16 * 1024;
pub const MAX_STORAGE_ERROR_CHARS: usize = 160;
pub const MIB: u64 = 1024 * 1024;
pub const MAX_STORAGE_QUOTA_BYTES: u64 = cp0_manifest::MAX_APP_STORAGE_MB as u64 * MIB;
pub const SYSTEM_PHOTO_LIBRARY_ID: &str = "dev.cardputerzero.photo-library";
pub const SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES: u64 = 1_u64 << 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub app_id: String,
    pub quota_bytes: u64,
    pub command: StorageCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StorageCommand {
    Put {
        key: String,
        value_base64: String,
    },
    Get {
        key: String,
    },
    Delete {
        key: String,
    },
    PutBlobChunk {
        key: String,
        offset: u32,
        total_bytes: u32,
        value_base64: String,
    },
    GetBlobChunk {
        key: String,
        offset: u32,
        length: u32,
    },
    OpenBlob {
        key: String,
        expected_bytes: u32,
    },
    DeleteBlob {
        key: String,
    },
    Usage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: StorageOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StorageOutcome {
    Stored {
        used_bytes: u64,
    },
    Value {
        value_base64: String,
    },
    NotFound,
    Deleted {
        existed: bool,
        used_bytes: u64,
    },
    Usage {
        used_bytes: u64,
    },
    BlobOpened {
        size_bytes: u32,
    },
    Error {
        code: StorageErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StorageErrorCode {
    InvalidRequest,
    Unauthorized,
    QuotaExceeded,
    Unavailable,
    Internal,
}

#[derive(Debug)]
pub enum StorageProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidAppId,
    InvalidQuota,
    InvalidKey,
    InvalidValue,
    InvalidUsage,
    InvalidErrorMessage,
}

impl fmt::Display for StorageProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "storage protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid storage protocol JSON: {error}"),
            Self::FrameTooLarge => write!(
                formatter,
                "storage protocol frame exceeds {MAX_STORAGE_FRAME_BYTES} bytes"
            ),
            Self::UnterminatedFrame => {
                formatter.write_str("storage protocol frame is not newline terminated")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported storage protocol version {version}")
            }
            Self::InvalidAppId => formatter.write_str("invalid storage application identity"),
            Self::InvalidQuota => formatter.write_str("invalid storage quota"),
            Self::InvalidKey => formatter.write_str("invalid private storage key"),
            Self::InvalidValue => formatter.write_str("invalid bounded storage value"),
            Self::InvalidUsage => formatter.write_str("invalid private storage usage"),
            Self::InvalidErrorMessage => {
                formatter.write_str("storage error message is empty, too long or contains controls")
            }
        }
    }
}

impl std::error::Error for StorageProtocolError {}

impl From<io::Error> for StorageProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for StorageProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl StorageRequest {
    pub fn put(request_id: u64, app_id: &str, quota_bytes: u64, key: &str, value: &[u8]) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            app_id: app_id.into(),
            quota_bytes,
            command: StorageCommand::Put {
                key: key.into(),
                value_base64: encode_base64(value),
            },
        }
    }

    pub fn get(request_id: u64, app_id: &str, quota_bytes: u64, key: &str) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            app_id: app_id.into(),
            quota_bytes,
            command: StorageCommand::Get { key: key.into() },
        }
    }

    pub fn delete(request_id: u64, app_id: &str, quota_bytes: u64, key: &str) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            app_id: app_id.into(),
            quota_bytes,
            command: StorageCommand::Delete { key: key.into() },
        }
    }

    pub fn usage(request_id: u64, app_id: &str, quota_bytes: u64) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            app_id: app_id.into(),
            quota_bytes,
            command: StorageCommand::Usage,
        }
    }

    pub fn put_blob_chunk(
        request_id: u64,
        app_id: &str,
        quota_bytes: u64,
        key: &str,
        offset: u32,
        total_bytes: u32,
        value: &[u8],
    ) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            app_id: app_id.into(),
            quota_bytes,
            command: StorageCommand::PutBlobChunk {
                key: key.into(),
                offset,
                total_bytes,
                value_base64: encode_base64(value),
            },
        }
    }

    pub fn get_blob_chunk(
        request_id: u64,
        app_id: &str,
        quota_bytes: u64,
        key: &str,
        offset: u32,
        length: u32,
    ) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            app_id: app_id.into(),
            quota_bytes,
            command: StorageCommand::GetBlobChunk {
                key: key.into(),
                offset,
                length,
            },
        }
    }

    pub fn delete_blob(request_id: u64, app_id: &str, quota_bytes: u64, key: &str) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            app_id: app_id.into(),
            quota_bytes,
            command: StorageCommand::DeleteBlob { key: key.into() },
        }
    }

    pub fn open_blob(
        request_id: u64,
        app_id: &str,
        quota_bytes: u64,
        key: &str,
        expected_bytes: u32,
    ) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            app_id: app_id.into(),
            quota_bytes,
            command: StorageCommand::OpenBlob {
                key: key.into(),
                expected_bytes,
            },
        }
    }

    pub fn validate(&self) -> Result<(), StorageProtocolError> {
        validate_version(self.protocol_version)?;
        if !cp0_manifest::is_valid_app_id(&self.app_id) {
            return Err(StorageProtocolError::InvalidAppId);
        }
        let system_photo_library = self.app_id == SYSTEM_PHOTO_LIBRARY_ID;
        let valid_quota = if system_photo_library {
            self.quota_bytes == SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES
        } else {
            self.quota_bytes >= MIB
                && self.quota_bytes <= MAX_STORAGE_QUOTA_BYTES
                && self.quota_bytes % MIB == 0
        };
        if !valid_quota {
            return Err(StorageProtocolError::InvalidQuota);
        }
        match &self.command {
            StorageCommand::Put { key, value_base64 } => {
                validate_key(key)?;
                decode_value(value_base64).map(|_| ())
            }
            StorageCommand::Get { key } | StorageCommand::Delete { key } => validate_key(key),
            StorageCommand::PutBlobChunk {
                key,
                offset,
                total_bytes,
                value_base64,
            } => {
                if !system_photo_library
                    || *total_bytes == 0
                    || *total_bytes as usize > MAX_STORAGE_BLOB_BYTES
                {
                    return Err(StorageProtocolError::InvalidValue);
                }
                validate_key(key)?;
                let value = decode_value(value_base64)?;
                if value.is_empty()
                    || *offset >= *total_bytes
                    || u64::from(*offset) + value.len() as u64 > u64::from(*total_bytes)
                {
                    return Err(StorageProtocolError::InvalidValue);
                }
                Ok(())
            }
            StorageCommand::GetBlobChunk {
                key,
                offset,
                length,
            } => {
                validate_key(key)?;
                if !system_photo_library
                    || *length == 0
                    || *length as usize > MAX_STORAGE_VALUE_BYTES
                    || u64::from(*offset) + u64::from(*length) > MAX_STORAGE_BLOB_BYTES as u64
                {
                    Err(StorageProtocolError::InvalidValue)
                } else {
                    Ok(())
                }
            }
            StorageCommand::OpenBlob {
                key,
                expected_bytes,
            } => {
                validate_key(key)?;
                if !system_photo_library
                    || *expected_bytes == 0
                    || *expected_bytes as usize > MAX_STORAGE_BLOB_BYTES
                {
                    Err(StorageProtocolError::InvalidValue)
                } else {
                    Ok(())
                }
            }
            StorageCommand::DeleteBlob { key } => {
                if !system_photo_library {
                    Err(StorageProtocolError::InvalidValue)
                } else {
                    validate_key(key)
                }
            }
            StorageCommand::Usage => Ok(()),
        }
    }
}

impl StorageResponse {
    pub const fn stored(request_id: u64, used_bytes: u64) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            outcome: StorageOutcome::Stored { used_bytes },
        }
    }

    pub fn value(request_id: u64, value: &[u8]) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            outcome: StorageOutcome::Value {
                value_base64: encode_base64(value),
            },
        }
    }

    pub const fn not_found(request_id: u64) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            outcome: StorageOutcome::NotFound,
        }
    }

    pub const fn deleted(request_id: u64, existed: bool, used_bytes: u64) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            outcome: StorageOutcome::Deleted {
                existed,
                used_bytes,
            },
        }
    }

    pub const fn usage(request_id: u64, used_bytes: u64) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            outcome: StorageOutcome::Usage { used_bytes },
        }
    }

    pub const fn blob_opened(request_id: u64, size_bytes: u32) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            outcome: StorageOutcome::BlobOpened { size_bytes },
        }
    }

    pub fn error(request_id: u64, code: StorageErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: STORAGE_PROTOCOL_VERSION,
            request_id,
            outcome: StorageOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), StorageProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.outcome {
            StorageOutcome::Stored { used_bytes }
            | StorageOutcome::Deleted { used_bytes, .. }
            | StorageOutcome::Usage { used_bytes }
                if *used_bytes > SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES =>
            {
                Err(StorageProtocolError::InvalidUsage)
            }
            StorageOutcome::Value { value_base64 } => decode_value(value_base64).map(|_| ()),
            StorageOutcome::BlobOpened { size_bytes }
                if *size_bytes == 0 || *size_bytes as usize > MAX_STORAGE_BLOB_BYTES =>
            {
                Err(StorageProtocolError::InvalidValue)
            }
            StorageOutcome::Error { message, .. }
                if message.is_empty()
                    || message.chars().count() > MAX_STORAGE_ERROR_CHARS
                    || message.chars().any(char::is_control) =>
            {
                Err(StorageProtocolError::InvalidErrorMessage)
            }
            _ => Ok(()),
        }
    }
}

pub fn validate_key(key: &str) -> Result<(), StorageProtocolError> {
    if key.is_empty()
        || key.len() > MAX_STORAGE_KEY_BYTES
        || key.starts_with('.')
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        Err(StorageProtocolError::InvalidKey)
    } else {
        Ok(())
    }
}

pub fn encode_base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[usize::from(first >> 2)] as char);
        output.push(ALPHABET[usize::from((first & 3) << 4 | second >> 4)] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[usize::from((second & 15) << 2 | third >> 6)] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[usize::from(third & 63)] as char
        } else {
            '='
        });
    }
    output
}

pub fn decode_value(input: &str) -> Result<Vec<u8>, StorageProtocolError> {
    let encoded = input.as_bytes();
    if encoded.is_empty()
        || encoded.len() % 4 != 0
        || encoded.len() > MAX_STORAGE_VALUE_BYTES.div_ceil(3) * 4
    {
        return Err(StorageProtocolError::InvalidValue);
    }
    let mut output = Vec::with_capacity(encoded.len() / 4 * 3);
    for (index, chunk) in encoded.chunks_exact(4).enumerate() {
        let last = index + 1 == encoded.len() / 4;
        let a = decode_digit(chunk[0])?;
        let b = decode_digit(chunk[1])?;
        let c = if chunk[2] == b'=' {
            if !last || chunk[3] != b'=' || b & 15 != 0 {
                return Err(StorageProtocolError::InvalidValue);
            }
            None
        } else {
            Some(decode_digit(chunk[2])?)
        };
        let d = if chunk[3] == b'=' {
            if !last || c.is_some_and(|value| value & 3 != 0) {
                return Err(StorageProtocolError::InvalidValue);
            }
            None
        } else {
            if c.is_none() {
                return Err(StorageProtocolError::InvalidValue);
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
    if output.is_empty() || output.len() > MAX_STORAGE_VALUE_BYTES {
        return Err(StorageProtocolError::InvalidValue);
    }
    Ok(output)
}

fn decode_digit(byte: u8) -> Result<u8, StorageProtocolError> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(StorageProtocolError::InvalidValue),
    }
}

pub fn write_request(
    writer: &mut impl Write,
    request: &StorageRequest,
) -> Result<(), StorageProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn read_request(
    reader: &mut impl BufRead,
) -> Result<Option<StorageRequest>, StorageProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: StorageRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn write_response(
    writer: &mut impl Write,
    response: &StorageResponse,
) -> Result<(), StorageProtocolError> {
    response.validate()?;
    write_frame(writer, response)
}

pub fn encode_response_frame(response: &StorageResponse) -> Result<Vec<u8>, StorageProtocolError> {
    response.validate()?;
    let mut encoded = serde_json::to_vec(response)?;
    if encoded.len() + 1 > MAX_STORAGE_FRAME_BYTES {
        return Err(StorageProtocolError::FrameTooLarge);
    }
    encoded.push(b'\n');
    Ok(encoded)
}

pub fn decode_response_frame(frame: &[u8]) -> Result<StorageResponse, StorageProtocolError> {
    let response: StorageResponse = serde_json::from_slice(frame)?;
    response.validate()?;
    Ok(response)
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<StorageResponse>, StorageProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: StorageResponse = serde_json::from_slice(&frame)?;
    response.validate()?;
    Ok(Some(response))
}

fn validate_version(version: u32) -> Result<(), StorageProtocolError> {
    if version == STORAGE_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(StorageProtocolError::UnsupportedVersion(version))
    }
}

fn write_frame(
    writer: &mut impl Write,
    value: &impl Serialize,
) -> Result<(), StorageProtocolError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_STORAGE_FRAME_BYTES {
        return Err(StorageProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, StorageProtocolError> {
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
        if frame.len() + consumed > MAX_STORAGE_FRAME_BYTES {
            return Err(StorageProtocolError::FrameTooLarge);
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
        return Err(StorageProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn round_trips_maximum_value_and_strict_key() {
        let value = vec![0xa5; MAX_STORAGE_VALUE_BYTES];
        let request = StorageRequest::put(1, "dev.cardputerzero.test", MIB, "state.v1", &value);
        let mut frame = Vec::new();
        write_request(&mut frame, &request).unwrap();
        assert!(frame.len() <= MAX_STORAGE_FRAME_BYTES);
        assert_eq!(
            read_request(&mut Cursor::new(frame)).unwrap(),
            Some(request)
        );
        let usage = StorageRequest::usage(2, "dev.cardputerzero.test", MIB);
        let mut frame = Vec::new();
        write_request(&mut frame, &usage).unwrap();
        assert_eq!(read_request(&mut Cursor::new(frame)).unwrap(), Some(usage));
        for key in ["", ".hidden", "../escape", "with/slash", "bad key"] {
            assert!(validate_key(key).is_err(), "accepted {key:?}");
        }
    }

    #[test]
    fn rejects_noncanonical_values_and_invalid_quota() {
        for encoded in ["", "A", "====", "AB==", "AAB=", "AA=A"] {
            assert!(decode_value(encoded).is_err(), "accepted {encoded}");
        }
        assert_eq!(decode_value("AA==").unwrap(), [0]);
        assert!(
            StorageRequest::get(1, "dev.cardputerzero.test", 0, "key")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn system_photo_blobs_are_bounded_and_identity_specific() {
        let value = vec![0x5a; MAX_STORAGE_VALUE_BYTES];
        let request = StorageRequest::put_blob_chunk(
            3,
            SYSTEM_PHOTO_LIBRARY_ID,
            SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES,
            "p000000000000002a.rgb565",
            0,
            (320 * 170 * 2) as u32,
            &value,
        );
        let mut frame = Vec::new();
        write_request(&mut frame, &request).unwrap();
        assert_eq!(
            read_request(&mut Cursor::new(frame)).unwrap(),
            Some(request)
        );

        assert!(
            StorageRequest::put_blob_chunk(
                4,
                "dev.cardputerzero.camera",
                MIB,
                "frame",
                0,
                10,
                b"x",
            )
            .validate()
            .is_err()
        );
        assert!(
            StorageRequest::put_blob_chunk(
                5,
                SYSTEM_PHOTO_LIBRARY_ID,
                SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES,
                "frame",
                0,
                (MAX_STORAGE_BLOB_BYTES + 1) as u32,
                b"x",
            )
            .validate()
            .is_err()
        );
        assert!(
            StorageResponse::stored(6, MAX_STORAGE_QUOTA_BYTES + 1)
                .validate()
                .is_ok()
        );
        assert!(
            StorageResponse::stored(7, SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES + 1)
                .validate()
                .is_err()
        );
        let open = StorageRequest::open_blob(
            8,
            SYSTEM_PHOTO_LIBRARY_ID,
            SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES,
            "p000000000000002a.rgb565",
            (320 * 170 * 2) as u32,
        );
        assert!(open.validate().is_ok());
        assert!(
            StorageRequest::open_blob(9, "dev.cardputerzero.camera", MIB, "frame", 1,)
                .validate()
                .is_err()
        );
        let opened = StorageResponse::blob_opened(10, (320 * 170 * 2) as u32);
        let frame = encode_response_frame(&opened).unwrap();
        assert_eq!(
            decode_response_frame(&frame[..frame.len() - 1]).unwrap(),
            opened
        );
        assert!(StorageResponse::blob_opened(11, 0).validate().is_err());
    }
}
