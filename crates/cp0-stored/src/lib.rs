use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::os::fd::AsFd;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cp0_appd::{
    APPD_PROTOCOL_VERSION, AppdCommand, AppdRequest, DevicePolicy, ResponseData, ResponseOutcome,
    StoreInstalledApp, read_response as read_appd_response, write_request as write_appd_request,
};
use cp0_networkd::PublicResolver;
use cp0_store_metadata::{StoreCategory, validate_png_structure};
use cp0_store_metrics::{
    AggregateMetricsReport, AppMetricRecord, MAX_METRIC_RECORDS, MAX_METRICS_REPORT_BYTES,
    MAX_WEEKLY_INSTALLS, MAX_WEEKLY_LAUNCHES, METRICS_SCHEMA_VERSION, WEEK_SECONDS, encode_report,
    week_start,
};
use cp0_store_protocol::{
    CatalogApp, CatalogCategoryIndex, CatalogEditorial, CatalogImageResource,
    CatalogObjectResource, MAX_CATALOG_APPS, MAX_CATALOG_BYTES, MAX_INSTALL_BATCH_APPS,
    SignedCatalogDocument, StoreAppDetails, StoreAppState, StoreAppSummary, StoreCommand,
    StoreControlAction, StoreEditorial, StoreEditorialCollection, StoreErrorCode,
    StoreFailureReason, StoreInstallAccepted, StoreInstallPreflight, StoreMediaMetadata,
    StoreMediaSelector, StoreRequest, StoreResponse, StoreResponseData, StoreRuntimeMetricEvent,
    decode_app_details, decode_signed_catalog_document, is_lower_hex, is_valid_https_url,
    read_request, response_requires_descriptor, send_response_with_fd, verify_catalog,
    verify_catalog_index, verify_catalog_shard_set, write_response,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ureq::config::Config;
use ureq::unversioned::transport::DefaultConnector;
use ureq::{Agent, Error as UreqError};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/cardputerzero/store.conf";
pub const DEFAULT_CACHE_ROOT: &str = "/var/lib/cardputerzero/store";
pub const DEFAULT_TRUST_ROOT: &str = "/etc/cardputerzero/trust/store";
pub const DEFAULT_APPD_INBOX: &str = "/run/cardputerzero-appd/store";
pub const DEFAULT_APPD_SOCKET: &str = "/run/cardputerzero-appd/control.sock";
pub const DEFAULT_DEVICE_POLICY: &str = "/etc/cardputerzero/device-policy.json";

const CLIENT_TIMEOUT: Duration = Duration::from_secs(3);
const APPD_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_REDIRECTS: u32 = 2;
const CLOCK_SKEW_SECONDS: u64 = 5 * 60;
const INSTALL_AUTHORIZATION_TTL: Duration = Duration::from_secs(60);
pub const AUTO_UPDATE_INTERVAL_SECONDS: u64 = 6 * 60 * 60;
const AUTO_UPDATE_POLL_INTERVAL: Duration = Duration::from_secs(60);
const AUTO_UPDATE_STATE_SCHEMA_VERSION: u32 = 1;
const MAX_AUTO_UPDATE_STATE_BYTES: u64 = 1024;
const METRICS_STATE_SCHEMA_VERSION: u32 = 1;
const MAX_METRICS_STATE_BYTES: u64 = 64 * 1024;
const MAX_INSTALLED_APPS: usize = 64;
const INSTALL_DATA_RESERVE_BYTES: u64 = 16 * 1024 * 1024;
const INSTALL_INBOX_RESERVE_BYTES: u64 = 8 * 1024 * 1024;
pub const ICON_CACHE_BUDGET_BYTES: u64 = 4 * 1024 * 1024;
pub const DETAILS_CACHE_BUDGET_BYTES: u64 = 1024 * 1024;
pub const SCREENSHOT_CACHE_BUDGET_BYTES: u64 = 8 * 1024 * 1024;

static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static AUTHORIZATION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreConfig {
    pub catalog_url: Option<String>,
    pub metrics_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StorePaths {
    pub cache_root: PathBuf,
    pub catalog_cache: PathBuf,
    pub trust_root: PathBuf,
    pub appd_inbox: PathBuf,
    pub appd_socket: PathBuf,
    pub device_policy: PathBuf,
    pub auto_update_state: PathBuf,
    pub metrics_state: PathBuf,
    pub enforce_root_trust: bool,
}

impl Default for StorePaths {
    fn default() -> Self {
        let cache_root = PathBuf::from(DEFAULT_CACHE_ROOT);
        Self {
            catalog_cache: cache_root.join("catalog.json"),
            auto_update_state: cache_root.join("auto-update.json"),
            metrics_state: cache_root.join("metrics.json"),
            cache_root,
            trust_root: PathBuf::from(DEFAULT_TRUST_ROOT),
            appd_inbox: PathBuf::from(DEFAULT_APPD_INBOX),
            appd_socket: PathBuf::from(DEFAULT_APPD_SOCKET),
            device_policy: PathBuf::from(DEFAULT_DEVICE_POLICY),
            enforce_root_trust: true,
        }
    }
}

#[derive(Debug)]
pub enum StoreServiceError {
    Io(io::Error),
    Invalid(String),
    Unconfigured,
    Unavailable(&'static str),
    Untrusted(String),
    NotFound,
    Busy,
    InvalidState,
    PolicyRestricted,
    InsufficientStorage,
    CatalogChanged,
}

impl fmt::Display for StoreServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "store I/O error: {error}"),
            Self::Invalid(error) => write!(formatter, "invalid store data: {error}"),
            Self::Unconfigured => formatter.write_str("store catalog URL is not configured"),
            Self::Unavailable(message) => write!(formatter, "store unavailable: {message}"),
            Self::Untrusted(error) => write!(formatter, "untrusted store data: {error}"),
            Self::NotFound => formatter.write_str("store application was not found"),
            Self::Busy => formatter.write_str("store is already processing an operation"),
            Self::InvalidState => {
                formatter.write_str("store operation is not valid in the current state")
            }
            Self::PolicyRestricted => {
                formatter.write_str("store installation is blocked by device policy")
            }
            Self::InsufficientStorage => {
                formatter.write_str("store installation does not have enough storage")
            }
            Self::CatalogChanged => {
                formatter.write_str("store catalog changed after installation preflight")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadControl {
    Continue,
    Pause,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadOutcome {
    Complete,
    Paused { progress_percent: u8 },
    Canceled,
}

impl std::error::Error for StoreServiceError {}

impl From<io::Error> for StoreServiceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub trait StoreNetwork: fmt::Debug + Send + Sync + 'static {
    fn fetch_catalog(&self, url: &str) -> Result<Vec<u8>, StoreServiceError>;

    fn fetch_catalog_shard(
        &self,
        url: &str,
        expected_bytes: u64,
    ) -> Result<Vec<u8>, StoreServiceError> {
        self.fetch_resource(url, expected_bytes, MAX_CATALOG_BYTES as u64)
    }

    fn fetch_resource(
        &self,
        _url: &str,
        _expected_bytes: u64,
        _max_bytes: u64,
    ) -> Result<Vec<u8>, StoreServiceError> {
        Err(StoreServiceError::Unavailable(
            "store media download is unavailable",
        ))
    }

    fn upload_metrics(&self, _url: &str, _encoded: &[u8]) -> Result<String, StoreServiceError> {
        Err(StoreServiceError::Unavailable(
            "store metrics upload is unavailable",
        ))
    }

    fn download_package(
        &self,
        url: &str,
        destination: &Path,
        expected_bytes: u64,
        control: &mut dyn FnMut(u8) -> DownloadControl,
    ) -> Result<DownloadOutcome, StoreServiceError>;
}

pub trait AppInstaller: fmt::Debug + Send + Sync + 'static {
    fn install(&self, app: &CatalogApp, staged_path: &Path) -> Result<(), StoreServiceError>;

    fn install_automatic(
        &self,
        app: &CatalogApp,
        staged_path: &Path,
    ) -> Result<(), StoreServiceError> {
        self.install(app, staged_path)
    }

    fn installed_apps(&self) -> Result<Vec<StoreInstalledApp>, StoreServiceError> {
        Err(StoreServiceError::Unavailable(
            "installed application query is unavailable",
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoUpdateConditions {
    pub charging: bool,
    pub unmetered_network: bool,
}

trait AutoUpdateProbe: fmt::Debug + Send + Sync + 'static {
    fn conditions(&self) -> AutoUpdateConditions;
}

#[derive(Debug)]
struct SystemAutoUpdateProbe;

impl AutoUpdateProbe for SystemAutoUpdateProbe {
    fn conditions(&self) -> AutoUpdateConditions {
        AutoUpdateConditions {
            charging: external_power_online(Path::new("/sys/class/power_supply")),
            unmetered_network: wired_default_route_available(),
        }
    }
}

trait StoreSpaceProbe: fmt::Debug + Send + Sync + 'static {
    fn available_bytes(&self, path: &Path) -> Result<u64, StoreServiceError>;
}

#[derive(Debug)]
struct SystemStoreSpaceProbe;

impl StoreSpaceProbe for SystemStoreSpaceProbe {
    fn available_bytes(&self, path: &Path) -> Result<u64, StoreServiceError> {
        filesystem_available_bytes(path)
    }
}

#[derive(Debug, Clone)]
pub struct UreqStoreNetwork {
    agent: Agent,
    #[cfg(test)]
    allow_http: bool,
}

impl Default for UreqStoreNetwork {
    fn default() -> Self {
        let config = Config::builder()
            .https_only(true)
            .proxy(None)
            .http_status_as_error(false)
            .max_redirects(MAX_REDIRECTS)
            .max_redirects_will_error(true)
            .timeout_global(Some(DOWNLOAD_TIMEOUT))
            .max_response_header_size(16 * 1024)
            .max_idle_connections(1)
            .max_idle_connections_per_host(1)
            .build();
        Self {
            agent: Agent::with_parts(
                config,
                DefaultConnector::default(),
                PublicResolver::default(),
            ),
            #[cfg(test)]
            allow_http: false,
        }
    }
}

#[cfg(test)]
impl UreqStoreNetwork {
    fn for_http_test() -> Self {
        let config = Config::builder()
            .https_only(false)
            .proxy(None)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(3)))
            .build();
        Self {
            agent: Agent::new_with_config(config),
            allow_http: true,
        }
    }
}

impl StoreNetwork for UreqStoreNetwork {
    fn fetch_catalog(&self, url: &str) -> Result<Vec<u8>, StoreServiceError> {
        require_https(url)?;
        let mut response = self.agent.get(url).call().map_err(map_network_error)?;
        if response.status().as_u16() != 200 {
            return Err(StoreServiceError::Unavailable(
                "catalog server returned a non-success status",
            ));
        }
        let encoded = response
            .body_mut()
            .with_config()
            .limit(cp0_store_protocol::MAX_CATALOG_BYTES as u64 + 1)
            .read_to_vec()
            .map_err(map_network_error)?;
        if encoded.len() > cp0_store_protocol::MAX_CATALOG_BYTES {
            return Err(StoreServiceError::Invalid(
                "catalog response exceeds the size limit".into(),
            ));
        }
        Ok(encoded)
    }

    fn fetch_resource(
        &self,
        url: &str,
        expected_bytes: u64,
        max_bytes: u64,
    ) -> Result<Vec<u8>, StoreServiceError> {
        require_https(url)?;
        if !(1..=max_bytes).contains(&expected_bytes) {
            return Err(StoreServiceError::Invalid(
                "media size is outside its cache class limit".into(),
            ));
        }
        let mut response = self
            .agent
            .get(url)
            .header("Accept-Encoding", "identity")
            .call()
            .map_err(map_network_error)?;
        if response.status().as_u16() != 200 {
            return Err(StoreServiceError::Unavailable(
                "media server returned a non-success status",
            ));
        }
        let encoded = response
            .body_mut()
            .with_config()
            .limit(expected_bytes + 1)
            .read_to_vec()
            .map_err(map_network_error)?;
        if encoded.len() as u64 != expected_bytes {
            return Err(StoreServiceError::Untrusted(
                "media response length does not match the signed descriptor".into(),
            ));
        }
        Ok(encoded)
    }

    fn upload_metrics(&self, url: &str, encoded: &[u8]) -> Result<String, StoreServiceError> {
        require_https(url)?;
        if encoded.is_empty() || encoded.len() > MAX_METRICS_REPORT_BYTES {
            return Err(StoreServiceError::Invalid(
                "store metrics report exceeds its bound".into(),
            ));
        }
        let mut response = self
            .agent
            .post(url)
            .header("Content-Type", "application/json")
            .header("Accept-Encoding", "identity")
            .send(encoded)
            .map_err(map_network_error)?;
        if response.status().as_u16() != 202 {
            return Err(StoreServiceError::Unavailable(
                "metrics server did not accept the aggregate",
            ));
        }
        let encoded = response
            .body_mut()
            .with_config()
            .limit(1025)
            .read_to_vec()
            .map_err(map_network_error)?;
        let accepted: MetricsAccepted = serde_json::from_slice(&encoded)
            .map_err(|_| StoreServiceError::Invalid("metrics acknowledgement is invalid".into()))?;
        if !accepted.accepted || !cp0_store_metrics::is_valid_batch_id(&accepted.batch_id) {
            return Err(StoreServiceError::Invalid(
                "metrics acknowledgement is invalid".into(),
            ));
        }
        Ok(accepted.batch_id)
    }

    fn download_package(
        &self,
        url: &str,
        destination: &Path,
        expected_bytes: u64,
        control: &mut dyn FnMut(u8) -> DownloadControl,
    ) -> Result<DownloadOutcome, StoreServiceError> {
        #[cfg(not(test))]
        require_https(url)?;
        #[cfg(test)]
        if !self.allow_http {
            require_https(url)?;
        }
        if !(1..=cp0_store_protocol::MAX_PACKAGE_BYTES).contains(&expected_bytes) {
            return Err(StoreServiceError::Invalid(
                "package size is outside limits".into(),
            ));
        }
        let mut file = open_resume_file(destination)?;
        let mut offset = file.metadata()?.len();
        if offset > expected_bytes {
            file.set_len(0)?;
            offset = 0;
        }
        let initial_progress = ((offset * 100) / expected_bytes) as u8;
        match control(initial_progress) {
            DownloadControl::Continue => {}
            DownloadControl::Pause => {
                file.sync_all()?;
                return Ok(DownloadOutcome::Paused {
                    progress_percent: initial_progress,
                });
            }
            DownloadControl::Cancel => {
                file.sync_all()?;
                return Ok(DownloadOutcome::Canceled);
            }
        }
        if offset == expected_bytes {
            return Ok(DownloadOutcome::Complete);
        }

        let mut request = self.agent.get(url).header("Accept-Encoding", "identity");
        if offset > 0 {
            request = request.header("Range", format!("bytes={offset}-"));
        }
        let mut response = request.call().map_err(map_network_error)?;
        let status = response.status().as_u16();
        if offset > 0 && status == 200 {
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            offset = 0;
        } else if offset > 0 && status == 206 {
            let content_range = response
                .headers()
                .get("content-range")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| {
                    StoreServiceError::Invalid("resume response omitted Content-Range".into())
                })?;
            validate_content_range(content_range, offset, expected_bytes)?;
            eprintln!("cp0-stored: resuming package download from byte {offset}");
        } else if offset == 0 && status != 200 {
            return Err(StoreServiceError::Unavailable(
                "package server returned a non-success status",
            ));
        } else if offset > 0 {
            return Err(StoreServiceError::Unavailable(
                "package server rejected the resume request",
            ));
        }

        file.seek(SeekFrom::Start(offset))?;
        let remaining = expected_bytes - offset;
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(remaining + 1)
            .reader();
        let mut buffer = [0_u8; 16 * 1024];
        let mut downloaded = offset;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|_| StoreServiceError::Unavailable("package response body read failed"))?;
            if read == 0 {
                break;
            }
            downloaded = downloaded
                .checked_add(read as u64)
                .ok_or_else(|| StoreServiceError::Invalid("package size overflow".into()))?;
            if downloaded > expected_bytes {
                return Err(StoreServiceError::Invalid(
                    "package response exceeds the signed catalog size".into(),
                ));
            }
            file.write_all(&buffer[..read])?;
            let progress_percent = ((downloaded * 100) / expected_bytes) as u8;
            match control(progress_percent) {
                DownloadControl::Continue => {}
                DownloadControl::Pause => {
                    file.sync_all()?;
                    return Ok(DownloadOutcome::Paused { progress_percent });
                }
                DownloadControl::Cancel => {
                    file.sync_all()?;
                    return Ok(DownloadOutcome::Canceled);
                }
            }
        }
        file.sync_all()?;
        if downloaded != expected_bytes {
            return Err(StoreServiceError::Unavailable(
                "package download ended before the signed catalog size",
            ));
        }
        Ok(DownloadOutcome::Complete)
    }
}

#[derive(Debug, Clone)]
pub struct AppdInstaller {
    socket: PathBuf,
}

impl AppdInstaller {
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    fn install_with_mode(
        &self,
        app: &CatalogApp,
        staged_path: &Path,
        automatic: bool,
    ) -> Result<(), StoreServiceError> {
        let package_name = staged_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| StoreServiceError::Invalid("staged package name is invalid".into()))?;
        let request = AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 1,
            command: AppdCommand::StoreInstall {
                package_name: package_name.into(),
                app_id: app.app_id.clone(),
                version: app.version.clone(),
                package_sha256: app.package_sha256.clone(),
                package_bytes: app.package_bytes,
                automatic,
            },
        };
        let response = self.request(&request)?;
        match response.outcome {
            ResponseOutcome::Ok {
                data:
                    ResponseData::Installed {
                        app_id, version, ..
                    },
            } if app_id == app.app_id && version == app.version => Ok(()),
            ResponseOutcome::Error { .. } => Err(StoreServiceError::Untrusted(
                "appd rejected the downloaded package".into(),
            )),
            _ => Err(StoreServiceError::Invalid(
                "appd returned an unexpected installation response".into(),
            )),
        }
    }

    fn request(&self, request: &AppdRequest) -> Result<cp0_appd::AppdResponse, StoreServiceError> {
        let mut stream = UnixStream::connect(&self.socket)?;
        stream.set_read_timeout(Some(APPD_TIMEOUT))?;
        stream.set_write_timeout(Some(APPD_TIMEOUT))?;
        write_appd_request(&mut stream, request)
            .map_err(|error| StoreServiceError::Invalid(error.to_string()))?;
        let response = read_appd_response(&mut BufReader::new(stream))
            .map_err(|error| StoreServiceError::Invalid(error.to_string()))?
            .ok_or(StoreServiceError::Unavailable("appd closed the request"))?;
        if response.request_id != request.request_id {
            return Err(StoreServiceError::Invalid(
                "appd response request ID does not match".into(),
            ));
        }
        Ok(response)
    }
}

impl AppInstaller for AppdInstaller {
    fn install(&self, app: &CatalogApp, staged_path: &Path) -> Result<(), StoreServiceError> {
        self.install_with_mode(app, staged_path, false)
    }

    fn install_automatic(
        &self,
        app: &CatalogApp,
        staged_path: &Path,
    ) -> Result<(), StoreServiceError> {
        self.install_with_mode(app, staged_path, true)
    }

