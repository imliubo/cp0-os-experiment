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
use sha2::{Digest, Sha256};

pub const STORE_PROTOCOL_VERSION: u32 = 1;
pub const CATALOG_SCHEMA_VERSION: u32 = 1;
pub const RICH_CATALOG_SCHEMA_VERSION: u32 = 2;
pub const MEDIA_CATALOG_SCHEMA_VERSION: u32 = 3;
pub const EDITORIAL_CATALOG_SCHEMA_VERSION: u32 = 4;
pub const CATALOG_INDEX_SCHEMA_VERSION: u32 = 1;
pub const CATALOG_SHARD_SCHEMA_VERSION: u32 = 1;
pub const APP_DETAILS_SCHEMA_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
pub const MAX_CATALOG_BYTES: usize = 48 * 1024;
pub const MAX_APP_DETAILS_BYTES: usize = 16 * 1024;
pub const MAX_CATALOG_APPS: usize = 64;
pub const MAX_CATALOG_SHARDS: usize = 16;
pub const MAX_SHARDED_CATALOG_APPS: usize = MAX_CATALOG_APPS * MAX_CATALOG_SHARDS;
pub const MAX_PACKAGE_BYTES: u64 = cp0_package::MAX_PAYLOAD_BYTES as u64 + 4096;
pub const MAX_PACKAGE_URL_BYTES: usize = 2048;
pub const MAX_SUMMARY_CHARS: usize = 96;
pub const MAX_SEARCH_QUERY_CHARS: usize = 32;
pub const MAX_SEARCH_QUERY_BYTES: usize = 96;
pub const MAX_SEARCH_PAGE_APPS: u8 = 8;
pub const MAX_INSTALL_BATCH_APPS: usize = 8;
pub const MAX_EDITORIAL_COLLECTIONS: usize = 2;
pub const MAX_EDITORIAL_COLLECTION_APPS: usize = 4;
pub const MAX_EDITORIAL_HEADLINE_CHARS: usize = 48;
pub const MAX_EDITORIAL_COLLECTION_TITLE_CHARS: usize = 32;
pub const MAX_ERROR_MESSAGE_CHARS: usize = 160;
pub const MAX_CATALOG_LIFETIME_SECONDS: u64 = 31 * 24 * 60 * 60;

