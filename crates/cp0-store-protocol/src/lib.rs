use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, BufRead, Write};

use cp0_manifest::Permission;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const STORE_PROTOCOL_VERSION: u32 = 1;
pub const CATALOG_SCHEMA_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
pub const MAX_CATALOG_BYTES: usize = 48 * 1024;
pub const MAX_CATALOG_APPS: usize = 64;
pub const MAX_PACKAGE_BYTES: u64 = cp0_package::MAX_PAYLOAD_BYTES as u64 + 4096;
pub const MAX_PACKAGE_URL_BYTES: usize = 2048;
pub const MAX_SUMMARY_CHARS: usize = 96;
pub const MAX_ERROR_MESSAGE_CHARS: usize = 160;
pub const MAX_CATALOG_LIFETIME_SECONDS: u64 = 31 * 24 * 60 * 60;

const CATALOG_SIGNATURE_DOMAIN: &[u8] = b"CardputerZero store catalog signature v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub sequence: u64,
    pub published_unix_seconds: u64,
    pub expires_unix_seconds: u64,
    pub apps: Vec<CatalogApp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogApp {
    pub app_id: String,
    pub name: String,
    pub version: String,
    pub sdk_version: String,
    pub summary: String,
    pub package_url: String,
    pub package_sha256: String,
    pub package_bytes: u64,
    pub permissions: Vec<Permission>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCatalog {
    pub catalog: Catalog,
    pub key_id: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: StoreCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StoreCommand {
    List,
    Refresh,
    Install { app_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: StoreOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StoreOutcome {
    Ok {
        data: StoreResponseData,
    },
    Error {
        code: StoreErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StoreResponseData {
    Catalog {
        sequence: u64,
        expires_unix_seconds: u64,
        stale: bool,
        apps: Vec<StoreAppSummary>,
    },
    RefreshAccepted,
    InstallAccepted {
        app_id: String,
        version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreAppSummary {
    pub app_id: String,
    pub name: String,
    pub version: String,
    pub summary: String,
    pub package_bytes: u64,
    pub permissions: Vec<Permission>,
    pub state: StoreAppState,
    pub progress_percent: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StoreAppState {
    Available,
    Queued,
    Downloading,
    Installing,
    Installed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StoreErrorCode {
    InvalidRequest,
    Unauthorized,
    Unconfigured,
    Unavailable,
    NotFound,
    Busy,
    Untrusted,
    ResourceExhausted,
    Internal,
}

#[derive(Debug)]
pub enum StoreProtocolError {
    Io(io::Error),
    InvalidJson(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    Invalid(String),
    Signature(String),
}

impl fmt::Display for StoreProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "store protocol I/O error: {error}"),
            Self::InvalidJson(error) => write!(formatter, "invalid store JSON: {error}"),
            Self::FrameTooLarge => {
                write!(formatter, "store frame exceeds {MAX_FRAME_BYTES} bytes")
            }
            Self::UnterminatedFrame => formatter.write_str("store frame is not newline terminated"),
            Self::Invalid(error) => write!(formatter, "invalid store data: {error}"),
            Self::Signature(error) => write!(formatter, "store signature error: {error}"),
        }
    }
}

impl std::error::Error for StoreProtocolError {}

impl From<io::Error> for StoreProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for StoreProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::InvalidJson(error)
    }
}

impl Catalog {
    pub fn validate(&self) -> Result<(), StoreProtocolError> {
        if self.schema_version != CATALOG_SCHEMA_VERSION {
            return Err(StoreProtocolError::Invalid(format!(
                "catalog schema must be {CATALOG_SCHEMA_VERSION}"
            )));
        }
        if self.sequence == 0 {
            return Err(StoreProtocolError::Invalid(
                "catalog sequence must be non-zero".into(),
            ));
        }
        let lifetime = self
            .expires_unix_seconds
            .checked_sub(self.published_unix_seconds)
            .filter(|lifetime| *lifetime > 0 && *lifetime <= MAX_CATALOG_LIFETIME_SECONDS)
            .ok_or_else(|| {
                StoreProtocolError::Invalid("catalog validity interval is outside limits".into())
            })?;
        debug_assert!(lifetime > 0);
        if self.apps.len() > MAX_CATALOG_APPS {
            return Err(StoreProtocolError::Invalid(
                "catalog contains too many applications".into(),
            ));
        }

        let mut previous_id: Option<&str> = None;
        let mut ids = BTreeSet::new();
        for app in &self.apps {
            app.validate()?;
            if !ids.insert(app.app_id.as_str()) {
                return Err(StoreProtocolError::Invalid(
                    "catalog contains a duplicate application ID".into(),
                ));
            }
            if previous_id.is_some_and(|previous| previous >= app.app_id.as_str()) {
                return Err(StoreProtocolError::Invalid(
                    "catalog applications are not sorted by ID".into(),
                ));
            }
            previous_id = Some(&app.app_id);
        }
        Ok(())
    }
}

impl CatalogApp {
    pub fn validate(&self) -> Result<(), StoreProtocolError> {
        if !cp0_manifest::is_valid_app_id(&self.app_id) {
            return Err(StoreProtocolError::Invalid(
                "catalog application ID is invalid".into(),
            ));
        }
        let name_chars = self.name.chars().count();
        if !(1..=32).contains(&name_chars) || has_unsafe_text(&self.name) {
            return Err(StoreProtocolError::Invalid(
                "catalog application name is invalid".into(),
            ));
        }
        if !cp0_manifest::is_valid_app_version(&self.version) {
            return Err(StoreProtocolError::Invalid(
                "catalog application version is invalid".into(),
            ));
        }
        if !matches!(self.sdk_version.as_str(), "1.0" | "0.1") {
            return Err(StoreProtocolError::Invalid(
                "catalog SDK version is not supported".into(),
            ));
        }
        let summary_chars = self.summary.chars().count();
        if !(1..=MAX_SUMMARY_CHARS).contains(&summary_chars) || has_unsafe_text(&self.summary) {
            return Err(StoreProtocolError::Invalid(
                "catalog application summary is invalid".into(),
            ));
        }
        if !is_valid_https_url(&self.package_url) {
            return Err(StoreProtocolError::Invalid(
                "catalog package URL must be bounded HTTPS without credentials or fragments".into(),
            ));
        }
        if !is_lower_hex(&self.package_sha256, 32) {
            return Err(StoreProtocolError::Invalid(
                "catalog package SHA-256 is invalid".into(),
            ));
        }
        if !(1..=MAX_PACKAGE_BYTES).contains(&self.package_bytes) {
            return Err(StoreProtocolError::Invalid(
                "catalog package size is outside limits".into(),
            ));
        }
        let mut previous = None;
        for permission in &self.permissions {
            let name = permission.as_str();
            if previous.is_some_and(|value| value >= name) {
                return Err(StoreProtocolError::Invalid(
                    "catalog permissions are duplicated or unsorted".into(),
                ));
            }
            previous = Some(name);
        }
        Ok(())
    }
}

impl StoreRequest {
    pub fn validate(&self) -> Result<(), StoreProtocolError> {
        if self.protocol_version != STORE_PROTOCOL_VERSION {
            return Err(StoreProtocolError::Invalid(
                "unsupported store protocol version".into(),
            ));
        }
        if self.request_id == 0 {
            return Err(StoreProtocolError::Invalid(
                "store request ID must be non-zero".into(),
            ));
        }
        if let StoreCommand::Install { app_id } = &self.command
            && !cp0_manifest::is_valid_app_id(app_id)
        {
            return Err(StoreProtocolError::Invalid(
                "store install application ID is invalid".into(),
            ));
        }
        Ok(())
    }
}

impl StoreResponse {
    pub fn success(request_id: u64, data: StoreResponseData) -> Self {
        Self {
            protocol_version: STORE_PROTOCOL_VERSION,
            request_id,
            outcome: StoreOutcome::Ok { data },
        }
    }

    pub fn error(request_id: u64, code: StoreErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: STORE_PROTOCOL_VERSION,
            request_id,
            outcome: StoreOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), StoreProtocolError> {
        if self.protocol_version != STORE_PROTOCOL_VERSION {
            return Err(StoreProtocolError::Invalid(
                "unsupported store response protocol version".into(),
            ));
        }
        match &self.outcome {
            StoreOutcome::Ok { data } => {
                if self.request_id == 0 {
                    return Err(StoreProtocolError::Invalid(
                        "successful store response has no request ID".into(),
                    ));
                }
                data.validate()
            }
            StoreOutcome::Error { code, message } => {
                if self.request_id == 0 && *code != StoreErrorCode::InvalidRequest {
                    return Err(StoreProtocolError::Invalid(
                        "uncorrelated store error is not an invalid-request response".into(),
                    ));
                }
                let message_chars = message.chars().count();
                if !(1..=MAX_ERROR_MESSAGE_CHARS).contains(&message_chars)
                    || has_unsafe_text(message)
                {
                    return Err(StoreProtocolError::Invalid(
                        "store error message is invalid".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

impl StoreResponseData {
    fn validate(&self) -> Result<(), StoreProtocolError> {
        match self {
            Self::Catalog {
                sequence,
                expires_unix_seconds,
                apps,
                ..
            } => {
                if *sequence == 0 || *expires_unix_seconds == 0 {
                    return Err(StoreProtocolError::Invalid(
                        "store response catalog metadata is invalid".into(),
                    ));
                }
                if apps.len() > MAX_CATALOG_APPS {
                    return Err(StoreProtocolError::Invalid(
                        "store response contains too many applications".into(),
                    ));
                }
                let mut previous_id: Option<&str> = None;
                for app in apps {
                    app.validate()?;
                    if previous_id.is_some_and(|previous| previous >= app.app_id.as_str()) {
                        return Err(StoreProtocolError::Invalid(
                            "store response applications are duplicated or unsorted".into(),
                        ));
                    }
                    previous_id = Some(&app.app_id);
                }
                Ok(())
            }
            Self::RefreshAccepted => Ok(()),
            Self::InstallAccepted { app_id, version } => {
                if !cp0_manifest::is_valid_app_id(app_id)
                    || !cp0_manifest::is_valid_app_version(version)
                {
                    return Err(StoreProtocolError::Invalid(
                        "store install response identity is invalid".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

impl StoreAppSummary {
    fn validate(&self) -> Result<(), StoreProtocolError> {
        if !cp0_manifest::is_valid_app_id(&self.app_id) {
            return Err(StoreProtocolError::Invalid(
                "store response application ID is invalid".into(),
            ));
        }
        let name_chars = self.name.chars().count();
        if !(1..=32).contains(&name_chars) || has_unsafe_text(&self.name) {
            return Err(StoreProtocolError::Invalid(
                "store response application name is invalid".into(),
            ));
        }
        if !cp0_manifest::is_valid_app_version(&self.version) {
            return Err(StoreProtocolError::Invalid(
                "store response application version is invalid".into(),
            ));
        }
        let summary_chars = self.summary.chars().count();
        if !(1..=MAX_SUMMARY_CHARS).contains(&summary_chars) || has_unsafe_text(&self.summary) {
            return Err(StoreProtocolError::Invalid(
                "store response application summary is invalid".into(),
            ));
        }
        if !(1..=MAX_PACKAGE_BYTES).contains(&self.package_bytes) {
            return Err(StoreProtocolError::Invalid(
                "store response package size is outside limits".into(),
            ));
        }
        let mut previous = None;
        for permission in &self.permissions {
            let name = permission.as_str();
            if previous.is_some_and(|value| value >= name) {
                return Err(StoreProtocolError::Invalid(
                    "store response permissions are duplicated or unsorted".into(),
                ));
            }
            previous = Some(name);
        }
        let valid_progress = match self.state {
            StoreAppState::Available | StoreAppState::Queued | StoreAppState::Failed => {
                self.progress_percent == 0
            }
            StoreAppState::Downloading => self.progress_percent <= 100,
            StoreAppState::Installing | StoreAppState::Installed => self.progress_percent == 100,
        };
        if !valid_progress {
            return Err(StoreProtocolError::Invalid(
                "store response progress is inconsistent with application state".into(),
            ));
        }
        Ok(())
    }
}

pub fn sign_catalog(
    catalog: Catalog,
    signing_key: &[u8; 32],
) -> Result<SignedCatalog, StoreProtocolError> {
    let canonical = canonical_catalog(&catalog)?;
    let key = SigningKey::from_bytes(signing_key);
    let public = key.verifying_key().to_bytes();
    let signature = key.sign(&catalog_signature_message(&canonical));
    Ok(SignedCatalog {
        catalog,
        key_id: lower_hex(&cp0_package::key_id(&public)),
        signature: lower_hex(&signature.to_bytes()),
    })
}

pub fn verify_catalog(
    signed: &SignedCatalog,
    public_key: &[u8; 32],
) -> Result<(), StoreProtocolError> {
    let canonical = canonical_catalog(&signed.catalog)?;
    let expected_key_id = lower_hex(&cp0_package::key_id(public_key));
    if signed.key_id != expected_key_id {
        return Err(StoreProtocolError::Signature(
            "catalog key ID does not match trusted key".into(),
        ));
    }
    let signature = decode_hex::<64>(&signed.signature)
        .ok_or_else(|| StoreProtocolError::Signature("catalog signature is invalid".into()))?;
    let key = VerifyingKey::from_bytes(public_key)
        .map_err(|_| StoreProtocolError::Signature("trusted store key is invalid".into()))?;
    key.verify(
        &catalog_signature_message(&canonical),
        &Signature::from_bytes(&signature),
    )
    .map_err(|_| StoreProtocolError::Signature("catalog signature does not match".into()))
}

pub fn encode_signed_catalog(signed: &SignedCatalog) -> Result<Vec<u8>, StoreProtocolError> {
    verify_signed_shape(signed)?;
    let encoded = serde_json::to_vec(signed)?;
    if encoded.len() > MAX_CATALOG_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    Ok(encoded)
}

pub fn decode_signed_catalog(encoded: &[u8]) -> Result<SignedCatalog, StoreProtocolError> {
    if encoded.len() > MAX_CATALOG_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    let signed: SignedCatalog = serde_json::from_slice(encoded)?;
    verify_signed_shape(&signed)?;
    Ok(signed)
}

pub fn read_request(reader: &mut impl BufRead) -> Result<Option<StoreRequest>, StoreProtocolError> {
    read_frame(reader)
}

pub fn write_request(
    writer: &mut impl Write,
    request: &StoreRequest,
) -> Result<(), StoreProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<StoreResponse>, StoreProtocolError> {
    let response: Option<StoreResponse> = read_frame(reader)?;
    if let Some(response) = &response {
        response.validate()?;
    }
    Ok(response)
}

pub fn write_response(
    writer: &mut impl Write,
    response: &StoreResponse,
) -> Result<(), StoreProtocolError> {
    response.validate()?;
    write_frame(writer, response)
}

fn canonical_catalog(catalog: &Catalog) -> Result<Vec<u8>, StoreProtocolError> {
    catalog.validate()?;
    serde_json::to_vec(catalog).map_err(StoreProtocolError::InvalidJson)
}

fn verify_signed_shape(signed: &SignedCatalog) -> Result<(), StoreProtocolError> {
    signed.catalog.validate()?;
    if !is_lower_hex(&signed.key_id, 32) || !is_lower_hex(&signed.signature, 64) {
        return Err(StoreProtocolError::Invalid(
            "catalog key ID or signature encoding is invalid".into(),
        ));
    }
    Ok(())
}

fn catalog_signature_message(canonical: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(CATALOG_SIGNATURE_DOMAIN.len() + canonical.len());
    message.extend_from_slice(CATALOG_SIGNATURE_DOMAIN);
    message.extend_from_slice(canonical);
    message
}

fn has_unsafe_text(value: &str) -> bool {
    value.chars().any(|character| character.is_control())
}

pub fn is_valid_https_url(url: &str) -> bool {
    url.len() <= MAX_PACKAGE_URL_BYTES
        && url.starts_with("https://")
        && !url.contains('@')
        && !url.contains('#')
        && !url
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        && url[8..].contains('.')
}

pub fn is_lower_hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes * 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn lower_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

pub fn decode_hex<const N: usize>(encoded: &str) -> Option<[u8; N]> {
    if !is_lower_hex(encoded, N) {
        return None;
    }
    let mut decoded = [0; N];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        decoded[index] = (hex_value(pair[0])? << 4) | hex_value(pair[1])?;
    }
    Some(decoded)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn read_frame<T: for<'de> Deserialize<'de>>(
    reader: &mut impl BufRead,
) -> Result<Option<T>, StoreProtocolError> {
    let mut frame = Vec::new();
    let mut limited = std::io::Read::take(reader, (MAX_FRAME_BYTES + 1) as u64);
    let read = limited.read_until(b'\n', &mut frame)?;
    if read == 0 {
        return Ok(None);
    }
    if frame.len() > MAX_FRAME_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    if frame.last() != Some(&b'\n') {
        return Err(StoreProtocolError::UnterminatedFrame);
    }
    let decoded = serde_json::from_slice(&frame[..frame.len() - 1])?;
    Ok(Some(decoded))
}

fn write_frame<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<(), StoreProtocolError> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    if encoded.len() > MAX_FRAME_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog {
            schema_version: CATALOG_SCHEMA_VERSION,
            sequence: 7,
            published_unix_seconds: 1_800_000_000,
            expires_unix_seconds: 1_800_086_400,
            apps: vec![CatalogApp {
                app_id: "dev.cardputerzero.example".into(),
                name: "Example".into(),
                version: "1.2.3".into(),
                sdk_version: "1.0".into(),
                summary: "A bounded example application".into(),
                package_url: "https://store.example.com/apps/example.capp".into(),
                package_sha256: "11".repeat(32),
                package_bytes: 4096,
                permissions: vec![Permission::NetworkClient, Permission::NotificationsPost],
            }],
        }
    }

    fn response_app() -> StoreAppSummary {
        let app = &catalog().apps[0];
        StoreAppSummary {
            app_id: app.app_id.clone(),
            name: app.name.clone(),
            version: app.version.clone(),
            summary: app.summary.clone(),
            package_bytes: app.package_bytes,
            permissions: app.permissions.clone(),
            state: StoreAppState::Available,
            progress_percent: 0,
        }
    }

    #[test]
    fn signs_round_trips_and_detects_catalog_tampering() {
        let secret = [7; 32];
        let public = cp0_package::public_key(&secret);
        let signed = sign_catalog(catalog(), &secret).unwrap();
        verify_catalog(&signed, &public).unwrap();
        let encoded = encode_signed_catalog(&signed).unwrap();
        let decoded = decode_signed_catalog(&encoded).unwrap();
        verify_catalog(&decoded, &public).unwrap();

        let mut tampered = decoded;
        tampered.catalog.apps[0].package_bytes += 1;
        assert!(verify_catalog(&tampered, &public).is_err());
    }

    #[test]
    fn rejects_unsorted_duplicate_and_unsafe_catalog_fields() {
        let mut invalid = catalog();
        invalid.apps[0].package_url = "http://store.example.com/app.capp".into();
        assert!(invalid.validate().is_err());

        let mut invalid = catalog();
        invalid.apps[0].permissions.reverse();
        assert!(invalid.validate().is_err());

        let mut invalid = catalog();
        invalid.apps.push(invalid.apps[0].clone());
        assert!(invalid.validate().is_err());

        let mut invalid = catalog();
        invalid.apps[0].summary = "unsafe\nsummary".into();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn protocol_is_bounded_strict_and_versioned() {
        let request = StoreRequest {
            protocol_version: STORE_PROTOCOL_VERSION,
            request_id: 9,
            command: StoreCommand::Install {
                app_id: "dev.cardputerzero.example".into(),
            },
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &request).unwrap();
        assert_eq!(
            read_request(&mut encoded.as_slice()).unwrap(),
            Some(request)
        );

        let unknown =
            br#"{"protocol_version":1,"request_id":1,"command":{"name":"list"},"extra":true}\n"#;
        assert!(read_request(&mut unknown.as_slice()).is_err());

        let oversized = vec![b'x'; MAX_FRAME_BYTES + 1];
        assert!(matches!(
            read_request(&mut oversized.as_slice()),
            Err(StoreProtocolError::FrameTooLarge)
        ));
    }

    #[test]
    fn rejects_malformed_or_inconsistent_responses() {
        let valid = StoreResponse::success(
            4,
            StoreResponseData::Catalog {
                sequence: 2,
                expires_unix_seconds: 1_900_000_000,
                stale: false,
                apps: vec![response_app()],
            },
        );
        let mut encoded = Vec::new();
        write_response(&mut encoded, &valid).unwrap();
        assert_eq!(read_response(&mut encoded.as_slice()).unwrap(), Some(valid));

        let mut invalid_progress = response_app();
        invalid_progress.state = StoreAppState::Installed;
        invalid_progress.progress_percent = 30;
        let invalid_progress = StoreResponse::success(
            4,
            StoreResponseData::Catalog {
                sequence: 2,
                expires_unix_seconds: 1_900_000_000,
                stale: false,
                apps: vec![invalid_progress],
            },
        );
        assert!(write_response(&mut Vec::new(), &invalid_progress).is_err());

        let mut duplicate = response_app();
        duplicate.name = "Duplicate".into();
        let duplicate = StoreResponse::success(
            4,
            StoreResponseData::Catalog {
                sequence: 2,
                expires_unix_seconds: 1_900_000_000,
                stale: false,
                apps: vec![response_app(), duplicate],
            },
        );
        assert!(write_response(&mut Vec::new(), &duplicate).is_err());

        let oversized_error = StoreResponse::error(
            4,
            StoreErrorCode::Unavailable,
            "x".repeat(MAX_ERROR_MESSAGE_CHARS + 1),
        );
        let mut raw = serde_json::to_vec(&oversized_error).unwrap();
        raw.push(b'\n');
        assert!(read_response(&mut raw.as_slice()).is_err());

        let wrong_version = br#"{"protocol_version":2,"request_id":4,"outcome":{"status":"ok","data":{"kind":"refresh-accepted"}}}\n"#;
        assert!(read_response(&mut wrong_version.as_slice()).is_err());
    }

    #[test]
    fn bounded_mutation_corpus_rejects_tampering_without_panics() {
        let secret = [19; 32];
        let public = cp0_package::public_key(&secret);
        let signed = sign_catalog(catalog(), &secret).unwrap();
        let encoded = encode_signed_catalog(&signed).unwrap();

        for index in 0..encoded.len() {
            let mut mutated = encoded.clone();
            mutated[index] ^= 1 + (index % 251) as u8;
            if let Ok(decoded) = decode_signed_catalog(&mutated) {
                assert!(verify_catalog(&decoded, &public).is_err());
            }
        }

        let mut random = 0x6d5a_56e9_4f31_2c87_u64;
        for iteration in 0..4096_usize {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let length = (random as usize ^ iteration) % 513;
            let mut bytes = Vec::with_capacity(length + 1);
            for _ in 0..length {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                bytes.push(random as u8);
            }
            let _ = decode_signed_catalog(&bytes);
            bytes.push(b'\n');
            let _ = read_request(&mut bytes.as_slice());
            let _ = read_response(&mut bytes.as_slice());
        }
    }
}