    fn installed_apps(&self) -> Result<Vec<StoreInstalledApp>, StoreServiceError> {
        let mut apps = Vec::new();
        let mut offset = 0_u16;
        let mut request_id = 1_u64;
        loop {
            let request = AppdRequest {
                protocol_version: APPD_PROTOCOL_VERSION,
                request_id,
                command: AppdCommand::StoreListInstalled {
                    offset,
                    limit: cp0_appd::MAX_APP_LIST_PAGE,
                },
            };
            let response = self.request(&request)?;
            let (page, next_offset) = match response.outcome {
                ResponseOutcome::Ok {
                    data: ResponseData::StoreApplications { apps, next_offset },
                } => (apps, next_offset),
                ResponseOutcome::Error { .. } => {
                    return Err(StoreServiceError::Unavailable(
                        "appd rejected the installed application query",
                    ));
                }
                _ => {
                    return Err(StoreServiceError::Invalid(
                        "appd returned an unexpected installed application response".into(),
                    ));
                }
            };
            if page.len() > usize::from(cp0_appd::MAX_APP_LIST_PAGE) {
                return Err(StoreServiceError::Invalid(
                    "appd installed application page exceeds its bound".into(),
                ));
            }
            for app in &page {
                if !cp0_manifest::is_valid_app_id(&app.app_id)
                    || !cp0_manifest::is_valid_app_version(&app.version)
                    || app.permissions.len() > cp0_manifest::Permission::ALL.len()
                    || app.permissions.windows(2).any(|pair| pair[0] >= pair[1])
                    || apps
                        .last()
                        .is_some_and(|previous: &StoreInstalledApp| previous.app_id >= app.app_id)
                {
                    return Err(StoreServiceError::Invalid(
                        "appd installed application snapshot is invalid".into(),
                    ));
                }
            }
            let consumed = usize::from(offset)
                .checked_add(page.len())
                .ok_or_else(|| StoreServiceError::Invalid("appd page offset overflow".into()))?;
            if consumed > MAX_INSTALLED_APPS {
                return Err(StoreServiceError::Invalid(
                    "appd installed application snapshot exceeds its bound".into(),
                ));
            }
            apps.extend(page);
            match next_offset {
                Some(next) if usize::from(next) == consumed && next > offset => {
                    offset = next;
                    request_id = request_id.saturating_add(1);
                }
                None => return Ok(apps),
                _ => {
                    return Err(StoreServiceError::Invalid(
                        "appd installed application pagination is inconsistent".into(),
                    ));
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct OperationState {
    version: String,
    package_sha256: String,
    state: StoreAppState,
    progress_percent: u8,
    failure_reason: Option<StoreFailureReason>,
    control: DownloadControl,
    automatic: bool,
}

#[derive(Debug)]
enum InstallOutcome {
    Installed,
    Paused { progress_percent: u8 },
    Canceled,
}

#[derive(Debug)]
struct InstallFailure {
    reason: StoreFailureReason,
    source: StoreServiceError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AutoUpdatePersistentState {
    schema_version: u32,
    enabled: bool,
    last_check_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricsPersistentState {
    schema_version: u32,
    enabled: bool,
    weeks: Vec<MetricsWeek>,
    pending: Option<AggregateMetricsReport>,
}

impl Default for MetricsPersistentState {
    fn default() -> Self {
        Self {
            schema_version: METRICS_STATE_SCHEMA_VERSION,
            enabled: false,
            weeks: Vec::new(),
            pending: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricsWeek {
    week_start_unix_seconds: u64,
    records: Vec<AppMetricRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricsAccepted {
    accepted: bool,
    batch_id: String,
}

impl Default for AutoUpdatePersistentState {
    fn default() -> Self {
        Self {
            schema_version: AUTO_UPDATE_STATE_SCHEMA_VERSION,
            enabled: false,
            last_check_unix_seconds: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AutoUpdateStatus {
    enabled: bool,
    policy_allowed: bool,
    charging: bool,
    unmetered_network: bool,
    due: bool,
    checking: bool,
}

#[derive(Debug, Clone, Copy)]
struct MetricsStatus {
    enabled: bool,
    policy_allowed: bool,
    configured: bool,
    pending: bool,
}

impl MetricsStatus {
    fn response(self) -> StoreResponseData {
        StoreResponseData::MetricsStatus {
            enabled: self.enabled,
            policy_allowed: self.policy_allowed,
            configured: self.configured,
            pending: self.pending,
        }
    }
}

impl AutoUpdateStatus {
    fn response(self) -> StoreResponseData {
        StoreResponseData::AutoUpdateStatus {
            enabled: self.enabled,
            policy_allowed: self.policy_allowed,
            charging: self.charging,
            unmetered_network: self.unmetered_network,
            due: self.due,
            checking: self.checking,
        }
    }
}

#[derive(Debug, Default)]
struct MutableState {
    catalog: Option<TrustedCatalog>,
    operations: BTreeMap<String, OperationState>,
    install_authorization: Option<InstallAuthorization>,
    active_job: bool,
    auto_update: AutoUpdatePersistentState,
    auto_update_running: bool,
    metrics: MetricsPersistentState,
    metrics_upload_running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrustedCatalog {
    sequence: u64,
    published_unix_seconds: u64,
    expires_unix_seconds: u64,
    identity_sha256: String,
    apps: Vec<CatalogApp>,
    categories: Vec<CatalogCategoryIndex>,
    editorial: Option<CatalogEditorial>,
    sharded: bool,
}

impl TrustedCatalog {
    fn app(&self, app_id: &str) -> Option<&CatalogApp> {
        self.apps
            .binary_search_by(|app| app.app_id.as_str().cmp(app_id))
            .ok()
            .map(|index| &self.apps[index])
    }
}

#[derive(Debug, Clone)]
struct InstallAuthorization {
    id: u64,
    catalog_sequence: u64,
    issued_at: Instant,
    apps: Vec<CatalogApp>,
}

#[derive(Debug)]
struct InstallPreflightResult {
    authorization_id: u64,
    catalog_sequence: u64,
    required_bytes: u64,
    available_bytes: u64,
    apps: Vec<StoreInstallPreflight>,
}

#[derive(Debug)]
struct InstallCapacity {
    required_bytes: u64,
    available_bytes: u64,
    apps: Vec<StoreInstallPreflight>,
}

#[derive(Clone, Copy)]
enum MediaKind {
    Icon,
    Details,
    Screenshot,
}

impl MediaKind {
    fn directory(self) -> &'static str {
        match self {
            Self::Icon => "icons",
            Self::Details => "details",
            Self::Screenshot => "screenshots",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Details => "json",
            Self::Icon | Self::Screenshot => "png",
        }
    }

    fn budget(self) -> u64 {
        match self {
            Self::Icon => ICON_CACHE_BUDGET_BYTES,
            Self::Details => DETAILS_CACHE_BUDGET_BYTES,
            Self::Screenshot => SCREENSHOT_CACHE_BUDGET_BYTES,
        }
    }
}

#[derive(Debug)]
pub struct StoreService {
    paths: StorePaths,
    config: StoreConfig,
    network: Arc<dyn StoreNetwork>,
    installer: Arc<dyn AppInstaller>,
    space: Arc<dyn StoreSpaceProbe>,
    auto_update_probe: Arc<dyn AutoUpdateProbe>,
    trusted_uids: BTreeSet<u32>,
    state: Mutex<MutableState>,
}

struct DispatchedResponse {
    response: StoreResponse,
    descriptor: Option<File>,
}

impl DispatchedResponse {
    fn without_descriptor(response: StoreResponse) -> Self {
        Self {
            response,
            descriptor: None,
        }
    }
}

impl StoreConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, StoreServiceError> {
        let encoded = fs::read_to_string(path)?;
        let mut catalog_url = None;
        let mut metrics_url = None;
        for line in encoded.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (target, value, duplicate_message, invalid_message) =
                if let Some(value) = line.strip_prefix("catalog_url=") {
                    (
                        &mut catalog_url,
                        value,
                        "store catalog URL is duplicated",
                        "store catalog URL must be bounded HTTPS",
                    )
                } else if let Some(value) = line.strip_prefix("metrics_url=") {
                    (
                        &mut metrics_url,
                        value,
                        "store metrics URL is duplicated",
                        "store metrics URL must be bounded HTTPS",
                    )
                } else {
                    return Err(StoreServiceError::Invalid(
                        "store configuration contains an unknown field".into(),
                    ));
                };
            if target.is_some() {
                return Err(StoreServiceError::Invalid(duplicate_message.into()));
            }
            if value.is_empty() {
                *target = Some(None);
            } else if is_valid_https_url(value) {
                *target = Some(Some(value.into()));
            } else {
                return Err(StoreServiceError::Invalid(invalid_message.into()));
            }
        }
        Ok(Self {
            catalog_url: catalog_url.unwrap_or(None),
            metrics_url: metrics_url.unwrap_or(None),
        })
    }
}

impl StoreService {
    pub fn new(
        paths: StorePaths,
        config: StoreConfig,
        network: Arc<dyn StoreNetwork>,
        installer: Arc<dyn AppInstaller>,
        trusted_uids: impl IntoIterator<Item = u32>,
    ) -> Result<Arc<Self>, StoreServiceError> {
        Self::new_with_space_probe(
            paths,
            config,
            network,
            installer,
            trusted_uids,
            Arc::new(SystemStoreSpaceProbe),
        )
    }

    fn new_with_space_probe(
        paths: StorePaths,
        config: StoreConfig,
        network: Arc<dyn StoreNetwork>,
        installer: Arc<dyn AppInstaller>,
        trusted_uids: impl IntoIterator<Item = u32>,
        space: Arc<dyn StoreSpaceProbe>,
    ) -> Result<Arc<Self>, StoreServiceError> {
        Self::new_with_probes(
            paths,
            config,
            network,
            installer,
            trusted_uids,
            space,
            Arc::new(SystemAutoUpdateProbe),
        )
    }

    fn new_with_probes(
        paths: StorePaths,
        config: StoreConfig,
        network: Arc<dyn StoreNetwork>,
        installer: Arc<dyn AppInstaller>,
        trusted_uids: impl IntoIterator<Item = u32>,
        space: Arc<dyn StoreSpaceProbe>,
        auto_update_probe: Arc<dyn AutoUpdateProbe>,
    ) -> Result<Arc<Self>, StoreServiceError> {
        prepare_private_cache_directory(&paths.cache_root)?;
        fs::create_dir_all(paths.cache_root.join("packages"))?;
        prepare_media_directories(&paths)?;
        cleanup_stale_appd_handoffs(&paths.appd_inbox)?;
        let catalog = match fs::read(&paths.catalog_cache) {
            Ok(encoded) => Some(load_cached_trusted_catalog(&encoded, &paths)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let auto_update = load_auto_update_state(&paths.auto_update_state)?;
        let mut metrics = load_metrics_state(&paths.metrics_state)?;
        let metrics_allowed =
            DevicePolicy::load_secure(&paths.device_policy, paths.enforce_root_trust)
                .map(|policy| policy.store_metrics_allowed)
                .unwrap_or(false)
                && config.metrics_url.is_some();
        if !metrics_allowed
            && (metrics.enabled || !metrics.weeks.is_empty() || metrics.pending.is_some())
        {
            metrics = MetricsPersistentState::default();
            save_metrics_state(&paths.metrics_state, &metrics)?;
        }
        let service = Arc::new(Self {
            paths,
            config,
            network,
            installer,
            space,
            auto_update_probe,
            trusted_uids: trusted_uids.into_iter().collect(),
            state: Mutex::new(MutableState {
                catalog,
                auto_update,
                metrics,
                ..MutableState::default()
            }),
        });
        if let Err(error) = service.reconcile_cached_media() {
            eprintln!("cp0-stored: discarded invalid cached media: {error}");
        }
        Ok(service)
    }

    pub fn serve(self: Arc<Self>, listener: UnixListener) -> io::Result<()> {
        let scheduler = Arc::clone(&self);
        thread::Builder::new()
            .name("cp0-store-auto-update".into())
            .spawn(move || {
                loop {
                    let _ = scheduler.start_auto_update_check();
                    let _ = scheduler.start_metrics_upload();
                    thread::sleep(AUTO_UPDATE_POLL_INTERVAL);
                }
            })?;
        loop {
            let (stream, _) = listener.accept()?;
            if let Err(error) = self.handle_connection(stream) {
                eprintln!("cp0-stored: rejected connection: {error}");
            }
        }
    }

    fn handle_connection(self: &Arc<Self>, mut stream: UnixStream) -> io::Result<()> {
        stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
        let uid = peer_uid(&stream)?;
        let request = match read_request(&mut BufReader::new(stream.try_clone()?)) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(error) => {
                write_response(
                    &mut stream,
                    &StoreResponse::error(0, StoreErrorCode::InvalidRequest, error.to_string()),
                )
                .map_err(protocol_io)?;
                return Ok(());
            }
        };
        let dispatched = if !self.trusted_uids.contains(&uid) {
            DispatchedResponse::without_descriptor(StoreResponse::error(
                request.request_id,
                StoreErrorCode::Unauthorized,
                "peer UID is not authorized to use the store",
            ))
        } else if let Err(error) = request.validate() {
            DispatchedResponse::without_descriptor(StoreResponse::error(
                request.request_id,
                StoreErrorCode::InvalidRequest,
                error.to_string(),
            ))
        } else {
            self.dispatch_connection(request, uid)
        };
        if response_requires_descriptor(&dispatched.response) != dispatched.descriptor.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "store response descriptor invariant failed",
            ));
        }
        match dispatched.descriptor {
            Some(descriptor) => {
                send_response_with_fd(&mut stream, &dispatched.response, descriptor.as_fd())
                    .map_err(protocol_io)
            }
            None => write_response(&mut stream, &dispatched.response).map_err(protocol_io),
        }
    }

    pub fn dispatch(self: &Arc<Self>, request: StoreRequest) -> StoreResponse {
        self.dispatch_connection(request, 0).response
    }

    fn dispatch_connection(
        self: &Arc<Self>,
        request: StoreRequest,
        peer_uid: u32,
    ) -> DispatchedResponse {
        let request_id = request.request_id;
        if matches!(&request.command, StoreCommand::RecordRuntimeMetric { .. }) && peer_uid != 0 {
            return DispatchedResponse::without_descriptor(StoreResponse::error(
                request_id,
                StoreErrorCode::Unauthorized,
                "runtime metrics can only be recorded by the system application service",
            ));
        }
        if let StoreCommand::Media { app_id, media } = request.command {
            return match self.media_response(&app_id, media) {
                Ok((data, descriptor)) => DispatchedResponse {
                    response: StoreResponse::success(request_id, data),
                    descriptor: Some(descriptor),
                },
                Err(error) => DispatchedResponse::without_descriptor(service_error_response(
                    request_id, &error,
                )),
            };
        }
        let result = match request.command {
            StoreCommand::List => self.catalog_response(),
            StoreCommand::Browse {
                category,
                offset,
                limit,
            } => self.browse_response(category, offset, limit),
            StoreCommand::Today => self.today_response(),
            StoreCommand::Search {
                query,
                offset,
                limit,
            } => self.search_response(query, offset, limit),
            StoreCommand::Refresh => self
                .start_refresh()
                .map(|()| StoreResponseData::RefreshAccepted),
            StoreCommand::GetAutoUpdate => {
                self.auto_update_status().map(AutoUpdateStatus::response)
            }
            StoreCommand::SetAutoUpdate { enabled } => self
                .set_auto_update(enabled)
                .map(AutoUpdateStatus::response),
            StoreCommand::RunAutoUpdate => self
                .start_auto_update_check()
                .map(|()| StoreResponseData::AutoUpdateAccepted),
            StoreCommand::GetMetrics => self.metrics_status().map(MetricsStatus::response),
            StoreCommand::SetMetrics { enabled } => {
                self.set_metrics(enabled).map(MetricsStatus::response)
            }
            StoreCommand::RecordRuntimeMetric {
                app_id,
                version,
                event,
            } => self
                .record_runtime_metric(&app_id, &version, event)
                .map(|()| StoreResponseData::MetricRecorded),
            StoreCommand::PreflightInstall {
                app_ids,
                catalog_sequence,
            } => self
                .preflight_install(&app_ids, catalog_sequence)
                .map(|preflight| StoreResponseData::InstallPreflight {
                    authorization_id: preflight.authorization_id,
                    catalog_sequence: preflight.catalog_sequence,
                    required_bytes: preflight.required_bytes,
                    available_bytes: preflight.available_bytes,
                    apps: preflight.apps,
                }),
            StoreCommand::Install {
                app_id,
                authorization_id,
            } => self
                .start_authorized_install(&app_id, authorization_id)
                .map(|version| StoreResponseData::InstallAccepted { app_id, version }),
            StoreCommand::InstallBatch {
                app_ids,
                authorization_id,
            } => self
                .start_authorized_install_batch(&app_ids, authorization_id)
                .map(|apps| StoreResponseData::InstallBatchAccepted { apps }),
            StoreCommand::Control { app_id, action } => self
                .control_operation(&app_id, action)
                .map(|version| StoreResponseData::OperationAccepted {
                    app_id,
                    version,
                    action,
                }),
            StoreCommand::Details { app_id } => self.details_response(&app_id),
            StoreCommand::Media { .. } => unreachable!(),
        };
        DispatchedResponse::without_descriptor(match result {
            Ok(data) => StoreResponse::success(request_id, data),
            Err(error) => service_error_response(request_id, &error),
        })
    }

    fn auto_update_status(&self) -> Result<AutoUpdateStatus, StoreServiceError> {
        let policy =
            DevicePolicy::load_secure(&self.paths.device_policy, self.paths.enforce_root_trust)
                .map_err(|_| StoreServiceError::Unavailable("device policy is unavailable"))?;
        let conditions = self.auto_update_probe.conditions();
        let state = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
        Ok(AutoUpdateStatus {
            enabled: state.auto_update.enabled,
            policy_allowed: policy.store_install_allowed && policy.store_auto_update_allowed,
            charging: conditions.charging,
            unmetered_network: conditions.unmetered_network,
            due: auto_update_due(&state.auto_update, unix_time()),
            checking: state.auto_update_running,
        })
    }

    fn set_auto_update(
        self: &Arc<Self>,
        enabled: bool,
    ) -> Result<AutoUpdateStatus, StoreServiceError> {
        if enabled {
            let policy =
                DevicePolicy::load_secure(&self.paths.device_policy, self.paths.enforce_root_trust)
                    .map_err(|_| StoreServiceError::Unavailable("device policy is unavailable"))?;
            if !policy.store_install_allowed || !policy.store_auto_update_allowed {
                return Err(StoreServiceError::PolicyRestricted);
            }
        }
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            if state.auto_update.enabled != enabled {
                let mut next = state.auto_update.clone();
                next.enabled = enabled;
                save_auto_update_state(&self.paths.auto_update_state, &next)?;
                state.auto_update = next;
            }
        }
        if enabled {
            let _ = self.start_auto_update_check();
        }
        self.auto_update_status()
    }

    fn metrics_status(&self) -> Result<MetricsStatus, StoreServiceError> {
        let configured = self.config.metrics_url.is_some();
        let policy =
            DevicePolicy::load_secure(&self.paths.device_policy, self.paths.enforce_root_trust);
        let policy_allowed = policy
            .as_ref()
            .map(|policy| policy.store_metrics_allowed)
            .unwrap_or(false);
        if !configured || !policy_allowed {
            self.clear_metrics()?;
        }
        if policy.is_err() {
            return Err(StoreServiceError::Unavailable(
                "device policy is unavailable",
            ));
        }
        let state = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
        Ok(MetricsStatus {
            enabled: state.metrics.enabled,
            policy_allowed,
            configured,
            pending: state.metrics.pending.is_some() || !state.metrics.weeks.is_empty(),
        })
    }

    fn set_metrics(&self, enabled: bool) -> Result<MetricsStatus, StoreServiceError> {
        if enabled {
            if self.config.metrics_url.is_none() {
                return Err(StoreServiceError::Unconfigured);
            }
            let policy =
                DevicePolicy::load_secure(&self.paths.device_policy, self.paths.enforce_root_trust)
                    .map_err(|_| StoreServiceError::Unavailable("device policy is unavailable"))?;
            if !policy.store_metrics_allowed {
                return Err(StoreServiceError::PolicyRestricted);
            }
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            if !state.metrics.enabled {
                let mut next = state.metrics.clone();
                next.enabled = true;
                save_metrics_state(&self.paths.metrics_state, &next)?;
                state.metrics = next;
            }
        } else {
            self.clear_metrics()?;
        }
        self.metrics_status()
    }

    fn clear_metrics(&self) -> Result<(), StoreServiceError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
        if state.metrics != MetricsPersistentState::default() {
            let next = MetricsPersistentState::default();
            save_metrics_state(&self.paths.metrics_state, &next)?;
            state.metrics = next;
        }
        Ok(())
    }

    fn record_runtime_metric(
        &self,
        app_id: &str,
        version: &str,
        event: StoreRuntimeMetricEvent,
    ) -> Result<(), StoreServiceError> {
        if !cp0_manifest::is_valid_app_id(app_id) || !cp0_manifest::is_valid_app_version(version) {
            return Err(StoreServiceError::Invalid(
                "runtime metric application identity is invalid".into(),
            ));
        }
        if !self.metrics_enabled()? {
            return Err(StoreServiceError::InvalidState);
        }
        let installed = self.installer.installed_apps()?;
        if !installed
            .iter()
            .any(|app| app.app_id == app_id && app.version == version)
        {
            return Err(StoreServiceError::NotFound);
        }
        self.record_metric(app_id, version, Some(event))
    }

    fn record_install_metric(&self, app_id: &str, version: &str) -> Result<(), StoreServiceError> {
        if !self.metrics_enabled()? {
            return Err(StoreServiceError::InvalidState);
        }
        self.record_metric(app_id, version, None)
    }

    fn metrics_enabled(&self) -> Result<bool, StoreServiceError> {
        self.state
            .lock()
            .map(|state| state.metrics.enabled)
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))
    }

    fn record_metric(
        &self,
        app_id: &str,
        version: &str,
        event: Option<StoreRuntimeMetricEvent>,
    ) -> Result<(), StoreServiceError> {
        let status = self.metrics_status()?;
        if !status.enabled {
            return Err(StoreServiceError::InvalidState);
        }
        let now = unix_time();
        let current_week = week_start(now);
        if current_week == 0 {
            return Err(StoreServiceError::Invalid(
                "system clock cannot produce a metrics week".into(),
            ));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
        let mut next = state.metrics.clone();
        normalize_metrics_state(&mut next, now);
        let week_index = match next
            .weeks
            .binary_search_by_key(&current_week, |week| week.week_start_unix_seconds)
        {
            Ok(index) => index,
            Err(index) => {
                next.weeks.insert(
                    index,
                    MetricsWeek {
                        week_start_unix_seconds: current_week,
                        records: Vec::new(),
                    },
                );
                index
            }
        };
        let records = &mut next.weeks[week_index].records;
        let identity = (app_id, version);
        let record_index = records.binary_search_by(|record| {
            (record.app_id.as_str(), record.version.as_str()).cmp(&identity)
        });
        let record = match record_index {
            Ok(index) => &mut records[index],
            Err(index) => {
                if records.len() == MAX_METRIC_RECORDS {
                    return Ok(());
                }
                records.insert(
                    index,
                    AppMetricRecord {
                        app_id: app_id.into(),
                        version: version.into(),
                        installs: 0,
                        launches: 0,
                        crashes: 0,
                    },
                );
                &mut records[index]
            }
        };
        match event {
            None => record.installs = record.installs.saturating_add(1).min(MAX_WEEKLY_INSTALLS),
            Some(StoreRuntimeMetricEvent::Launch) => {
                record.launches = record.launches.saturating_add(1).min(MAX_WEEKLY_LAUNCHES);
            }
            Some(StoreRuntimeMetricEvent::Crash) => {
                if record.crashes < record.launches {
                    record.crashes += 1;
                }
            }
        }
        if record.installs == 0 && record.launches == 0 {
            records.remove(record_index.unwrap_or_else(|index| index));
            if records.is_empty() {
                next.weeks.remove(week_index);
            }
            return Ok(());
        }
        save_metrics_state(&self.paths.metrics_state, &next)?;
        state.metrics = next;
        Ok(())
    }

    fn start_metrics_upload(self: &Arc<Self>) -> Result<(), StoreServiceError> {
        let status = self.metrics_status()?;
        if !status.enabled {
            return Err(StoreServiceError::InvalidState);
        }
        if !status.policy_allowed {
            return Err(StoreServiceError::PolicyRestricted);
        }
        let metrics_url = self
            .config
            .metrics_url
            .clone()
            .ok_or(StoreServiceError::Unconfigured)?;
        let now = unix_time();
        let current_week = week_start(now);
        let previous_week = current_week
            .checked_sub(WEEK_SECONDS)
            .ok_or(StoreServiceError::InvalidState)?;
        let report = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            if state.metrics_upload_running {
                return Err(StoreServiceError::Busy);
            }
            let mut next = state.metrics.clone();
            normalize_metrics_state(&mut next, now);
            if next.pending.is_none() {
                let week = next
                    .weeks
                    .iter()
                    .find(|week| week.week_start_unix_seconds == previous_week)
                    .ok_or(StoreServiceError::InvalidState)?;
                if week.records.is_empty() {
                    return Err(StoreServiceError::InvalidState);
                }
                next.pending = Some(AggregateMetricsReport {
                    schema_version: METRICS_SCHEMA_VERSION,
                    batch_id: random_metrics_batch_id()?,
                    week_start_unix_seconds: previous_week,
                    records: week.records.clone(),
                });
            }
            let report = next
                .pending
                .clone()
                .ok_or(StoreServiceError::InvalidState)?;
            let encoded = encode_report(&report)
                .map_err(|error| StoreServiceError::Invalid(error.to_string()))?;
            save_metrics_state(&self.paths.metrics_state, &next)?;
            state.metrics = next;
            state.metrics_upload_running = true;
            (report, encoded)
        };

        let service = Arc::clone(self);
        thread::Builder::new()
            .name("cp0-store-metrics".into())
            .spawn(move || {
                let result = service.network.upload_metrics(&metrics_url, &report.1);
                if let Err(error) = &result {
                    eprintln!("cp0-stored: aggregate metrics upload failed: {error}");
                }
                service.finish_metrics_upload(&report.0, result);
            })
            .map_err(|error| {
                if let Ok(mut state) = self.state.lock() {
                    state.metrics_upload_running = false;
                }
                StoreServiceError::Io(error)
            })?;
        Ok(())
    }

    fn finish_metrics_upload(
        &self,
        report: &AggregateMetricsReport,
        result: Result<String, StoreServiceError>,
    ) {
        let policy_allowed =
            DevicePolicy::load_secure(&self.paths.device_policy, self.paths.enforce_root_trust)
                .is_ok_and(|policy| policy.store_metrics_allowed);
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.metrics_upload_running = false;
        if !policy_allowed || self.config.metrics_url.is_none() {
            let next = MetricsPersistentState::default();
            if let Err(error) = save_metrics_state(&self.paths.metrics_state, &next) {
                eprintln!("cp0-stored: failed to clear revoked metrics: {error}");
            } else {
                state.metrics = next;
            }
            return;
        }
        let Ok(batch_id) = result else {
            return;
        };
        if batch_id != report.batch_id || state.metrics.pending.as_ref() != Some(report) {
            eprintln!("cp0-stored: ignored a mismatched metrics acknowledgement");
            return;
        }
        let mut next = state.metrics.clone();
        next.pending = None;
        next.weeks
            .retain(|week| week.week_start_unix_seconds != report.week_start_unix_seconds);
        if let Err(error) = save_metrics_state(&self.paths.metrics_state, &next) {
            eprintln!("cp0-stored: failed to commit metrics acknowledgement: {error}");
        } else {
            state.metrics = next;
        }
    }

    fn start_auto_update_check(self: &Arc<Self>) -> Result<(), StoreServiceError> {
        {
            let state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            if !state.auto_update.enabled
                || !auto_update_due(&state.auto_update, unix_time())
                || state.auto_update_running
            {
                return Err(StoreServiceError::InvalidState);
            }
        }
        let status = self.auto_update_status()?;
        if !status.enabled || !status.due || status.checking {
            return Err(StoreServiceError::InvalidState);
        }
        if !status.policy_allowed {
            return Err(StoreServiceError::PolicyRestricted);
        }
        if !status.charging {
            return Err(StoreServiceError::Unavailable(
                "automatic updates require external power",
            ));
        }
        if !status.unmetered_network {
            return Err(StoreServiceError::Unavailable(
                "automatic updates require a wired default route",
            ));
        }

        let now = unix_time();
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            if !state.auto_update.enabled || !auto_update_due(&state.auto_update, now) {
                return Err(StoreServiceError::InvalidState);
            }
            if state.active_job || state.auto_update_running {
                return Err(StoreServiceError::Busy);
            }
            let previous = state.auto_update.clone();
            state.active_job = true;
            state.auto_update_running = true;
            state.auto_update.last_check_unix_seconds = now;
            if let Err(error) =
                save_auto_update_state(&self.paths.auto_update_state, &state.auto_update)
            {
                state.auto_update = previous;
                state.active_job = false;
                state.auto_update_running = false;
                return Err(error);
            }
        }

        let service = Arc::clone(self);
        thread::Builder::new()
            .name("cp0-store-auto-check".into())
            .spawn(move || {
                if let Err(error) = service.run_auto_update_now() {
                    eprintln!("cp0-stored: automatic update check failed: {error}");
                }
                service.finish_auto_update_check();
            })
            .map_err(|error| {
                self.finish_auto_update_check();
                StoreServiceError::Io(error)
            })?;
        Ok(())
    }

    fn run_auto_update_now(&self) -> Result<(), StoreServiceError> {
        self.refresh_now()?;
        let conditions = self.auto_update_probe.conditions();
        if !conditions.charging || !conditions.unmetered_network {
            return Err(StoreServiceError::Unavailable(
                "automatic update conditions changed during the check",
            ));
        }
        if !self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?
            .auto_update
            .enabled
        {
            return Err(StoreServiceError::InvalidState);
        }
        let installed = self.installer.installed_apps()?;
        let apps = self.auto_update_candidates(&installed)?;
        if apps.is_empty() {
            return Ok(());
        }
        self.validate_auto_update_preconditions(&apps)?;
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            if !state.auto_update.enabled {
                return Err(StoreServiceError::InvalidState);
            }
            let catalog = state
                .catalog
                .as_ref()
                .ok_or(StoreServiceError::Unconfigured)?;
            if !catalog_contains_exact_apps(catalog, &apps) {
                return Err(StoreServiceError::CatalogChanged);
            }
            for app in &apps {
                state.operations.insert(
                    app.app_id.clone(),
                    OperationState {
                        version: app.version.clone(),
                        package_sha256: app.package_sha256.clone(),
                        state: StoreAppState::Queued,
                        progress_percent: 0,
                        failure_reason: None,
                        control: DownloadControl::Continue,
                        automatic: true,
                    },
                );
            }
        }
        for app in apps {
            let result = self.install_now(&app, true);
            if let Err(failure) = &result {
                eprintln!(
                    "cp0-stored: {} automatic update failed: {}",
                    app.app_id, failure.source
                );
            }
            self.finish_install_operation(&app, result);
        }
        Ok(())
    }

    fn auto_update_candidates(
        &self,
        installed: &[StoreInstalledApp],
    ) -> Result<Vec<CatalogApp>, StoreServiceError> {
        let state = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
        let catalog = state
            .catalog
            .as_ref()
            .ok_or(StoreServiceError::Unconfigured)?;
        if unix_time() >= catalog.expires_unix_seconds {
            return Err(StoreServiceError::Untrusted(
                "catalog expired during automatic update selection".into(),
            ));
        }
        let installed = installed
            .iter()
            .map(|app| (app.app_id.as_str(), app))
            .collect::<BTreeMap<_, _>>();
        let mut candidates = Vec::new();
        for app in &catalog.apps {
            let Some(current) = installed.get(app.app_id.as_str()) else {
                continue;
            };
            let current_version = Version::parse(&current.version)
                .map_err(|_| StoreServiceError::Invalid("installed version is invalid".into()))?;
            let catalog_version = Version::parse(&app.version)
                .map_err(|_| StoreServiceError::Invalid("catalog version is invalid".into()))?;
            if catalog_version <= current_version
                || app
                    .permissions
                    .iter()
                    .any(|permission| current.permissions.binary_search(permission).is_err())
            {
                continue;
            }
            candidates.push(app.clone());
            if candidates.len() == MAX_INSTALL_BATCH_APPS {
                break;
            }
        }
        Ok(candidates)
    }

    fn validate_auto_update_preconditions(
        &self,
        apps: &[CatalogApp],
    ) -> Result<(), StoreServiceError> {
        if !self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?
            .auto_update
            .enabled
        {
            return Err(StoreServiceError::InvalidState);
        }
        let conditions = self.auto_update_probe.conditions();
        if !conditions.charging || !conditions.unmetered_network {
            return Err(StoreServiceError::Unavailable(
                "automatic update conditions are no longer satisfied",
            ));
        }
        let policy =
            DevicePolicy::load_secure(&self.paths.device_policy, self.paths.enforce_root_trust)
                .map_err(|_| StoreServiceError::Unavailable("device policy is unavailable"))?;
        if apps.iter().any(|app| {
            !policy.store_install_allowed
                || !policy.store_auto_update_allowed
                || !policy.allows_app(&app.app_id)
        }) {
            return Err(StoreServiceError::PolicyRestricted);
        }
        self.validate_install_preconditions(apps).map(|_| ())
    }

    fn finish_auto_update_check(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.active_job = false;
            state.auto_update_running = false;
        }
    }

    fn details_response(&self, app_id: &str) -> Result<StoreResponseData, StoreServiceError> {
        self.reserve_job()?;
        let result = (|| {
            let app = self.catalog_app(app_id)?;
            let discovery = app.discovery.as_ref().ok_or(StoreServiceError::NotFound)?;
            let details = self.cache_app_details_inner(app_id)?;
            Ok(StoreResponseData::AppDetails {
                app_id: app.app_id,
                version: app.version,
                developer: discovery.developer.clone(),
                category: discovery.category,
                age_rating: discovery.age_rating,
                privacy_url: discovery.privacy_url.clone(),
                support_url: discovery.support_url.clone(),
                description: details.description,
                release_notes: details.release_notes,
                screenshot_count: details.screenshots.len() as u8,
            })
        })();
        self.release_job();
        result
    }

    fn media_response(
        &self,
        app_id: &str,
        selector: StoreMediaSelector,
    ) -> Result<(StoreResponseData, File), StoreServiceError> {
        self.reserve_job()?;
        let result = (|| {
            let app = self.catalog_app(app_id)?;
            let (metadata, descriptor) = match selector {
                StoreMediaSelector::Icon => {
                    let resource = app
                        .resources
                        .as_ref()
                        .ok_or(StoreServiceError::NotFound)?
                        .icon
                        .clone();
                    let descriptor = cache_image_descriptor(
                        &self.paths,
                        self.network.as_ref(),
                        MediaKind::Icon,
                        &resource,
                    )?;
                    let metadata = StoreMediaMetadata::Icon {
                        sha256: resource.sha256,
                        bytes: resource.bytes,
                        width: resource.width,
                        height: resource.height,
                    };
                    (metadata, descriptor)
                }
                StoreMediaSelector::Screenshot { index } => {
                    let details = self.cache_app_details_inner(app_id)?;
                    let resource = details
                        .screenshots
                        .get(usize::from(index))
                        .cloned()
                        .ok_or(StoreServiceError::NotFound)?;
                    let descriptor = cache_image_descriptor(
                        &self.paths,
                        self.network.as_ref(),
                        MediaKind::Screenshot,
                        &resource,
                    )?;
                    let metadata = StoreMediaMetadata::Screenshot {
                        index,
                        sha256: resource.sha256,
                        bytes: resource.bytes,
                        width: resource.width,
                        height: resource.height,
                    };
                    (metadata, descriptor)
                }
            };
            Ok((
                StoreResponseData::Media {
                    app_id: app.app_id,
                    version: app.version,
                    media: metadata,
                },
                descriptor,
            ))
        })();
        self.release_job();
        result
    }

    fn catalog_app(&self, app_id: &str) -> Result<CatalogApp, StoreServiceError> {
        self.state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.app(app_id))
            .cloned()
            .ok_or(StoreServiceError::NotFound)
    }

    fn catalog_response(&self) -> Result<StoreResponseData, StoreServiceError> {
        let state = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
        let catalog = state
            .catalog
            .as_ref()
            .ok_or(StoreServiceError::Unconfigured)?;
        let now = unix_time();
        let apps = catalog
            .apps
            .iter()
            .take(MAX_CATALOG_APPS)
            .map(|app| store_app_summary(app, state.operations.get(&app.app_id)))
            .collect();
        Ok(StoreResponseData::Catalog {
            sequence: catalog.sequence,
            expires_unix_seconds: catalog.expires_unix_seconds,
            stale: now >= catalog.expires_unix_seconds,
            apps,
        })
    }

    fn today_response(&self) -> Result<StoreResponseData, StoreServiceError> {
        let state = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
        let catalog = state
            .catalog
            .as_ref()
            .ok_or(StoreServiceError::Unconfigured)?;
        let editorial = catalog
            .editorial
            .as_ref()
            .map(|editorial| {
                let summary = |app_id: &str| {
                    catalog
                        .app(app_id)
                        .map(|app| store_app_summary(app, state.operations.get(app_id)))
                        .ok_or_else(|| {
                            StoreServiceError::Untrusted(
                                "signed editorial application is missing from the Catalog".into(),
                            )
                        })
                };
                let featured = summary(&editorial.featured_app_id)?;
                let collections = editorial
                    .collections
                    .iter()
                    .map(|collection| {
                        let apps = collection
                            .app_ids
                            .iter()
                            .map(|app_id| summary(app_id))
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok(StoreEditorialCollection {
                            title: collection.title.clone(),
                            apps,
                        })
                    })
                    .collect::<Result<Vec<_>, StoreServiceError>>()?;
                Ok::<_, StoreServiceError>(StoreEditorial {
                    headline: editorial.headline.clone(),
                    featured,
                    collections,
                })
            })
            .transpose()?;
        Ok(StoreResponseData::Today {
            sequence: catalog.sequence,
            expires_unix_seconds: catalog.expires_unix_seconds,
            stale: unix_time() >= catalog.expires_unix_seconds,
            editorial,
        })
    }

    fn browse_response(
        &self,
        category: Option<StoreCategory>,
        offset: u16,
        limit: u8,
    ) -> Result<StoreResponseData, StoreServiceError> {
        let state = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
        let catalog = state
            .catalog
            .as_ref()
            .ok_or(StoreServiceError::Unconfigured)?;
        let matching = catalog
            .apps
            .iter()
            .filter(|app| {
                category.is_none_or(|category| {
                    app.discovery
                        .as_ref()
                        .is_some_and(|discovery| discovery.category == category)
                })
            })
            .collect::<Vec<_>>();
        let indexed_total = category.map_or(catalog.apps.len(), |category| {
            catalog
                .categories
                .iter()
                .find(|entry| entry.category == category)
                .map(|entry| usize::from(entry.app_count))
                .unwrap_or(0)
        });
        if matching.len() != indexed_total {
            return Err(StoreServiceError::Untrusted(
                "verified category index differs from the application set".into(),
            ));
        }
        let total = u16::try_from(matching.len())
            .map_err(|_| StoreServiceError::Invalid("browse result count overflow".into()))?;
        let apps = matching
            .into_iter()
            .skip(usize::from(offset))
            .take(usize::from(limit))
            .map(|app| store_app_summary(app, state.operations.get(&app.app_id)))
            .collect::<Vec<_>>();
        let next_offset = offset
            .checked_add(apps.len() as u16)
            .filter(|next| *next < total);
        Ok(StoreResponseData::BrowseResults {
            category,
            offset,
            limit,
            total,
            next_offset,
            sequence: catalog.sequence,
            expires_unix_seconds: catalog.expires_unix_seconds,
            stale: unix_time() >= catalog.expires_unix_seconds,
            apps,
        })
    }

    fn search_response(
        &self,
        query: String,
        offset: u16,
        limit: u8,
    ) -> Result<StoreResponseData, StoreServiceError> {
        let state = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
        let catalog = state
            .catalog
            .as_ref()
            .ok_or(StoreServiceError::Unconfigured)?;
        let normalized_query = query.to_lowercase();
        let mut matches = catalog
            .apps
            .iter()
            .filter_map(|app| {
                search_rank(app, &normalized_query).map(|rank| (rank, app.name.to_lowercase(), app))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.app_id.cmp(&right.2.app_id))
        });

        let total = matches.len() as u16;
        let apps = matches
            .into_iter()
            .skip(usize::from(offset))
            .take(usize::from(limit))
            .map(|(_, _, app)| store_app_summary(app, state.operations.get(&app.app_id)))
            .collect::<Vec<_>>();
        let next_offset = offset
            .checked_add(apps.len() as u16)
            .filter(|next| *next < total);
        Ok(StoreResponseData::SearchResults {
            query,
            offset,
            limit,
            total,
            next_offset,
            sequence: catalog.sequence,
            expires_unix_seconds: catalog.expires_unix_seconds,
            stale: unix_time() >= catalog.expires_unix_seconds,
            apps,
        })
    }

    fn start_refresh(self: &Arc<Self>) -> Result<(), StoreServiceError> {
        if self.config.catalog_url.is_none() {
            return Err(StoreServiceError::Unconfigured);
        }
        self.reserve_job()?;
        let service = Arc::clone(self);
        thread::Builder::new()
            .name("cp0-store-refresh".into())
            .spawn(move || {
                if let Err(error) = service.refresh_now() {
                    eprintln!("cp0-stored: catalog refresh failed: {error}");
                }
                service.release_job();
            })
            .map_err(|error| {
                self.release_job();
                StoreServiceError::Io(error)
            })?;
        Ok(())
    }

    #[cfg(test)]
    fn current_catalog_sequence(&self) -> Result<u64, StoreServiceError> {
        self.state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?
            .catalog
            .as_ref()
            .map(|catalog| catalog.sequence)
            .ok_or(StoreServiceError::Unconfigured)
    }

    fn validate_install_preconditions(
        &self,
        apps: &[CatalogApp],
    ) -> Result<InstallCapacity, StoreServiceError> {
        let policy =
            DevicePolicy::load_secure(&self.paths.device_policy, self.paths.enforce_root_trust)
                .map_err(|_| StoreServiceError::Unavailable("device policy is unavailable"))?;
        if apps
            .iter()
            .any(|app| !policy.store_install_allowed || !policy.allows_app(&app.app_id))
        {
            return Err(StoreServiceError::PolicyRestricted);
        }
        let packages = self.paths.cache_root.join("packages");
        let mut required_bytes = INSTALL_DATA_RESERVE_BYTES;
        let mut largest_package = 0_u64;
        let mut preflight_apps = Vec::with_capacity(apps.len());
        for app in apps {
            let partial = packages.join(format!("{}.part", app.package_sha256));
            let retained = safe_partial_length(&partial, app.package_bytes)?;
            let missing_download = app.package_bytes.saturating_sub(retained);
            required_bytes = required_bytes
                .checked_add(app.package_bytes)
                .and_then(|value| value.checked_add(missing_download))
                .ok_or_else(|| {
                    StoreServiceError::Invalid("install storage requirement overflow".into())
                })?;
            largest_package = largest_package.max(app.package_bytes);
            preflight_apps.push(StoreInstallPreflight {
                app_id: app.app_id.clone(),
                version: app.version.clone(),
                permissions: app.permissions.clone(),
                policy_denied_permissions: app
                    .permissions
                    .iter()
                    .copied()
                    .filter(|permission| policy.denies_permission(*permission))
                    .collect(),
            });
        }
        let available_bytes = self.space.available_bytes(&self.paths.cache_root)?;
        if available_bytes < required_bytes {
            return Err(StoreServiceError::InsufficientStorage);
        }
        let inbox_required = largest_package
            .checked_add(INSTALL_INBOX_RESERVE_BYTES)
            .ok_or_else(|| {
                StoreServiceError::Invalid("install inbox requirement overflow".into())
            })?;
        if self.space.available_bytes(&self.paths.appd_inbox)? < inbox_required {
            return Err(StoreServiceError::InsufficientStorage);
        }
        Ok(InstallCapacity {
            required_bytes,
            available_bytes,
            apps: preflight_apps,
        })
    }

    fn preflight_install(
        &self,
        app_ids: &[String],
        catalog_sequence: u64,
    ) -> Result<InstallPreflightResult, StoreServiceError> {
        validate_install_ids(app_ids)?;
        let apps = {
            let state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            if state.active_job {
                return Err(StoreServiceError::Busy);
            }
            let catalog = state
                .catalog
                .as_ref()
                .ok_or(StoreServiceError::Unconfigured)?;
            if catalog.sequence != catalog_sequence {
                return Err(StoreServiceError::CatalogChanged);
            }
            if unix_time() >= catalog.expires_unix_seconds {
                return Err(StoreServiceError::Untrusted(
                    "catalog has expired; refresh before installing".into(),
                ));
            }
            collect_install_apps(&state, app_ids)?
        };
        let capacity = self.validate_install_preconditions(&apps)?;
        let authorization_id = AUTHORIZATION_SEQUENCE
            .fetch_add(1, Ordering::Relaxed)
            .max(1);
        let mut state = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
        if state.active_job {
            return Err(StoreServiceError::Busy);
        }
        let catalog = state
            .catalog
            .as_ref()
            .ok_or(StoreServiceError::Unconfigured)?;
        if catalog.sequence != catalog_sequence || !catalog_contains_exact_apps(catalog, &apps) {
            return Err(StoreServiceError::CatalogChanged);
        }
        state.install_authorization = Some(InstallAuthorization {
            id: authorization_id,
            catalog_sequence,
            issued_at: Instant::now(),
            apps: apps.clone(),
        });
        Ok(InstallPreflightResult {
            authorization_id,
            catalog_sequence,
            required_bytes: capacity.required_bytes,
            available_bytes: capacity.available_bytes,
            apps: capacity.apps,
        })
    }

    #[cfg(test)]
    fn start_install(self: &Arc<Self>, app_id: &str) -> Result<String, StoreServiceError> {
        let app_ids = [app_id.to_owned()];
        let sequence = self.current_catalog_sequence()?;
        let preflight = self.preflight_install(&app_ids, sequence)?;
        self.start_authorized_install(app_id, preflight.authorization_id)
    }

    fn start_authorized_install(
        self: &Arc<Self>,
        app_id: &str,
        authorization_id: u64,
    ) -> Result<String, StoreServiceError> {
        let app_ids = [app_id.to_owned()];
        let accepted = self.start_authorized_install_batch(&app_ids, authorization_id)?;
        Ok(accepted
            .into_iter()
            .next()
            .expect("single install batch returned no identity")
            .version)
    }

    #[cfg(test)]
    fn start_install_batch(
        self: &Arc<Self>,
        app_ids: &[String],
    ) -> Result<Vec<StoreInstallAccepted>, StoreServiceError> {
        let sequence = self.current_catalog_sequence()?;
        let preflight = self.preflight_install(app_ids, sequence)?;
        self.start_authorized_install_batch(app_ids, preflight.authorization_id)
    }

    fn start_authorized_install_batch(
        self: &Arc<Self>,
        app_ids: &[String],
        authorization_id: u64,
    ) -> Result<Vec<StoreInstallAccepted>, StoreServiceError> {
        validate_install_ids(app_ids)?;
        let apps = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            if state.active_job {
                return Err(StoreServiceError::Busy);
            }
            let authorization = state
                .install_authorization
                .take()
                .ok_or(StoreServiceError::InvalidState)?;
            if authorization.id != authorization_id
                || authorization.issued_at.elapsed() > INSTALL_AUTHORIZATION_TTL
                || authorization.apps.len() != app_ids.len()
                || authorization
                    .apps
                    .iter()
                    .zip(app_ids)
                    .any(|(app, requested)| app.app_id != *requested)
            {
                return Err(StoreServiceError::InvalidState);
            }
            let catalog = state
                .catalog
                .as_ref()
                .ok_or(StoreServiceError::Unconfigured)?;
            let now = unix_time();
            if now >= catalog.expires_unix_seconds {
                return Err(StoreServiceError::Untrusted(
                    "catalog has expired; refresh before installing".into(),
                ));
            }
            if catalog.sequence != authorization.catalog_sequence
                || !catalog_contains_exact_apps(catalog, &authorization.apps)
            {
                return Err(StoreServiceError::CatalogChanged);
            }
            state.active_job = true;
            authorization.apps
        };
        if let Err(error) = self.validate_install_preconditions(&apps) {
            self.release_job();
            return Err(error);
        }
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            for app in &apps {
                state.operations.insert(
                    app.app_id.clone(),
                    OperationState {
                        version: app.version.clone(),
                        package_sha256: app.package_sha256.clone(),
                        state: StoreAppState::Queued,
                        progress_percent: 0,
                        failure_reason: None,
                        control: DownloadControl::Continue,
                        automatic: false,
                    },
                );
            }
        }
        let accepted = apps
            .iter()
            .map(|app| StoreInstallAccepted {
                app_id: app.app_id.clone(),
                version: app.version.clone(),
            })
            .collect();
        self.spawn_install_worker(apps, false)?;
        Ok(accepted)
    }

    fn resume_install(self: &Arc<Self>, app_id: &str) -> Result<String, StoreServiceError> {
        let (app, automatic) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            if state.active_job {
                return Err(StoreServiceError::Busy);
            }
            let catalog = state
                .catalog
                .as_ref()
                .ok_or(StoreServiceError::Unconfigured)?;
            if unix_time() >= catalog.expires_unix_seconds {
                return Err(StoreServiceError::Untrusted(
                    "catalog has expired; refresh before installing".into(),
                ));
            }
            let app = catalog
                .app(app_id)
                .cloned()
                .ok_or(StoreServiceError::NotFound)?;
            let operation = state
                .operations
                .get(app_id)
                .ok_or(StoreServiceError::InvalidState)?;
            if operation.state != StoreAppState::Paused
                || operation.version != app.version
                || operation.package_sha256 != app.package_sha256
            {
                return Err(StoreServiceError::InvalidState);
            }
            let automatic = operation.automatic;
            state.active_job = true;
            (app, automatic)
        };
        let preconditions = if automatic {
            self.validate_auto_update_preconditions(std::slice::from_ref(&app))
        } else {
            self.validate_install_preconditions(std::slice::from_ref(&app))
                .map(|_| ())
        };
        if let Err(error) = preconditions {
            self.release_job();
            return Err(error);
        }
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            state.operations.insert(
                app.app_id.clone(),
                OperationState {
                    version: app.version.clone(),
                    package_sha256: app.package_sha256.clone(),
                    state: StoreAppState::Queued,
                    progress_percent: 0,
                    failure_reason: None,
                    control: DownloadControl::Continue,
                    automatic,
                },
            );
        }
        let version = app.version.clone();
        self.spawn_install_worker(vec![app], automatic)?;
        Ok(version)
    }

    fn spawn_install_worker(
        self: &Arc<Self>,
        apps: Vec<CatalogApp>,
        automatic: bool,
    ) -> Result<(), StoreServiceError> {
        let identities = apps
            .iter()
            .map(|app| {
                (
                    app.app_id.clone(),
                    app.version.clone(),
                    app.package_sha256.clone(),
                )
            })
            .collect::<Vec<_>>();
        let service = Arc::clone(self);
        thread::Builder::new()
            .name("cp0-store-install".into())
            .spawn(move || {
                for app in apps {
                    let result = service.install_now(&app, automatic);
                    if let Err(failure) = &result {
                        eprintln!(
                            "cp0-stored: {} installation failed: {}",
                            app.app_id, failure.source
                        );
                    }
                    service.finish_install_operation(&app, result);
                }
                service.release_job();
            })
            .map_err(|error| {
                if let Ok(mut state) = self.state.lock() {
                    state.active_job = false;
                    for (app_id, version, package_sha256) in &identities {
                        if let Some(operation) = state.operations.get_mut(app_id) {
                            if operation.version == *version
                                && operation.package_sha256 == *package_sha256
                            {
                                operation.state = StoreAppState::Failed;
                                operation.progress_percent = 0;
                                operation.failure_reason = Some(StoreFailureReason::Internal);
                            }
                        }
                    }
                }
                StoreServiceError::Io(error)
            })?;
        Ok(())
    }

    fn control_operation(
        self: &Arc<Self>,
        app_id: &str,
        action: StoreControlAction,
    ) -> Result<String, StoreServiceError> {
        if action == StoreControlAction::Resume {
            return self.resume_install(app_id);
        }
        let mut deferred_cancel = None;
        let version = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            let operation = state
                .operations
                .get(app_id)
                .ok_or(StoreServiceError::InvalidState)?;
            let operation_state = operation.state;
            let version = operation.version.clone();
            let package_sha256 = operation.package_sha256.clone();
            let reserve_cleanup_job = action == StoreControlAction::Cancel
                && matches!(
                    operation_state,
                    StoreAppState::Paused | StoreAppState::Failed
                );
            if reserve_cleanup_job {
                if state.active_job {
                    return Err(StoreServiceError::Busy);
                }
                state.active_job = true;
            }
            let operation = state
                .operations
                .get_mut(app_id)
                .ok_or(StoreServiceError::InvalidState)?;
            match action {
                StoreControlAction::Pause => match operation.state {
                    StoreAppState::Queued | StoreAppState::Downloading => match operation.control {
                        DownloadControl::Continue => {
                            operation.control = DownloadControl::Pause;
                        }
                        DownloadControl::Pause => {}
                        DownloadControl::Cancel => {
                            return Err(StoreServiceError::InvalidState);
                        }
                    },
                    StoreAppState::Paused => {}
                    _ => return Err(StoreServiceError::InvalidState),
                },
                StoreControlAction::Cancel => match operation_state {
                    StoreAppState::Queued | StoreAppState::Downloading => {
                        operation.control = DownloadControl::Cancel;
                    }
                    StoreAppState::Paused | StoreAppState::Failed => {
                        operation.control = DownloadControl::Cancel;
                        deferred_cancel = Some(package_sha256);
                    }
                    StoreAppState::Canceled => {}
                    _ => return Err(StoreServiceError::InvalidState),
                },
                StoreControlAction::Resume => unreachable!(),
            }
            version
        };
        if let Some(package_sha256) = deferred_cancel {
            let partial = self
                .paths
                .cache_root
                .join("packages")
                .join(format!("{package_sha256}.part"));
            let cleanup = remove_partial_package(&partial);
            if let Ok(mut state) = self.state.lock() {
                state.active_job = false;
                if let Some(operation) = state.operations.get_mut(app_id) {
                    if operation.version == version && operation.package_sha256 == package_sha256 {
                        if cleanup.is_ok() {
                            operation.state = StoreAppState::Canceled;
                            operation.progress_percent = 0;
                            operation.failure_reason = None;
                            operation.control = DownloadControl::Continue;
                        } else {
                            operation.state = StoreAppState::Failed;
                            operation.progress_percent = 0;
                            operation.failure_reason = Some(StoreFailureReason::Storage);
                            operation.control = DownloadControl::Continue;
                        }
                    }
                }
            }
            cleanup?;
        }
        Ok(version)
    }

    pub fn cache_app_details(&self, app_id: &str) -> Result<StoreAppDetails, StoreServiceError> {
        self.reserve_job()?;
        let result = self.cache_app_details_inner(app_id);
        self.release_job();
        result
    }

    pub fn cache_screenshot(&self, app_id: &str, index: usize) -> Result<(), StoreServiceError> {
        self.reserve_job()?;
        let result = (|| {
            let details = self.cache_app_details_inner(app_id)?;
            let screenshot = details
                .screenshots
                .get(index)
                .ok_or(StoreServiceError::NotFound)?;
            cache_image_resource(
                &self.paths,
                self.network.as_ref(),
                MediaKind::Screenshot,
                screenshot,
            )?;
            Ok(())
        })();
        self.release_job();
        result
    }

    fn reserve_job(&self) -> Result<(), StoreServiceError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
        if state.active_job {
            return Err(StoreServiceError::Busy);
        }
        state.active_job = true;
        Ok(())
    }

    fn release_job(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.active_job = false;
        }
    }

    fn refresh_now(&self) -> Result<(), StoreServiceError> {
        let url = self
            .config
            .catalog_url
            .as_deref()
            .ok_or(StoreServiceError::Unconfigured)?;
        let encoded = self.network.fetch_catalog(url)?;
        let loaded = load_remote_trusted_catalog(&encoded, &self.paths, self.network.as_ref())?;
        let catalog = loaded.catalog;
        let now = unix_time();
        if catalog.published_unix_seconds > now.saturating_add(CLOCK_SKEW_SECONDS)
            || catalog.expires_unix_seconds <= now
        {
            return Err(StoreServiceError::Untrusted(
                "catalog validity window does not include the current time".into(),
            ));
        }
        {
            let state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            if let Some(current) = &state.catalog {
                if catalog.sequence < current.sequence {
                    return Err(StoreServiceError::Untrusted(
                        "catalog sequence rollback was rejected".into(),
                    ));
                }
                if catalog.sequence == current.sequence
                    && catalog.identity_sha256 != current.identity_sha256
                {
                    return Err(StoreServiceError::Untrusted(
                        "catalog sequence was reused for different content".into(),
                    ));
                }
            }
        }
        commit_catalog_cache(&self.paths, &encoded, &catalog, &loaded.encoded_shards)?;
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            state.operations.retain(|app_id, operation| {
                let Some(app) = catalog.app(app_id) else {
                    return false;
                };
                let identity_changed = operation.version != app.version
                    || operation.package_sha256 != app.package_sha256;
                if identity_changed
                    && matches!(
                        operation.state,
                        StoreAppState::Paused | StoreAppState::Failed
                    )
                {
                    operation.state = StoreAppState::Failed;
                    operation.progress_percent = 0;
                    operation.failure_reason = Some(StoreFailureReason::CatalogChanged);
                    operation.control = DownloadControl::Continue;
                }
                true
            });
            state.catalog = Some(catalog.clone());
        }
        if let Err(error) = self.prefetch_catalog_icons(&catalog) {
            eprintln!("cp0-stored: Catalog accepted without complete icon cache: {error}");
        }
        Ok(())
    }

    fn reconcile_cached_media(&self) -> Result<(), StoreServiceError> {
        let catalog = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?
            .catalog
            .clone();
        let Some(catalog) = catalog else {
            return Ok(());
        };
        reconcile_media_for_catalog(&self.paths, &catalog)
    }

    fn prefetch_catalog_icons(&self, catalog: &TrustedCatalog) -> Result<(), StoreServiceError> {
        reconcile_media_for_catalog(&self.paths, catalog)?;
        let icons = catalog
            .apps
            .iter()
            .filter_map(|app| app.resources.as_ref().map(|resources| &resources.icon))
            .take(MAX_CATALOG_APPS)
            .collect::<Vec<_>>();
        validate_resource_budget(
            icons
                .iter()
                .map(|resource| (resource.sha256.as_str(), resource.bytes)),
            MediaKind::Icon.budget(),
        )?;
        for icon in icons {
            cache_image_resource(&self.paths, self.network.as_ref(), MediaKind::Icon, icon)?;
        }
        Ok(())
    }

    fn cache_app_details_inner(&self, app_id: &str) -> Result<StoreAppDetails, StoreServiceError> {
        let app = self
            .state
            .lock()
            .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?
            .catalog
            .as_ref()
            .and_then(|catalog| catalog.app(app_id))
            .cloned()
            .ok_or(StoreServiceError::NotFound)?;
        let resource = &app
            .resources
            .as_ref()
            .ok_or(StoreServiceError::NotFound)?
            .details;
        let encoded = cache_object_resource(
            &self.paths,
            self.network.as_ref(),
            MediaKind::Details,
            resource,
            |encoded| validate_details_for_app(encoded, &app).map(|_| ()),
        )?;
        validate_details_for_app(&encoded, &app)
    }

    fn update_download_control(&self, app: &CatalogApp, progress_percent: u8) -> DownloadControl {
        let Ok(mut state) = self.state.lock() else {
            return DownloadControl::Cancel;
        };
        let Some(operation) = state.operations.get_mut(&app.app_id) else {
            return DownloadControl::Cancel;
        };
        if operation.version != app.version || operation.package_sha256 != app.package_sha256 {
            return DownloadControl::Cancel;
        }
        if operation.control == DownloadControl::Continue {
            operation.state = StoreAppState::Downloading;
            operation.progress_percent = progress_percent.min(100);
            operation.failure_reason = None;
        }
        operation.control
    }

    fn begin_install_handoff(&self, app: &CatalogApp) -> DownloadControl {
        let Ok(mut state) = self.state.lock() else {
            return DownloadControl::Cancel;
        };
        let Some(operation) = state.operations.get_mut(&app.app_id) else {
            return DownloadControl::Cancel;
        };
        if operation.version != app.version || operation.package_sha256 != app.package_sha256 {
            return DownloadControl::Cancel;
        }
        if operation.control == DownloadControl::Continue {
            operation.state = StoreAppState::Installing;
            operation.progress_percent = 100;
            operation.failure_reason = None;
        }
        operation.control
    }

    fn finish_install_operation(
        &self,
        app: &CatalogApp,
        result: Result<InstallOutcome, InstallFailure>,
    ) {
        let installed = matches!(&result, Ok(InstallOutcome::Installed));
        if let Ok(mut state) = self.state.lock() {
            let Some(operation) = state.operations.get_mut(&app.app_id) else {
                return;
            };
            if operation.version != app.version || operation.package_sha256 != app.package_sha256 {
                return;
            }
            operation.control = DownloadControl::Continue;
            match result {
                Ok(InstallOutcome::Installed) => {
                    operation.state = StoreAppState::Installed;
                    operation.progress_percent = 100;
                    operation.failure_reason = None;
                }
                Ok(InstallOutcome::Paused { progress_percent }) => {
                    operation.state = StoreAppState::Paused;
                    operation.progress_percent = progress_percent.min(100);
                    operation.failure_reason = None;
                }
                Ok(InstallOutcome::Canceled) => {
                    operation.state = StoreAppState::Canceled;
                    operation.progress_percent = 0;
                    operation.failure_reason = None;
                }
                Err(failure) => {
                    operation.state = StoreAppState::Failed;
                    operation.progress_percent = 0;
                    operation.failure_reason = Some(failure.reason);
                }
            }
        }
        if installed {
            if let Err(error) = self.record_install_metric(&app.app_id, &app.version) {
                if !matches!(error, StoreServiceError::InvalidState) {
                    eprintln!("cp0-stored: failed to record install aggregate: {error}");
                }
            }
        }
    }

    fn install_now(
        &self,
        app: &CatalogApp,
        automatic: bool,
    ) -> Result<InstallOutcome, InstallFailure> {
        let packages = self.paths.cache_root.join("packages");
        let partial = packages.join(format!("{}.part", app.package_sha256));
        let download = self
            .network
            .download_package(
                &app.package_url,
                &partial,
                app.package_bytes,
                &mut |progress| self.update_download_control(app, progress),
            )
            .map_err(|source| InstallFailure {
                reason: download_failure_reason(&source),
                source,
            })?;
        match download {
            DownloadOutcome::Paused { progress_percent } => {
                return Ok(InstallOutcome::Paused { progress_percent });
            }
            DownloadOutcome::Canceled => {
                remove_partial_package(&partial).map_err(|source| InstallFailure {
                    reason: StoreFailureReason::Storage,
                    source,
                })?;
                return Ok(InstallOutcome::Canceled);
            }
            DownloadOutcome::Complete => {}
        }
        verify_package_file(&partial, app).map_err(|source| InstallFailure {
            reason: StoreFailureReason::Verification,
            source,
        })?;
        match self.begin_install_handoff(app) {
            DownloadControl::Continue => {}
            DownloadControl::Pause => {
                return Ok(InstallOutcome::Paused {
                    progress_percent: 100,
                });
            }
            DownloadControl::Cancel => {
                remove_partial_package(&partial).map_err(|source| InstallFailure {
                    reason: StoreFailureReason::Storage,
                    source,
                })?;
                return Ok(InstallOutcome::Canceled);
            }
        }
        let staged =
            stage_for_appd(&partial, &self.paths.appd_inbox).map_err(|source| InstallFailure {
                reason: StoreFailureReason::Storage,
                source,
            })?;
        let install_result = if automatic {
            self.installer.install_automatic(app, &staged)
        } else {
            self.installer.install(app, &staged)
        };
        if let Err(error) = fs::remove_file(&staged) {
            eprintln!("cp0-stored: failed to remove appd staging file after handoff: {error}");
        }
        install_result.map_err(|source| InstallFailure {
            reason: StoreFailureReason::Installer,
            source,
        })?;
        Ok(InstallOutcome::Installed)
    }
}

