use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, BufRead, Write};
use std::mem;
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::os::unix::net::UnixStream;

use cp0_manifest::Permission;
use cp0_store_metadata::{AgeRating, StoreCategory};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const STORE_PROTOCOL_VERSION: u32 = 1;
pub const CATALOG_SCHEMA_VERSION: u32 = 1;
pub const RICH_CATALOG_SCHEMA_VERSION: u32 = 2;
pub const MEDIA_CATALOG_SCHEMA_VERSION: u32 = 3;
pub const APP_DETAILS_SCHEMA_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
pub const MAX_CATALOG_BYTES: usize = 48 * 1024;
pub const MAX_APP_DETAILS_BYTES: usize = 16 * 1024;
pub const MAX_CATALOG_APPS: usize = 64;
pub const MAX_PACKAGE_BYTES: u64 = cp0_package::MAX_PAYLOAD_BYTES as u64 + 4096;
pub const MAX_PACKAGE_URL_BYTES: usize = 2048;
pub const MAX_SUMMARY_CHARS: usize = 96;
pub const MAX_SEARCH_QUERY_CHARS: usize = 32;
pub const MAX_SEARCH_QUERY_BYTES: usize = 96;
pub const MAX_SEARCH_PAGE_APPS: u8 = 8;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery: Option<CatalogDiscovery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<CatalogResources>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogDiscovery {
    pub developer: String,
    pub subtitle: String,
    pub category: StoreCategory,
    pub keywords: Vec<String>,
    pub age_rating: AgeRating,
    pub privacy_url: String,
    pub support_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogResources {
    pub icon: CatalogImageResource,
    pub details: CatalogObjectResource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogObjectResource {
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogImageResource {
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreAppDetails {
    pub schema_version: u32,
    pub app_id: String,
    pub version: String,
    pub description: String,
    pub release_notes: String,
    pub screenshots: Vec<CatalogImageResource>,
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
    Search {
        query: String,
        offset: u16,
        limit: u8,
    },
    Refresh,
    Install {
        app_id: String,
    },
    Control {
        app_id: String,
        action: StoreControlAction,
    },
    Details {
        app_id: String,
    },
    Media {
        app_id: String,
        media: StoreMediaSelector,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StoreControlAction {
    Pause,
    Resume,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StoreMediaSelector {
    Icon,
    Screenshot { index: u8 },
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
    SearchResults {
        query: String,
        offset: u16,
        limit: u8,
        total: u16,
        next_offset: Option<u16>,
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
    OperationAccepted {
        app_id: String,
        version: String,
        action: StoreControlAction,
    },
    AppDetails {
        app_id: String,
        version: String,
        developer: String,
        category: StoreCategory,
        age_rating: AgeRating,
        privacy_url: String,
        support_url: String,
        description: String,
        release_notes: String,
        screenshot_count: u8,
    },
    Media {
        app_id: String,
        version: String,
        media: StoreMediaMetadata,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StoreMediaMetadata {
    Icon {
        sha256: String,
        bytes: u64,
        width: u16,
        height: u16,
    },
    Screenshot {
        index: u8,
        sha256: String,
        bytes: u64,
        width: u16,
        height: u16,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<StoreFailureReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StoreAppState {
    Available,
    Queued,
    Downloading,
    Paused,
    Installing,
    Installed,
    Canceled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StoreFailureReason {
    Network,
    Storage,
    Verification,
    Installer,
    CatalogChanged,
    Internal,
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
    InvalidState,
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
    InvalidDescriptor,
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
            Self::InvalidDescriptor => {
                formatter.write_str("invalid store media descriptor transfer")
            }
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
        if !matches!(
            self.schema_version,
            CATALOG_SCHEMA_VERSION | RICH_CATALOG_SCHEMA_VERSION | MEDIA_CATALOG_SCHEMA_VERSION
        ) {
            return Err(StoreProtocolError::Invalid(format!(
                "catalog schema must be {CATALOG_SCHEMA_VERSION}, {RICH_CATALOG_SCHEMA_VERSION} or {MEDIA_CATALOG_SCHEMA_VERSION}"
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
            match (
                self.schema_version,
                app.discovery.is_some(),
                app.resources.is_some(),
            ) {
                (CATALOG_SCHEMA_VERSION, false, false)
                | (RICH_CATALOG_SCHEMA_VERSION, true, false)
                | (MEDIA_CATALOG_SCHEMA_VERSION, true, true) => {}
                (CATALOG_SCHEMA_VERSION, _, _) => {
                    return Err(StoreProtocolError::Invalid(
                        "Catalog v1 application contains newer metadata".into(),
                    ));
                }
                (RICH_CATALOG_SCHEMA_VERSION, _, _) => {
                    return Err(StoreProtocolError::Invalid(
                        "Catalog v2 application metadata is incomplete or includes v3 resources"
                            .into(),
                    ));
                }
                (MEDIA_CATALOG_SCHEMA_VERSION, _, _) => {
                    return Err(StoreProtocolError::Invalid(
                        "Catalog v3 application is missing discovery or resource metadata".into(),
                    ));
                }
                _ => unreachable!("catalog schema was validated above"),
            }
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
        if let Some(discovery) = &self.discovery {
            discovery.validate(&self.summary)?;
        }
        if let Some(resources) = &self.resources {
            resources.validate()?;
        }
        Ok(())
    }
}

impl CatalogResources {
    fn validate(&self) -> Result<(), StoreProtocolError> {
        self.icon.validate_icon()?;
        self.details
            .validate(MAX_APP_DETAILS_BYTES as u64, "details")
    }
}

impl CatalogObjectResource {
    fn validate(&self, max_bytes: u64, kind: &str) -> Result<(), StoreProtocolError> {
        if !is_valid_https_url(&self.url)
            || !is_lower_hex(&self.sha256, 32)
            || !(1..=max_bytes).contains(&self.bytes)
        {
            return Err(StoreProtocolError::Invalid(format!(
                "catalog {kind} resource is invalid"
            )));
        }
        Ok(())
    }
}

impl CatalogImageResource {
    fn validate_icon(&self) -> Result<(), StoreProtocolError> {
        if !matches!((self.width, self.height), (32, 32) | (48, 48)) {
            return Err(StoreProtocolError::Invalid(
                "catalog icon dimensions are invalid".into(),
            ));
        }
        self.validate(cp0_store_metadata::MAX_ICON_BYTES, "icon")
    }

    fn validate_screenshot(&self) -> Result<(), StoreProtocolError> {
        if (self.width, self.height) != (320, 170) {
            return Err(StoreProtocolError::Invalid(
                "catalog screenshot dimensions are invalid".into(),
            ));
        }
        self.validate(cp0_store_metadata::MAX_SCREENSHOT_BYTES, "screenshot")
    }

    fn validate(&self, max_bytes: u64, kind: &str) -> Result<(), StoreProtocolError> {
        CatalogObjectResource {
            url: self.url.clone(),
            sha256: self.sha256.clone(),
            bytes: self.bytes,
        }
        .validate(max_bytes, kind)
    }
}

impl StoreAppDetails {
    pub fn validate(&self) -> Result<(), StoreProtocolError> {
        if self.schema_version != APP_DETAILS_SCHEMA_VERSION
            || !cp0_manifest::is_valid_app_id(&self.app_id)
            || !cp0_manifest::is_valid_app_version(&self.version)
        {
            return Err(StoreProtocolError::Invalid(
                "Store application details identity is invalid".into(),
            ));
        }
        validate_detail_text(&self.description, 1024, "description")?;
        validate_detail_text(&self.release_notes, 512, "release notes")?;
        if !(1..=cp0_store_metadata::MAX_SCREENSHOTS).contains(&self.screenshots.len()) {
            return Err(StoreProtocolError::Invalid(
                "Store application screenshot count is outside limits".into(),
            ));
        }
        let mut urls = BTreeSet::new();
        for screenshot in &self.screenshots {
            screenshot.validate_screenshot()?;
            if !urls.insert(screenshot.url.as_str()) {
                return Err(StoreProtocolError::Invalid(
                    "Store application screenshot URLs are duplicated".into(),
                ));
            }
        }
        Ok(())
    }
}

fn validate_detail_text(
    value: &str,
    max_chars: usize,
    kind: &str,
) -> Result<(), StoreProtocolError> {
    let chars = value.chars().count();
    if !(1..=max_chars).contains(&chars)
        || value.trim() != value
        || value
            .chars()
            .any(|character| character.is_control() && character != '\n')
    {
        return Err(StoreProtocolError::Invalid(format!(
            "Store application {kind} is invalid"
        )));
    }
    Ok(())
}

impl CatalogDiscovery {
    fn validate(&self, summary: &str) -> Result<(), StoreProtocolError> {
        let developer_chars = self.developer.chars().count();
        if !(1..=80).contains(&developer_chars) || has_unsafe_text(&self.developer) {
            return Err(StoreProtocolError::Invalid(
                "catalog developer name is invalid".into(),
            ));
        }
        let subtitle_chars = self.subtitle.chars().count();
        if !(1..=48).contains(&subtitle_chars)
            || has_unsafe_text(&self.subtitle)
            || self.subtitle != summary
        {
            return Err(StoreProtocolError::Invalid(
                "catalog subtitle is invalid or differs from the summary".into(),
            ));
        }
        if self.keywords.len() > cp0_store_metadata::MAX_KEYWORDS {
            return Err(StoreProtocolError::Invalid(
                "catalog keyword count is outside limits".into(),
            ));
        }
        let mut previous_keyword: Option<&str> = None;
        for keyword in &self.keywords {
            let chars = keyword.chars().count();
            if !(1..=24).contains(&chars)
                || keyword.len() > 48
                || has_unsafe_text(keyword)
                || previous_keyword.is_some_and(|previous| previous >= keyword.as_str())
            {
                return Err(StoreProtocolError::Invalid(
                    "catalog keywords are invalid, duplicated or unsorted".into(),
                ));
            }
            previous_keyword = Some(keyword);
        }
        if !is_valid_https_url(&self.privacy_url) || !is_valid_https_url(&self.support_url) {
            return Err(StoreProtocolError::Invalid(
                "catalog privacy and support URLs must be bounded HTTPS URLs".into(),
            ));
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
        match &self.command {
            StoreCommand::Search {
                query,
                offset,
                limit,
            } => {
                validate_search_query(query)?;
                validate_search_page(*offset, *limit)?;
            }
            StoreCommand::Install { app_id }
            | StoreCommand::Control { app_id, .. }
            | StoreCommand::Details { app_id }
            | StoreCommand::Media { app_id, .. } => {
                if !cp0_manifest::is_valid_app_id(app_id) {
                    return Err(StoreProtocolError::Invalid(
                        "store command application ID is invalid".into(),
                    ));
                }
            }
            StoreCommand::List | StoreCommand::Refresh => {}
        }
        if let StoreCommand::Media {
            media: StoreMediaSelector::Screenshot { index },
            ..
        } = &self.command
        {
            if usize::from(*index) >= cp0_store_metadata::MAX_SCREENSHOTS {
                return Err(StoreProtocolError::Invalid(
                    "store screenshot index is outside limits".into(),
                ));
            }
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
            Self::SearchResults {
                query,
                offset,
                limit,
                total,
                next_offset,
                sequence,
                expires_unix_seconds,
                apps,
                ..
            } => {
                validate_search_query(query)?;
                validate_search_page(*offset, *limit)?;
                if *sequence == 0 || *expires_unix_seconds == 0 {
                    return Err(StoreProtocolError::Invalid(
                        "store search catalog metadata is invalid".into(),
                    ));
                }
                if usize::from(*total) > MAX_CATALOG_APPS {
                    return Err(StoreProtocolError::Invalid(
                        "store search total exceeds the catalog limit".into(),
                    ));
                }
                let remaining = total.saturating_sub(*offset);
                let expected_page = remaining.min(u16::from(*limit));
                if apps.len() != usize::from(expected_page) {
                    return Err(StoreProtocolError::Invalid(
                        "store search result page length is inconsistent".into(),
                    ));
                }
                let expected_next = offset
                    .checked_add(expected_page)
                    .filter(|next| *next < *total);
                if *next_offset != expected_next {
                    return Err(StoreProtocolError::Invalid(
                        "store search next offset is inconsistent".into(),
                    ));
                }
                let mut ids = BTreeSet::new();
                for app in apps {
                    app.validate()?;
                    if !ids.insert(app.app_id.as_str()) {
                        return Err(StoreProtocolError::Invalid(
                            "store search response contains duplicate applications".into(),
                        ));
                    }
                }
                Ok(())
            }
            Self::RefreshAccepted => Ok(()),
            Self::InstallAccepted { app_id, version }
            | Self::OperationAccepted {
                app_id, version, ..
            } => {
                if !cp0_manifest::is_valid_app_id(app_id)
                    || !cp0_manifest::is_valid_app_version(version)
                {
                    return Err(StoreProtocolError::Invalid(
                        "store install response identity is invalid".into(),
                    ));
                }
                Ok(())
            }
            Self::AppDetails {
                app_id,
                version,
                developer,
                privacy_url,
                support_url,
                description,
                release_notes,
                screenshot_count,
                ..
            } => {
                if !cp0_manifest::is_valid_app_id(app_id)
                    || !cp0_manifest::is_valid_app_version(version)
                {
                    return Err(StoreProtocolError::Invalid(
                        "store details response identity is invalid".into(),
                    ));
                }
                let developer_chars = developer.chars().count();
                if !(1..=80).contains(&developer_chars) || has_unsafe_text(developer) {
                    return Err(StoreProtocolError::Invalid(
                        "store details developer is invalid".into(),
                    ));
                }
                if !is_valid_https_url(privacy_url) || !is_valid_https_url(support_url) {
                    return Err(StoreProtocolError::Invalid(
                        "store details links are invalid".into(),
                    ));
                }
                validate_detail_text(description, 1024, "response description")?;
                validate_detail_text(release_notes, 512, "response release notes")?;
                if !(1..=cp0_store_metadata::MAX_SCREENSHOTS)
                    .contains(&usize::from(*screenshot_count))
                {
                    return Err(StoreProtocolError::Invalid(
                        "store details screenshot count is outside limits".into(),
                    ));
                }
                Ok(())
            }
            Self::Media {
                app_id,
                version,
                media,
            } => {
                if !cp0_manifest::is_valid_app_id(app_id)
                    || !cp0_manifest::is_valid_app_version(version)
                {
                    return Err(StoreProtocolError::Invalid(
                        "store media response identity is invalid".into(),
                    ));
                }
                media.validate()
            }
        }
    }
}

impl StoreMediaMetadata {
    fn validate(&self) -> Result<(), StoreProtocolError> {
        let (sha256, bytes, width, height, maximum_bytes) = match self {
            Self::Icon {
                sha256,
                bytes,
                width,
                height,
            } => {
                if !matches!((*width, *height), (32, 32) | (48, 48)) {
                    return Err(StoreProtocolError::Invalid(
                        "store icon response dimensions are invalid".into(),
                    ));
                }
                (
                    sha256,
                    bytes,
                    width,
                    height,
                    cp0_store_metadata::MAX_ICON_BYTES,
                )
            }
            Self::Screenshot {
                index,
                sha256,
                bytes,
                width,
                height,
            } => {
                if usize::from(*index) >= cp0_store_metadata::MAX_SCREENSHOTS
                    || (*width, *height) != (320, 170)
                {
                    return Err(StoreProtocolError::Invalid(
                        "store screenshot response metadata is invalid".into(),
                    ));
                }
                (
                    sha256,
                    bytes,
                    width,
                    height,
                    cp0_store_metadata::MAX_SCREENSHOT_BYTES,
                )
            }
        };
        if !is_lower_hex(sha256, 32) || !(1..=maximum_bytes).contains(bytes) {
            return Err(StoreProtocolError::Invalid(
                "store media response descriptor is invalid".into(),
            ));
        }
        let _ = (width, height);
        Ok(())
    }
}

pub fn validate_search_query(query: &str) -> Result<(), StoreProtocolError> {
    let chars = query.chars().count();
    if !(1..=MAX_SEARCH_QUERY_CHARS).contains(&chars)
        || query.len() > MAX_SEARCH_QUERY_BYTES
        || query.trim() != query
        || has_unsafe_text(query)
    {
        return Err(StoreProtocolError::Invalid(
            "store search query is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_search_page(offset: u16, limit: u8) -> Result<(), StoreProtocolError> {
    if usize::from(offset) > MAX_CATALOG_APPS || !(1..=MAX_SEARCH_PAGE_APPS).contains(&limit) {
        return Err(StoreProtocolError::Invalid(
            "store search page is outside limits".into(),
        ));
    }
    Ok(())
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
            StoreAppState::Available
            | StoreAppState::Queued
            | StoreAppState::Canceled
            | StoreAppState::Failed => self.progress_percent == 0,
            StoreAppState::Downloading | StoreAppState::Paused => self.progress_percent <= 100,
            StoreAppState::Installing | StoreAppState::Installed => self.progress_percent == 100,
        };
        if !valid_progress {
            return Err(StoreProtocolError::Invalid(
                "store response progress is inconsistent with application state".into(),
            ));
        }
        if (self.state == StoreAppState::Failed) != self.failure_reason.is_some() {
            return Err(StoreProtocolError::Invalid(
                "store response failure reason is inconsistent with application state".into(),
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

pub fn encode_app_details(details: &StoreAppDetails) -> Result<Vec<u8>, StoreProtocolError> {
    details.validate()?;
    let encoded = serde_json::to_vec(details)?;
    if encoded.len() > MAX_APP_DETAILS_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    Ok(encoded)
}

pub fn decode_app_details(encoded: &[u8]) -> Result<StoreAppDetails, StoreProtocolError> {
    if encoded.len() > MAX_APP_DETAILS_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    let details: StoreAppDetails = serde_json::from_slice(encoded)?;
    details.validate()?;
    Ok(details)
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

pub fn response_requires_descriptor(response: &StoreResponse) -> bool {
    matches!(
        response.outcome,
        StoreOutcome::Ok {
            data: StoreResponseData::Media { .. }
        }
    )
}

pub fn encode_response_frame(response: &StoreResponse) -> Result<Vec<u8>, StoreProtocolError> {
    response.validate()?;
    encode_frame(response)
}

pub fn send_response_with_fd(
    stream: &mut UnixStream,
    response: &StoreResponse,
    descriptor: BorrowedFd<'_>,
) -> Result<(), StoreProtocolError> {
    if !response_requires_descriptor(response) {
        return Err(StoreProtocolError::InvalidDescriptor);
    }
    let frame = encode_response_frame(response)?;
    let mut io_vector = libc::iovec {
        iov_base: frame.as_ptr().cast_mut().cast(),
        iov_len: frame.len(),
    };
    let control_length = unsafe { libc::CMSG_SPACE(mem::size_of::<RawFd>() as u32) } as usize;
    let control_words = control_length.div_ceil(mem::size_of::<usize>());
    let mut control = vec![0_usize; control_words];
    let mut message: libc::msghdr = unsafe { mem::zeroed() };
    message.msg_iov = &raw mut io_vector;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = control_length
        .try_into()
        .map_err(|_| StoreProtocolError::InvalidDescriptor)?;

    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        if header.is_null() {
            return Err(StoreProtocolError::InvalidDescriptor);
        }
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(mem::size_of::<RawFd>() as u32) as _;
        libc::CMSG_DATA(header)
            .cast::<RawFd>()
            .write(descriptor.as_raw_fd());
    }
    let count = loop {
        let result = unsafe { libc::sendmsg(stream.as_raw_fd(), &message, libc::MSG_NOSIGNAL) };
        if result < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if result < 0 {
            return Err(StoreProtocolError::Io(io::Error::last_os_error()));
        }
        break result as usize;
    };
    if count == 0 || count > frame.len() {
        return Err(StoreProtocolError::InvalidDescriptor);
    }
    stream.write_all(&frame[count..])?;
    stream.flush()?;
    Ok(())
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
    let encoded = encode_frame(value)?;
    writer.write_all(&encoded)?;
    writer.flush()?;
    Ok(())
}

fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, StoreProtocolError> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    if encoded.len() > MAX_FRAME_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

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
                discovery: None,
                resources: None,
            }],
        }
    }

    fn rich_catalog() -> Catalog {
        let mut catalog = catalog();
        catalog.schema_version = RICH_CATALOG_SCHEMA_VERSION;
        catalog.apps[0].discovery = Some(CatalogDiscovery {
            developer: "CardputerZero Labs".into(),
            subtitle: catalog.apps[0].summary.clone(),
            category: StoreCategory::Utilities,
            keywords: vec!["example".into(), "utility".into()],
            age_rating: AgeRating::FourPlus,
            privacy_url: "https://example.com/privacy".into(),
            support_url: "https://example.com/support".into(),
        });
        catalog.apps[0].resources = None;
        catalog
    }

    fn media_catalog() -> Catalog {
        let mut catalog = rich_catalog();
        catalog.schema_version = MEDIA_CATALOG_SCHEMA_VERSION;
        catalog.apps[0].resources = Some(CatalogResources {
            icon: CatalogImageResource {
                url: "https://store.example.com/generations/7/assets/example/icon.png".into(),
                sha256: "22".repeat(32),
                bytes: 2048,
                width: 48,
                height: 48,
            },
            details: CatalogObjectResource {
                url: "https://store.example.com/generations/7/assets/example/details.json".into(),
                sha256: "33".repeat(32),
                bytes: 1024,
            },
        });
        catalog
    }

    fn app_details() -> StoreAppDetails {
        StoreAppDetails {
            schema_version: APP_DETAILS_SCHEMA_VERSION,
            app_id: "dev.cardputerzero.example".into(),
            version: "1.2.3".into(),
            description: "A complete bounded application description.".into(),
            release_notes: "Adds signed media resources.".into(),
            screenshots: vec![CatalogImageResource {
                url: "https://store.example.com/generations/7/assets/example/screenshots/0.png"
                    .into(),
                sha256: "44".repeat(32),
                bytes: 8192,
                width: 320,
                height: 170,
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
            failure_reason: None,
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
    fn validates_strict_rich_discovery_catalogs_without_weakening_v1() {
        rich_catalog().validate().unwrap();

        let mut missing = rich_catalog();
        missing.apps[0].discovery = None;
        assert!(missing.validate().is_err());

        let mut legacy_with_rich_fields = rich_catalog();
        legacy_with_rich_fields.schema_version = CATALOG_SCHEMA_VERSION;
        assert!(legacy_with_rich_fields.validate().is_err());

        let mut noncanonical = rich_catalog();
        noncanonical.apps[0]
            .discovery
            .as_mut()
            .unwrap()
            .keywords
            .reverse();
        assert!(noncanonical.validate().is_err());

        let mut mismatched = rich_catalog();
        mismatched.apps[0].summary = "A different summary".into();
        assert!(mismatched.validate().is_err());
    }

    #[test]
    fn validates_media_catalogs_and_bounded_app_details() {
        media_catalog().validate().unwrap();
        let encoded = encode_app_details(&app_details()).unwrap();
        assert_eq!(decode_app_details(&encoded).unwrap(), app_details());

        let mut v2_with_resources = media_catalog();
        v2_with_resources.schema_version = RICH_CATALOG_SCHEMA_VERSION;
        assert!(v2_with_resources.validate().is_err());

        let mut v3_without_resources = media_catalog();
        v3_without_resources.apps[0].resources = None;
        assert!(v3_without_resources.validate().is_err());

        let mut invalid_icon = media_catalog();
        invalid_icon.apps[0].resources.as_mut().unwrap().icon.width = 47;
        assert!(invalid_icon.validate().is_err());

        let mut duplicate = app_details();
        duplicate.screenshots.push(duplicate.screenshots[0].clone());
        assert!(duplicate.validate().is_err());

        let mut unsafe_details = app_details();
        unsafe_details.description = "unsafe\tdescription".into();
        assert!(unsafe_details.validate().is_err());

        let mut multiline = app_details();
        multiline.description = "First paragraph.\nSecond paragraph.".into();
        multiline.validate().unwrap();
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

        let search = StoreRequest {
            protocol_version: STORE_PROTOCOL_VERSION,
            request_id: 10,
            command: StoreCommand::Search {
                query: "notes and tools".into(),
                offset: 8,
                limit: MAX_SEARCH_PAGE_APPS,
            },
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &search).unwrap();
        assert_eq!(read_request(&mut encoded.as_slice()).unwrap(), Some(search));

        for query in ["", " leading", "trailing ", "unsafe\nquery"] {
            let invalid = StoreRequest {
                protocol_version: STORE_PROTOCOL_VERSION,
                request_id: 10,
                command: StoreCommand::Search {
                    query: query.into(),
                    offset: 0,
                    limit: 1,
                },
            };
            assert!(invalid.validate().is_err());
        }
        assert!(validate_search_query(&"界".repeat(MAX_SEARCH_QUERY_CHARS)).is_ok());
        assert!(validate_search_query(&"界".repeat(MAX_SEARCH_QUERY_CHARS + 1)).is_err());
        assert!(validate_search_query(&"a".repeat(MAX_SEARCH_QUERY_BYTES + 1)).is_err());

        for (offset, limit) in [(0, 0), (0, MAX_SEARCH_PAGE_APPS + 1), (65, 1)] {
            let invalid = StoreRequest {
                protocol_version: STORE_PROTOCOL_VERSION,
                request_id: 10,
                command: StoreCommand::Search {
                    query: "notes".into(),
                    offset,
                    limit,
                },
            };
            assert!(invalid.validate().is_err());
        }

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

        let search = StoreResponse::success(
            4,
            StoreResponseData::SearchResults {
                query: "example".into(),
                offset: 0,
                limit: 1,
                total: 2,
                next_offset: Some(1),
                sequence: 2,
                expires_unix_seconds: 1_900_000_000,
                stale: true,
                apps: vec![response_app()],
            },
        );
        let mut encoded = Vec::new();
        write_response(&mut encoded, &search).unwrap();
        assert_eq!(
            read_response(&mut encoded.as_slice()).unwrap(),
            Some(search)
        );

        let invalid_next = StoreResponse::success(
            4,
            StoreResponseData::SearchResults {
                query: "example".into(),
                offset: 0,
                limit: 1,
                total: 2,
                next_offset: None,
                sequence: 2,
                expires_unix_seconds: 1_900_000_000,
                stale: false,
                apps: vec![response_app()],
            },
        );
        assert!(write_response(&mut Vec::new(), &invalid_next).is_err());

        let invalid_page_length = StoreResponse::success(
            4,
            StoreResponseData::SearchResults {
                query: "example".into(),
                offset: 0,
                limit: 2,
                total: 2,
                next_offset: None,
                sequence: 2,
                expires_unix_seconds: 1_900_000_000,
                stale: false,
                apps: vec![response_app()],
            },
        );
        assert!(write_response(&mut Vec::new(), &invalid_page_length).is_err());

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
    fn validates_rich_details_and_media_contracts() {
        let details = StoreResponse::success(
            41,
            StoreResponseData::AppDetails {
                app_id: "dev.cardputerzero.example".into(),
                version: "1.2.3".into(),
                developer: "CardputerZero Labs".into(),
                category: StoreCategory::Utilities,
                age_rating: AgeRating::FourPlus,
                privacy_url: "https://example.com/privacy".into(),
                support_url: "https://example.com/support".into(),
                description: "A complete signed description.".into(),
                release_notes: "Adds verified screenshots.".into(),
                screenshot_count: 2,
            },
        );
        let mut encoded = Vec::new();
        write_response(&mut encoded, &details).unwrap();
        assert_eq!(
            read_response(&mut encoded.as_slice()).unwrap(),
            Some(details)
        );

        let media = StoreResponse::success(
            42,
            StoreResponseData::Media {
                app_id: "dev.cardputerzero.example".into(),
                version: "1.2.3".into(),
                media: StoreMediaMetadata::Screenshot {
                    index: 1,
                    sha256: "44".repeat(32),
                    bytes: 8192,
                    width: 320,
                    height: 170,
                },
            },
        );
        assert!(response_requires_descriptor(&media));
        assert!(write_response(&mut Vec::new(), &media).is_ok());

        let invalid_request = StoreRequest {
            protocol_version: STORE_PROTOCOL_VERSION,
            request_id: 43,
            command: StoreCommand::Media {
                app_id: "dev.cardputerzero.example".into(),
                media: StoreMediaSelector::Screenshot { index: 5 },
            },
        };
        assert!(invalid_request.validate().is_err());

        let mut invalid_media = media;
        if let StoreOutcome::Ok {
            data:
                StoreResponseData::Media {
                    media: StoreMediaMetadata::Screenshot { width, .. },
                    ..
                },
        } = &mut invalid_media.outcome
        {
            *width = 319;
        }
        assert!(invalid_media.validate().is_err());
    }

    #[test]
    fn validates_bounded_download_controls_and_failure_reasons() {
        let request = StoreRequest {
            protocol_version: STORE_PROTOCOL_VERSION,
            request_id: 44,
            command: StoreCommand::Control {
                app_id: "dev.cardputerzero.example".into(),
                action: StoreControlAction::Pause,
            },
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &request).unwrap();
        assert_eq!(
            read_request(&mut encoded.as_slice()).unwrap(),
            Some(request)
        );

        let accepted = StoreResponse::success(
            44,
            StoreResponseData::OperationAccepted {
                app_id: "dev.cardputerzero.example".into(),
                version: "1.2.3".into(),
                action: StoreControlAction::Pause,
            },
        );
        write_response(&mut Vec::new(), &accepted).unwrap();

        let mut failed = response_app();
        failed.state = StoreAppState::Failed;
        failed.failure_reason = Some(StoreFailureReason::Network);
        let failed_response = StoreResponse::success(
            45,
            StoreResponseData::Catalog {
                sequence: 1,
                expires_unix_seconds: 1_900_000_000,
                stale: false,
                apps: vec![failed.clone()],
            },
        );
        write_response(&mut Vec::new(), &failed_response).unwrap();

        failed.state = StoreAppState::Paused;
        failed.progress_percent = 42;
        assert!(
            StoreResponse::success(
                46,
                StoreResponseData::Catalog {
                    sequence: 1,
                    expires_unix_seconds: 1_900_000_000,
                    stale: false,
                    apps: vec![failed],
                },
            )
            .validate()
            .is_err()
        );

        let mut missing_reason = response_app();
        missing_reason.state = StoreAppState::Failed;
        assert!(
            StoreResponse::success(
                47,
                StoreResponseData::Catalog {
                    sequence: 1,
                    expires_unix_seconds: 1_900_000_000,
                    stale: false,
                    apps: vec![missing_reason],
                },
            )
            .validate()
            .is_err()
        );
    }

    #[test]
    fn transfers_exactly_one_media_descriptor() {
        let response = StoreResponse::success(
            51,
            StoreResponseData::Media {
                app_id: "dev.cardputerzero.example".into(),
                version: "1.2.3".into(),
                media: StoreMediaMetadata::Icon {
                    sha256: "22".repeat(32),
                    bytes: 2048,
                    width: 48,
                    height: 48,
                },
            },
        );
        let descriptor = std::fs::File::open("Cargo.toml").unwrap();
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        send_response_with_fd(&mut sender, &response, descriptor.as_fd()).unwrap();

        let mut frame = [0_u8; MAX_FRAME_BYTES];
        let mut io_vector = libc::iovec {
            iov_base: frame.as_mut_ptr().cast(),
            iov_len: frame.len(),
        };
        let control_length = unsafe { libc::CMSG_SPACE(mem::size_of::<RawFd>() as u32) } as usize;
        let mut control = vec![0_u8; control_length];
        let mut message: libc::msghdr = unsafe { mem::zeroed() };
        message.msg_iov = &raw mut io_vector;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = control_length.try_into().unwrap();
        let count = unsafe { libc::recvmsg(receiver.as_raw_fd(), &raw mut message, 0) };
        assert!(count > 0);
        assert_eq!(frame[count as usize - 1], b'\n');
        unsafe {
            let header = libc::CMSG_FIRSTHDR(&message);
            assert!(!header.is_null());
            assert_eq!((*header).cmsg_level, libc::SOL_SOCKET);
            assert_eq!((*header).cmsg_type, libc::SCM_RIGHTS);
            let received = libc::CMSG_DATA(header).cast::<RawFd>().read();
            assert!(received >= 0);
            libc::close(received);
            assert!(libc::CMSG_NXTHDR(&message, header).is_null());
        }
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