const CATALOG_SIGNATURE_DOMAIN: &[u8] = b"CardputerZero store catalog signature v1\0";
const CATALOG_INDEX_SIGNATURE_DOMAIN: &[u8] = b"CardputerZero store catalog index signature v1\0";
const CATALOG_SHARD_SIGNATURE_DOMAIN: &[u8] = b"CardputerZero store catalog shard signature v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub sequence: u64,
    pub published_unix_seconds: u64,
    pub expires_unix_seconds: u64,
    pub apps: Vec<CatalogApp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editorial: Option<CatalogEditorial>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEditorial {
    pub headline: String,
    pub featured_app_id: String,
    pub collections: Vec<CatalogEditorialCollection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEditorialCollection {
    pub title: String,
    pub app_ids: Vec<String>,
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
pub struct CatalogIndex {
    pub schema_version: u32,
    pub catalog_schema_version: u32,
    pub sequence: u64,
    pub published_unix_seconds: u64,
    pub expires_unix_seconds: u64,
    pub total_app_count: u16,
    pub shards: Vec<CatalogShardDescriptor>,
    pub categories: Vec<CatalogCategoryIndex>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editorial: Option<CatalogEditorial>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogShardDescriptor {
    pub index: u16,
    pub url: String,
    pub sha256: String,
    pub bytes: u32,
    pub app_count: u16,
    pub first_app_id: String,
    pub last_app_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogCategoryIndex {
    pub category: StoreCategory,
    pub app_count: u16,
    pub shard_indices: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogShard {
    pub schema_version: u32,
    pub catalog_schema_version: u32,
    pub sequence: u64,
    pub index: u16,
    pub apps: Vec<CatalogApp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCatalogIndex {
    pub catalog_index: CatalogIndex,
    pub key_id: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCatalogShard {
    pub catalog_shard: CatalogShard,
    pub key_id: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SignedCatalogDocument {
    Catalog(SignedCatalog),
    Index(SignedCatalogIndex),
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
    Browse {
        #[serde(skip_serializing_if = "Option::is_none")]
        category: Option<StoreCategory>,
        offset: u16,
        limit: u8,
    },
    Today,
    Search {
        query: String,
        offset: u16,
        limit: u8,
    },
    Refresh,
    GetAutoUpdate,
    SetAutoUpdate {
        enabled: bool,
    },
    RunAutoUpdate,
    GetMetrics,
    SetMetrics {
        enabled: bool,
    },
    RecordRuntimeMetric {
        app_id: String,
        version: String,
        event: StoreRuntimeMetricEvent,
    },
    PreflightInstall {
        app_ids: Vec<String>,
        catalog_sequence: u64,
    },
    Install {
        app_id: String,
        authorization_id: u64,
    },
    InstallBatch {
        app_ids: Vec<String>,
        authorization_id: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StoreRuntimeMetricEvent {
    Launch,
    Crash,
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
    Today {
        sequence: u64,
        expires_unix_seconds: u64,
        stale: bool,
        editorial: Option<StoreEditorial>,
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
    BrowseResults {
        #[serde(skip_serializing_if = "Option::is_none")]
        category: Option<StoreCategory>,
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
    AutoUpdateStatus {
        enabled: bool,
        policy_allowed: bool,
        charging: bool,
        unmetered_network: bool,
        due: bool,
        checking: bool,
    },
    AutoUpdateAccepted,
    MetricsStatus {
        enabled: bool,
        policy_allowed: bool,
        configured: bool,
        pending: bool,
    },
    MetricRecorded,
    InstallPreflight {
        authorization_id: u64,
        catalog_sequence: u64,
        required_bytes: u64,
        available_bytes: u64,
        apps: Vec<StoreInstallPreflight>,
    },
    InstallAccepted {
        app_id: String,
        version: String,
    },
    InstallBatchAccepted {
        apps: Vec<StoreInstallAccepted>,
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
#[serde(deny_unknown_fields)]
pub struct StoreInstallAccepted {
    pub app_id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreInstallPreflight {
    pub app_id: String,
    pub version: String,
    pub permissions: Vec<Permission>,
    pub policy_denied_permissions: Vec<Permission>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreEditorial {
    pub headline: String,
    pub featured: StoreAppSummary,
    pub collections: Vec<StoreEditorialCollection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreEditorialCollection {
    pub title: String,
    pub apps: Vec<StoreAppSummary>,
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
    PolicyRestricted,
    InsufficientStorage,
    CatalogChanged,
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
            CATALOG_SCHEMA_VERSION
                | RICH_CATALOG_SCHEMA_VERSION
                | MEDIA_CATALOG_SCHEMA_VERSION
                | EDITORIAL_CATALOG_SCHEMA_VERSION
        ) {
            return Err(StoreProtocolError::Invalid(format!(
                "catalog schema must be {CATALOG_SCHEMA_VERSION}, {RICH_CATALOG_SCHEMA_VERSION}, {MEDIA_CATALOG_SCHEMA_VERSION} or {EDITORIAL_CATALOG_SCHEMA_VERSION}"
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
        match (self.schema_version, self.editorial.is_some()) {
            (EDITORIAL_CATALOG_SCHEMA_VERSION, true)
            | (CATALOG_SCHEMA_VERSION, false)
            | (RICH_CATALOG_SCHEMA_VERSION, false)
            | (MEDIA_CATALOG_SCHEMA_VERSION, false) => {}
            (EDITORIAL_CATALOG_SCHEMA_VERSION, false) => {
                return Err(StoreProtocolError::Invalid(
                    "Catalog v4 is missing editorial metadata".into(),
                ));
            }
            _ => {
                return Err(StoreProtocolError::Invalid(
                    "older Catalog schema contains v4 editorial metadata".into(),
                ));
            }
        }

        let mut previous_id: Option<&str> = None;
        let mut ids = BTreeSet::new();
        for app in &self.apps {
            app.validate()?;
            match (
                self.schema_version,
                app.discovery.is_some(),
                app.resources.is_some(),
                self.editorial.is_some(),
            ) {
                (CATALOG_SCHEMA_VERSION, false, false, false)
                | (RICH_CATALOG_SCHEMA_VERSION, true, false, false)
                | (MEDIA_CATALOG_SCHEMA_VERSION, true, true, false)
                | (EDITORIAL_CATALOG_SCHEMA_VERSION, true, true, true) => {}
                (CATALOG_SCHEMA_VERSION, _, _, _) => {
                    return Err(StoreProtocolError::Invalid(
                        "Catalog v1 application contains newer metadata".into(),
                    ));
                }
                (RICH_CATALOG_SCHEMA_VERSION, _, _, _) => {
                    return Err(StoreProtocolError::Invalid(
                        "Catalog v2 application metadata is incomplete or includes v3 resources"
                            .into(),
                    ));
                }
                (MEDIA_CATALOG_SCHEMA_VERSION, _, _, _) => {
                    return Err(StoreProtocolError::Invalid(
                        "Catalog v3 application metadata is incomplete or includes v4 editorial data"
                            .into(),
                    ));
                }
                (EDITORIAL_CATALOG_SCHEMA_VERSION, _, _, _) => {
                    return Err(StoreProtocolError::Invalid(
                        "Catalog v4 application or editorial metadata is incomplete".into(),
                    ));
                }
                _ => unreachable!("catalog schema was validated above"),
            }
            if !ids.insert(app.app_id.clone()) {
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
        if let Some(editorial) = &self.editorial {
            editorial.validate(&ids)?;
        }
        Ok(())
    }
}

impl CatalogIndex {
    pub fn validate(&self) -> Result<(), StoreProtocolError> {
        if self.schema_version != CATALOG_INDEX_SCHEMA_VERSION
            || !matches!(
                self.catalog_schema_version,
                CATALOG_SCHEMA_VERSION
                    | RICH_CATALOG_SCHEMA_VERSION
                    | MEDIA_CATALOG_SCHEMA_VERSION
                    | EDITORIAL_CATALOG_SCHEMA_VERSION
            )
            || self.sequence == 0
        {
            return Err(StoreProtocolError::Invalid(
                "catalog index identity or schema is invalid".into(),
            ));
        }
        validate_catalog_lifetime(self.published_unix_seconds, self.expires_unix_seconds)?;
        let total = usize::from(self.total_app_count);
        if !(1..=MAX_SHARDED_CATALOG_APPS).contains(&total)
            || !(1..=MAX_CATALOG_SHARDS).contains(&self.shards.len())
        {
            return Err(StoreProtocolError::Invalid(
                "catalog index application or shard count is outside limits".into(),
            ));
        }
        match (self.catalog_schema_version, self.editorial.is_some()) {
            (EDITORIAL_CATALOG_SCHEMA_VERSION, true)
            | (CATALOG_SCHEMA_VERSION, false)
            | (RICH_CATALOG_SCHEMA_VERSION, false)
            | (MEDIA_CATALOG_SCHEMA_VERSION, false) => {}
            _ => {
                return Err(StoreProtocolError::Invalid(
                    "catalog index editorial metadata is inconsistent with its content schema"
                        .into(),
                ));
            }
        }
        if let Some(editorial) = &self.editorial {
            editorial.validate_structure()?;
        }

        let mut app_count = 0_usize;
        let mut previous_last: Option<&str> = None;
        for (expected_index, shard) in self.shards.iter().enumerate() {
            if usize::from(shard.index) != expected_index
                || !is_valid_https_url(&shard.url)
                || !is_lower_hex(&shard.sha256, 32)
                || !(1..=MAX_CATALOG_BYTES as u32).contains(&shard.bytes)
                || !(1..=MAX_CATALOG_APPS).contains(&usize::from(shard.app_count))
                || !cp0_manifest::is_valid_app_id(&shard.first_app_id)
                || !cp0_manifest::is_valid_app_id(&shard.last_app_id)
                || shard.first_app_id > shard.last_app_id
                || previous_last.is_some_and(|previous| previous >= shard.first_app_id.as_str())
            {
                return Err(StoreProtocolError::Invalid(
                    "catalog shard descriptor is invalid or unordered".into(),
                ));
            }
            app_count = app_count
                .checked_add(usize::from(shard.app_count))
                .ok_or_else(|| StoreProtocolError::Invalid("catalog app count overflow".into()))?;
            previous_last = Some(&shard.last_app_id);
        }
        if app_count != total {
            return Err(StoreProtocolError::Invalid(
                "catalog index total differs from its shard descriptors".into(),
            ));
        }

        let mut previous_category: Option<&str> = None;
        let mut category_total = 0_usize;
        for category in &self.categories {
            let name = category.category.as_str();
            if previous_category.is_some_and(|previous| previous >= name)
                || category.app_count == 0
                || category.shard_indices.is_empty()
                || category.shard_indices.len() > self.shards.len()
                || category
                    .shard_indices
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
                || category
                    .shard_indices
                    .iter()
                    .any(|index| usize::from(*index) >= self.shards.len())
            {
                return Err(StoreProtocolError::Invalid(
                    "catalog category index is invalid or unordered".into(),
                ));
            }
            category_total = category_total
                .checked_add(usize::from(category.app_count))
                .ok_or_else(|| StoreProtocolError::Invalid("category count overflow".into()))?;
            previous_category = Some(name);
        }
        if category_total != total {
            return Err(StoreProtocolError::Invalid(
                "catalog category total differs from the application total".into(),
            ));
        }
        Ok(())
    }
}

impl CatalogShard {
    pub fn validate(&self) -> Result<(), StoreProtocolError> {
        if self.schema_version != CATALOG_SHARD_SCHEMA_VERSION
            || self.sequence == 0
            || usize::from(self.index) >= MAX_CATALOG_SHARDS
            || self.apps.is_empty()
        {
            return Err(StoreProtocolError::Invalid(
                "catalog shard identity or bounds are invalid".into(),
            ));
        }
        let validation_schema = match self.catalog_schema_version {
            EDITORIAL_CATALOG_SCHEMA_VERSION => MEDIA_CATALOG_SCHEMA_VERSION,
            CATALOG_SCHEMA_VERSION | RICH_CATALOG_SCHEMA_VERSION | MEDIA_CATALOG_SCHEMA_VERSION => {
                self.catalog_schema_version
            }
            _ => {
                return Err(StoreProtocolError::Invalid(
                    "catalog shard content schema is invalid".into(),
                ));
            }
        };
        Catalog {
            schema_version: validation_schema,
            sequence: self.sequence,
            published_unix_seconds: 1,
            expires_unix_seconds: 2,
            apps: self.apps.clone(),
            editorial: None,
        }
        .validate()
    }
}

impl CatalogEditorial {
    fn validate(&self, catalog_app_ids: &BTreeSet<String>) -> Result<(), StoreProtocolError> {
        self.validate_structure()?;
        let mut referenced = BTreeSet::from([self.featured_app_id.as_str()]);
        if !catalog_app_ids.contains(self.featured_app_id.as_str()) {
            return Err(StoreProtocolError::Invalid(
                "catalog editorial application reference is invalid or duplicated".into(),
            ));
        }
        for collection in &self.collections {
            for app_id in &collection.app_ids {
                if !catalog_app_ids.contains(app_id.as_str()) || !referenced.insert(app_id) {
                    return Err(StoreProtocolError::Invalid(
                        "catalog editorial application reference is invalid or duplicated".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_structure(&self) -> Result<(), StoreProtocolError> {
        let headline_chars = self.headline.chars().count();
        if !(1..=MAX_EDITORIAL_HEADLINE_CHARS).contains(&headline_chars)
            || has_unsafe_text(&self.headline)
        {
            return Err(StoreProtocolError::Invalid(
                "catalog editorial headline is invalid".into(),
            ));
        }
        if !cp0_manifest::is_valid_app_id(&self.featured_app_id)
            || !(1..=MAX_EDITORIAL_COLLECTIONS).contains(&self.collections.len())
        {
            return Err(StoreProtocolError::Invalid(
                "catalog editorial identity or collection count is invalid".into(),
            ));
        }
        let mut titles = BTreeSet::new();
        let mut referenced = BTreeSet::from([self.featured_app_id.as_str()]);
        for collection in &self.collections {
            let title_chars = collection.title.chars().count();
            if !(1..=MAX_EDITORIAL_COLLECTION_TITLE_CHARS).contains(&title_chars)
                || has_unsafe_text(&collection.title)
                || !titles.insert(collection.title.as_str())
                || !(1..=MAX_EDITORIAL_COLLECTION_APPS).contains(&collection.app_ids.len())
            {
                return Err(StoreProtocolError::Invalid(
                    "catalog editorial collection is invalid".into(),
                ));
            }
            for app_id in &collection.app_ids {
                if !cp0_manifest::is_valid_app_id(app_id) || !referenced.insert(app_id) {
                    return Err(StoreProtocolError::Invalid(
                        "catalog editorial application reference is invalid or duplicated".into(),
                    ));
                }
            }
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
            StoreCommand::Browse { offset, limit, .. } => {
                validate_search_page(*offset, *limit)?;
            }
            StoreCommand::Search {
                query,
                offset,
                limit,
            } => {
                validate_search_query(query)?;
                validate_search_page(*offset, *limit)?;
            }
            StoreCommand::Install {
                app_id,
                authorization_id,
            } => {
                if !cp0_manifest::is_valid_app_id(app_id) || *authorization_id == 0 {
                    return Err(StoreProtocolError::Invalid(
                        "store authorized install identity is invalid".into(),
                    ));
                }
            }
            StoreCommand::Control { app_id, .. }
            | StoreCommand::Details { app_id }
            | StoreCommand::Media { app_id, .. } => {
                if !cp0_manifest::is_valid_app_id(app_id) {
                    return Err(StoreProtocolError::Invalid(
                        "store command application ID is invalid".into(),
                    ));
                }
            }
            StoreCommand::RecordRuntimeMetric {
                app_id, version, ..
            } => {
                if !cp0_manifest::is_valid_app_id(app_id)
                    || !cp0_manifest::is_valid_app_version(version)
                {
                    return Err(StoreProtocolError::Invalid(
                        "store runtime metric identity is invalid".into(),
                    ));
                }
            }
            StoreCommand::PreflightInstall {
                app_ids,
                catalog_sequence,
            } => {
                validate_install_batch_ids(app_ids)?;
                if *catalog_sequence == 0 {
                    return Err(StoreProtocolError::Invalid(
                        "store preflight catalog sequence must be non-zero".into(),
                    ));
                }
            }
            StoreCommand::InstallBatch {
                app_ids,
                authorization_id,
            } => {
                validate_install_batch_ids(app_ids)?;
                if *authorization_id == 0 {
                    return Err(StoreProtocolError::Invalid(
                        "store install authorization ID must be non-zero".into(),
                    ));
                }
            }
            StoreCommand::List
            | StoreCommand::Today
            | StoreCommand::Refresh
            | StoreCommand::GetAutoUpdate
            | StoreCommand::SetAutoUpdate { .. }
            | StoreCommand::RunAutoUpdate
            | StoreCommand::GetMetrics
            | StoreCommand::SetMetrics { .. } => {}
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
            Self::Today {
                sequence,
                expires_unix_seconds,
                editorial,
                ..
            } => {
                if *sequence == 0 || *expires_unix_seconds == 0 {
                    return Err(StoreProtocolError::Invalid(
                        "store Today catalog metadata is invalid".into(),
                    ));
                }
                if let Some(editorial) = editorial {
                    editorial.validate()?;
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
                if usize::from(*total) > MAX_SHARDED_CATALOG_APPS {
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
            Self::BrowseResults {
                category,
                offset,
                limit,
                total,
                next_offset,
                sequence,
                expires_unix_seconds,
                apps,
                ..
            } => {
                validate_search_page(*offset, *limit)?;
                if *sequence == 0
                    || *expires_unix_seconds == 0
                    || usize::from(*total) > MAX_SHARDED_CATALOG_APPS
                {
                    return Err(StoreProtocolError::Invalid(
                        "store browse catalog metadata is invalid".into(),
                    ));
                }
                let remaining = total.saturating_sub(*offset);
                let expected_page = remaining.min(u16::from(*limit));
                if apps.len() != usize::from(expected_page) {
                    return Err(StoreProtocolError::Invalid(
                        "store browse page length is inconsistent".into(),
                    ));
                }
                let expected_next = offset
                    .checked_add(expected_page)
                    .filter(|next| *next < *total);
                if *next_offset != expected_next {
                    return Err(StoreProtocolError::Invalid(
                        "store browse next offset is inconsistent".into(),
                    ));
                }
                let mut previous_id: Option<&str> = None;
                for app in apps {
                    app.validate()?;
                    if previous_id.is_some_and(|previous| previous >= app.app_id.as_str()) {
                        return Err(StoreProtocolError::Invalid(
                            "store browse applications are duplicated or unsorted".into(),
                        ));
                    }
                    previous_id = Some(&app.app_id);
                }
                let _ = category;
                Ok(())
            }
            Self::RefreshAccepted | Self::AutoUpdateAccepted | Self::MetricRecorded => Ok(()),
            Self::AutoUpdateStatus {
                enabled,
                due,
                checking,
                ..
            } => {
                if !enabled && (*due || *checking) {
                    return Err(StoreProtocolError::Invalid(
                        "disabled automatic update status cannot be due or checking".into(),
                    ));
                }
                Ok(())
            }
            Self::MetricsStatus {
                enabled,
                policy_allowed,
                configured,
                pending,
            } => {
                if *enabled && (!*policy_allowed || !*configured) || !*enabled && *pending {
                    return Err(StoreProtocolError::Invalid(
                        "store metrics status is inconsistent".into(),
                    ));
                }
                Ok(())
            }
            Self::InstallPreflight {
                authorization_id,
                catalog_sequence,
                required_bytes,
                available_bytes,
                apps,
            } => {
                if *authorization_id == 0
                    || *catalog_sequence == 0
                    || *required_bytes == 0
                    || *available_bytes < *required_bytes
                    || apps.is_empty()
                    || apps.len() > MAX_INSTALL_BATCH_APPS
                {
                    return Err(StoreProtocolError::Invalid(
                        "store install preflight bounds are invalid".into(),
                    ));
                }
                let mut previous = None;
                for app in apps {
                    app.validate()?;
                    if previous.is_some_and(|value| value >= app.app_id.as_str()) {
                        return Err(StoreProtocolError::Invalid(
                            "store install preflight applications are duplicated or unsorted"
                                .into(),
                        ));
                    }
                    previous = Some(app.app_id.as_str());
                }
                Ok(())
            }
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
            Self::InstallBatchAccepted { apps } => {
                if apps.is_empty() || apps.len() > MAX_INSTALL_BATCH_APPS {
                    return Err(StoreProtocolError::Invalid(
                        "store install batch response count is outside limits".into(),
                    ));
                }
                let mut previous = None;
                for app in apps {
                    if !cp0_manifest::is_valid_app_id(&app.app_id)
                        || !cp0_manifest::is_valid_app_version(&app.version)
                        || previous.is_some_and(|value| value >= app.app_id.as_str())
                    {
                        return Err(StoreProtocolError::Invalid(
                            "store install batch response identity is invalid".into(),
                        ));
                    }
                    previous = Some(app.app_id.as_str());
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

impl StoreEditorial {
    fn validate(&self) -> Result<(), StoreProtocolError> {
        let headline_chars = self.headline.chars().count();
        if !(1..=MAX_EDITORIAL_HEADLINE_CHARS).contains(&headline_chars)
            || has_unsafe_text(&self.headline)
            || !(1..=MAX_EDITORIAL_COLLECTIONS).contains(&self.collections.len())
        {
            return Err(StoreProtocolError::Invalid(
                "store Today editorial bounds are invalid".into(),
            ));
        }
        self.featured.validate()?;
        let mut app_ids = BTreeSet::from([self.featured.app_id.as_str()]);
        let mut titles = BTreeSet::new();
        for collection in &self.collections {
            let title_chars = collection.title.chars().count();
            if !(1..=MAX_EDITORIAL_COLLECTION_TITLE_CHARS).contains(&title_chars)
                || has_unsafe_text(&collection.title)
                || !titles.insert(collection.title.as_str())
                || !(1..=MAX_EDITORIAL_COLLECTION_APPS).contains(&collection.apps.len())
            {
                return Err(StoreProtocolError::Invalid(
                    "store Today collection bounds are invalid".into(),
                ));
            }
            for app in &collection.apps {
                app.validate()?;
                if !app_ids.insert(app.app_id.as_str()) {
                    return Err(StoreProtocolError::Invalid(
                        "store Today contains duplicate applications".into(),
                    ));
                }
            }
        }
        Ok(())
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
    if usize::from(offset) > MAX_SHARDED_CATALOG_APPS
        || !(1..=MAX_SEARCH_PAGE_APPS).contains(&limit)
    {
        return Err(StoreProtocolError::Invalid(
            "store search page is outside limits".into(),
        ));
    }
    Ok(())
}

fn validate_install_batch_ids(app_ids: &[String]) -> Result<(), StoreProtocolError> {
    if app_ids.is_empty() || app_ids.len() > MAX_INSTALL_BATCH_APPS {
        return Err(StoreProtocolError::Invalid(
            "store install batch count is outside limits".into(),
        ));
    }
    let mut previous = None;
    for app_id in app_ids {
        if !cp0_manifest::is_valid_app_id(app_id)
            || previous.is_some_and(|value| value >= app_id.as_str())
        {
            return Err(StoreProtocolError::Invalid(
                "store install batch IDs are invalid, duplicated or unsorted".into(),
            ));
        }
        previous = Some(app_id.as_str());
    }
    Ok(())
}

impl StoreInstallPreflight {
    fn validate(&self) -> Result<(), StoreProtocolError> {
        if !cp0_manifest::is_valid_app_id(&self.app_id)
            || !cp0_manifest::is_valid_app_version(&self.version)
        {
            return Err(StoreProtocolError::Invalid(
                "store install preflight identity is invalid".into(),
            ));
        }
        validate_permission_list(&self.permissions, "preflight")?;
        validate_permission_list(&self.policy_denied_permissions, "policy denied")?;
        if self
            .policy_denied_permissions
            .iter()
            .any(|permission| self.permissions.binary_search(permission).is_err())
        {
            return Err(StoreProtocolError::Invalid(
                "store policy denied permissions are not requested by the application".into(),
            ));
        }
        Ok(())
    }
}

fn validate_permission_list(
    permissions: &[Permission],
    label: &str,
) -> Result<(), StoreProtocolError> {
    if permissions.len() > Permission::ALL.len()
        || permissions.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(StoreProtocolError::Invalid(format!(
            "store {label} permissions are duplicated or unsorted"
        )));
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

pub fn sign_catalog_index(
    catalog_index: CatalogIndex,
    signing_key: &[u8; 32],
) -> Result<SignedCatalogIndex, StoreProtocolError> {
    let canonical = canonical_catalog_index(&catalog_index)?;
    let key = SigningKey::from_bytes(signing_key);
    let public = key.verifying_key().to_bytes();
    let signature = key.sign(&signature_message(
        CATALOG_INDEX_SIGNATURE_DOMAIN,
        &canonical,
    ));
    Ok(SignedCatalogIndex {
        catalog_index,
        key_id: lower_hex(&cp0_package::key_id(&public)),
        signature: lower_hex(&signature.to_bytes()),
    })
}

pub fn verify_catalog_index(
    signed: &SignedCatalogIndex,
    public_key: &[u8; 32],
) -> Result<(), StoreProtocolError> {
    let canonical = canonical_catalog_index(&signed.catalog_index)?;
    verify_signature(
        &signed.key_id,
        &signed.signature,
        public_key,
        CATALOG_INDEX_SIGNATURE_DOMAIN,
        &canonical,
        "catalog index",
    )
}

pub fn encode_signed_catalog_index(
    signed: &SignedCatalogIndex,
) -> Result<Vec<u8>, StoreProtocolError> {
    verify_signed_index_shape(signed)?;
    let encoded = serde_json::to_vec(signed)?;
    if encoded.len() > MAX_CATALOG_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    Ok(encoded)
}

pub fn decode_signed_catalog_index(
    encoded: &[u8],
) -> Result<SignedCatalogIndex, StoreProtocolError> {
    if encoded.len() > MAX_CATALOG_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    let signed: SignedCatalogIndex = serde_json::from_slice(encoded)?;
    verify_signed_index_shape(&signed)?;
    Ok(signed)
}

pub fn sign_catalog_shard(
    catalog_shard: CatalogShard,
    signing_key: &[u8; 32],
) -> Result<SignedCatalogShard, StoreProtocolError> {
    let canonical = canonical_catalog_shard(&catalog_shard)?;
    let key = SigningKey::from_bytes(signing_key);
    let public = key.verifying_key().to_bytes();
    let signature = key.sign(&signature_message(
        CATALOG_SHARD_SIGNATURE_DOMAIN,
        &canonical,
    ));
    Ok(SignedCatalogShard {
        catalog_shard,
        key_id: lower_hex(&cp0_package::key_id(&public)),
        signature: lower_hex(&signature.to_bytes()),
    })
}

pub fn verify_catalog_shard(
    signed: &SignedCatalogShard,
    public_key: &[u8; 32],
) -> Result<(), StoreProtocolError> {
    let canonical = canonical_catalog_shard(&signed.catalog_shard)?;
    verify_signature(
        &signed.key_id,
        &signed.signature,
        public_key,
        CATALOG_SHARD_SIGNATURE_DOMAIN,
        &canonical,
        "catalog shard",
    )
}

pub fn encode_signed_catalog_shard(
    signed: &SignedCatalogShard,
) -> Result<Vec<u8>, StoreProtocolError> {
    verify_signed_shard_shape(signed)?;
    let encoded = serde_json::to_vec(signed)?;
    if encoded.len() > MAX_CATALOG_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    Ok(encoded)
}

pub fn decode_signed_catalog_shard(
    encoded: &[u8],
) -> Result<SignedCatalogShard, StoreProtocolError> {
    if encoded.len() > MAX_CATALOG_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    let signed: SignedCatalogShard = serde_json::from_slice(encoded)?;
    verify_signed_shard_shape(&signed)?;
    Ok(signed)
}

pub fn decode_signed_catalog_document(
    encoded: &[u8],
) -> Result<SignedCatalogDocument, StoreProtocolError> {
    if encoded.len() > MAX_CATALOG_BYTES {
        return Err(StoreProtocolError::FrameTooLarge);
    }
    let document: SignedCatalogDocument = serde_json::from_slice(encoded)?;
    match &document {
        SignedCatalogDocument::Catalog(signed) => verify_signed_shape(signed)?,
        SignedCatalogDocument::Index(signed) => verify_signed_index_shape(signed)?,
    }
    Ok(document)
}

pub fn verify_catalog_shard_set(
    signed_index: &SignedCatalogIndex,
    encoded_shards: &[Vec<u8>],
    public_key: &[u8; 32],
) -> Result<Vec<SignedCatalogShard>, StoreProtocolError> {
    verify_catalog_index(signed_index, public_key)?;
    let index = &signed_index.catalog_index;
    if encoded_shards.len() != index.shards.len() {
        return Err(StoreProtocolError::Invalid(
            "catalog shard set is incomplete".into(),
        ));
    }
    let mut signed_shards = Vec::with_capacity(encoded_shards.len());
    let mut category_members = StoreCategory::ALL
        .into_iter()
        .map(|category| (category, 0_u16, Vec::<u16>::new()))
        .collect::<Vec<_>>();
    let mut all_ids = BTreeSet::new();
    let mut previous_id: Option<String> = None;
    for (descriptor, encoded) in index.shards.iter().zip(encoded_shards) {
        if encoded.len() != descriptor.bytes as usize
            || lower_hex(&Sha256::digest(encoded)) != descriptor.sha256
        {
            return Err(StoreProtocolError::Invalid(
                "catalog shard bytes differ from the signed descriptor".into(),
            ));
        }
        let signed = decode_signed_catalog_shard(encoded)?;
        verify_catalog_shard(&signed, public_key)?;
        let shard = &signed.catalog_shard;
        let first = shard.apps.first().expect("validated shard is non-empty");
        let last = shard.apps.last().expect("validated shard is non-empty");
        if shard.sequence != index.sequence
            || shard.catalog_schema_version != index.catalog_schema_version
            || shard.index != descriptor.index
            || shard.apps.len() != usize::from(descriptor.app_count)
            || first.app_id != descriptor.first_app_id
            || last.app_id != descriptor.last_app_id
        {
            return Err(StoreProtocolError::Invalid(
                "catalog shard identity differs from the signed descriptor".into(),
            ));
        }
        for app in &shard.apps {
            if previous_id
                .as_deref()
                .is_some_and(|previous| previous >= app.app_id.as_str())
                || !all_ids.insert(app.app_id.clone())
            {
                return Err(StoreProtocolError::Invalid(
                    "catalog shard applications overlap or are unordered".into(),
                ));
            }
            previous_id = Some(app.app_id.clone());
            let discovery = app.discovery.as_ref().ok_or_else(|| {
                StoreProtocolError::Invalid(
                    "sharded catalog application is missing discovery metadata".into(),
                )
            })?;
            let (_, count, shard_indices) = category_members
                .iter_mut()
                .find(|(category, _, _)| *category == discovery.category)
                .expect("all Store categories are indexed");
            *count = count.checked_add(1).ok_or_else(|| {
                StoreProtocolError::Invalid("catalog category count overflow".into())
            })?;
            if shard_indices.last() != Some(&shard.index) {
                shard_indices.push(shard.index);
            }
        }
        signed_shards.push(signed);
    }
    if all_ids.len() != usize::from(index.total_app_count) {
        return Err(StoreProtocolError::Invalid(
            "verified shard total differs from the signed index".into(),
        ));
    }
    let expected_categories = category_members
        .into_iter()
        .filter(|(_, count, _)| *count > 0)
        .map(
            |(category, app_count, shard_indices)| CatalogCategoryIndex {
                category,
                app_count,
                shard_indices,
            },
        )
        .collect::<Vec<_>>();
    if index.categories != expected_categories {
        return Err(StoreProtocolError::Invalid(
            "catalog category index differs from verified shard contents".into(),
        ));
    }
    if let Some(editorial) = &index.editorial {
        editorial.validate(&all_ids)?;
    }
    Ok(signed_shards)
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

fn canonical_catalog_index(index: &CatalogIndex) -> Result<Vec<u8>, StoreProtocolError> {
    index.validate()?;
    serde_json::to_vec(index).map_err(StoreProtocolError::InvalidJson)
}

fn canonical_catalog_shard(shard: &CatalogShard) -> Result<Vec<u8>, StoreProtocolError> {
    shard.validate()?;
    serde_json::to_vec(shard).map_err(StoreProtocolError::InvalidJson)
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

fn verify_signed_index_shape(signed: &SignedCatalogIndex) -> Result<(), StoreProtocolError> {
    signed.catalog_index.validate()?;
    validate_signed_encodings(&signed.key_id, &signed.signature, "catalog index")
}

fn verify_signed_shard_shape(signed: &SignedCatalogShard) -> Result<(), StoreProtocolError> {
    signed.catalog_shard.validate()?;
    validate_signed_encodings(&signed.key_id, &signed.signature, "catalog shard")
}

fn validate_signed_encodings(
    key_id: &str,
    signature: &str,
    kind: &str,
) -> Result<(), StoreProtocolError> {
    if !is_lower_hex(key_id, 32) || !is_lower_hex(signature, 64) {
        return Err(StoreProtocolError::Invalid(format!(
            "{kind} key ID or signature encoding is invalid"
        )));
    }
    Ok(())
}

fn catalog_signature_message(canonical: &[u8]) -> Vec<u8> {
    signature_message(CATALOG_SIGNATURE_DOMAIN, canonical)
}

fn signature_message(domain: &[u8], canonical: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(domain.len() + canonical.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(canonical);
    message
}

fn verify_signature(
    key_id: &str,
    signature: &str,
    public_key: &[u8; 32],
    domain: &[u8],
    canonical: &[u8],
    kind: &str,
) -> Result<(), StoreProtocolError> {
    let expected_key_id = lower_hex(&cp0_package::key_id(public_key));
    if key_id != expected_key_id {
        return Err(StoreProtocolError::Signature(format!(
            "{kind} key ID does not match trusted key"
        )));
    }
    let signature = decode_hex::<64>(signature)
        .ok_or_else(|| StoreProtocolError::Signature(format!("{kind} signature is invalid")))?;
    let key = VerifyingKey::from_bytes(public_key)
        .map_err(|_| StoreProtocolError::Signature("trusted store key is invalid".into()))?;
    key.verify(
        &signature_message(domain, canonical),
        &Signature::from_bytes(&signature),
    )
    .map_err(|_| StoreProtocolError::Signature(format!("{kind} signature does not match")))
}

fn validate_catalog_lifetime(
    published_unix_seconds: u64,
    expires_unix_seconds: u64,
) -> Result<(), StoreProtocolError> {
    expires_unix_seconds
        .checked_sub(published_unix_seconds)
        .filter(|lifetime| *lifetime > 0 && *lifetime <= MAX_CATALOG_LIFETIME_SECONDS)
        .map(|_| ())
        .ok_or_else(|| {
            StoreProtocolError::Invalid("catalog validity interval is outside limits".into())
        })
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
            editorial: None,
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

    fn sharded_catalog(secret: &[u8; 32]) -> (SignedCatalogIndex, Vec<Vec<u8>>) {
        let template = rich_catalog().apps.remove(0);
        let apps = (0..65)
            .map(|index| {
                let mut app = template.clone();
                app.app_id = format!("dev.cardputerzero.app{index:03}");
                app.name = format!("App {index:03}");
                app.package_url = format!("https://store.example.com/apps/app{index:03}.capp");
                app.package_sha256 = format!("{index:064x}");
                app.discovery.as_mut().unwrap().category = if index % 2 == 0 {
                    StoreCategory::Utilities
                } else {
                    StoreCategory::Productivity
                };
                app
            })
            .collect::<Vec<_>>();
        let mut descriptors = Vec::new();
        let mut encoded_shards = Vec::new();
        for (index, chunk) in apps.chunks(MAX_CATALOG_APPS).enumerate() {
            let signed = sign_catalog_shard(
                CatalogShard {
                    schema_version: CATALOG_SHARD_SCHEMA_VERSION,
                    catalog_schema_version: RICH_CATALOG_SCHEMA_VERSION,
                    sequence: 9,
                    index: index as u16,
                    apps: chunk.to_vec(),
                },
                secret,
            )
            .unwrap();
            let encoded = encode_signed_catalog_shard(&signed).unwrap();
            descriptors.push(CatalogShardDescriptor {
                index: index as u16,
                url: format!("https://store.example.com/generations/9/shards/{index:04}.json"),
                sha256: lower_hex(&Sha256::digest(&encoded)),
                bytes: encoded.len() as u32,
                app_count: chunk.len() as u16,
                first_app_id: chunk.first().unwrap().app_id.clone(),
                last_app_id: chunk.last().unwrap().app_id.clone(),
            });
            encoded_shards.push(encoded);
        }
        let signed_index = sign_catalog_index(
            CatalogIndex {
                schema_version: CATALOG_INDEX_SCHEMA_VERSION,
                catalog_schema_version: RICH_CATALOG_SCHEMA_VERSION,
                sequence: 9,
                published_unix_seconds: 1_800_000_000,
                expires_unix_seconds: 1_800_086_400,
                total_app_count: 65,
                shards: descriptors,
                categories: vec![
                    CatalogCategoryIndex {
                        category: StoreCategory::Productivity,
                        app_count: 32,
                        shard_indices: vec![0],
                    },
                    CatalogCategoryIndex {
                        category: StoreCategory::Utilities,
                        app_count: 33,
                        shard_indices: vec![0, 1],
                    },
                ],
                editorial: None,
            },
            secret,
        )
        .unwrap();
        (signed_index, encoded_shards)
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

    fn editorial_catalog() -> Catalog {
        let mut catalog = media_catalog();
        let mut second = catalog.apps[0].clone();
        second.app_id = "dev.cardputerzero.second".into();
        second.name = "Second".into();
        second.package_url = "https://store.example.com/apps/second.capp".into();
        second.package_sha256 = "55".repeat(32);
        catalog.apps.push(second);
        catalog.schema_version = EDITORIAL_CATALOG_SCHEMA_VERSION;
        catalog.editorial = Some(CatalogEditorial {
            headline: "Made for CardputerZero".into(),
            featured_app_id: "dev.cardputerzero.example".into(),
            collections: vec![CatalogEditorialCollection {
                title: "New and useful".into(),
                app_ids: vec!["dev.cardputerzero.second".into()],
            }],
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
    fn signs_and_verifies_a_complete_bounded_sharded_catalog() {
        let secret = [19; 32];
        let public = cp0_package::public_key(&secret);
        let (signed_index, encoded_shards) = sharded_catalog(&secret);
        verify_catalog_index(&signed_index, &public).unwrap();
        let encoded_index = encode_signed_catalog_index(&signed_index).unwrap();
        assert!(matches!(
            decode_signed_catalog_document(&encoded_index).unwrap(),
            SignedCatalogDocument::Index(_)
        ));
        let shards = verify_catalog_shard_set(&signed_index, &encoded_shards, &public).unwrap();
        assert_eq!(shards.len(), 2);
        assert_eq!(
            shards
                .iter()
                .map(|shard| shard.catalog_shard.apps.len())
                .sum::<usize>(),
            65
        );
    }

    #[test]
    fn rejects_incomplete_reordered_replaced_or_misindexed_shards() {
        let secret = [20; 32];
        let public = cp0_package::public_key(&secret);
        let (signed_index, encoded_shards) = sharded_catalog(&secret);

        assert!(verify_catalog_shard_set(&signed_index, &encoded_shards[..1], &public).is_err());
        let mut reordered = encoded_shards.clone();
        reordered.swap(0, 1);
        assert!(verify_catalog_shard_set(&signed_index, &reordered, &public).is_err());
        let mut replaced = encoded_shards.clone();
        replaced[0][0] ^= 1;
        assert!(verify_catalog_shard_set(&signed_index, &replaced, &public).is_err());

        let mut wrong_category = signed_index.catalog_index.clone();
        wrong_category.categories[0].app_count += 1;
        wrong_category.categories[1].app_count -= 1;
        let wrong_category = sign_catalog_index(wrong_category, &secret).unwrap();
        assert!(verify_catalog_shard_set(&wrong_category, &encoded_shards, &public).is_err());

        let mut wrong_range = signed_index.catalog_index.clone();
        wrong_range.shards[0].last_app_id = "dev.cardputerzero.app062".into();
        let wrong_range = sign_catalog_index(wrong_range, &secret).unwrap();
        assert!(verify_catalog_shard_set(&wrong_range, &encoded_shards, &public).is_err());
    }

    #[test]
    fn catalog_signature_domains_cannot_be_replayed_across_document_types() {
        let secret = [21; 32];
        let public = cp0_package::public_key(&secret);
        let (mut signed_index, encoded_shards) = sharded_catalog(&secret);
        let signed_shard = decode_signed_catalog_shard(&encoded_shards[0]).unwrap();
        signed_index.signature = signed_shard.signature;
        assert!(verify_catalog_index(&signed_index, &public).is_err());
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
    fn validates_editorial_catalogs_against_the_signed_application_set() {
        editorial_catalog().validate().unwrap();

        let mut missing = editorial_catalog();
        missing.editorial = None;
        assert!(missing.validate().is_err());

        let mut legacy = editorial_catalog();
        legacy.schema_version = MEDIA_CATALOG_SCHEMA_VERSION;
        assert!(legacy.validate().is_err());

        let mut unknown = editorial_catalog();
        unknown.editorial.as_mut().unwrap().collections[0].app_ids[0] =
            "dev.cardputerzero.missing".into();
        assert!(unknown.validate().is_err());

        let mut duplicate = editorial_catalog();
        duplicate.editorial.as_mut().unwrap().collections[0].app_ids[0] =
            "dev.cardputerzero.example".into();
        assert!(duplicate.validate().is_err());

        let mut unsafe_title = editorial_catalog();
        unsafe_title.editorial.as_mut().unwrap().collections[0].title = "Unsafe\nTitle".into();
        assert!(unsafe_title.validate().is_err());
    }

    #[test]
    fn protocol_is_bounded_strict_and_versioned() {
        let request = StoreRequest {
            protocol_version: STORE_PROTOCOL_VERSION,
            request_id: 9,
            command: StoreCommand::Install {
                app_id: "dev.cardputerzero.example".into(),
                authorization_id: 1,
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

        for command in [
            StoreCommand::GetAutoUpdate,
            StoreCommand::SetAutoUpdate { enabled: true },
            StoreCommand::RunAutoUpdate,
            StoreCommand::GetMetrics,
            StoreCommand::SetMetrics { enabled: true },
            StoreCommand::RecordRuntimeMetric {
                app_id: "dev.cardputerzero.example".into(),
                version: "1.2.3".into(),
                event: StoreRuntimeMetricEvent::Launch,
            },
        ] {
            let request = StoreRequest {
                protocol_version: STORE_PROTOCOL_VERSION,
                request_id: 11,
                command,
            };
            let mut encoded = Vec::new();
            write_request(&mut encoded, &request).unwrap();
            assert_eq!(
                read_request(&mut encoded.as_slice()).unwrap(),
                Some(request)
            );
        }

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

        for (offset, limit) in [(0, 0), (0, MAX_SEARCH_PAGE_APPS + 1), (1025, 1)] {
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

        let browse = StoreRequest {
            protocol_version: STORE_PROTOCOL_VERSION,
            request_id: 11,
            command: StoreCommand::Browse {
                category: Some(StoreCategory::Utilities),
                offset: MAX_SHARDED_CATALOG_APPS as u16,
                limit: MAX_SEARCH_PAGE_APPS,
            },
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &browse).unwrap();
        assert_eq!(read_request(&mut encoded.as_slice()).unwrap(), Some(browse));

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

        let auto_update = StoreResponse::success(
            5,
            StoreResponseData::AutoUpdateStatus {
                enabled: true,
                policy_allowed: true,
                charging: true,
                unmetered_network: false,
                due: true,
                checking: false,
            },
        );
        let mut encoded = Vec::new();
        write_response(&mut encoded, &auto_update).unwrap();
        assert_eq!(
            read_response(&mut encoded.as_slice()).unwrap(),
            Some(auto_update)
        );
        let invalid_auto_update = StoreResponse::success(
            5,
            StoreResponseData::AutoUpdateStatus {
                enabled: false,
                policy_allowed: true,
                charging: true,
                unmetered_network: true,
                due: true,
                checking: false,
            },
        );
        assert!(write_response(&mut Vec::new(), &invalid_auto_update).is_err());

        let metrics = StoreResponse::success(
            6,
            StoreResponseData::MetricsStatus {
                enabled: true,
                policy_allowed: true,
                configured: true,
                pending: true,
            },
        );
        let mut encoded = Vec::new();
        write_response(&mut encoded, &metrics).unwrap();
        assert_eq!(
            read_response(&mut encoded.as_slice()).unwrap(),
            Some(metrics)
        );
        let invalid_metrics = StoreResponse::success(
            6,
            StoreResponseData::MetricsStatus {
                enabled: false,
                policy_allowed: true,
                configured: true,
                pending: true,
            },
        );
        assert!(write_response(&mut Vec::new(), &invalid_metrics).is_err());

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

        let browse = StoreResponse::success(
            4,
            StoreResponseData::BrowseResults {
                category: Some(StoreCategory::Utilities),
                offset: 0,
                limit: 1,
                total: 2,
                next_offset: Some(1),
                sequence: 2,
                expires_unix_seconds: 1_900_000_000,
                stale: false,
                apps: vec![response_app()],
            },
        );
        let mut encoded = Vec::new();
        write_response(&mut encoded, &browse).unwrap();
        assert_eq!(
            read_response(&mut encoded.as_slice()).unwrap(),
            Some(browse.clone())
        );

        let mut invalid_browse_next = browse.clone();
        let StoreOutcome::Ok {
            data: StoreResponseData::BrowseResults { next_offset, .. },
        } = &mut invalid_browse_next.outcome
        else {
            unreachable!();
        };
        *next_offset = None;
        assert!(write_response(&mut Vec::new(), &invalid_browse_next).is_err());

        let mut oversized_browse = browse;
        let StoreOutcome::Ok {
            data: StoreResponseData::BrowseResults { total, .. },
        } = &mut oversized_browse.outcome
        else {
            unreachable!();
        };
        *total = MAX_SHARDED_CATALOG_APPS as u16 + 1;
        assert!(write_response(&mut Vec::new(), &oversized_browse).is_err());

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
    fn validates_strict_today_requests_and_editorial_responses() {
        let request = StoreRequest {
            protocol_version: STORE_PROTOCOL_VERSION,
            request_id: 52,
            command: StoreCommand::Today,
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &request).unwrap();
        assert_eq!(
            read_request(&mut encoded.as_slice()).unwrap(),
            Some(request)
        );

        let featured = response_app();
        let mut collection_app = response_app();
        collection_app.app_id = "dev.cardputerzero.second".into();
        collection_app.name = "Second".into();
        let response = StoreResponse::success(
            52,
            StoreResponseData::Today {
                sequence: 7,
                expires_unix_seconds: 1_900_000_000,
                stale: false,
                editorial: Some(StoreEditorial {
                    headline: "Made for CardputerZero".into(),
                    featured: featured.clone(),
                    collections: vec![StoreEditorialCollection {
                        title: "New and useful".into(),
                        apps: vec![collection_app.clone()],
                    }],
                }),
            },
        );
        let mut encoded = Vec::new();
        write_response(&mut encoded, &response).unwrap();
        assert_eq!(
            read_response(&mut encoded.as_slice()).unwrap(),
            Some(response.clone())
        );
        write_response(
            &mut Vec::new(),
            &StoreResponse::success(
                53,
                StoreResponseData::Today {
                    sequence: 7,
                    expires_unix_seconds: 1_900_000_000,
                    stale: false,
                    editorial: None,
                },
            ),
        )
        .unwrap();

        let mut invalid_metadata = response.clone();
        let StoreOutcome::Ok {
            data: StoreResponseData::Today { sequence, .. },
        } = &mut invalid_metadata.outcome
        else {
            unreachable!()
        };
        *sequence = 0;
        assert!(invalid_metadata.validate().is_err());

        let mut duplicate = response.clone();
        let StoreOutcome::Ok {
            data:
                StoreResponseData::Today {
                    editorial: Some(editorial),
                    ..
                },
        } = &mut duplicate.outcome
        else {
            unreachable!()
        };
        editorial.collections[0].apps[0] = featured;
        assert!(duplicate.validate().is_err());

        let mut unsafe_title = response.clone();
        let StoreOutcome::Ok {
            data:
                StoreResponseData::Today {
                    editorial: Some(editorial),
                    ..
                },
        } = &mut unsafe_title.outcome
        else {
            unreachable!()
        };
        editorial.collections[0].title = "Unsafe\nTitle".into();
        assert!(unsafe_title.validate().is_err());

        let mut oversized = response;
        let StoreOutcome::Ok {
            data:
                StoreResponseData::Today {
                    editorial: Some(editorial),
                    ..
                },
        } = &mut oversized.outcome
        else {
            unreachable!()
        };
        editorial.headline = "x".repeat(MAX_EDITORIAL_HEADLINE_CHARS + 1);
        assert!(oversized.validate().is_err());
    }

    #[test]
    fn validates_bounded_install_batches_and_bound_responses() {
        let app_ids = vec![
            "dev.cardputerzero.alpha".into(),
            "dev.cardputerzero.beta".into(),
        ];
        let request = StoreRequest {
            protocol_version: STORE_PROTOCOL_VERSION,
            request_id: 48,
            command: StoreCommand::InstallBatch {
                app_ids: app_ids.clone(),
                authorization_id: 1,
            },
        };
        write_request(&mut Vec::new(), &request).unwrap();

        let preflight_request = StoreRequest {
            protocol_version: STORE_PROTOCOL_VERSION,
            request_id: 47,
            command: StoreCommand::PreflightInstall {
                app_ids: app_ids.clone(),
                catalog_sequence: 9,
            },
        };
        write_request(&mut Vec::new(), &preflight_request).unwrap();
        let preflight = StoreResponse::success(
            47,
            StoreResponseData::InstallPreflight {
                authorization_id: 77,
                catalog_sequence: 9,
                required_bytes: 4096,
                available_bytes: 8192,
                apps: vec![
                    StoreInstallPreflight {
                        app_id: app_ids[0].clone(),
                        version: "1.0.0".into(),
                        permissions: vec![Permission::CameraCapture],
                        policy_denied_permissions: vec![Permission::CameraCapture],
                    },
                    StoreInstallPreflight {
                        app_id: app_ids[1].clone(),
                        version: "2.0.0".into(),
                        permissions: vec![Permission::NetworkClient],
                        policy_denied_permissions: Vec::new(),
                    },
                ],
            },
        );
        write_response(&mut Vec::new(), &preflight).unwrap();
        let mut invalid_preflight = preflight.clone();
        let StoreOutcome::Ok {
            data: StoreResponseData::InstallPreflight { apps, .. },
        } = &mut invalid_preflight.outcome
        else {
            unreachable!()
        };
        apps[0].policy_denied_permissions = vec![Permission::RadioLora];
        assert!(invalid_preflight.validate().is_err());

        for invalid in [
            Vec::new(),
            vec![app_ids[1].clone(), app_ids[0].clone()],
            vec![app_ids[0].clone(), app_ids[0].clone()],
            (0..=MAX_INSTALL_BATCH_APPS)
                .map(|index| format!("dev.cardputerzero.batch{index}"))
                .collect(),
        ] {
            assert!(
                StoreRequest {
                    protocol_version: STORE_PROTOCOL_VERSION,
                    request_id: 49,
                    command: StoreCommand::InstallBatch {
                        app_ids: invalid,
                        authorization_id: 1,
                    },
                }
                .validate()
                .is_err()
            );
        }

        let accepted = StoreResponse::success(
            48,
            StoreResponseData::InstallBatchAccepted {
                apps: vec![
                    StoreInstallAccepted {
                        app_id: app_ids[0].clone(),
                        version: "1.0.0".into(),
                    },
                    StoreInstallAccepted {
                        app_id: app_ids[1].clone(),
                        version: "2.0.0".into(),
                    },
                ],
            },
        );
        write_response(&mut Vec::new(), &accepted).unwrap();

        let invalid_response = StoreResponse::success(
            48,
            StoreResponseData::InstallBatchAccepted {
                apps: vec![
                    StoreInstallAccepted {
                        app_id: app_ids[1].clone(),
                        version: "2.0.0".into(),
                    },
                    StoreInstallAccepted {
                        app_id: app_ids[0].clone(),
                        version: "1.0.0".into(),
                    },
                ],
            },
        );
        assert!(invalid_response.validate().is_err());
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