fn validate_install_ids(app_ids: &[String]) -> Result<(), StoreServiceError> {
    if app_ids.is_empty()
        || app_ids.len() > MAX_INSTALL_BATCH_APPS
        || app_ids
            .windows(2)
            .any(|pair| pair[0].as_str() >= pair[1].as_str())
    {
        return Err(StoreServiceError::Invalid(
            "install batch IDs are invalid, duplicated or unsorted".into(),
        ));
    }
    Ok(())
}

fn collect_install_apps(
    state: &MutableState,
    app_ids: &[String],
) -> Result<Vec<CatalogApp>, StoreServiceError> {
    let catalog = state
        .catalog
        .as_ref()
        .ok_or(StoreServiceError::Unconfigured)?;
    let mut apps = Vec::with_capacity(app_ids.len());
    for app_id in app_ids {
        let app = catalog
            .app(app_id)
            .cloned()
            .ok_or(StoreServiceError::NotFound)?;
        if state.operations.get(app_id).is_some_and(|operation| {
            matches!(
                operation.state,
                StoreAppState::Queued
                    | StoreAppState::Downloading
                    | StoreAppState::Paused
                    | StoreAppState::Installing
            )
        }) {
            return Err(StoreServiceError::InvalidState);
        }
        apps.push(app);
    }
    Ok(apps)
}

fn catalog_contains_exact_apps(catalog: &TrustedCatalog, apps: &[CatalogApp]) -> bool {
    apps.iter()
        .all(|expected| catalog.app(&expected.app_id) == Some(expected))
}

fn store_app_summary(app: &CatalogApp, operation: Option<&OperationState>) -> StoreAppSummary {
    let operation = operation.filter(|operation| {
        (operation.version == app.version && operation.package_sha256 == app.package_sha256)
            || (operation.state == StoreAppState::Failed
                && operation.failure_reason == Some(StoreFailureReason::CatalogChanged))
    });
    StoreAppSummary {
        app_id: app.app_id.clone(),
        name: app.name.clone(),
        version: app.version.clone(),
        summary: app.summary.clone(),
        package_bytes: app.package_bytes,
        permissions: app.permissions.clone(),
        state: operation
            .map(|operation| operation.state)
            .unwrap_or(StoreAppState::Available),
        progress_percent: operation
            .map(|operation| operation.progress_percent)
            .unwrap_or(0),
        failure_reason: operation.and_then(|operation| operation.failure_reason),
    }
}

fn download_failure_reason(error: &StoreServiceError) -> StoreFailureReason {
    match error {
        StoreServiceError::Unavailable(_) => StoreFailureReason::Network,
        StoreServiceError::Io(_) => StoreFailureReason::Storage,
        StoreServiceError::Invalid(_) | StoreServiceError::Untrusted(_) => {
            StoreFailureReason::Verification
        }
        StoreServiceError::Unconfigured | StoreServiceError::NotFound => {
            StoreFailureReason::CatalogChanged
        }
        StoreServiceError::InsufficientStorage => StoreFailureReason::Storage,
        StoreServiceError::CatalogChanged => StoreFailureReason::CatalogChanged,
        StoreServiceError::Busy
        | StoreServiceError::InvalidState
        | StoreServiceError::PolicyRestricted => StoreFailureReason::Internal,
    }
}

fn remove_partial_package(path: &Path) -> Result<(), StoreServiceError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StoreServiceError::Io(error)),
    }
}

fn safe_partial_length(path: &Path, expected_bytes: u64) -> Result<u64, StoreServiceError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(StoreServiceError::Io(error)),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.mode() & 0o077 != 0
        || metadata.len() > expected_bytes
    {
        return Err(StoreServiceError::Invalid(
            "partial package metadata is invalid during preflight".into(),
        ));
    }
    Ok(metadata.len())
}

fn filesystem_available_bytes(path: &Path) -> Result<u64, StoreServiceError> {
    let encoded = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| StoreServiceError::Invalid("storage preflight path contains NUL".into()))?;
    let mut status = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `encoded` is NUL terminated and `status` points to writable storage.
    if unsafe { libc::statvfs(encoded.as_ptr(), status.as_mut_ptr()) } != 0 {
        return Err(StoreServiceError::Io(io::Error::last_os_error()));
    }
    // SAFETY: a successful statvfs call initialized the complete structure.
    let status = unsafe { status.assume_init() };
    #[cfg(target_pointer_width = "64")]
    let fragment_size = status.f_frsize;
    #[cfg(target_pointer_width = "32")]
    let fragment_size = u64::from(status.f_frsize);
    Ok(u64::from(status.f_bavail).saturating_mul(fragment_size))
}

fn search_rank(app: &CatalogApp, normalized_query: &str) -> Option<u8> {
    let name = app.name.to_lowercase();
    if name == normalized_query {
        Some(0)
    } else if name.starts_with(normalized_query) {
        Some(1)
    } else if name.contains(normalized_query) {
        Some(2)
    } else if app.discovery.as_ref().is_some_and(|discovery| {
        discovery
            .keywords
            .iter()
            .any(|keyword| keyword.to_lowercase() == normalized_query)
    }) {
        Some(3)
    } else if app.summary.to_lowercase().contains(normalized_query)
        || app.app_id.contains(normalized_query)
        || app.discovery.as_ref().is_some_and(|discovery| {
            discovery
                .developer
                .to_lowercase()
                .contains(normalized_query)
                || discovery.subtitle.to_lowercase().contains(normalized_query)
                || discovery.category.as_str().contains(normalized_query)
                || discovery
                    .keywords
                    .iter()
                    .any(|keyword| keyword.to_lowercase().contains(normalized_query))
        })
    {
        Some(4)
    } else {
        None
    }
}

fn prepare_media_directories(paths: &StorePaths) -> Result<(), StoreServiceError> {
    let media = paths.cache_root.join("media");
    prepare_private_cache_directory(&media)?;
    for kind in [MediaKind::Icon, MediaKind::Details, MediaKind::Screenshot] {
        prepare_private_cache_directory(&media.join(kind.directory()))?;
    }
    Ok(())
}

fn prepare_private_cache_directory(directory: &Path) -> Result<(), StoreServiceError> {
    fs::create_dir_all(directory)?;
    let metadata = fs::symlink_metadata(directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(StoreServiceError::Untrusted(
            "media cache directory is not a real directory".into(),
        ));
    }
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn media_directory(paths: &StorePaths, kind: MediaKind) -> PathBuf {
    paths.cache_root.join("media").join(kind.directory())
}

fn media_path(
    paths: &StorePaths,
    kind: MediaKind,
    sha256: &str,
) -> Result<PathBuf, StoreServiceError> {
    if !is_lower_hex(sha256, 32) {
        return Err(StoreServiceError::Invalid(
            "media cache digest is invalid".into(),
        ));
    }
    Ok(media_directory(paths, kind).join(format!("{sha256}.{}", kind.extension())))
}

fn reconcile_media_for_catalog(
    paths: &StorePaths,
    catalog: &TrustedCatalog,
) -> Result<(), StoreServiceError> {
    let mut icon_files = BTreeSet::new();
    let mut detail_files = BTreeSet::new();
    let mut icons = Vec::new();
    let mut details = Vec::new();
    for app in &catalog.apps {
        let Some(resources) = &app.resources else {
            continue;
        };
        icon_files.insert(format!("{}.png", resources.icon.sha256));
        detail_files.insert(format!("{}.json", resources.details.sha256));
        icons.push((&resources.icon, app));
        details.push((&resources.details, app));
    }
    prune_cache_directory(&media_directory(paths, MediaKind::Icon), &icon_files)?;
    prune_cache_directory(&media_directory(paths, MediaKind::Details), &detail_files)?;
    enforce_cache_capacity(
        &media_directory(paths, MediaKind::Icon),
        MediaKind::Icon.budget(),
        0,
        None,
    )?;
    enforce_cache_capacity(
        &media_directory(paths, MediaKind::Details),
        MediaKind::Details.budget(),
        0,
        None,
    )?;
    prune_unrecognized_cache_files(
        &media_directory(paths, MediaKind::Screenshot),
        MediaKind::Screenshot,
    )?;
    enforce_cache_capacity(
        &media_directory(paths, MediaKind::Screenshot),
        MediaKind::Screenshot.budget(),
        0,
        None,
    )?;

    for (resource, _) in icons {
        discard_invalid_cached_resource(
            &media_path(paths, MediaKind::Icon, &resource.sha256)?,
            &resource.sha256,
            resource.bytes,
            |encoded| validate_image_bytes(encoded, resource),
        )?;
    }
    for (resource, app) in details {
        discard_invalid_cached_resource(
            &media_path(paths, MediaKind::Details, &resource.sha256)?,
            &resource.sha256,
            resource.bytes,
            |encoded| validate_details_for_app(encoded, app).map(|_| ()),
        )?;
    }
    Ok(())
}

fn validate_resource_budget<'a>(
    resources: impl IntoIterator<Item = (&'a str, u64)>,
    budget: u64,
) -> Result<(), StoreServiceError> {
    let mut unique = BTreeMap::<&str, u64>::new();
    for (digest, bytes) in resources {
        match unique.insert(digest, bytes) {
            Some(previous) if previous != bytes => {
                return Err(StoreServiceError::Untrusted(
                    "one media digest has conflicting signed sizes".into(),
                ));
            }
            _ => {}
        }
    }
    let total = unique
        .into_values()
        .try_fold(0_u64, u64::checked_add)
        .ok_or_else(|| StoreServiceError::Invalid("media cache budget overflow".into()))?;
    if total > budget {
        return Err(StoreServiceError::Invalid(
            "signed media set exceeds its cache budget".into(),
        ));
    }
    Ok(())
}

fn prune_cache_directory(
    directory: &Path,
    retained: &BTreeSet<String>,
) -> Result<(), StoreServiceError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StoreServiceError::Invalid("media cache filename is invalid".into()))?;
        if metadata.is_dir() {
            return Err(StoreServiceError::Invalid(
                "media cache contains an unexpected directory".into(),
            ));
        }
        if !retained.contains(&name) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn prune_unrecognized_cache_files(
    directory: &Path,
    kind: MediaKind,
) -> Result<(), StoreServiceError> {
    let suffix = format!(".{}", kind.extension());
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            return Err(StoreServiceError::Invalid(
                "media cache contains an unexpected directory".into(),
            ));
        }
        let recognized = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_suffix(&suffix))
            .is_some_and(|digest| is_lower_hex(digest, 32));
        if !recognized {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn discard_invalid_cached_resource(
    path: &Path,
    expected_sha256: &str,
    expected_bytes: u64,
    validate: impl Fn(&[u8]) -> Result<(), StoreServiceError>,
) -> Result<(), StoreServiceError> {
    match read_cached_resource(path, expected_sha256, expected_bytes, false, validate) {
        Ok(_) => Ok(()),
        Err(StoreServiceError::Io(error)) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        },
    }
}

fn cache_image_resource(
    paths: &StorePaths,
    network: &dyn StoreNetwork,
    kind: MediaKind,
    resource: &CatalogImageResource,
) -> Result<Vec<u8>, StoreServiceError> {
    let max_bytes = match kind {
        MediaKind::Icon => cp0_store_metadata::MAX_ICON_BYTES,
        MediaKind::Screenshot => cp0_store_metadata::MAX_SCREENSHOT_BYTES,
        MediaKind::Details => {
            return Err(StoreServiceError::Invalid(
                "details resource was passed to the image cache".into(),
            ));
        }
    };
    cache_resource(
        paths,
        network,
        kind,
        &resource.url,
        &resource.sha256,
        resource.bytes,
        max_bytes,
        |encoded| validate_image_bytes(encoded, resource),
    )
}

fn cache_image_descriptor(
    paths: &StorePaths,
    network: &dyn StoreNetwork,
    kind: MediaKind,
    resource: &CatalogImageResource,
) -> Result<File, StoreServiceError> {
    cache_image_resource(paths, network, kind, resource)?;
    let path = media_path(paths, kind, &resource.sha256)?;
    let descriptor = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = descriptor.metadata()?;
    if !metadata.is_file() || metadata.mode() & 0o777 != 0o600 || metadata.len() != resource.bytes {
        return Err(StoreServiceError::Untrusted(
            "cached media descriptor metadata is invalid".into(),
        ));
    }
    Ok(descriptor)
}

fn cache_object_resource(
    paths: &StorePaths,
    network: &dyn StoreNetwork,
    kind: MediaKind,
    resource: &CatalogObjectResource,
    validate: impl Fn(&[u8]) -> Result<(), StoreServiceError>,
) -> Result<Vec<u8>, StoreServiceError> {
    if !matches!(kind, MediaKind::Details) {
        return Err(StoreServiceError::Invalid(
            "object resource was passed to an image cache".into(),
        ));
    }
    cache_resource(
        paths,
        network,
        kind,
        &resource.url,
        &resource.sha256,
        resource.bytes,
        cp0_store_protocol::MAX_APP_DETAILS_BYTES as u64,
        validate,
    )
}

#[allow(clippy::too_many_arguments)]
fn cache_resource(
    paths: &StorePaths,
    network: &dyn StoreNetwork,
    kind: MediaKind,
    url: &str,
    expected_sha256: &str,
    expected_bytes: u64,
    max_bytes: u64,
    validate: impl Fn(&[u8]) -> Result<(), StoreServiceError>,
) -> Result<Vec<u8>, StoreServiceError> {
    if !(1..=max_bytes).contains(&expected_bytes) {
        return Err(StoreServiceError::Invalid(
            "signed media size is outside its per-file limit".into(),
        ));
    }
    let path = media_path(paths, kind, expected_sha256)?;
    match read_cached_resource(
        &path,
        expected_sha256,
        expected_bytes,
        matches!(kind, MediaKind::Screenshot),
        &validate,
    ) {
        Ok(encoded) => return Ok(encoded),
        Err(StoreServiceError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {}
        Err(_) => match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        },
    }
    enforce_cache_capacity(
        &media_directory(paths, kind),
        kind.budget(),
        expected_bytes,
        Some(&path),
    )?;
    let encoded = network.fetch_resource(url, expected_bytes, max_bytes)?;
    verify_resource_bytes(&encoded, expected_sha256, expected_bytes)?;
    validate(&encoded)?;
    atomic_write_media(&path, &encoded)?;
    Ok(encoded)
}

fn read_cached_resource(
    path: &Path,
    expected_sha256: &str,
    expected_bytes: u64,
    touch: bool,
    validate: impl Fn(&[u8]) -> Result<(), StoreServiceError>,
) -> Result<Vec<u8>, StoreServiceError> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.mode() & 0o077 != 0 || metadata.len() != expected_bytes {
        return Err(StoreServiceError::Untrusted(
            "cached media metadata does not match its signed descriptor".into(),
        ));
    }
    let capacity = usize::try_from(expected_bytes)
        .map_err(|_| StoreServiceError::Invalid("media size cannot be represented".into()))?;
    let mut encoded = Vec::with_capacity(capacity);
    file.read_to_end(&mut encoded)?;
    verify_resource_bytes(&encoded, expected_sha256, expected_bytes)?;
    validate(&encoded)?;
    if touch {
        file.set_modified(SystemTime::now())?;
    }
    Ok(encoded)
}

fn verify_resource_bytes(
    encoded: &[u8],
    expected_sha256: &str,
    expected_bytes: u64,
) -> Result<(), StoreServiceError> {
    if encoded.len() as u64 != expected_bytes
        || cp0_store_protocol::lower_hex(&Sha256::digest(encoded)) != expected_sha256
    {
        return Err(StoreServiceError::Untrusted(
            "media bytes do not match their signed descriptor".into(),
        ));
    }
    Ok(())
}

fn validate_image_bytes(
    encoded: &[u8],
    resource: &CatalogImageResource,
) -> Result<(), StoreServiceError> {
    validate_png_structure(encoded, resource.width, resource.height)
        .map_err(StoreServiceError::Untrusted)
}

fn validate_details_for_app(
    encoded: &[u8],
    app: &CatalogApp,
) -> Result<StoreAppDetails, StoreServiceError> {
    let details = decode_app_details(encoded)
        .map_err(|error| StoreServiceError::Untrusted(error.to_string()))?;
    if details.app_id != app.app_id || details.version != app.version {
        return Err(StoreServiceError::Untrusted(
            "Store details identity differs from the signed Catalog app".into(),
        ));
    }
    Ok(details)
}

fn enforce_cache_capacity(
    directory: &Path,
    budget: u64,
    required: u64,
    protected: Option<&Path>,
) -> Result<(), StoreServiceError> {
    if required > budget {
        return Err(StoreServiceError::Invalid(
            "media resource exceeds its cache budget".into(),
        ));
    }
    loop {
        let mut used = 0_u64;
        let mut candidates = Vec::new();
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                fs::remove_file(path)?;
                continue;
            }
            if !metadata.is_file() {
                return Err(StoreServiceError::Invalid(
                    "media cache contains a non-file entry".into(),
                ));
            }
            used = used
                .checked_add(metadata.len())
                .ok_or_else(|| StoreServiceError::Invalid("media cache size overflow".into()))?;
            if protected != Some(path.as_path()) {
                candidates.push((
                    metadata.modified().unwrap_or(UNIX_EPOCH),
                    entry.file_name(),
                    path,
                ));
            }
        }
        if used
            .checked_add(required)
            .is_some_and(|projected| projected <= budget)
        {
            return Ok(());
        }
        candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        let Some((_, _, oldest)) = candidates.into_iter().next() else {
            return Err(StoreServiceError::Unavailable(
                "media cache cannot free enough bounded storage",
            ));
        };
        fs::remove_file(oldest)?;
    }
}

fn atomic_write_media(path: &Path, contents: &[u8]) -> Result<(), StoreServiceError> {
    let parent = path
        .parent()
        .ok_or_else(|| StoreServiceError::Invalid("media cache has no parent".into()))?;
    let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".media-{}-{sequence}.tmp", std::process::id()));
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(StoreServiceError::Io)
}

struct LoadedRemoteCatalog {
    catalog: TrustedCatalog,
    encoded_shards: Vec<Vec<u8>>,
}

fn load_cached_trusted_catalog(
    encoded: &[u8],
    paths: &StorePaths,
) -> Result<TrustedCatalog, StoreServiceError> {
    let document = decode_signed_catalog_document(encoded)
        .map_err(|error| StoreServiceError::Untrusted(error.to_string()))?;
    match document {
        SignedCatalogDocument::Catalog(signed) => {
            let public = load_trusted_catalog_key(paths, &signed.key_id)?;
            verify_catalog(&signed, &public)
                .map_err(|error| StoreServiceError::Untrusted(error.to_string()))?;
            Ok(trusted_legacy_catalog(signed, encoded))
        }
        SignedCatalogDocument::Index(signed) => {
            let public = load_trusted_catalog_key(paths, &signed.key_id)?;
            verify_catalog_index(&signed, &public)
                .map_err(|error| StoreServiceError::Untrusted(error.to_string()))?;
            let mut encoded_shards = Vec::with_capacity(signed.catalog_index.shards.len());
            let directory = catalog_shard_cache_directory(paths, signed.catalog_index.sequence);
            for descriptor in &signed.catalog_index.shards {
                let path = directory.join(format!("{:04}.json", descriptor.index));
                let file = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                    .open(path)?;
                let metadata = file.metadata()?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.mode() & 0o077 != 0
                    || metadata.len() != u64::from(descriptor.bytes)
                {
                    return Err(StoreServiceError::Untrusted(
                        "cached Catalog shard metadata is invalid".into(),
                    ));
                }
                let mut bytes = Vec::with_capacity(descriptor.bytes as usize);
                BufReader::new(file).read_to_end(&mut bytes)?;
                encoded_shards.push(bytes);
            }
            trusted_indexed_catalog(signed, encoded, encoded_shards, &public)
        }
    }
}

fn load_remote_trusted_catalog(
    encoded: &[u8],
    paths: &StorePaths,
    network: &dyn StoreNetwork,
) -> Result<LoadedRemoteCatalog, StoreServiceError> {
    let document = decode_signed_catalog_document(encoded)
        .map_err(|error| StoreServiceError::Untrusted(error.to_string()))?;
    match document {
        SignedCatalogDocument::Catalog(signed) => {
            let public = load_trusted_catalog_key(paths, &signed.key_id)?;
            verify_catalog(&signed, &public)
                .map_err(|error| StoreServiceError::Untrusted(error.to_string()))?;
            Ok(LoadedRemoteCatalog {
                catalog: trusted_legacy_catalog(signed, encoded),
                encoded_shards: Vec::new(),
            })
        }
        SignedCatalogDocument::Index(signed) => {
            let public = load_trusted_catalog_key(paths, &signed.key_id)?;
            verify_catalog_index(&signed, &public)
                .map_err(|error| StoreServiceError::Untrusted(error.to_string()))?;
            let mut encoded_shards = Vec::with_capacity(signed.catalog_index.shards.len());
            for descriptor in &signed.catalog_index.shards {
                encoded_shards.push(
                    network.fetch_catalog_shard(&descriptor.url, u64::from(descriptor.bytes))?,
                );
            }
            let catalog =
                trusted_indexed_catalog(signed, encoded, encoded_shards.clone(), &public)?;
            Ok(LoadedRemoteCatalog {
                catalog,
                encoded_shards,
            })
        }
    }
}

fn load_trusted_catalog_key(
    paths: &StorePaths,
    key_id: &str,
) -> Result<[u8; 32], StoreServiceError> {
    let key_path = paths.trust_root.join(format!("{key_id}.pub"));
    let key_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&key_path)?;
    let metadata = key_file.metadata()?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.mode() & 0o022 != 0
        || (paths.enforce_root_trust && metadata.uid() != 0)
    {
        return Err(StoreServiceError::Untrusted(
            "trusted catalog key metadata is invalid".into(),
        ));
    }
    let mut public = Vec::new();
    BufReader::new(key_file).read_to_end(&mut public)?;
    public
        .try_into()
        .map_err(|_| StoreServiceError::Untrusted("trusted catalog key length is invalid".into()))
}

fn trusted_legacy_catalog(
    signed: cp0_store_protocol::SignedCatalog,
    encoded: &[u8],
) -> TrustedCatalog {
    let catalog = signed.catalog;
    let categories = derive_categories(&catalog.apps, false);
    TrustedCatalog {
        sequence: catalog.sequence,
        published_unix_seconds: catalog.published_unix_seconds,
        expires_unix_seconds: catalog.expires_unix_seconds,
        identity_sha256: cp0_store_protocol::lower_hex(&Sha256::digest(encoded)),
        apps: catalog.apps,
        categories,
        editorial: catalog.editorial,
        sharded: false,
    }
}

fn trusted_indexed_catalog(
    signed: cp0_store_protocol::SignedCatalogIndex,
    encoded_root: &[u8],
    encoded_shards: Vec<Vec<u8>>,
    public: &[u8; 32],
) -> Result<TrustedCatalog, StoreServiceError> {
    let verified = verify_catalog_shard_set(&signed, &encoded_shards, public)
        .map_err(|error| StoreServiceError::Untrusted(error.to_string()))?;
    let index = signed.catalog_index;
    let apps = verified
        .into_iter()
        .flat_map(|shard| shard.catalog_shard.apps)
        .collect::<Vec<_>>();
    Ok(TrustedCatalog {
        sequence: index.sequence,
        published_unix_seconds: index.published_unix_seconds,
        expires_unix_seconds: index.expires_unix_seconds,
        identity_sha256: cp0_store_protocol::lower_hex(&Sha256::digest(encoded_root)),
        apps,
        categories: index.categories,
        editorial: index.editorial,
        sharded: true,
    })
}

fn derive_categories(apps: &[CatalogApp], include_shard: bool) -> Vec<CatalogCategoryIndex> {
    StoreCategory::ALL
        .into_iter()
        .filter_map(|category| {
            let count = apps
                .iter()
                .filter(|app| {
                    app.discovery
                        .as_ref()
                        .is_some_and(|discovery| discovery.category == category)
                })
                .count();
            (count > 0).then(|| CatalogCategoryIndex {
                category,
                app_count: count as u16,
                shard_indices: include_shard.then_some(0).into_iter().collect(),
            })
        })
        .collect()
}

fn catalog_shard_cache_directory(paths: &StorePaths, sequence: u64) -> PathBuf {
    paths
        .cache_root
        .join("catalog-shards")
        .join(sequence.to_string())
}

fn commit_catalog_cache(
    paths: &StorePaths,
    encoded_root: &[u8],
    catalog: &TrustedCatalog,
    encoded_shards: &[Vec<u8>],
) -> Result<(), StoreServiceError> {
    if catalog.sharded {
        let base = paths.cache_root.join("catalog-shards");
        prepare_private_cache_directory(&base)?;
        let final_directory = catalog_shard_cache_directory(paths, catalog.sequence);
        if final_directory.exists() {
            verify_cached_catalog_shards(&final_directory, encoded_shards)?;
        } else {
            let staging = base.join(format!(
                ".tmp-{}-{}-{}",
                catalog.sequence,
                std::process::id(),
                STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            let result = (|| -> io::Result<()> {
                fs::create_dir(&staging)?;
                fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))?;
                for (index, encoded) in encoded_shards.iter().enumerate() {
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(staging.join(format!("{index:04}.json")))?;
                    file.write_all(encoded)?;
                    file.sync_all()?;
                }
                File::open(&staging)?.sync_all()?;
                fs::rename(&staging, &final_directory)?;
                File::open(&base)?.sync_all()?;
                Ok(())
            })();
            if result.is_err() {
                let _ = fs::remove_dir_all(&staging);
            }
            result?;
            verify_cached_catalog_shards(&final_directory, encoded_shards)?;
        }
    } else if !encoded_shards.is_empty() {
        return Err(StoreServiceError::Invalid(
            "legacy Catalog unexpectedly has shard bytes".into(),
        ));
    }
    atomic_write(&paths.catalog_cache, encoded_root)
}

fn verify_cached_catalog_shards(
    directory: &Path,
    expected: &[Vec<u8>],
) -> Result<(), StoreServiceError> {
    let metadata = fs::symlink_metadata(directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || metadata.mode() & 0o077 != 0 {
        return Err(StoreServiceError::Untrusted(
            "Catalog shard cache directory metadata is invalid".into(),
        ));
    }
    for (index, expected) in expected.iter().enumerate() {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory.join(format!("{index:04}.json")))?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.mode() & 0o077 != 0
            || metadata.len() != expected.len() as u64
        {
            return Err(StoreServiceError::Untrusted(
                "Catalog shard cache file metadata is invalid".into(),
            ));
        }
        let mut actual = Vec::new();
        file.read_to_end(&mut actual)?;
        if actual != *expected {
            return Err(StoreServiceError::Untrusted(
                "cached Catalog shard differs from the verified generation".into(),
            ));
        }
    }
    Ok(())
}

fn open_resume_file(path: &Path) -> Result<File, StoreServiceError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.mode() & 0o077 != 0 {
        return Err(StoreServiceError::Invalid(
            "partial package file metadata is invalid".into(),
        ));
    }
    Ok(file)
}

fn verify_package_file(path: &Path, app: &CatalogApp) -> Result<(), StoreServiceError> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    if file.metadata()?.len() != app.package_bytes {
        return Err(StoreServiceError::Invalid(
            "downloaded package size changed before verification".into(),
        ));
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if cp0_store_protocol::lower_hex(&hasher.finalize()) != app.package_sha256 {
        file.set_len(0)?;
        file.sync_all()?;
        return Err(StoreServiceError::Untrusted(
            "downloaded package SHA-256 does not match the catalog".into(),
        ));
    }
    Ok(())
}

fn stage_for_appd(source: &Path, inbox: &Path) -> Result<PathBuf, StoreServiceError> {
    let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let destination = inbox.join(format!("store-{}-{sequence}.capp", std::process::id()));
    let result = (|| -> Result<(), StoreServiceError> {
        let mut input = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(source)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&destination)?;
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        File::open(inbox)?.sync_all()?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&destination);
        return Err(error);
    }
    Ok(destination)
}

fn cleanup_stale_appd_handoffs(inbox: &Path) -> Result<(), StoreServiceError> {
    let metadata = fs::symlink_metadata(inbox)?;
    // SAFETY: geteuid has no pointer arguments or caller-side preconditions.
    let effective_uid = unsafe { libc::geteuid() };
    // The inbox is dedicated to cp0-stored. A service restart may remove only
    // its strict generated names, never arbitrary files or directories.
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != effective_uid
        || metadata.mode() & 0o077 != 0
    {
        return Err(StoreServiceError::Invalid(
            "appd handoff directory metadata is invalid".into(),
        ));
    }
    let mut removed = false;
    for entry in fs::read_dir(inbox)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(identifiers) = name
            .strip_prefix("store-")
            .and_then(|name| name.strip_suffix(".capp"))
        else {
            continue;
        };
        let Some((pid, sequence)) = identifiers.split_once('-') else {
            continue;
        };
        if pid.is_empty()
            || sequence.is_empty()
            || pid.parse::<u32>().is_err()
            || sequence.parse::<u64>().is_err()
        {
            continue;
        }
        if entry.file_type()?.is_dir() {
            return Err(StoreServiceError::Invalid(
                "appd handoff path is unexpectedly a directory".into(),
            ));
        }
        fs::remove_file(entry.path())?;
        removed = true;
    }
    if removed {
        File::open(inbox)?.sync_all()?;
    }
    Ok(())
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), StoreServiceError> {
    let parent = path
        .parent()
        .ok_or_else(|| StoreServiceError::Invalid("catalog cache has no parent".into()))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".catalog-{}.tmp", std::process::id()));
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(StoreServiceError::Io)
}

fn validate_content_range(
    value: &str,
    expected_start: u64,
    expected_total: u64,
) -> Result<(), StoreServiceError> {
    let value = value
        .strip_prefix("bytes ")
        .ok_or_else(|| StoreServiceError::Invalid("Content-Range unit is invalid".into()))?;
    let (range, total) = value
        .split_once('/')
        .ok_or_else(|| StoreServiceError::Invalid("Content-Range is malformed".into()))?;
    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| StoreServiceError::Invalid("Content-Range is malformed".into()))?;
    let start = start
        .parse::<u64>()
        .map_err(|_| StoreServiceError::Invalid("Content-Range start is invalid".into()))?;
    let end = end
        .parse::<u64>()
        .map_err(|_| StoreServiceError::Invalid("Content-Range end is invalid".into()))?;
    let total = total
        .parse::<u64>()
        .map_err(|_| StoreServiceError::Invalid("Content-Range total is invalid".into()))?;
    if expected_total == 0
        || start != expected_start
        || total != expected_total
        || end < start
        || end != total - 1
    {
        return Err(StoreServiceError::Invalid(
            "Content-Range does not match the signed package".into(),
        ));
    }
    Ok(())
}

fn require_https(url: &str) -> Result<(), StoreServiceError> {
    is_valid_https_url(url)
        .then_some(())
        .ok_or_else(|| StoreServiceError::Invalid("store URL is not bounded HTTPS".into()))
}

fn map_network_error(error: UreqError) -> StoreServiceError {
    match error {
        UreqError::BodyExceedsLimit(_) => {
            StoreServiceError::Invalid("store response exceeds its signed size limit".into())
        }
        UreqError::BadUri(_) | UreqError::RequireHttpsOnly(_) | UreqError::Http(_) => {
            StoreServiceError::Invalid("store URL is invalid".into())
        }
        UreqError::Tls(_) | UreqError::Rustls(_) | UreqError::TlsRequired => {
            StoreServiceError::Unavailable("TLS validation failed")
        }
        UreqError::Timeout(_) => StoreServiceError::Unavailable("HTTPS request timed out"),
        _ => StoreServiceError::Unavailable("HTTPS request failed"),
    }
}

fn service_error_response(request_id: u64, error: &StoreServiceError) -> StoreResponse {
    let (code, message) = match error {
        StoreServiceError::Unconfigured => (
            StoreErrorCode::Unconfigured,
            "store catalog URL is not configured",
        ),
        StoreServiceError::NotFound => (StoreErrorCode::NotFound, "application is not in catalog"),
        StoreServiceError::Busy => (StoreErrorCode::Busy, "store operation is already active"),
        StoreServiceError::InvalidState => (
            StoreErrorCode::InvalidState,
            "store operation is not valid in the current state",
        ),
        StoreServiceError::PolicyRestricted => (
            StoreErrorCode::PolicyRestricted,
            "store installation is blocked by device policy",
        ),
        StoreServiceError::InsufficientStorage => (
            StoreErrorCode::InsufficientStorage,
            "store installation does not have enough storage",
        ),
        StoreServiceError::CatalogChanged => (
            StoreErrorCode::CatalogChanged,
            "store catalog changed after installation preflight",
        ),
        StoreServiceError::Untrusted(_) => {
            (StoreErrorCode::Untrusted, "store trust verification failed")
        }
        StoreServiceError::Invalid(_) => (StoreErrorCode::InvalidRequest, "store data is invalid"),
        StoreServiceError::Unavailable(_) => {
            (StoreErrorCode::Unavailable, "store service is unavailable")
        }
        StoreServiceError::Io(_) => (StoreErrorCode::Internal, "store operation failed"),
    };
    StoreResponse::error(request_id, code, message)
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn auto_update_due(state: &AutoUpdatePersistentState, now: u64) -> bool {
    state.enabled
        && (state.last_check_unix_seconds == 0
            || now < state.last_check_unix_seconds
            || now.saturating_sub(state.last_check_unix_seconds) >= AUTO_UPDATE_INTERVAL_SECONDS)
}

fn normalize_metrics_state(state: &mut MetricsPersistentState, now: u64) {
    let current_week = week_start(now);
    let previous_week = current_week.saturating_sub(WEEK_SECONDS);
    state.weeks.retain(|week| {
        week.week_start_unix_seconds == current_week
            || week.week_start_unix_seconds == previous_week
    });
    if state
        .pending
        .as_ref()
        .is_some_and(|pending| pending.week_start_unix_seconds != previous_week)
    {
        state.pending = None;
    }
}

fn random_metrics_batch_id() -> Result<String, StoreServiceError> {
    let mut random = [0_u8; 16];
    let mut source = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open("/dev/urandom")?;
    source.read_exact(&mut random)?;
    Ok(format!("batch_{}", cp0_store_protocol::lower_hex(&random)))
}

fn validate_metrics_state(state: &MetricsPersistentState) -> Result<(), StoreServiceError> {
    if state.schema_version != METRICS_STATE_SCHEMA_VERSION {
        return Err(StoreServiceError::Invalid(
            "metrics state schema is unsupported".into(),
        ));
    }
    if !state.enabled && (!state.weeks.is_empty() || state.pending.is_some()) {
        return Err(StoreServiceError::Invalid(
            "disabled metrics state contains unsent data".into(),
        ));
    }
    if state.weeks.len() > 2 {
        return Err(StoreServiceError::Invalid(
            "metrics state retains too many weeks".into(),
        ));
    }
    let mut previous_week = None;
    for week in &state.weeks {
        if !cp0_store_metrics::is_week_start(week.week_start_unix_seconds)
            || previous_week.is_some_and(|previous| previous >= week.week_start_unix_seconds)
            || week.records.is_empty()
            || week.records.len() > MAX_METRIC_RECORDS
        {
            return Err(StoreServiceError::Invalid(
                "metrics week is outside limits".into(),
            ));
        }
        let report = AggregateMetricsReport {
            schema_version: METRICS_SCHEMA_VERSION,
            batch_id: "batch_00000000000000000000000000000000".into(),
            week_start_unix_seconds: week.week_start_unix_seconds,
            records: week.records.clone(),
        };
        report
            .validate()
            .map_err(|error| StoreServiceError::Invalid(error.to_string()))?;
        previous_week = Some(week.week_start_unix_seconds);
    }
    if let Some(pending) = &state.pending {
        pending
            .validate()
            .map_err(|error| StoreServiceError::Invalid(error.to_string()))?;
        if !state.weeks.iter().any(|week| {
            week.week_start_unix_seconds == pending.week_start_unix_seconds
                && week.records == pending.records
        }) {
            return Err(StoreServiceError::Invalid(
                "pending metrics do not match retained aggregates".into(),
            ));
        }
    }
    Ok(())
}

fn load_metrics_state(path: &Path) -> Result<MetricsPersistentState, StoreServiceError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(MetricsPersistentState::default());
        }
        Err(error) => return Err(error.into()),
    };
    // SAFETY: geteuid has no pointer arguments or caller-side preconditions.
    let effective_uid = unsafe { libc::geteuid() };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != effective_uid
        || metadata.mode() & 0o077 != 0
        || metadata.len() == 0
        || metadata.len() > MAX_METRICS_STATE_BYTES
    {
        return Err(StoreServiceError::Untrusted(
            "metrics state metadata is invalid".into(),
        ));
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let opened = file.metadata()?;
    if opened.dev() != metadata.dev()
        || opened.ino() != metadata.ino()
        || opened.len() != metadata.len()
    {
        return Err(StoreServiceError::Untrusted(
            "metrics state changed while opening".into(),
        ));
    }
    let mut encoded = Vec::with_capacity(opened.len() as usize);
    file.read_to_end(&mut encoded)?;
    let state: MetricsPersistentState = serde_json::from_slice(&encoded)
        .map_err(|error| StoreServiceError::Invalid(error.to_string()))?;
    validate_metrics_state(&state)?;
    Ok(state)
}

fn save_metrics_state(
    path: &Path,
    state: &MetricsPersistentState,
) -> Result<(), StoreServiceError> {
    validate_metrics_state(state)?;
    let mut encoded =
        serde_json::to_vec(state).map_err(|error| StoreServiceError::Invalid(error.to_string()))?;
    encoded.push(b'\n');
    if encoded.len() as u64 > MAX_METRICS_STATE_BYTES {
        return Err(StoreServiceError::Invalid(
            "metrics state exceeds its bound".into(),
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| StoreServiceError::Invalid("metrics state has no parent".into()))?;
    let parent_metadata = fs::symlink_metadata(parent)?;
    // SAFETY: geteuid has no pointer arguments or caller-side preconditions.
    let effective_uid = unsafe { libc::geteuid() };
    if !parent_metadata.is_dir()
        || parent_metadata.file_type().is_symlink()
        || parent_metadata.uid() != effective_uid
        || parent_metadata.mode() & 0o077 != 0
    {
        return Err(StoreServiceError::Untrusted(
            "metrics state directory is not private".into(),
        ));
    }
    let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".metrics-{}-{sequence}.tmp", std::process::id()));
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(StoreServiceError::Io)
}

fn load_auto_update_state(path: &Path) -> Result<AutoUpdatePersistentState, StoreServiceError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(AutoUpdatePersistentState::default());
        }
        Err(error) => return Err(error.into()),
    };
    // SAFETY: geteuid has no pointer arguments or caller-side preconditions.
    let effective_uid = unsafe { libc::geteuid() };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != effective_uid
        || metadata.mode() & 0o077 != 0
        || metadata.len() == 0
        || metadata.len() > MAX_AUTO_UPDATE_STATE_BYTES
    {
        return Err(StoreServiceError::Untrusted(
            "automatic update state metadata is invalid".into(),
        ));
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let opened = file.metadata()?;
    if opened.dev() != metadata.dev()
        || opened.ino() != metadata.ino()
        || opened.len() != metadata.len()
    {
        return Err(StoreServiceError::Untrusted(
            "automatic update state changed while opening".into(),
        ));
    }
    let mut encoded = Vec::with_capacity(opened.len() as usize);
    file.read_to_end(&mut encoded)?;
    let state: AutoUpdatePersistentState = serde_json::from_slice(&encoded)
        .map_err(|error| StoreServiceError::Invalid(error.to_string()))?;
    if state.schema_version != AUTO_UPDATE_STATE_SCHEMA_VERSION {
        return Err(StoreServiceError::Invalid(
            "automatic update state schema is unsupported".into(),
        ));
    }
    Ok(state)
}

fn save_auto_update_state(
    path: &Path,
    state: &AutoUpdatePersistentState,
) -> Result<(), StoreServiceError> {
    if state.schema_version != AUTO_UPDATE_STATE_SCHEMA_VERSION {
        return Err(StoreServiceError::Invalid(
            "automatic update state schema is unsupported".into(),
        ));
    }
    let mut encoded =
        serde_json::to_vec(state).map_err(|error| StoreServiceError::Invalid(error.to_string()))?;
    encoded.push(b'\n');
    if encoded.len() as u64 > MAX_AUTO_UPDATE_STATE_BYTES {
        return Err(StoreServiceError::Invalid(
            "automatic update state exceeds its bound".into(),
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| StoreServiceError::Invalid("automatic update state has no parent".into()))?;
    let parent_metadata = fs::symlink_metadata(parent)?;
    // SAFETY: geteuid has no pointer arguments or caller-side preconditions.
    let effective_uid = unsafe { libc::geteuid() };
    if !parent_metadata.is_dir()
        || parent_metadata.file_type().is_symlink()
        || parent_metadata.uid() != effective_uid
        || parent_metadata.mode() & 0o077 != 0
    {
        return Err(StoreServiceError::Untrusted(
            "automatic update state directory is not private".into(),
        ));
    }
    let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".auto-update-{}-{sequence}.tmp",
        std::process::id()
    ));
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(StoreServiceError::Io)
}

fn external_power_online(power_supply_root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(power_supply_root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let supply_type = fs::read_to_string(path.join("type"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        if supply_type == "Battery" {
            let status = fs::read_to_string(path.join("status")).unwrap_or_default();
            if matches!(status.trim(), "Charging" | "Full" | "Not charging") {
                return true;
            }
        } else if fs::read_to_string(path.join("online")).is_ok_and(|online| online.trim() == "1") {
            return true;
        }
    }
    false
}

#[cfg(any(test, target_os = "linux"))]
fn wired_interface_for_index(sys_class_net: &Path, interface_index: u32) -> bool {
    let Ok(entries) = fs::read_dir(sys_class_net) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let index = fs::read_to_string(path.join("ifindex"))
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok());
        if index != Some(interface_index) {
            continue;
        }
        let carrier =
            fs::read_to_string(path.join("carrier")).is_ok_and(|value| value.trim() == "1");
        let ethernet = fs::read_to_string(path.join("type")).is_ok_and(|value| value.trim() == "1");
        return carrier && ethernet && !path.join("wireless").exists();
    }
    false
}

#[cfg(not(target_os = "linux"))]
fn wired_default_route_available() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn wired_default_route_available() -> bool {
    #[repr(C)]
    struct NetlinkHeader {
        length: u32,
        message_type: u16,
        flags: u16,
        sequence: u32,
        port_id: u32,
    }
    #[repr(C)]
    struct RouteMessage {
        family: u8,
        destination_length: u8,
        source_length: u8,
        tos: u8,
        table: u8,
        protocol: u8,
        scope: u8,
        route_type: u8,
        flags: u32,
    }
    #[repr(C)]
    struct RouteAttribute {
        length: u16,
        attribute_type: u16,
    }
    #[repr(C)]
    struct RouteRequest {
        header: NetlinkHeader,
        route: RouteMessage,
    }

    const RTM_NEWROUTE: u16 = 24;
    const RTM_GETROUTE: u16 = 26;
    const NLMSG_DONE: u16 = 3;
    const NLMSG_ERROR: u16 = 2;
    const NLM_F_REQUEST: u16 = 1;
    const NLM_F_DUMP: u16 = 0x300;
    const RT_TABLE_MAIN: u32 = 254;
    const RTN_UNICAST: u8 = 1;
    const RTA_OIF: u16 = 4;
    const RTA_TABLE: u16 = 15;

    fn aligned(length: usize) -> usize {
        (length + 3) & !3
    }

    // SAFETY: all pointers below refer to initialized fixed-size C-compatible
    // structures or bounded receive buffers for the duration of each syscall.
    unsafe {
        let descriptor = libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            libc::NETLINK_ROUTE,
        );
        if descriptor < 0 {
            return false;
        }
        let timeout = libc::timeval {
            tv_sec: 1,
            tv_usec: 0,
        };
        let _ = libc::setsockopt(
            descriptor,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&timeout as *const libc::timeval).cast(),
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        );
        let request = RouteRequest {
            header: NetlinkHeader {
                length: std::mem::size_of::<RouteRequest>() as u32,
                message_type: RTM_GETROUTE,
                flags: NLM_F_REQUEST | NLM_F_DUMP,
                sequence: 1,
                port_id: 0,
            },
            route: RouteMessage {
                family: libc::AF_UNSPEC as u8,
                destination_length: 0,
                source_length: 0,
                tos: 0,
                table: 0,
                protocol: 0,
                scope: 0,
                route_type: 0,
                flags: 0,
            },
        };
        let mut kernel: libc::sockaddr_nl = std::mem::zeroed();
        kernel.nl_family = libc::AF_NETLINK as u16;
        let sent = libc::sendto(
            descriptor,
            (&request as *const RouteRequest).cast(),
            std::mem::size_of::<RouteRequest>(),
            0,
            (&kernel as *const libc::sockaddr_nl).cast(),
            std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        );
        if sent != std::mem::size_of::<RouteRequest>() as isize {
            libc::close(descriptor);
            return false;
        }
        let mut buffer = [0_u8; 32 * 1024];
        loop {
            let received = libc::recv(descriptor, buffer.as_mut_ptr().cast(), buffer.len(), 0);
            if received <= 0 {
                libc::close(descriptor);
                return false;
            }
            let mut offset = 0_usize;
            while offset + std::mem::size_of::<NetlinkHeader>() <= received as usize {
                let header =
                    std::ptr::read_unaligned(buffer.as_ptr().add(offset).cast::<NetlinkHeader>());
                let length = header.length as usize;
                if length < std::mem::size_of::<NetlinkHeader>()
                    || offset + length > received as usize
                {
                    libc::close(descriptor);
                    return false;
                }
                if header.message_type == NLMSG_DONE {
                    libc::close(descriptor);
                    return false;
                }
                if header.message_type == NLMSG_ERROR {
                    libc::close(descriptor);
                    return false;
                }
                if header.message_type == RTM_NEWROUTE
                    && length
                        >= std::mem::size_of::<NetlinkHeader>()
                            + std::mem::size_of::<RouteMessage>()
                {
                    let route_offset = offset + std::mem::size_of::<NetlinkHeader>();
                    let route = std::ptr::read_unaligned(
                        buffer.as_ptr().add(route_offset).cast::<RouteMessage>(),
                    );
                    let mut table = u32::from(route.table);
                    let mut interface_index = None;
                    let mut attribute_offset =
                        route_offset + aligned(std::mem::size_of::<RouteMessage>());
                    while attribute_offset + std::mem::size_of::<RouteAttribute>()
                        <= offset + length
                    {
                        let attribute = std::ptr::read_unaligned(
                            buffer
                                .as_ptr()
                                .add(attribute_offset)
                                .cast::<RouteAttribute>(),
                        );
                        let attribute_length = usize::from(attribute.length);
                        if attribute_length < std::mem::size_of::<RouteAttribute>()
                            || attribute_offset + attribute_length > offset + length
                        {
                            break;
                        }
                        let value_offset = attribute_offset + std::mem::size_of::<RouteAttribute>();
                        let value_length = attribute_length - std::mem::size_of::<RouteAttribute>();
                        if attribute.attribute_type == RTA_OIF
                            && value_length >= std::mem::size_of::<u32>()
                        {
                            interface_index = Some(std::ptr::read_unaligned(
                                buffer.as_ptr().add(value_offset).cast::<u32>(),
                            ));
                        } else if attribute.attribute_type == RTA_TABLE
                            && value_length >= std::mem::size_of::<u32>()
                        {
                            table = std::ptr::read_unaligned(
                                buffer.as_ptr().add(value_offset).cast::<u32>(),
                            );
                        }
                        attribute_offset += aligned(attribute_length);
                    }
                    if route.destination_length == 0
                        && route.route_type == RTN_UNICAST
                        && table == RT_TABLE_MAIN
                        && interface_index.is_some_and(|index| {
                            wired_interface_for_index(Path::new("/sys/class/net"), index)
                        })
                    {
                        libc::close(descriptor);
                        return true;
                    }
                }
                offset += aligned(length);
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: credentials and length point to writable objects with the sizes
    // passed to getsockopt, and stream owns a valid Unix socket descriptor.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SO_PEERCRED returned an unexpected size",
        ));
    }
    Ok(credentials.uid)
}

#[cfg(not(target_os = "linux"))]
fn peer_uid(_stream: &UnixStream) -> io::Result<u32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "peer credentials require Linux",
    ))
}

fn protocol_io(error: cp0_store_protocol::StoreProtocolError) -> io::Error {
    match error {
        cp0_store_protocol::StoreProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp0_store_metadata::{AgeRating, StoreCategory};
    use cp0_store_protocol::{
        APP_DETAILS_SCHEMA_VERSION, CATALOG_INDEX_SCHEMA_VERSION, CATALOG_SCHEMA_VERSION,
        CATALOG_SHARD_SCHEMA_VERSION, Catalog, CatalogCategoryIndex, CatalogDiscovery,
        CatalogEditorial, CatalogEditorialCollection, CatalogImageResource, CatalogIndex,
        CatalogObjectResource, CatalogResources, CatalogShard, CatalogShardDescriptor,
        EDITORIAL_CATALOG_SCHEMA_VERSION, MEDIA_CATALOG_SCHEMA_VERSION,
        RICH_CATALOG_SCHEMA_VERSION, StoreAppDetails, decode_signed_catalog, encode_app_details,
        encode_signed_catalog, encode_signed_catalog_index, encode_signed_catalog_shard,
        sign_catalog, sign_catalog_index, sign_catalog_shard,
    };
    use std::net::TcpListener;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::sync::Barrier;
    use std::sync::atomic::AtomicBool;

    #[derive(Debug)]
    struct MockSpaceProbe(u64);

    impl StoreSpaceProbe for MockSpaceProbe {
        fn available_bytes(&self, _path: &Path) -> Result<u64, StoreServiceError> {
            Ok(self.0)
        }
    }

    #[derive(Debug)]
    struct MockNetwork {
        catalog: Vec<u8>,
        package: Vec<u8>,
    }

    impl StoreNetwork for MockNetwork {
        fn fetch_catalog(&self, _url: &str) -> Result<Vec<u8>, StoreServiceError> {
            Ok(self.catalog.clone())
        }

        fn download_package(
            &self,
            _url: &str,
            destination: &Path,
            expected_bytes: u64,
            control: &mut dyn FnMut(u8) -> DownloadControl,
        ) -> Result<DownloadOutcome, StoreServiceError> {
            let mut file = open_resume_file(destination)?;
            let offset = file.metadata()?.len() as usize;
            match control(((offset as u64 * 100) / expected_bytes) as u8) {
                DownloadControl::Pause => {
                    return Ok(DownloadOutcome::Paused {
                        progress_percent: ((offset as u64 * 100) / expected_bytes) as u8,
                    });
                }
                DownloadControl::Cancel => return Ok(DownloadOutcome::Canceled),
                DownloadControl::Continue => {}
            }
            file.seek(SeekFrom::End(0))?;
            file.write_all(&self.package[offset..])?;
            file.sync_all()?;
            assert_eq!(self.package.len() as u64, expected_bytes);
            Ok(match control(100) {
                DownloadControl::Continue => DownloadOutcome::Complete,
                DownloadControl::Pause => DownloadOutcome::Paused {
                    progress_percent: 100,
                },
                DownloadControl::Cancel => DownloadOutcome::Canceled,
            })
        }
    }

    #[derive(Debug)]
    struct ShardedNetwork {
        root: Vec<u8>,
        shards: BTreeMap<String, Vec<u8>>,
        package: Vec<u8>,
    }

    impl StoreNetwork for ShardedNetwork {
        fn fetch_catalog(&self, _url: &str) -> Result<Vec<u8>, StoreServiceError> {
            Ok(self.root.clone())
        }

        fn fetch_catalog_shard(
            &self,
            url: &str,
            expected_bytes: u64,
        ) -> Result<Vec<u8>, StoreServiceError> {
            let encoded = self
                .shards
                .get(url)
                .cloned()
                .ok_or(StoreServiceError::Unavailable(
                    "Catalog shard is unavailable",
                ))?;
            if encoded.len() as u64 != expected_bytes {
                return Err(StoreServiceError::Untrusted(
                    "Catalog shard length differs from its descriptor".into(),
                ));
            }
            Ok(encoded)
        }

        fn download_package(
            &self,
            _url: &str,
            destination: &Path,
            expected_bytes: u64,
            control: &mut dyn FnMut(u8) -> DownloadControl,
        ) -> Result<DownloadOutcome, StoreServiceError> {
            if self.package.len() as u64 != expected_bytes {
                return Err(StoreServiceError::Untrusted(
                    "test package length is invalid".into(),
                ));
            }
            let mut file = open_resume_file(destination)?;
            file.set_len(0)?;
            file.write_all(&self.package)?;
            file.sync_all()?;
            Ok(match control(100) {
                DownloadControl::Continue => DownloadOutcome::Complete,
                DownloadControl::Pause => DownloadOutcome::Paused {
                    progress_percent: 100,
                },
                DownloadControl::Cancel => DownloadOutcome::Canceled,
            })
        }
    }

    #[derive(Debug)]
    struct MetricsNetwork {
        fail_first: AtomicBool,
        uploads: Mutex<Vec<AggregateMetricsReport>>,
    }

    impl MetricsNetwork {
        fn new(fail_first: bool) -> Self {
            Self {
                fail_first: AtomicBool::new(fail_first),
                uploads: Mutex::new(Vec::new()),
            }
        }
    }

    impl StoreNetwork for MetricsNetwork {
        fn fetch_catalog(&self, _url: &str) -> Result<Vec<u8>, StoreServiceError> {
            Err(StoreServiceError::Unavailable(
                "mock catalog is unavailable",
            ))
        }

        fn upload_metrics(&self, _url: &str, encoded: &[u8]) -> Result<String, StoreServiceError> {
            let report = cp0_store_metrics::decode_report(encoded)
                .map_err(|error| StoreServiceError::Invalid(error.to_string()))?;
            let batch_id = report.batch_id.clone();
            self.uploads.lock().unwrap().push(report);
            if self.fail_first.swap(false, Ordering::SeqCst) {
                Err(StoreServiceError::Unavailable(
                    "mock metrics endpoint disconnected",
                ))
            } else {
                Ok(batch_id)
            }
        }

        fn download_package(
            &self,
            _url: &str,
            _destination: &Path,
            _expected_bytes: u64,
            _control: &mut dyn FnMut(u8) -> DownloadControl,
        ) -> Result<DownloadOutcome, StoreServiceError> {
            Err(StoreServiceError::Unavailable(
                "mock package is unavailable",
            ))
        }
    }

    #[derive(Debug)]
    struct PausableNetwork {
        package: Vec<u8>,
        first_download: AtomicBool,
        first_chunk_ready: Barrier,
        continue_first_download: Barrier,
    }

    impl PausableNetwork {
        fn new(package: Vec<u8>) -> Self {
            Self {
                package,
                first_download: AtomicBool::new(true),
                first_chunk_ready: Barrier::new(2),
                continue_first_download: Barrier::new(2),
            }
        }
    }

    impl StoreNetwork for PausableNetwork {
        fn fetch_catalog(&self, _url: &str) -> Result<Vec<u8>, StoreServiceError> {
            Err(StoreServiceError::Unavailable(
                "mock refresh is unavailable",
            ))
        }

        fn download_package(
            &self,
            _url: &str,
            destination: &Path,
            expected_bytes: u64,
            control: &mut dyn FnMut(u8) -> DownloadControl,
        ) -> Result<DownloadOutcome, StoreServiceError> {
            assert_eq!(self.package.len() as u64, expected_bytes);
            let mut file = open_resume_file(destination)?;
            let offset = file.metadata()?.len() as usize;
            let initial_control = control(((offset as u64 * 100) / expected_bytes) as u8);
            if initial_control != DownloadControl::Continue {
                return Ok(match initial_control {
                    DownloadControl::Pause => DownloadOutcome::Paused {
                        progress_percent: ((offset as u64 * 100) / expected_bytes) as u8,
                    },
                    DownloadControl::Cancel => DownloadOutcome::Canceled,
                    DownloadControl::Continue => unreachable!(),
                });
            }
            file.seek(SeekFrom::End(0))?;
            if offset == 0 && self.first_download.swap(false, Ordering::SeqCst) {
                let split = self.package.len() / 2;
                file.write_all(&self.package[..split])?;
                file.sync_all()?;
                self.first_chunk_ready.wait();
                self.continue_first_download.wait();
                match control(((split as u64 * 100) / expected_bytes) as u8) {
                    DownloadControl::Pause => {
                        return Ok(DownloadOutcome::Paused {
                            progress_percent: ((split as u64 * 100) / expected_bytes) as u8,
                        });
                    }
                    DownloadControl::Cancel => return Ok(DownloadOutcome::Canceled),
                    DownloadControl::Continue => {}
                }
                file.write_all(&self.package[split..])?;
            } else {
                file.write_all(&self.package[offset..])?;
            }
            file.sync_all()?;
            Ok(match control(100) {
                DownloadControl::Continue => DownloadOutcome::Complete,
                DownloadControl::Pause => DownloadOutcome::Paused {
                    progress_percent: 100,
                },
                DownloadControl::Cancel => DownloadOutcome::Canceled,
            })
        }
    }

    #[derive(Debug)]
    struct BatchNetwork {
        packages: BTreeMap<String, Vec<u8>>,
        first_download: AtomicBool,
        first_chunk_ready: Barrier,
        continue_first_download: Barrier,
    }

    impl BatchNetwork {
        fn new(packages: BTreeMap<String, Vec<u8>>) -> Self {
            Self {
                packages,
                first_download: AtomicBool::new(true),
                first_chunk_ready: Barrier::new(2),
                continue_first_download: Barrier::new(2),
            }
        }
    }

    impl StoreNetwork for BatchNetwork {
        fn fetch_catalog(&self, _url: &str) -> Result<Vec<u8>, StoreServiceError> {
            Err(StoreServiceError::Unavailable(
                "mock refresh is unavailable",
            ))
        }

        fn download_package(
            &self,
            url: &str,
            destination: &Path,
            expected_bytes: u64,
            control: &mut dyn FnMut(u8) -> DownloadControl,
        ) -> Result<DownloadOutcome, StoreServiceError> {
            let package = self
                .packages
                .get(url)
                .ok_or(StoreServiceError::Unavailable(
                    "mock package is unavailable",
                ))?;
            assert_eq!(package.len() as u64, expected_bytes);
            let mut file = open_resume_file(destination)?;
            let offset = file.metadata()?.len() as usize;
            let initial_progress = ((offset as u64 * 100) / expected_bytes) as u8;
            match control(initial_progress) {
                DownloadControl::Pause => {
                    return Ok(DownloadOutcome::Paused {
                        progress_percent: initial_progress,
                    });
                }
                DownloadControl::Cancel => return Ok(DownloadOutcome::Canceled),
                DownloadControl::Continue => {}
            }
            file.seek(SeekFrom::End(0))?;
            if offset == 0 && self.first_download.swap(false, Ordering::SeqCst) {
                let split = package.len() / 2;
                file.write_all(&package[..split])?;
                file.sync_all()?;
                self.first_chunk_ready.wait();
                self.continue_first_download.wait();
                match control(((split as u64 * 100) / expected_bytes) as u8) {
                    DownloadControl::Pause => {
                        return Ok(DownloadOutcome::Paused {
                            progress_percent: ((split as u64 * 100) / expected_bytes) as u8,
                        });
                    }
                    DownloadControl::Cancel => return Ok(DownloadOutcome::Canceled),
                    DownloadControl::Continue => {}
                }
                file.write_all(&package[split..])?;
            } else {
                file.write_all(&package[offset..])?;
            }
            file.sync_all()?;
            Ok(match control(100) {
                DownloadControl::Continue => DownloadOutcome::Complete,
                DownloadControl::Pause => DownloadOutcome::Paused {
                    progress_percent: 100,
                },
                DownloadControl::Cancel => DownloadOutcome::Canceled,
            })
        }
    }

    #[derive(Debug)]
    struct FailingNetwork;

    impl StoreNetwork for FailingNetwork {
        fn fetch_catalog(&self, _url: &str) -> Result<Vec<u8>, StoreServiceError> {
            Err(StoreServiceError::Unavailable(
                "mock catalog is unavailable",
            ))
        }

        fn download_package(
            &self,
            _url: &str,
            _destination: &Path,
            _expected_bytes: u64,
            _control: &mut dyn FnMut(u8) -> DownloadControl,
        ) -> Result<DownloadOutcome, StoreServiceError> {
            Err(StoreServiceError::Unavailable(
                "mock package is unavailable",
            ))
        }
    }

    #[derive(Debug)]
    struct DisconnectingNetwork {
        package: Vec<u8>,
    }

    impl StoreNetwork for DisconnectingNetwork {
        fn fetch_catalog(&self, _url: &str) -> Result<Vec<u8>, StoreServiceError> {
            Err(StoreServiceError::Unavailable(
                "mock catalog is unavailable",
            ))
        }

        fn download_package(
            &self,
            _url: &str,
            destination: &Path,
            expected_bytes: u64,
            control: &mut dyn FnMut(u8) -> DownloadControl,
        ) -> Result<DownloadOutcome, StoreServiceError> {
            assert_eq!(self.package.len() as u64, expected_bytes);
            let mut file = open_resume_file(destination)?;
            assert_eq!(file.metadata()?.len(), 0);
            assert_eq!(control(0), DownloadControl::Continue);
            let split = self.package.len() / 2;
            file.write_all(&self.package[..split])?;
            file.sync_all()?;
            let _ = control(((split as u64 * 100) / expected_bytes) as u8);
            Err(StoreServiceError::Unavailable(
                "mock network disconnected during package response",
            ))
        }
    }

    #[derive(Debug)]
    struct MediaNetwork {
        catalog: Vec<u8>,
        package: Vec<u8>,
        resources: BTreeMap<String, Vec<u8>>,
    }

    impl StoreNetwork for MediaNetwork {
        fn fetch_catalog(&self, _url: &str) -> Result<Vec<u8>, StoreServiceError> {
            Ok(self.catalog.clone())
        }

        fn fetch_resource(
            &self,
            url: &str,
            expected_bytes: u64,
            max_bytes: u64,
        ) -> Result<Vec<u8>, StoreServiceError> {
            let encoded = self
                .resources
                .get(url)
                .cloned()
                .ok_or(StoreServiceError::Unavailable("mock media is unavailable"))?;
            assert!(expected_bytes <= max_bytes);
            Ok(encoded)
        }

        fn download_package(
            &self,
            _url: &str,
            destination: &Path,
            expected_bytes: u64,
            control: &mut dyn FnMut(u8) -> DownloadControl,
        ) -> Result<DownloadOutcome, StoreServiceError> {
            let mut file = open_resume_file(destination)?;
            let offset = file.metadata()?.len() as usize;
            match control(((offset as u64 * 100) / expected_bytes) as u8) {
                DownloadControl::Pause => {
                    return Ok(DownloadOutcome::Paused {
                        progress_percent: ((offset as u64 * 100) / expected_bytes) as u8,
                    });
                }
                DownloadControl::Cancel => return Ok(DownloadOutcome::Canceled),
                DownloadControl::Continue => {}
            }
            file.seek(SeekFrom::End(0))?;
            file.write_all(&self.package[offset..])?;
            file.sync_all()?;
            assert_eq!(self.package.len() as u64, expected_bytes);
            Ok(match control(100) {
                DownloadControl::Continue => DownloadOutcome::Complete,
                DownloadControl::Pause => DownloadOutcome::Paused {
                    progress_percent: 100,
                },
                DownloadControl::Cancel => DownloadOutcome::Canceled,
            })
        }
    }

    #[derive(Debug, Default)]
    struct MockInstaller {
        installed: Mutex<Vec<String>>,
    }

    #[derive(Debug)]
    struct AutoInstaller {
        snapshot: Mutex<Vec<StoreInstalledApp>>,
        automatic_installs: Mutex<Vec<String>>,
    }

    impl AutoInstaller {
        fn new(snapshot: Vec<StoreInstalledApp>) -> Self {
            Self {
                snapshot: Mutex::new(snapshot),
                automatic_installs: Mutex::new(Vec::new()),
            }
        }
    }

    impl AppInstaller for AutoInstaller {
        fn install(&self, _app: &CatalogApp, _staged_path: &Path) -> Result<(), StoreServiceError> {
            panic!("automatic update test used the manual appd installation path")
        }

        fn install_automatic(
            &self,
            app: &CatalogApp,
            staged_path: &Path,
        ) -> Result<(), StoreServiceError> {
            assert!(staged_path.is_file());
            self.automatic_installs
                .lock()
                .unwrap()
                .push(app.app_id.clone());
            if let Some(installed) = self
                .snapshot
                .lock()
                .unwrap()
                .iter_mut()
                .find(|installed| installed.app_id == app.app_id)
            {
                installed.version = app.version.clone();
                installed.permissions = app.permissions.clone();
            }
            Ok(())
        }

        fn installed_apps(&self) -> Result<Vec<StoreInstalledApp>, StoreServiceError> {
            Ok(self.snapshot.lock().unwrap().clone())
        }
    }

    #[derive(Debug)]
    struct MockAutoUpdateProbe {
        conditions: Mutex<AutoUpdateConditions>,
    }

    impl MockAutoUpdateProbe {
        fn new(charging: bool, unmetered_network: bool) -> Self {
            Self {
                conditions: Mutex::new(AutoUpdateConditions {
                    charging,
                    unmetered_network,
                }),
            }
        }

        fn set(&self, charging: bool, unmetered_network: bool) {
            *self.conditions.lock().unwrap() = AutoUpdateConditions {
                charging,
                unmetered_network,
            };
        }
    }

    impl AutoUpdateProbe for MockAutoUpdateProbe {
        fn conditions(&self) -> AutoUpdateConditions {
            *self.conditions.lock().unwrap()
        }
    }

    impl AppInstaller for MockInstaller {
        fn install(&self, app: &CatalogApp, staged_path: &Path) -> Result<(), StoreServiceError> {
            assert!(staged_path.is_file());
            self.installed.lock().unwrap().push(app.app_id.clone());
            Ok(())
        }
    }

    #[derive(Debug)]
    struct FailingInstaller;

    impl AppInstaller for FailingInstaller {
        fn install(&self, _app: &CatalogApp, _staged_path: &Path) -> Result<(), StoreServiceError> {
            Err(StoreServiceError::Unavailable(
                "mock appd handoff is unavailable",
            ))
        }
    }

    #[derive(Debug)]
    struct CommitThenDisconnectInstaller {
        first_handoff: AtomicBool,
    }

    impl CommitThenDisconnectInstaller {
        fn new() -> Self {
            Self {
                first_handoff: AtomicBool::new(true),
            }
        }
    }

    impl AppInstaller for CommitThenDisconnectInstaller {
        fn install(&self, _app: &CatalogApp, staged_path: &Path) -> Result<(), StoreServiceError> {
            assert!(staged_path.is_file());
            if self.first_handoff.swap(false, Ordering::SeqCst) {
                Err(StoreServiceError::Unavailable(
                    "mock appd committed before the connection closed",
                ))
            } else {
                Ok(())
            }
        }
    }

    #[derive(Debug)]
    struct BlockingInstaller {
        handoff_started: Barrier,
        finish_handoff: Barrier,
    }

    impl BlockingInstaller {
        fn new() -> Self {
            Self {
                handoff_started: Barrier::new(2),
                finish_handoff: Barrier::new(2),
            }
        }
    }

    impl AppInstaller for BlockingInstaller {
        fn install(&self, _app: &CatalogApp, staged_path: &Path) -> Result<(), StoreServiceError> {
            assert!(staged_path.is_file());
            self.handoff_started.wait();
            self.finish_handoff.wait();
            Ok(())
        }
    }

    fn install_synchronously(service: &Arc<StoreService>, app: &CatalogApp) {
        {
            let mut state = service.state.lock().unwrap();
            state.active_job = true;
            state.operations.insert(
                app.app_id.clone(),
                OperationState {
                    version: app.version.clone(),
                    package_sha256: app.package_sha256.clone(),
                    state: StoreAppState::Queued,
                    progress_percent: 0,
                    failure_reason: None,
                    control: DownloadControl::Continue,
                    automatic: false,
                },
            );
        }
        let result = service.install_now(app, false);
        assert!(matches!(&result, Ok(InstallOutcome::Installed)));
        service.finish_install_operation(app, result);
        service.release_job();
    }

    fn wait_for_store_state(service: &StoreService, expected: StoreAppState) -> StoreAppSummary {
        for _ in 0..2_000 {
            if let Ok(StoreResponseData::Catalog { apps, .. }) = service.catalog_response() {
                if apps[0].state == expected {
                    return apps[0].clone();
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("store operation did not reach {expected:?}");
    }

    fn wait_for_app_state(
        service: &StoreService,
        app_id: &str,
        expected: StoreAppState,
    ) -> StoreAppSummary {
        for _ in 0..2_000 {
            if let Ok(StoreResponseData::Catalog { apps, .. }) = service.catalog_response() {
                if let Some(app) = apps
                    .into_iter()
                    .find(|app| app.app_id == app_id && app.state == expected)
                {
                    return app;
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("{app_id} did not reach {expected:?}");
    }

    fn wait_for_metrics_upload(service: &StoreService) {
        for _ in 0..2_000 {
            if !service.state.lock().unwrap().metrics_upload_running {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("metrics upload did not finish");
    }

    fn wait_for_auto_update_check(service: &StoreService) {
        for _ in 0..2_000 {
            if !service.state.lock().unwrap().auto_update_running {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("automatic update check did not finish");
    }

    struct Fixture {
        root: PathBuf,
        paths: StorePaths,
        secret: [u8; 32],
        package: Vec<u8>,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/test-tmp")
                .join(format!("stored-{name}-{}", std::process::id()));
            if root.exists() {
                fs::remove_dir_all(&root).unwrap();
            }
            let cache_root = root.join("cache");
            let trust_root = root.join("trust");
            let inbox = root.join("inbox");
            let device_policy = root.join("device-policy.json");
            for directory in [&cache_root, &trust_root, &inbox] {
                fs::create_dir_all(directory).unwrap();
            }
            fs::set_permissions(&inbox, fs::Permissions::from_mode(0o700)).unwrap();
            let secret = [9; 32];
            let public = cp0_package::public_key(&secret);
            let key_id = cp0_store_protocol::lower_hex(&cp0_package::key_id(&public));
            fs::write(trust_root.join(format!("{key_id}.pub")), public).unwrap();
            fs::write(
                &device_policy,
                br#"{"schema_version":1,"authority":"personal","developer_mode_allowed":true,"recovery_mode_allowed":true,"store_install_allowed":true,"store_auto_update_allowed":true,"store_metrics_allowed":true,"app_launch_policy":"allow-all","allowed_apps":[],"denied_permissions":[]}"#,
            )
            .unwrap();
            Self {
                paths: StorePaths {
                    catalog_cache: cache_root.join("catalog.json"),
                    auto_update_state: cache_root.join("auto-update.json"),
                    metrics_state: cache_root.join("metrics.json"),
                    cache_root,
                    trust_root,
                    appd_inbox: inbox,
                    appd_socket: root.join("appd.sock"),
                    device_policy,
                    enforce_root_trust: false,
                },
                root,
                secret,
                package: b"complete signed package fixture".to_vec(),
            }
        }

        fn signed_catalog(&self, sequence: u64) -> Vec<u8> {
            let now = unix_time();
            self.signed_catalog_with_apps(
                sequence,
                now + 3600,
                vec![self.catalog_app(
                    "dev.cardputerzero.storetest",
                    "Store Test",
                    "Tests trusted resumable installation",
                )],
            )
        }

        fn catalog_app(&self, app_id: &str, name: &str, summary: &str) -> CatalogApp {
            let digest = cp0_store_protocol::lower_hex(&Sha256::digest(&self.package));
            CatalogApp {
                app_id: app_id.into(),
                name: name.into(),
                version: "1.0.0".into(),
                sdk_version: "1.0".into(),
                summary: summary.into(),
                package_url: "https://store.example.com/storetest.capp".into(),
                package_sha256: digest,
                package_bytes: self.package.len() as u64,
                permissions: Vec::new(),
                discovery: None,
                resources: None,
            }
        }

        fn signed_catalog_with_apps(
            &self,
            sequence: u64,
            expires_unix_seconds: u64,
            apps: Vec<CatalogApp>,
        ) -> Vec<u8> {
            let catalog = Catalog {
                schema_version: CATALOG_SCHEMA_VERSION,
                sequence,
                published_unix_seconds: expires_unix_seconds.saturating_sub(3600),
                expires_unix_seconds,
                apps,
                editorial: None,
            };
            encode_signed_catalog(&sign_catalog(catalog, &self.secret).unwrap()).unwrap()
        }
    }

    fn sharded_catalog_fixture(
        fixture: &Fixture,
        sequence: u64,
    ) -> (Vec<u8>, BTreeMap<String, Vec<u8>>) {
        let apps = (0..65)
            .map(|index| {
                let summary = "Verified sharded discovery application";
                let mut app = fixture.catalog_app(
                    &format!("dev.cardputerzero.sharded{index:03}"),
                    &format!("Sharded {index:03}"),
                    summary,
                );
                app.package_url = format!("https://store.example.com/app{index:03}.capp");
                app.discovery = Some(CatalogDiscovery {
                    developer: "CardputerZero Labs".into(),
                    subtitle: summary.into(),
                    category: if index % 2 == 0 {
                        StoreCategory::Utilities
                    } else {
                        StoreCategory::Productivity
                    },
                    keywords: vec!["sharded".into()],
                    age_rating: AgeRating::FourPlus,
                    privacy_url: "https://example.com/privacy".into(),
                    support_url: "https://example.com/support".into(),
                });
                app
            })
            .collect::<Vec<_>>();
        let mut descriptors = Vec::new();
        let mut resources = BTreeMap::new();
        for (index, chunk) in apps.chunks(MAX_CATALOG_APPS).enumerate() {
            let signed = sign_catalog_shard(
                CatalogShard {
                    schema_version: CATALOG_SHARD_SCHEMA_VERSION,
                    catalog_schema_version: RICH_CATALOG_SCHEMA_VERSION,
                    sequence,
                    index: index as u16,
                    apps: chunk.to_vec(),
                },
                &fixture.secret,
            )
            .unwrap();
            let encoded = encode_signed_catalog_shard(&signed).unwrap();
            let url =
                format!("https://store.example.com/generations/{sequence}/shards/{index:04}.json");
            descriptors.push(CatalogShardDescriptor {
                index: index as u16,
                url: url.clone(),
                sha256: cp0_store_protocol::lower_hex(&Sha256::digest(&encoded)),
                bytes: encoded.len() as u32,
                app_count: chunk.len() as u16,
                first_app_id: chunk.first().unwrap().app_id.clone(),
                last_app_id: chunk.last().unwrap().app_id.clone(),
            });
            resources.insert(url, encoded);
        }
        let now = unix_time();
        let index = CatalogIndex {
            schema_version: CATALOG_INDEX_SCHEMA_VERSION,
            catalog_schema_version: RICH_CATALOG_SCHEMA_VERSION,
            sequence,
            published_unix_seconds: now,
            expires_unix_seconds: now + 3600,
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
        };
        let root =
            encode_signed_catalog_index(&sign_catalog_index(index, &fixture.secret).unwrap())
                .unwrap();
        (root, resources)
    }

    fn media_catalog(
        fixture: &Fixture,
        sequence: u64,
    ) -> (Vec<u8>, BTreeMap<String, Vec<u8>>, CatalogApp) {
        let icon_url = "https://store.example.com/generations/1/assets/app/icon.png";
        let details_url = "https://store.example.com/generations/1/assets/app/details.json";
        let screenshot_url = "https://store.example.com/generations/1/assets/app/screenshots/0.png";
        let icon = png_fixture(48, 48);
        let screenshot = png_fixture(320, 170);
        let screenshot_resource = CatalogImageResource {
            url: screenshot_url.into(),
            sha256: cp0_store_protocol::lower_hex(&Sha256::digest(&screenshot)),
            bytes: screenshot.len() as u64,
            width: 320,
            height: 170,
        };
        let mut app = fixture.catalog_app(
            "dev.cardputerzero.storetest",
            "Store Test",
            "Tests trusted media caching",
        );
        app.discovery = Some(CatalogDiscovery {
            developer: "CardputerZero Labs".into(),
            subtitle: app.summary.clone(),
            category: StoreCategory::Utilities,
            keywords: vec!["media".into()],
            age_rating: AgeRating::FourPlus,
            privacy_url: "https://example.com/privacy".into(),
            support_url: "https://example.com/support".into(),
        });
        let details = encode_app_details(&StoreAppDetails {
            schema_version: APP_DETAILS_SCHEMA_VERSION,
            app_id: app.app_id.clone(),
            version: app.version.clone(),
            description: "Verified application details.".into(),
            release_notes: "Adds immutable media.".into(),
            screenshots: vec![screenshot_resource],
        })
        .unwrap();
        app.resources = Some(CatalogResources {
            icon: CatalogImageResource {
                url: icon_url.into(),
                sha256: cp0_store_protocol::lower_hex(&Sha256::digest(&icon)),
                bytes: icon.len() as u64,
                width: 48,
                height: 48,
            },
            details: CatalogObjectResource {
                url: details_url.into(),
                sha256: cp0_store_protocol::lower_hex(&Sha256::digest(&details)),
                bytes: details.len() as u64,
            },
        });
        let now = unix_time();
        let catalog = Catalog {
            schema_version: MEDIA_CATALOG_SCHEMA_VERSION,
            sequence,
            published_unix_seconds: now,
            expires_unix_seconds: now + 3600,
            apps: vec![app.clone()],
            editorial: None,
        };
        let catalog =
            encode_signed_catalog(&sign_catalog(catalog, &fixture.secret).unwrap()).unwrap();
        let resources = BTreeMap::from([
            (icon_url.into(), icon),
            (details_url.into(), details),
            (screenshot_url.into(), screenshot),
        ]);
        (catalog, resources, app)
    }

    fn png_fixture(width: u16, height: u16) -> Vec<u8> {
        let mut encoded = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::with_capacity(13);
        header.extend_from_slice(&u32::from(width).to_be_bytes());
        header.extend_from_slice(&u32::from(height).to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        append_png_chunk(&mut encoded, b"IHDR", &header);
        append_png_chunk(&mut encoded, b"IDAT", &[0]);
        append_png_chunk(&mut encoded, b"IEND", &[]);
        encoded
    }

    fn append_png_chunk(encoded: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        encoded.extend_from_slice(&(data.len() as u32).to_be_bytes());
        encoded.extend_from_slice(kind);
        encoded.extend_from_slice(data);
        let mut crc_input = kind.to_vec();
        crc_input.extend_from_slice(data);
        encoded.extend_from_slice(&test_crc32(&crc_input).to_be_bytes());
    }

    fn test_crc32(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn refresh_rejects_sequence_rollback_and_persists_verified_catalog() {
        let fixture = Fixture::new("refresh");
        let first = fixture.signed_catalog(5);
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: first.clone(),
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        service.refresh_now().unwrap();
        assert_eq!(fs::read(&fixture.paths.catalog_cache).unwrap(), first);

        let rollback = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: fixture.signed_catalog(4),
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        assert!(matches!(
            rollback.refresh_now(),
            Err(StoreServiceError::Untrusted(_))
        ));

        let mut changed = decode_signed_catalog(&first).unwrap().catalog;
        changed.apps[0].summary = "Different content under a reused sequence".into();
        let changed =
            encode_signed_catalog(&sign_catalog(changed, &fixture.secret).unwrap()).unwrap();
        let equivocation = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: changed,
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        assert!(matches!(
            equivocation.refresh_now(),
            Err(StoreServiceError::Untrusted(_))
        ));
        assert!(
            fixture.root.starts_with(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp")
            )
        );
    }

    #[test]
    fn today_projects_verified_v4_editorial_and_legacy_catalogs_return_null() {
        let fixture = Fixture::new("today");
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        let now = unix_time();
        let v1 = Catalog {
            schema_version: CATALOG_SCHEMA_VERSION,
            sequence: 21,
            published_unix_seconds: now,
            expires_unix_seconds: now + 3600,
            apps: vec![fixture.catalog_app(
                "dev.cardputerzero.storetest",
                "Store Test",
                "Legacy Store application",
            )],
            editorial: None,
        };
        let mut v2 = v1.clone();
        v2.schema_version = RICH_CATALOG_SCHEMA_VERSION;
        v2.sequence = 22;
        v2.apps[0].discovery = Some(CatalogDiscovery {
            developer: "CardputerZero Labs".into(),
            subtitle: v2.apps[0].summary.clone(),
            category: StoreCategory::Utilities,
            keywords: vec!["legacy".into()],
            age_rating: AgeRating::FourPlus,
            privacy_url: "https://example.com/privacy".into(),
            support_url: "https://example.com/support".into(),
        });
        let (v3_encoded, _, _) = media_catalog(&fixture, 23);
        let v3 = decode_signed_catalog(&v3_encoded).unwrap().catalog;
        for catalog in [v1.clone(), v2, v3] {
            let signed = sign_catalog(catalog.clone(), &fixture.secret).unwrap();
            signed.catalog.validate().unwrap();
            let encoded = encode_signed_catalog(&signed).unwrap();
            service.state.lock().unwrap().catalog = Some(trusted_legacy_catalog(signed, &encoded));
            assert!(matches!(
                service.today_response().unwrap(),
                StoreResponseData::Today {
                    sequence,
                    editorial: None,
                    ..
                } if sequence == catalog.sequence
            ));
        }

        let (v4_base, _, _) = media_catalog(&fixture, 24);
        let mut v4 = decode_signed_catalog(&v4_base).unwrap().catalog;
        let mut second = v4.apps[0].clone();
        second.app_id = "dev.cardputerzero.tools".into();
        second.name = "Tools".into();
        second.summary = "Reviewed small-screen utilities".into();
        second.package_url = "https://store.example.com/tools.capp".into();
        second.discovery.as_mut().unwrap().subtitle = second.summary.clone();
        v4.apps.push(second);
        v4.schema_version = EDITORIAL_CATALOG_SCHEMA_VERSION;
        v4.editorial = Some(CatalogEditorial {
            headline: "Made for CardputerZero".into(),
            featured_app_id: "dev.cardputerzero.storetest".into(),
            collections: vec![CatalogEditorialCollection {
                title: "Small-screen essentials".into(),
                app_ids: vec!["dev.cardputerzero.tools".into()],
            }],
        });
        let signed_v4 = sign_catalog(v4, &fixture.secret).unwrap();
        signed_v4.catalog.validate().unwrap();
        let encoded_v4 = encode_signed_catalog(&signed_v4).unwrap();
        service.state.lock().unwrap().catalog =
            Some(trusted_legacy_catalog(signed_v4, &encoded_v4));
        let response = service.dispatch(StoreRequest {
            protocol_version: cp0_store_protocol::STORE_PROTOCOL_VERSION,
            request_id: 24,
            command: StoreCommand::Today,
        });
        response.validate().unwrap();
        let cp0_store_protocol::StoreOutcome::Ok {
            data:
                StoreResponseData::Today {
                    sequence,
                    editorial: Some(editorial),
                    ..
                },
        } = response.outcome
        else {
            panic!("Today must return verified v4 editorial data");
        };
        assert_eq!(sequence, 24);
        assert_eq!(editorial.headline, "Made for CardputerZero");
        assert_eq!(editorial.featured.app_id, "dev.cardputerzero.storetest");
        assert_eq!(editorial.collections.len(), 1);
        assert_eq!(editorial.collections[0].title, "Small-screen essentials");
        assert_eq!(
            editorial.collections[0].apps[0].app_id,
            "dev.cardputerzero.tools"
        );
    }

    #[test]
    fn atomically_caches_catalog_bound_icons_details_and_screenshots() {
        let fixture = Fixture::new("media-cache");
        let (catalog, resources, app) = media_catalog(&fixture, 1);
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(MediaNetwork {
                catalog: catalog.clone(),
                package: fixture.package.clone(),
                resources,
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();

        service.refresh_now().unwrap();
        assert_eq!(fs::read(&fixture.paths.catalog_cache).unwrap(), catalog);
        let app_resources = app.resources.as_ref().unwrap();
        let icon_path =
            media_path(&fixture.paths, MediaKind::Icon, &app_resources.icon.sha256).unwrap();
        assert_eq!(fs::read(&icon_path).unwrap(), png_fixture(48, 48));
        assert_eq!(
            fs::metadata(&icon_path).unwrap().permissions().mode() & 0o077,
            0
        );

        let details = service.cache_app_details(&app.app_id).unwrap();
        assert_eq!(details.app_id, app.app_id);
        let details_path = media_path(
            &fixture.paths,
            MediaKind::Details,
            &app_resources.details.sha256,
        )
        .unwrap();
        assert!(details_path.is_file());
        assert_eq!(
            fs::metadata(&details_path).unwrap().permissions().mode() & 0o077,
            0
        );

        service.cache_screenshot(&app.app_id, 0).unwrap();
        let screenshot_path = media_path(
            &fixture.paths,
            MediaKind::Screenshot,
            &details.screenshots[0].sha256,
        )
        .unwrap();
        assert_eq!(fs::read(&screenshot_path).unwrap(), png_fixture(320, 170));
        assert_eq!(
            fs::metadata(&screenshot_path).unwrap().permissions().mode() & 0o077,
            0
        );

        let rich = service.dispatch_connection(
            StoreRequest {
                protocol_version: cp0_store_protocol::STORE_PROTOCOL_VERSION,
                request_id: 71,
                command: StoreCommand::Details {
                    app_id: app.app_id.clone(),
                },
            },
            0,
        );
        assert!(rich.descriptor.is_none());
        assert!(matches!(
            rich.response.outcome,
            cp0_store_protocol::StoreOutcome::Ok {
                data: StoreResponseData::AppDetails {
                    screenshot_count: 1,
                    ..
                }
            }
        ));

        for (request_id, selector, expected) in [
            (72, StoreMediaSelector::Icon, png_fixture(48, 48)),
            (
                73,
                StoreMediaSelector::Screenshot { index: 0 },
                png_fixture(320, 170),
            ),
        ] {
            let media = service.dispatch_connection(
                StoreRequest {
                    protocol_version: cp0_store_protocol::STORE_PROTOCOL_VERSION,
                    request_id,
                    command: StoreCommand::Media {
                        app_id: app.app_id.clone(),
                        media: selector,
                    },
                },
                0,
            );
            assert!(response_requires_descriptor(&media.response));
            let mut descriptor = media.descriptor.unwrap();
            let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFL) };
            assert_eq!(flags & libc::O_ACCMODE, libc::O_RDONLY);
            let mut received = Vec::new();
            descriptor.read_to_end(&mut received).unwrap();
            assert_eq!(received, expected);
        }
    }

    #[test]
    fn media_tampering_never_replaces_catalog_or_blocks_package_installation() {
        let fixture = Fixture::new("media-tamper");
        let (catalog, mut resources, app) = media_catalog(&fixture, 1);
        let app_resources = app.resources.as_ref().unwrap();
        resources.insert(app_resources.icon.url.clone(), b"truncated icon".to_vec());
        resources.insert(
            app_resources.details.url.clone(),
            b"substituted details".to_vec(),
        );
        let installer = Arc::new(MockInstaller::default());
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(MediaNetwork {
                catalog: catalog.clone(),
                package: fixture.package.clone(),
                resources,
            }),
            installer.clone(),
            [0],
        )
        .unwrap();

        service.refresh_now().unwrap();
        assert_eq!(fs::read(&fixture.paths.catalog_cache).unwrap(), catalog);
        assert!(
            !media_path(&fixture.paths, MediaKind::Icon, &app_resources.icon.sha256)
                .unwrap()
                .exists()
        );
        assert!(matches!(
            service.cache_app_details(&app.app_id),
            Err(StoreServiceError::Untrusted(_))
        ));
        assert!(
            !media_path(
                &fixture.paths,
                MediaKind::Details,
                &app_resources.details.sha256
            )
            .unwrap()
            .exists()
        );

        install_synchronously(&service, &app);
        assert_eq!(installer.installed.lock().unwrap().as_slice(), [app.app_id]);
    }

    #[test]
    fn screenshot_cache_evicts_the_oldest_file_before_crossing_its_budget() {
        let fixture = Fixture::new("media-lru");
        prepare_media_directories(&fixture.paths).unwrap();
        let directory = media_directory(&fixture.paths, MediaKind::Screenshot);
        let oldest = directory.join(format!("{}.png", "11".repeat(32)));
        let newest = directory.join(format!("{}.png", "22".repeat(32)));
        for path in [&oldest, &newest] {
            fs::write(path, [0_u8; 4]).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        File::open(&oldest)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(1))
            .unwrap();
        File::open(&newest)
            .unwrap()
            .set_modified(UNIX_EPOCH + Duration::from_secs(2))
            .unwrap();

        enforce_cache_capacity(&directory, 8, 4, None).unwrap();
        assert!(!oldest.exists());
        assert!(newest.exists());
        let used = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .sum::<u64>();
        assert!(used + 4 <= 8);
    }

    #[test]
    fn rejects_a_symbolic_link_media_cache_root_before_changing_permissions() {
        let fixture = Fixture::new("media-root-link");
        let target = fixture.root.join("media-target");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        symlink(&target, fixture.paths.cache_root.join("media")).unwrap();
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        );
        assert!(matches!(service, Err(StoreServiceError::Untrusted(_))));
        assert_ne!(
            fs::metadata(target).unwrap().permissions().mode() & 0o077,
            0
        );
    }

    #[test]
    fn recovers_digest_bound_partial_after_service_power_loss() {
        let fixture = Fixture::new("install");
        let catalog = fixture.signed_catalog(1);
        fs::write(&fixture.paths.catalog_cache, &catalog).unwrap();
        let signed = decode_signed_catalog(&catalog).unwrap();
        let app = signed.catalog.apps[0].clone();
        let partial_dir = fixture.paths.cache_root.join("packages");
        fs::create_dir_all(&partial_dir).unwrap();
        let partial = partial_dir.join(format!("{}.part", app.package_sha256));
        fs::write(&partial, &fixture.package[..7]).unwrap();
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o600)).unwrap();
        let installer = Arc::new(MockInstaller::default());
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog,
                package: fixture.package.clone(),
            }),
            installer.clone(),
            [0],
        )
        .unwrap();

        install_synchronously(&service, &app);
        assert_eq!(installer.installed.lock().unwrap().as_slice(), [app.app_id]);
        assert!(
            fs::read_dir(&fixture.paths.appd_inbox)
                .unwrap()
                .next()
                .is_none()
        );
        assert!(matches!(
            service.catalog_response().unwrap(),
            StoreResponseData::Catalog { apps, .. }
                if apps[0].state == StoreAppState::Installed && apps[0].progress_percent == 100
        ));
    }

    #[test]
    fn pauses_and_resumes_the_same_bounded_partial_download() {
        let fixture = Fixture::new("pause-resume");
        let catalog = fixture.signed_catalog(1);
        fs::write(&fixture.paths.catalog_cache, &catalog).unwrap();
        let app = decode_signed_catalog(&catalog).unwrap().catalog.apps[0].clone();
        let network = Arc::new(PausableNetwork::new(fixture.package.clone()));
        let installer = Arc::new(MockInstaller::default());
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            network.clone(),
            installer.clone(),
            [0],
        )
        .unwrap();

        assert_eq!(service.start_install(&app.app_id).unwrap(), app.version);
        network.first_chunk_ready.wait();
        assert_eq!(
            service
                .control_operation(&app.app_id, StoreControlAction::Pause)
                .unwrap(),
            app.version
        );
        assert!(matches!(
            service.control_operation(&app.app_id, StoreControlAction::Resume),
            Err(StoreServiceError::Busy)
        ));
        network.continue_first_download.wait();
        let paused = wait_for_store_state(&service, StoreAppState::Paused);
        let expected_progress = ((fixture.package.len() / 2) * 100 / fixture.package.len()) as u8;
        assert_eq!(paused.progress_percent, expected_progress);
        let partial = fixture
            .paths
            .cache_root
            .join("packages")
            .join(format!("{}.part", app.package_sha256));
        assert_eq!(
            fs::metadata(&partial).unwrap().len(),
            (fixture.package.len() / 2) as u64
        );

        assert_eq!(
            service
                .control_operation(&app.app_id, StoreControlAction::Resume)
                .unwrap(),
            app.version
        );
        let installed = wait_for_store_state(&service, StoreAppState::Installed);
        assert_eq!(installed.progress_percent, 100);
        assert_eq!(installer.installed.lock().unwrap().as_slice(), [app.app_id]);
    }

    #[test]
    fn cancel_during_download_removes_the_partial_package() {
        let fixture = Fixture::new("download-cancel");
        let catalog = fixture.signed_catalog(1);
        fs::write(&fixture.paths.catalog_cache, &catalog).unwrap();
        let app = decode_signed_catalog(&catalog).unwrap().catalog.apps[0].clone();
        let network = Arc::new(PausableNetwork::new(fixture.package.clone()));
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            network.clone(),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();

        service.start_install(&app.app_id).unwrap();
        network.first_chunk_ready.wait();
        service
            .control_operation(&app.app_id, StoreControlAction::Cancel)
            .unwrap();
        assert!(matches!(
            service.control_operation(&app.app_id, StoreControlAction::Pause),
            Err(StoreServiceError::InvalidState)
        ));
        service
            .control_operation(&app.app_id, StoreControlAction::Cancel)
            .unwrap();
        network.continue_first_download.wait();
        let canceled = wait_for_store_state(&service, StoreAppState::Canceled);
        assert_eq!(canceled.progress_percent, 0);
        assert!(
            !fixture
                .paths
                .cache_root
                .join("packages")
                .join(format!("{}.part", app.package_sha256))
                .exists()
        );
    }

    #[test]
    fn cancel_removes_a_paused_partial_and_allows_a_clean_retry() {
        let fixture = Fixture::new("pause-cancel");
        let catalog = fixture.signed_catalog(1);
        fs::write(&fixture.paths.catalog_cache, &catalog).unwrap();
        let app = decode_signed_catalog(&catalog).unwrap().catalog.apps[0].clone();
        let network = Arc::new(PausableNetwork::new(fixture.package.clone()));
        let installer = Arc::new(MockInstaller::default());
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            network.clone(),
            installer.clone(),
            [0],
        )
        .unwrap();

        service.start_install(&app.app_id).unwrap();
        network.first_chunk_ready.wait();
        service
            .control_operation(&app.app_id, StoreControlAction::Pause)
            .unwrap();
        network.continue_first_download.wait();
        wait_for_store_state(&service, StoreAppState::Paused);
        let partial = fixture
            .paths
            .cache_root
            .join("packages")
            .join(format!("{}.part", app.package_sha256));
        assert!(partial.is_file());

        service
            .control_operation(&app.app_id, StoreControlAction::Cancel)
            .unwrap();
        assert!(!partial.exists());
        assert_eq!(
            wait_for_store_state(&service, StoreAppState::Canceled).progress_percent,
            0
        );
        assert!(matches!(
            service.control_operation(&app.app_id, StoreControlAction::Resume),
            Err(StoreServiceError::InvalidState)
        ));
        assert!(installer.installed.lock().unwrap().is_empty());

        service.start_install(&app.app_id).unwrap();
        wait_for_store_state(&service, StoreAppState::Installed);
        assert_eq!(installer.installed.lock().unwrap().as_slice(), [app.app_id]);
    }

    #[test]
    fn install_batch_is_atomic_bounded_and_continues_after_item_control() {
        let fixture = Fixture::new("install-batch");
        let definitions = [
            ("dev.cardputerzero.alpha", b"alpha package".to_vec()),
            ("dev.cardputerzero.beta", b"beta package bytes".to_vec()),
            ("dev.cardputerzero.gamma", b"gamma package payload".to_vec()),
        ];
        let mut apps = Vec::new();
        let mut packages = BTreeMap::new();
        for (app_id, package) in definitions {
            let mut app = fixture.catalog_app(app_id, app_id, "Batch update fixture");
            app.package_url = format!(
                "https://store.example.com/{}.capp",
                app_id.rsplit('.').next().unwrap()
            );
            app.package_sha256 = cp0_store_protocol::lower_hex(&Sha256::digest(&package));
            app.package_bytes = package.len() as u64;
            packages.insert(app.package_url.clone(), package);
            apps.push(app);
        }
        let catalog = fixture.signed_catalog_with_apps(1, unix_time() + 3600, apps.clone());
        fs::write(&fixture.paths.catalog_cache, catalog).unwrap();
        let network = Arc::new(BatchNetwork::new(packages.clone()));
        let installer = Arc::new(MockInstaller::default());
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            network.clone(),
            installer.clone(),
            [0],
        )
        .unwrap();
        let app_ids = apps
            .iter()
            .map(|app| app.app_id.clone())
            .collect::<Vec<_>>();

        assert!(matches!(
            service.start_install_batch(&[app_ids[0].clone(), "dev.cardputerzero.missing".into()]),
            Err(StoreServiceError::NotFound)
        ));
        {
            let state = service.state.lock().unwrap();
            assert!(!state.active_job && state.operations.is_empty());
        }

        let accepted = service.start_install_batch(&app_ids).unwrap();
        assert_eq!(
            accepted
                .iter()
                .map(|app| app.app_id.as_str())
                .collect::<Vec<_>>(),
            app_ids.iter().map(String::as_str).collect::<Vec<_>>()
        );
        network.first_chunk_ready.wait();
        assert!(matches!(
            service.start_refresh(),
            Err(StoreServiceError::Busy)
        ));
        assert_eq!(
            wait_for_app_state(&service, &app_ids[0], StoreAppState::Downloading).state,
            StoreAppState::Downloading
        );
        assert_eq!(
            wait_for_app_state(&service, &app_ids[1], StoreAppState::Queued).state,
            StoreAppState::Queued
        );
        service
            .control_operation(&app_ids[0], StoreControlAction::Pause)
            .unwrap();
        service
            .control_operation(&app_ids[1], StoreControlAction::Cancel)
            .unwrap();
        network.continue_first_download.wait();

        wait_for_app_state(&service, &app_ids[0], StoreAppState::Paused);
        wait_for_app_state(&service, &app_ids[1], StoreAppState::Canceled);
        wait_for_app_state(&service, &app_ids[2], StoreAppState::Installed);
        let package_path = |app: &CatalogApp| {
            fixture
                .paths
                .cache_root
                .join("packages")
                .join(format!("{}.part", app.package_sha256))
        };
        assert_eq!(
            fs::metadata(package_path(&apps[0])).unwrap().len(),
            (packages[&apps[0].package_url].len() / 2) as u64
        );
        assert!(!package_path(&apps[1]).exists());
        assert_eq!(
            installer.installed.lock().unwrap().as_slice(),
            [app_ids[2].clone()]
        );

        service
            .control_operation(&app_ids[0], StoreControlAction::Resume)
            .unwrap();
        wait_for_app_state(&service, &app_ids[0], StoreAppState::Installed);
        assert_eq!(
            installer.installed.lock().unwrap().as_slice(),
            [app_ids[2].clone(), app_ids[0].clone()]
        );
    }

    #[test]
    fn classifies_closed_install_failure_reasons() {
        let network_fixture = Fixture::new("failure-network");
        let network_catalog = network_fixture.signed_catalog(1);
        fs::write(&network_fixture.paths.catalog_cache, &network_catalog).unwrap();
        let network_app = decode_signed_catalog(&network_catalog)
            .unwrap()
            .catalog
            .apps[0]
            .clone();
        let network_service = StoreService::new(
            network_fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(FailingNetwork),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        network_service.start_install(&network_app.app_id).unwrap();
        assert_eq!(
            wait_for_store_state(&network_service, StoreAppState::Failed).failure_reason,
            Some(StoreFailureReason::Network)
        );

        let storage_fixture = Fixture::new("failure-storage");
        let storage_catalog = storage_fixture.signed_catalog(1);
        fs::write(&storage_fixture.paths.catalog_cache, &storage_catalog).unwrap();
        let storage_app = decode_signed_catalog(&storage_catalog)
            .unwrap()
            .catalog
            .apps[0]
            .clone();
        let storage_service = StoreService::new(
            storage_fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: storage_fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        let packages = storage_fixture.paths.cache_root.join("packages");
        fs::remove_dir(&packages).unwrap();
        fs::write(&packages, b"not a directory").unwrap();
        assert!(matches!(
            storage_service.start_install(&storage_app.app_id),
            Err(StoreServiceError::Io(_))
        ));

        let verification_fixture = Fixture::new("failure-verification");
        let verification_catalog = verification_fixture.signed_catalog(1);
        fs::write(
            &verification_fixture.paths.catalog_cache,
            &verification_catalog,
        )
        .unwrap();
        let verification_app = decode_signed_catalog(&verification_catalog)
            .unwrap()
            .catalog
            .apps[0]
            .clone();
        let verification_service = StoreService::new(
            verification_fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: vec![b'x'; verification_fixture.package.len()],
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        verification_service
            .start_install(&verification_app.app_id)
            .unwrap();
        assert_eq!(
            wait_for_store_state(&verification_service, StoreAppState::Failed).failure_reason,
            Some(StoreFailureReason::Verification)
        );

        let installer_fixture = Fixture::new("failure-installer");
        let installer_catalog = installer_fixture.signed_catalog(1);
        fs::write(&installer_fixture.paths.catalog_cache, &installer_catalog).unwrap();
        let installer_app = decode_signed_catalog(&installer_catalog)
            .unwrap()
            .catalog
            .apps[0]
            .clone();
        let installer_service = StoreService::new(
            installer_fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: installer_fixture.package.clone(),
            }),
            Arc::new(FailingInstaller),
            [0],
        )
        .unwrap();
        installer_service
            .start_install(&installer_app.app_id)
            .unwrap();
        assert_eq!(
            wait_for_store_state(&installer_service, StoreAppState::Failed).failure_reason,
            Some(StoreFailureReason::Installer)
        );
    }

    #[test]
    fn rejects_control_after_appd_handoff_begins() {
        let fixture = Fixture::new("install-control");
        let catalog = fixture.signed_catalog(1);
        fs::write(&fixture.paths.catalog_cache, &catalog).unwrap();
        let app = decode_signed_catalog(&catalog).unwrap().catalog.apps[0].clone();
        let installer = Arc::new(BlockingInstaller::new());
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            installer.clone(),
            [0],
        )
        .unwrap();

        service.start_install(&app.app_id).unwrap();
        installer.handoff_started.wait();
        assert_eq!(
            wait_for_store_state(&service, StoreAppState::Installing).progress_percent,
            100
        );
        for action in [StoreControlAction::Pause, StoreControlAction::Cancel] {
            assert!(matches!(
                service.control_operation(&app.app_id, action),
                Err(StoreServiceError::InvalidState)
            ));
        }
        installer.finish_handoff.wait();
        wait_for_store_state(&service, StoreAppState::Installed);
    }

    #[test]
    fn recovers_after_network_disconnect_and_service_restart() {
        let fixture = Fixture::new("network-restart");
        let catalog = fixture.signed_catalog(1);
        fs::write(&fixture.paths.catalog_cache, &catalog).unwrap();
        let app = decode_signed_catalog(&catalog).unwrap().catalog.apps[0].clone();
        let partial = fixture
            .paths
            .cache_root
            .join("packages")
            .join(format!("{}.part", app.package_sha256));
        let first_service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(DisconnectingNetwork {
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        first_service.start_install(&app.app_id).unwrap();
        assert_eq!(
            wait_for_store_state(&first_service, StoreAppState::Failed).failure_reason,
            Some(StoreFailureReason::Network)
        );
        assert_eq!(
            fs::metadata(&partial).unwrap().len(),
            (fixture.package.len() / 2) as u64
        );
        drop(first_service);

        let installer = Arc::new(MockInstaller::default());
        let restarted_service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            installer.clone(),
            [0],
        )
        .unwrap();
        restarted_service.start_install(&app.app_id).unwrap();
        wait_for_store_state(&restarted_service, StoreAppState::Installed);
        assert_eq!(fs::read(&partial).unwrap(), fixture.package);
        assert_eq!(installer.installed.lock().unwrap().as_slice(), [app.app_id]);
    }

    #[test]
    fn truncates_bad_digest_and_recovers_from_a_clean_retry() {
        let fixture = Fixture::new("digest-recovery");
        let catalog = fixture.signed_catalog(1);
        fs::write(&fixture.paths.catalog_cache, &catalog).unwrap();
        let app = decode_signed_catalog(&catalog).unwrap().catalog.apps[0].clone();
        let partial = fixture
            .paths
            .cache_root
            .join("packages")
            .join(format!("{}.part", app.package_sha256));
        let rejected_installer = Arc::new(MockInstaller::default());
        let bad_service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: vec![b'x'; fixture.package.len()],
            }),
            rejected_installer.clone(),
            [0],
        )
        .unwrap();
        bad_service.start_install(&app.app_id).unwrap();
        assert_eq!(
            wait_for_store_state(&bad_service, StoreAppState::Failed).failure_reason,
            Some(StoreFailureReason::Verification)
        );
        assert_eq!(fs::metadata(&partial).unwrap().len(), 0);
        assert!(rejected_installer.installed.lock().unwrap().is_empty());
        drop(bad_service);

        let recovered_installer = Arc::new(MockInstaller::default());
        let recovered_service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            recovered_installer.clone(),
            [0],
        )
        .unwrap();
        recovered_service.start_install(&app.app_id).unwrap();
        wait_for_store_state(&recovered_service, StoreAppState::Installed);
        assert_eq!(
            recovered_installer.installed.lock().unwrap().as_slice(),
            [app.app_id]
        );
    }

    #[test]
    fn retries_an_appd_commit_after_the_response_connection_is_lost() {
        let fixture = Fixture::new("appd-replay");
        let catalog = fixture.signed_catalog(1);
        fs::write(&fixture.paths.catalog_cache, &catalog).unwrap();
        let app = decode_signed_catalog(&catalog).unwrap().catalog.apps[0].clone();
        let installer = Arc::new(CommitThenDisconnectInstaller::new());
        let first_service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            installer.clone(),
            [0],
        )
        .unwrap();
        first_service.start_install(&app.app_id).unwrap();
        assert_eq!(
            wait_for_store_state(&first_service, StoreAppState::Failed).failure_reason,
            Some(StoreFailureReason::Installer)
        );
        assert!(
            fs::read_dir(&fixture.paths.appd_inbox)
                .unwrap()
                .next()
                .is_none()
        );
        drop(first_service);

        let restarted_service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            installer,
            [0],
        )
        .unwrap();
        restarted_service.start_install(&app.app_id).unwrap();
        wait_for_store_state(&restarted_service, StoreAppState::Installed);
        assert!(
            fs::read_dir(&fixture.paths.appd_inbox)
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn startup_removes_only_strict_stale_appd_handoffs() {
        let fixture = Fixture::new("stale-handoff");
        let stale = fixture.paths.appd_inbox.join("store-123-7.capp");
        let unrelated = fixture.paths.appd_inbox.join("operator-note");
        fs::write(&stale, b"stale handoff").unwrap();
        fs::write(&unrelated, b"preserve").unwrap();
        StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(FailingNetwork),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        assert!(!stale.exists());
        assert_eq!(fs::read(unrelated).unwrap(), b"preserve");
    }

    #[test]
    fn startup_refuses_generated_handoff_directories_without_removing_them() {
        let fixture = Fixture::new("handoff-directory");
        let suspicious = fixture.paths.appd_inbox.join("store-123-8.capp");
        fs::create_dir(&suspicious).unwrap();
        assert!(matches!(
            StoreService::new(
                fixture.paths.clone(),
                StoreConfig {
                    catalog_url: None,
                    metrics_url: None
                },
                Arc::new(FailingNetwork),
                Arc::new(MockInstaller::default()),
                [0],
            ),
            Err(StoreServiceError::Invalid(_))
        ));
        assert!(suspicious.is_dir());
    }

    #[test]
    fn failed_appd_staging_copy_removes_its_incomplete_destination() {
        let fixture = Fixture::new("handoff-copy-failure");
        let invalid_source = fixture.root.join("source-directory");
        fs::create_dir(&invalid_source).unwrap();
        assert!(stage_for_appd(&invalid_source, &fixture.paths.appd_inbox).is_err());
        assert!(
            fs::read_dir(&fixture.paths.appd_inbox)
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn catalog_digest_change_invalidates_resume_and_remains_cancelable() {
        let fixture = Fixture::new("catalog-changed");
        let original_catalog = fixture.signed_catalog(1);
        fs::write(&fixture.paths.catalog_cache, &original_catalog).unwrap();
        let original_app = decode_signed_catalog(&original_catalog)
            .unwrap()
            .catalog
            .apps[0]
            .clone();
        let replacement_package = b"replacement signed package fixture".to_vec();
        let mut replacement_app = original_app.clone();
        replacement_app.package_sha256 =
            cp0_store_protocol::lower_hex(&Sha256::digest(&replacement_package));
        replacement_app.package_bytes = replacement_package.len() as u64;
        let replacement_catalog =
            fixture.signed_catalog_with_apps(2, unix_time() + 3600, vec![replacement_app.clone()]);
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: replacement_catalog,
                package: replacement_package,
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        let original_partial = fixture
            .paths
            .cache_root
            .join("packages")
            .join(format!("{}.part", original_app.package_sha256));
        fs::write(&original_partial, &fixture.package[..7]).unwrap();
        fs::set_permissions(&original_partial, fs::Permissions::from_mode(0o600)).unwrap();
        service.state.lock().unwrap().operations.insert(
            original_app.app_id.clone(),
            OperationState {
                version: original_app.version.clone(),
                package_sha256: original_app.package_sha256.clone(),
                state: StoreAppState::Paused,
                progress_percent: 20,
                failure_reason: None,
                control: DownloadControl::Continue,
                automatic: false,
            },
        );

        service.refresh_now().unwrap();
        let changed = wait_for_store_state(&service, StoreAppState::Failed);
        assert_eq!(
            changed.failure_reason,
            Some(StoreFailureReason::CatalogChanged)
        );
        assert_eq!(changed.version, replacement_app.version);
        assert!(matches!(
            service.control_operation(&original_app.app_id, StoreControlAction::Resume),
            Err(StoreServiceError::InvalidState)
        ));
        service
            .control_operation(&original_app.app_id, StoreControlAction::Cancel)
            .unwrap();
        assert!(!original_partial.exists());
        assert!(matches!(
            service.catalog_response().unwrap(),
            StoreResponseData::Catalog { apps, .. }
                if apps[0].state == StoreAppState::Available && apps[0].failure_reason.is_none()
        ));
    }

    #[test]
    fn searches_verified_catalog_with_stable_ranking_and_pagination() {
        let fixture = Fixture::new("search");
        let apps = vec![
            fixture.catalog_app(
                "dev.cardputerzero.alpha",
                "Noteworthy",
                "Starts with the query",
            ),
            fixture.catalog_app("dev.cardputerzero.exact", "Note", "Exact name match"),
            fixture.catalog_app(
                "dev.cardputerzero.middle",
                "Quick Notes",
                "Contains the query",
            ),
            fixture.catalog_app(
                "dev.cardputerzero.noteutility",
                "Utility",
                "Matches through the application ID",
            ),
            fixture.catalog_app(
                "dev.cardputerzero.summary",
                "Journal",
                "A private note keeper",
            ),
        ];
        let catalog = fixture.signed_catalog_with_apps(8, unix_time() + 3600, apps);
        fs::write(&fixture.paths.catalog_cache, catalog).unwrap();
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();

        let first = service.search_response("NOTE".into(), 0, 2).unwrap();
        assert!(matches!(
            first,
            StoreResponseData::SearchResults {
                total: 5,
                next_offset: Some(2),
                stale: false,
                apps,
                ..
            } if apps.iter().map(|app| app.name.as_str()).collect::<Vec<_>>()
                == ["Note", "Noteworthy"]
        ));

        let middle = service.search_response("note".into(), 2, 2).unwrap();
        assert!(matches!(
            middle,
            StoreResponseData::SearchResults {
                total: 5,
                next_offset: Some(4),
                apps,
                ..
            } if apps.iter().map(|app| app.name.as_str()).collect::<Vec<_>>()
                == ["Quick Notes", "Journal"]
        ));

        let last = service.search_response("note".into(), 4, 2).unwrap();
        assert!(matches!(
            last,
            StoreResponseData::SearchResults {
                total: 5,
                next_offset: None,
                apps,
                ..
            } if apps.iter().map(|app| app.name.as_str()).collect::<Vec<_>>() == ["Utility"]
        ));
    }

    #[test]
    fn atomically_loads_shards_and_browses_the_signed_category_index() {
        let fixture = Fixture::new("sharded-browse");
        let (root, shards) = sharded_catalog_fixture(&fixture, 40);
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(ShardedNetwork {
                root: root.clone(),
                shards,
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();

        service.refresh_now().unwrap();
        assert_eq!(service.current_catalog_sequence().unwrap(), 40);
        assert_eq!(fs::read(&fixture.paths.catalog_cache).unwrap(), root);
        let shard_directory = catalog_shard_cache_directory(&fixture.paths, 40);
        assert!(shard_directory.join("0000.json").is_file());
        assert!(shard_directory.join("0001.json").is_file());

        let browse = service
            .browse_response(Some(StoreCategory::Utilities), 32, 8)
            .unwrap();
        assert!(matches!(
            &browse,
            StoreResponseData::BrowseResults {
                category: Some(StoreCategory::Utilities),
                total: 33,
                next_offset: None,
                apps,
                ..
            } if apps.len() == 1 && apps[0].app_id == "dev.cardputerzero.sharded064"
        ));
        StoreResponse::success(1, browse).validate().unwrap();

        assert!(matches!(
            service.search_response("sharded".into(), 0, 8).unwrap(),
            StoreResponseData::SearchResults {
                total: 65,
                next_offset: Some(8),
                apps,
                ..
            } if apps.len() == 8
        ));
    }

    #[test]
    fn missing_shard_preserves_the_previous_cached_catalog() {
        let fixture = Fixture::new("sharded-incomplete");
        let legacy = fixture.signed_catalog(41);
        fs::write(&fixture.paths.catalog_cache, &legacy).unwrap();
        let (root, mut shards) = sharded_catalog_fixture(&fixture, 42);
        let missing = shards.keys().next_back().unwrap().clone();
        shards.remove(&missing);
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(ShardedNetwork {
                root,
                shards,
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();

        assert!(matches!(
            service.refresh_now(),
            Err(StoreServiceError::Unavailable(
                "Catalog shard is unavailable"
            ))
        ));
        assert_eq!(service.current_catalog_sequence().unwrap(), 41);
        assert_eq!(fs::read(&fixture.paths.catalog_cache).unwrap(), legacy);
        assert!(!catalog_shard_cache_directory(&fixture.paths, 42).exists());
    }

    #[test]
    fn searches_signed_rich_discovery_fields_from_catalog_v2() {
        let fixture = Fixture::new("rich-search");
        let mut app = fixture.catalog_app(
            "dev.cardputerzero.signal",
            "Signal Lab",
            "Inspect local radio signals",
        );
        app.discovery = Some(CatalogDiscovery {
            developer: "CardputerZero Labs".into(),
            subtitle: app.summary.clone(),
            category: StoreCategory::DeveloperTools,
            keywords: vec!["diagnostics".into(), "radio".into()],
            age_rating: AgeRating::FourPlus,
            privacy_url: "https://example.com/privacy".into(),
            support_url: "https://example.com/support".into(),
        });
        let now = unix_time();
        let catalog = Catalog {
            schema_version: RICH_CATALOG_SCHEMA_VERSION,
            sequence: 9,
            published_unix_seconds: now,
            expires_unix_seconds: now + 3600,
            apps: vec![app],
            editorial: None,
        };
        let encoded =
            encode_signed_catalog(&sign_catalog(catalog, &fixture.secret).unwrap()).unwrap();
        fs::write(&fixture.paths.catalog_cache, encoded).unwrap();
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();

        for query in ["cardputerzero", "developer-tools", "diagnostics", "radio"] {
            assert!(matches!(
                service.search_response(query.into(), 0, 8).unwrap(),
                StoreResponseData::SearchResults { total: 1, apps, .. }
                    if apps[0].app_id == "dev.cardputerzero.signal"
            ));
        }
    }

    #[test]
    fn preflight_binds_policy_permissions_capacity_catalog_and_authorization() {
        let fixture = Fixture::new("install-preflight");
        let mut app = fixture.catalog_app(
            "dev.cardputerzero.storetest",
            "Store Test",
            "Tests exact install preflight binding",
        );
        app.permissions = vec![
            cp0_manifest::Permission::CameraCapture,
            cp0_manifest::Permission::NetworkClient,
        ];
        let catalog = fixture.signed_catalog_with_apps(7, unix_time() + 3600, vec![app]);
        fs::write(&fixture.paths.catalog_cache, catalog).unwrap();
        fs::write(
            &fixture.paths.device_policy,
            br#"{"schema_version":1,"authority":"personal","developer_mode_allowed":true,"recovery_mode_allowed":true,"store_install_allowed":true,"app_launch_policy":"allow-all","allowed_apps":[],"denied_permissions":["camera.capture"]}"#,
        )
        .unwrap();
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        let app_ids = vec!["dev.cardputerzero.storetest".into()];
        assert!(matches!(
            service.preflight_install(&app_ids, 6),
            Err(StoreServiceError::CatalogChanged)
        ));
        let preflight = service.preflight_install(&app_ids, 7).unwrap();
        assert!(preflight.required_bytes >= INSTALL_DATA_RESERVE_BYTES);
        assert!(preflight.available_bytes >= preflight.required_bytes);
        assert_eq!(
            preflight.apps[0].permissions,
            vec![
                cp0_manifest::Permission::CameraCapture,
                cp0_manifest::Permission::NetworkClient,
            ]
        );
        assert_eq!(
            preflight.apps[0].policy_denied_permissions,
            vec![cp0_manifest::Permission::CameraCapture]
        );
        assert!(matches!(
            service.start_authorized_install(&app_ids[0], preflight.authorization_id + 1),
            Err(StoreServiceError::InvalidState)
        ));
        let preflight = service.preflight_install(&app_ids, 7).unwrap();
        service
            .start_authorized_install(&app_ids[0], preflight.authorization_id)
            .unwrap();
        wait_for_store_state(&service, StoreAppState::Installed);
        assert!(matches!(
            service.start_authorized_install(&app_ids[0], preflight.authorization_id),
            Err(StoreServiceError::InvalidState)
        ));

        fs::write(
            &fixture.paths.device_policy,
            br#"{"schema_version":1,"authority":"personal","developer_mode_allowed":true,"recovery_mode_allowed":true,"store_install_allowed":false,"app_launch_policy":"allow-all","allowed_apps":[],"denied_permissions":["camera.capture"]}"#,
        )
        .unwrap();
        assert!(matches!(
            service.preflight_install(&app_ids, 7),
            Err(StoreServiceError::PolicyRestricted)
        ));

        fs::write(
            &fixture.paths.device_policy,
            br#"{"schema_version":1,"authority":"personal","developer_mode_allowed":true,"recovery_mode_allowed":true,"store_install_allowed":true,"app_launch_policy":"allow-all","allowed_apps":[],"denied_permissions":["camera.capture"]}"#,
        )
        .unwrap();
        let low_space = StoreService::new_with_space_probe(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
            Arc::new(MockSpaceProbe(1)),
        )
        .unwrap();
        assert!(matches!(
            low_space.preflight_install(&app_ids, 7),
            Err(StoreServiceError::InsufficientStorage)
        ));
    }

    #[test]
    fn permits_search_but_rejects_install_from_a_stale_catalog() {
        let fixture = Fixture::new("stale-search");
        let catalog = fixture.signed_catalog_with_apps(
            2,
            unix_time().saturating_sub(1),
            vec![fixture.catalog_app(
                "dev.cardputerzero.storetest",
                "Store Test",
                "Tests stale local search",
            )],
        );
        fs::write(&fixture.paths.catalog_cache, catalog).unwrap();
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: Vec::new(),
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();

        assert!(matches!(
            service.search_response("store".into(), 0, 8).unwrap(),
            StoreResponseData::SearchResults {
                stale: true,
                total: 1,
                ..
            }
        ));
        assert!(matches!(
            service.start_install("dev.cardputerzero.storetest"),
            Err(StoreServiceError::Untrusted(_))
        ));
    }

    #[test]
    fn persists_default_off_auto_update_preference_and_reports_closed_gates() {
        let fixture = Fixture::new("auto-update-preference");
        let probe = Arc::new(MockAutoUpdateProbe::new(false, false));
        let service = StoreService::new_with_probes(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog: fixture.signed_catalog(1),
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
            Arc::new(MockSpaceProbe(u64::MAX)),
            probe.clone(),
        )
        .unwrap();
        let status = service.auto_update_status().unwrap();
        assert!(!status.enabled && status.policy_allowed && !status.due);
        assert!(!status.charging && !status.unmetered_network && !status.checking);

        let status = service.set_auto_update(true).unwrap();
        assert!(status.enabled && status.due);
        let metadata = fs::symlink_metadata(&fixture.paths.auto_update_state).unwrap();
        assert!(metadata.is_file() && metadata.mode() & 0o777 == 0o600);
        drop(service);

        let restarted = StoreService::new_with_probes(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(FailingNetwork),
            Arc::new(MockInstaller::default()),
            [0],
            Arc::new(MockSpaceProbe(u64::MAX)),
            probe.clone(),
        )
        .unwrap();
        assert!(restarted.auto_update_status().unwrap().enabled);
        probe.set(true, false);
        let status = restarted.auto_update_status().unwrap();
        assert!(status.charging && !status.unmetered_network);
        assert!(matches!(
            restarted.start_auto_update_check(),
            Err(StoreServiceError::Unavailable(_))
        ));
        restarted.set_auto_update(false).unwrap();
        drop(restarted);

        let disabled = load_auto_update_state(&fixture.paths.auto_update_state).unwrap();
        assert!(!disabled.enabled && disabled.last_check_unix_seconds == 0);
        fs::write(
            &fixture.paths.device_policy,
            br#"{"schema_version":1,"authority":"personal","developer_mode_allowed":true,"recovery_mode_allowed":true,"store_install_allowed":true,"store_auto_update_allowed":false,"app_launch_policy":"allow-all","allowed_apps":[],"denied_permissions":[]}"#,
        )
        .unwrap();
        let locked = StoreService::new_with_probes(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: None,
            },
            Arc::new(FailingNetwork),
            Arc::new(MockInstaller::default()),
            [0],
            Arc::new(MockSpaceProbe(u64::MAX)),
            probe,
        )
        .unwrap();
        assert!(matches!(
            locked.set_auto_update(true),
            Err(StoreServiceError::PolicyRestricted)
        ));
        drop(locked);
        fs::set_permissions(
            &fixture.paths.auto_update_state,
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(matches!(
            load_auto_update_state(&fixture.paths.auto_update_state),
            Err(StoreServiceError::Untrusted(_))
        ));
    }

    #[test]
    fn metrics_are_default_off_bounded_and_cleared_on_consent_or_policy_revocation() {
        let fixture = Fixture::new("metrics-consent");
        let app_id = "dev.cardputerzero.metrics";
        let version = "1.2.3";
        let installer = Arc::new(AutoInstaller::new(vec![StoreInstalledApp {
            app_id: app_id.into(),
            version: version.into(),
            permissions: Vec::new(),
        }]));
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: Some("https://store.example.com/metrics/v1/aggregate".into()),
            },
            Arc::new(MetricsNetwork::new(false)),
            installer,
            [0],
        )
        .unwrap();

        let status = service.metrics_status().unwrap();
        assert!(!status.enabled && status.policy_allowed && status.configured && !status.pending);
        assert!(matches!(
            service.record_runtime_metric(app_id, version, StoreRuntimeMetricEvent::Launch),
            Err(StoreServiceError::InvalidState)
        ));
        service.set_metrics(true).unwrap();
        service
            .record_runtime_metric(app_id, version, StoreRuntimeMetricEvent::Launch)
            .unwrap();
        service
            .record_runtime_metric(app_id, version, StoreRuntimeMetricEvent::Crash)
            .unwrap();
        service.record_install_metric(app_id, version).unwrap();
        {
            let state = service.state.lock().unwrap();
            let record = &state.metrics.weeks[0].records[0];
            assert_eq!(
                (record.installs, record.launches, record.crashes),
                (1, 1, 1)
            );
        }
        assert!(
            fs::read_to_string(&fixture.paths.metrics_state)
                .unwrap()
                .contains(app_id)
        );

        service.set_metrics(false).unwrap();
        assert_eq!(
            load_metrics_state(&fixture.paths.metrics_state).unwrap(),
            MetricsPersistentState::default()
        );
        service.set_metrics(true).unwrap();
        service.record_install_metric(app_id, version).unwrap();
        let mut policy: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.paths.device_policy).unwrap()).unwrap();
        policy["store_metrics_allowed"] = serde_json::Value::Bool(false);
        fs::write(
            &fixture.paths.device_policy,
            serde_json::to_vec(&policy).unwrap(),
        )
        .unwrap();
        let status = service.metrics_status().unwrap();
        assert!(!status.enabled && !status.policy_allowed && !status.pending);
        assert_eq!(
            load_metrics_state(&fixture.paths.metrics_state).unwrap(),
            MetricsPersistentState::default()
        );

        let denied = service.dispatch_connection(
            StoreRequest {
                protocol_version: cp0_store_protocol::STORE_PROTOCOL_VERSION,
                request_id: 44,
                command: StoreCommand::RecordRuntimeMetric {
                    app_id: app_id.into(),
                    version: version.into(),
                    event: StoreRuntimeMetricEvent::Launch,
                },
            },
            1000,
        );
        assert!(matches!(
            denied.response.outcome,
            cp0_store_protocol::StoreOutcome::Error {
                code: StoreErrorCode::Unauthorized,
                ..
            }
        ));
    }

    #[test]
    fn metrics_retry_reuses_batch_and_clears_only_after_exact_acknowledgement() {
        let fixture = Fixture::new("metrics-retry");
        let network = Arc::new(MetricsNetwork::new(true));
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: None,
                metrics_url: Some("https://store.example.com/metrics/v1/aggregate".into()),
            },
            network.clone(),
            Arc::new(MockInstaller::default()),
            [0],
        )
        .unwrap();
        service.set_metrics(true).unwrap();
        let previous_week = week_start(unix_time()) - WEEK_SECONDS;
        {
            let mut state = service.state.lock().unwrap();
            state.metrics.weeks.push(MetricsWeek {
                week_start_unix_seconds: previous_week,
                records: vec![AppMetricRecord {
                    app_id: "dev.cardputerzero.metrics".into(),
                    version: "1.2.3".into(),
                    installs: 1,
                    launches: 2,
                    crashes: 1,
                }],
            });
            save_metrics_state(&fixture.paths.metrics_state, &state.metrics).unwrap();
        }

        service.start_metrics_upload().unwrap();
        wait_for_metrics_upload(&service);
        let pending = service
            .state
            .lock()
            .unwrap()
            .metrics
            .pending
            .as_ref()
            .unwrap()
            .clone();
        let first_batch = pending.batch_id.clone();
        assert!(cp0_store_metrics::is_valid_batch_id(&first_batch));
        service.finish_metrics_upload(
            &pending,
            Ok("batch_ffffffffffffffffffffffffffffffff".into()),
        );
        assert!(service.state.lock().unwrap().metrics.pending.is_some());
        service.start_metrics_upload().unwrap();
        wait_for_metrics_upload(&service);
        let state = service.state.lock().unwrap();
        assert!(state.metrics.pending.is_none() && state.metrics.weeks.is_empty());
        drop(state);
        let uploads = network.uploads.lock().unwrap();
        assert_eq!(uploads.len(), 2);
        assert_eq!(uploads[0].batch_id, first_batch);
        assert_eq!(uploads[1].batch_id, first_batch);
    }

    #[test]
    fn auto_update_selects_only_strict_permission_preserving_upgrades() {
        let fixture = Fixture::new("auto-update-selection");
        let mut alpha = fixture.catalog_app(
            "dev.cardputerzero.alpha",
            "Alpha",
            "Permission preserving update",
        );
        alpha.version = "2.0.0".into();
        alpha.permissions = vec![cp0_manifest::Permission::NetworkClient];
        let mut beta = fixture.catalog_app(
            "dev.cardputerzero.beta",
            "Beta",
            "Update requesting a new permission",
        );
        beta.version = "2.0.0".into();
        beta.permissions = vec![
            cp0_manifest::Permission::CameraCapture,
            cp0_manifest::Permission::NetworkClient,
        ];
        let mut gamma = fixture.catalog_app(
            "dev.cardputerzero.gamma",
            "Gamma",
            "Already current application",
        );
        gamma.permissions = vec![cp0_manifest::Permission::NetworkClient];
        let mut new_app = fixture.catalog_app(
            "dev.cardputerzero.newapp",
            "New App",
            "Not installed on this device",
        );
        new_app.version = "2.0.0".into();
        let catalog = fixture.signed_catalog_with_apps(
            9,
            unix_time() + 3600,
            vec![alpha.clone(), beta, gamma, new_app],
        );
        let installer = Arc::new(AutoInstaller::new(vec![
            StoreInstalledApp {
                app_id: alpha.app_id.clone(),
                version: "1.0.0".into(),
                permissions: alpha.permissions.clone(),
            },
            StoreInstalledApp {
                app_id: "dev.cardputerzero.beta".into(),
                version: "1.0.0".into(),
                permissions: vec![cp0_manifest::Permission::CameraCapture],
            },
            StoreInstalledApp {
                app_id: "dev.cardputerzero.gamma".into(),
                version: "1.0.0".into(),
                permissions: vec![cp0_manifest::Permission::NetworkClient],
            },
        ]));
        let service = StoreService::new_with_probes(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog,
                package: fixture.package.clone(),
            }),
            installer.clone(),
            [0],
            Arc::new(MockSpaceProbe(u64::MAX)),
            Arc::new(MockAutoUpdateProbe::new(true, true)),
        )
        .unwrap();

        service.set_auto_update(true).unwrap();
        wait_for_app_state(&service, &alpha.app_id, StoreAppState::Installed);
        wait_for_auto_update_check(&service);
        assert_eq!(
            installer.automatic_installs.lock().unwrap().as_slice(),
            [alpha.app_id.as_str()]
        );
        let state = service.state.lock().unwrap();
        assert!(state.auto_update.last_check_unix_seconds > 0);
        assert!(!state.auto_update_running && !auto_update_due(&state.auto_update, unix_time()));
        assert!(!state.operations.contains_key("dev.cardputerzero.beta"));
        assert!(!state.operations.contains_key("dev.cardputerzero.gamma"));
        assert!(!state.operations.contains_key("dev.cardputerzero.newapp"));
        drop(state);
        assert!(matches!(
            service.start_auto_update_check(),
            Err(StoreServiceError::InvalidState)
        ));
    }

    #[test]
    fn auto_update_candidate_batch_is_bounded_to_eight() {
        let fixture = Fixture::new("auto-update-bound");
        let mut catalog_apps = Vec::new();
        let mut installed_apps = Vec::new();
        for index in 0..10 {
            let app_id = format!("dev.cardputerzero.app{index:02}");
            let mut app = fixture.catalog_app(
                &app_id,
                &format!("App {index:02}"),
                "Bounded automatic update candidate",
            );
            app.version = "2.0.0".into();
            installed_apps.push(StoreInstalledApp {
                app_id,
                version: "1.0.0".into(),
                permissions: Vec::new(),
            });
            catalog_apps.push(app);
        }
        let catalog = fixture.signed_catalog_with_apps(10, unix_time() + 3600, catalog_apps);
        let service = StoreService::new_with_probes(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
                metrics_url: None,
            },
            Arc::new(MockNetwork {
                catalog,
                package: fixture.package.clone(),
            }),
            Arc::new(MockInstaller::default()),
            [0],
            Arc::new(MockSpaceProbe(u64::MAX)),
            Arc::new(MockAutoUpdateProbe::new(true, true)),
        )
        .unwrap();
        service.refresh_now().unwrap();
        let candidates = service.auto_update_candidates(&installed_apps).unwrap();
        assert_eq!(candidates.len(), MAX_INSTALL_BATCH_APPS);
        assert_eq!(candidates[0].app_id, "dev.cardputerzero.app00");
        assert_eq!(candidates[7].app_id, "dev.cardputerzero.app07");
    }

    #[test]
    fn power_and_wired_interface_probes_fail_closed() {
        let fixture = Fixture::new("auto-update-probes");
        let power = fixture.root.join("power");
        let battery = power.join("battery");
        fs::create_dir_all(&battery).unwrap();
        fs::write(battery.join("type"), "Battery\n").unwrap();
        fs::write(battery.join("status"), "Discharging\n").unwrap();
        assert!(!external_power_online(&power));
        fs::write(battery.join("status"), "Charging\n").unwrap();
        assert!(external_power_online(&power));

        let interfaces = fixture.root.join("net");
        let ethernet = interfaces.join("eth0");
        fs::create_dir_all(ethernet.join("wireless")).unwrap();
        fs::write(ethernet.join("ifindex"), "2\n").unwrap();
        fs::write(ethernet.join("carrier"), "1\n").unwrap();
        fs::write(ethernet.join("type"), "1\n").unwrap();
        assert!(!wired_interface_for_index(&interfaces, 2));
        fs::remove_dir(ethernet.join("wireless")).unwrap();
        assert!(wired_interface_for_index(&interfaces, 2));
        fs::write(ethernet.join("carrier"), "0\n").unwrap();
        assert!(!wired_interface_for_index(&interfaces, 2));
    }

    #[test]
    fn rejects_mismatched_http_range_without_appending_and_then_recovers() {
        let fixture = Fixture::new("http-range-recovery");
        let packages = fixture.paths.cache_root.join("packages");
        fs::create_dir_all(&packages).unwrap();
        let partial = packages.join("http-range.part");
        let package = b"0123456789abcdef";
        fs::write(&partial, &package[..4]).unwrap();
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o600)).unwrap();
        let network = UreqStoreNetwork::for_http_test();

        let serve_once = |content_range: String| {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let body = package[4..].to_vec();
            let handle = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 512];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).unwrap();
                    assert_ne!(read, 0);
                    request.extend_from_slice(&buffer[..read]);
                }
                let response = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: {content_range}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                stream.write_all(&body).unwrap();
                String::from_utf8(request).unwrap()
            });
            (format!("http://{address}/package.capp"), handle)
        };

        let (bad_url, bad_server) = serve_once(format!("bytes 3-15/{}", package.len()));
        assert!(matches!(
            network.download_package(&bad_url, &partial, package.len() as u64, &mut |_| {
                DownloadControl::Continue
            },),
            Err(StoreServiceError::Invalid(_))
        ));
        assert_eq!(fs::read(&partial).unwrap(), &package[..4]);
        assert!(
            bad_server
                .join()
                .unwrap()
                .to_ascii_lowercase()
                .contains("\r\nrange: bytes=4-\r\n")
        );

        let (good_url, good_server) = serve_once(format!("bytes 4-15/{}", package.len()));
        assert_eq!(
            network
                .download_package(&good_url, &partial, package.len() as u64, &mut |_| {
                    DownloadControl::Continue
                },)
                .unwrap(),
            DownloadOutcome::Complete
        );
        assert_eq!(fs::read(&partial).unwrap(), package);
        assert!(
            good_server
                .join()
                .unwrap()
                .to_ascii_lowercase()
                .contains("\r\nrange: bytes=4-\r\n")
        );
    }

    #[test]
    fn validates_resume_content_range_and_configuration() {
        validate_content_range("bytes 100-199/200", 100, 200).unwrap();
        for invalid in [
            "",
            "bytes 99-199/200",
            "bytes 100-198/200",
            "bytes 100-200/200",
            "bytes 100-199/201",
            "bytes 100-199/200/extra",
            "bytes 100--199/200",
            "bytes -100-199/200",
            "bytes 100-199/18446744073709551616",
            "bytes 100-199/200\r\nX-Test: injected",
            "items 100-199/200",
            "bytes 100-199/*",
        ] {
            assert!(validate_content_range(invalid, 100, 200).is_err());
        }
        assert!(validate_content_range("bytes 0-0/0", 0, 0).is_err());

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("stored-config-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let config = root.join("store.conf");
        fs::write(
            &config,
            "catalog_url=https://store.example.com/v1/catalog.json\nmetrics_url=https://store.example.com/metrics/v1/aggregate\n",
        )
        .unwrap();
        let loaded = StoreConfig::load(&config).unwrap();
        assert!(loaded.catalog_url.is_some() && loaded.metrics_url.is_some());
        fs::write(
            &config,
            "catalog_url=http://store.example.com/catalog.json\n",
        )
        .unwrap();
        assert!(StoreConfig::load(&config).is_err());
    }
}
