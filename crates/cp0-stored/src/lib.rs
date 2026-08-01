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
    read_response as read_appd_response, write_request as write_appd_request,
};
use cp0_networkd::PublicResolver;
use cp0_store_metadata::validate_png_structure;
use cp0_store_protocol::{
    CatalogApp, CatalogImageResource, CatalogObjectResource, MAX_INSTALL_BATCH_APPS, SignedCatalog,
    StoreAppDetails, StoreAppState, StoreAppSummary, StoreCommand, StoreControlAction,
    StoreErrorCode, StoreFailureReason, StoreInstallAccepted, StoreInstallPreflight,
    StoreMediaMetadata, StoreMediaSelector, StoreRequest, StoreResponse, StoreResponseData,
    decode_app_details, decode_signed_catalog, is_lower_hex, is_valid_https_url, read_request,
    response_requires_descriptor, send_response_with_fd, verify_catalog, write_response,
};
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
}

#[derive(Debug, Clone)]
pub struct StorePaths {
    pub cache_root: PathBuf,
    pub catalog_cache: PathBuf,
    pub trust_root: PathBuf,
    pub appd_inbox: PathBuf,
    pub appd_socket: PathBuf,
    pub device_policy: PathBuf,
    pub enforce_root_trust: bool,
}

impl Default for StorePaths {
    fn default() -> Self {
        let cache_root = PathBuf::from(DEFAULT_CACHE_ROOT);
        Self {
            catalog_cache: cache_root.join("catalog.json"),
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

    fn download_package(
        &self,
        url: &str,
        destination: &Path,
        expected_bytes: u64,
        control: &mut dyn FnMut(u8) -> DownloadControl,
    ) -> Result<DownloadOutcome, StoreServiceError> {
        require_https(url)?;
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
}

impl AppInstaller for AppdInstaller {
    fn install(&self, app: &CatalogApp, staged_path: &Path) -> Result<(), StoreServiceError> {
        let package_name = staged_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| StoreServiceError::Invalid("staged package name is invalid".into()))?;
        let mut stream = UnixStream::connect(&self.socket)?;
        stream.set_read_timeout(Some(APPD_TIMEOUT))?;
        stream.set_write_timeout(Some(APPD_TIMEOUT))?;
        let request = AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 1,
            command: AppdCommand::StoreInstall {
                package_name: package_name.into(),
                app_id: app.app_id.clone(),
                version: app.version.clone(),
                package_sha256: app.package_sha256.clone(),
                package_bytes: app.package_bytes,
            },
        };
        write_appd_request(&mut stream, &request)
            .map_err(|error| StoreServiceError::Invalid(error.to_string()))?;
        let response = read_appd_response(&mut BufReader::new(stream))
            .map_err(|error| StoreServiceError::Invalid(error.to_string()))?
            .ok_or(StoreServiceError::Unavailable(
                "appd closed the installation request",
            ))?;
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
}

#[derive(Debug, Clone)]
struct OperationState {
    version: String,
    package_sha256: String,
    state: StoreAppState,
    progress_percent: u8,
    failure_reason: Option<StoreFailureReason>,
    control: DownloadControl,
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

#[derive(Debug, Default)]
struct MutableState {
    catalog: Option<SignedCatalog>,
    operations: BTreeMap<String, OperationState>,
    install_authorization: Option<InstallAuthorization>,
    active_job: bool,
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
        for line in encoded.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some(value) = line.strip_prefix("catalog_url=") else {
                return Err(StoreServiceError::Invalid(
                    "store configuration contains an unknown field".into(),
                ));
            };
            if catalog_url.is_some() {
                return Err(StoreServiceError::Invalid(
                    "store catalog URL is duplicated".into(),
                ));
            }
            if value.is_empty() {
                catalog_url = Some(None);
            } else if is_valid_https_url(value) {
                catalog_url = Some(Some(value.into()));
            } else {
                return Err(StoreServiceError::Invalid(
                    "store catalog URL must be bounded HTTPS".into(),
                ));
            }
        }
        Ok(Self {
            catalog_url: catalog_url.unwrap_or(None),
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
        fs::create_dir_all(paths.cache_root.join("packages"))?;
        prepare_media_directories(&paths)?;
        let catalog = match fs::read(&paths.catalog_cache) {
            Ok(encoded) => Some(load_trusted_catalog(&encoded, &paths)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let service = Arc::new(Self {
            paths,
            config,
            network,
            installer,
            space,
            trusted_uids: trusted_uids.into_iter().collect(),
            state: Mutex::new(MutableState {
                catalog,
                ..MutableState::default()
            }),
        });
        if let Err(error) = service.reconcile_cached_media() {
            eprintln!("cp0-stored: discarded invalid cached media: {error}");
        }
        Ok(service)
    }

    pub fn serve(self: Arc<Self>, listener: UnixListener) -> io::Result<()> {
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
            self.dispatch_connection(request)
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
        self.dispatch_connection(request).response
    }

    fn dispatch_connection(self: &Arc<Self>, request: StoreRequest) -> DispatchedResponse {
        let request_id = request.request_id;
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
            StoreCommand::Search {
                query,
                offset,
                limit,
            } => self.search_response(query, offset, limit),
            StoreCommand::Refresh => self
                .start_refresh()
                .map(|()| StoreResponseData::RefreshAccepted),
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
            .and_then(|catalog| catalog.catalog.apps.iter().find(|app| app.app_id == app_id))
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
            .catalog
            .apps
            .iter()
            .map(|app| store_app_summary(app, state.operations.get(&app.app_id)))
            .collect();
        Ok(StoreResponseData::Catalog {
            sequence: catalog.catalog.sequence,
            expires_unix_seconds: catalog.catalog.expires_unix_seconds,
            stale: now >= catalog.catalog.expires_unix_seconds,
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
            .catalog
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
            sequence: catalog.catalog.sequence,
            expires_unix_seconds: catalog.catalog.expires_unix_seconds,
            stale: unix_time() >= catalog.catalog.expires_unix_seconds,
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
            .map(|catalog| catalog.catalog.sequence)
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
            if catalog.catalog.sequence != catalog_sequence {
                return Err(StoreServiceError::CatalogChanged);
            }
            if unix_time() >= catalog.catalog.expires_unix_seconds {
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
        if catalog.catalog.sequence != catalog_sequence
            || !catalog_contains_exact_apps(catalog, &apps)
        {
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
            if now >= catalog.catalog.expires_unix_seconds {
                return Err(StoreServiceError::Untrusted(
                    "catalog has expired; refresh before installing".into(),
                ));
            }
            if catalog.catalog.sequence != authorization.catalog_sequence
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
        self.spawn_install_worker(apps)?;
        Ok(accepted)
    }

    fn resume_install(self: &Arc<Self>, app_id: &str) -> Result<String, StoreServiceError> {
        let app = {
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
            if unix_time() >= catalog.catalog.expires_unix_seconds {
                return Err(StoreServiceError::Untrusted(
                    "catalog has expired; refresh before installing".into(),
                ));
            }
            let app = catalog
                .catalog
                .apps
                .iter()
                .find(|app| app.app_id == app_id)
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
            state.active_job = true;
            app
        };
        if let Err(error) = self.validate_install_preconditions(std::slice::from_ref(&app)) {
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
                },
            );
        }
        let version = app.version.clone();
        self.spawn_install_worker(vec![app])?;
        Ok(version)
    }

    fn spawn_install_worker(
        self: &Arc<Self>,
        apps: Vec<CatalogApp>,
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
                    let result = service.install_now(&app);
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
        let signed = load_trusted_catalog(&encoded, &self.paths)?;
        let now = unix_time();
        if signed.catalog.published_unix_seconds > now.saturating_add(CLOCK_SKEW_SECONDS)
            || signed.catalog.expires_unix_seconds <= now
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
                if signed.catalog.sequence < current.catalog.sequence {
                    return Err(StoreServiceError::Untrusted(
                        "catalog sequence rollback was rejected".into(),
                    ));
                }
                if signed.catalog.sequence == current.catalog.sequence && signed != *current {
                    return Err(StoreServiceError::Untrusted(
                        "catalog sequence was reused for different content".into(),
                    ));
                }
            }
        }
        atomic_write(&self.paths.catalog_cache, &encoded)?;
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
            state.operations.retain(|app_id, operation| {
                let Some(app) = signed.catalog.apps.iter().find(|app| app.app_id == *app_id) else {
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
            state.catalog = Some(signed.clone());
        }
        if let Err(error) = self.prefetch_catalog_icons(&signed) {
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

    fn prefetch_catalog_icons(&self, catalog: &SignedCatalog) -> Result<(), StoreServiceError> {
        reconcile_media_for_catalog(&self.paths, catalog)?;
        let icons = catalog
            .catalog
            .apps
            .iter()
            .filter_map(|app| app.resources.as_ref().map(|resources| &resources.icon))
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
            .and_then(|catalog| catalog.catalog.apps.iter().find(|app| app.app_id == app_id))
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
    }

    fn install_now(&self, app: &CatalogApp) -> Result<InstallOutcome, InstallFailure> {
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
        let install_result = self.installer.install(app, &staged);
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
            .catalog
            .apps
            .iter()
            .find(|app| app.app_id == *app_id)
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

fn catalog_contains_exact_apps(catalog: &SignedCatalog, apps: &[CatalogApp]) -> bool {
    apps.iter().all(|expected| {
        catalog
            .catalog
            .apps
            .iter()
            .find(|app| app.app_id == expected.app_id)
            == Some(expected)
    })
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
    Ok((status.f_bavail as u64).saturating_mul(status.f_frsize as u64))
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
    catalog: &SignedCatalog,
) -> Result<(), StoreServiceError> {
    let mut icon_files = BTreeSet::new();
    let mut detail_files = BTreeSet::new();
    let mut icons = Vec::new();
    let mut details = Vec::new();
    for app in &catalog.catalog.apps {
        let Some(resources) = &app.resources else {
            continue;
        };
        icon_files.insert(format!("{}.png", resources.icon.sha256));
        detail_files.insert(format!("{}.json", resources.details.sha256));
        icons.push((&resources.icon, app));
        details.push((&resources.details, app));
    }
    validate_resource_budget(
        icons
            .iter()
            .map(|(resource, _)| (resource.sha256.as_str(), resource.bytes)),
        MediaKind::Icon.budget(),
    )?;
    validate_resource_budget(
        details
            .iter()
            .map(|(resource, _)| (resource.sha256.as_str(), resource.bytes)),
        MediaKind::Details.budget(),
    )?;
    prune_cache_directory(&media_directory(paths, MediaKind::Icon), &icon_files)?;
    prune_cache_directory(&media_directory(paths, MediaKind::Details), &detail_files)?;
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

fn load_trusted_catalog(
    encoded: &[u8],
    paths: &StorePaths,
) -> Result<SignedCatalog, StoreServiceError> {
    let signed = decode_signed_catalog(encoded)
        .map_err(|error| StoreServiceError::Untrusted(error.to_string()))?;
    let key_path = paths.trust_root.join(format!("{}.pub", signed.key_id));
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
    let public: [u8; 32] = public.try_into().map_err(|_| {
        StoreServiceError::Untrusted("trusted catalog key length is invalid".into())
    })?;
    verify_catalog(&signed, &public)
        .map_err(|error| StoreServiceError::Untrusted(error.to_string()))?;
    Ok(signed)
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
    Ok(destination)
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
        APP_DETAILS_SCHEMA_VERSION, CATALOG_SCHEMA_VERSION, Catalog, CatalogDiscovery,
        CatalogImageResource, CatalogObjectResource, CatalogResources,
        MEDIA_CATALOG_SCHEMA_VERSION, RICH_CATALOG_SCHEMA_VERSION, StoreAppDetails,
        encode_app_details, encode_signed_catalog, sign_catalog,
    };
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
                },
            );
        }
        let result = service.install_now(app);
        assert!(matches!(&result, Ok(InstallOutcome::Installed)));
        service.finish_install_operation(app, result);
        service.release_job();
    }

    fn wait_for_store_state(service: &StoreService, expected: StoreAppState) -> StoreAppSummary {
        for _ in 0..2_000 {
            if let StoreResponseData::Catalog { apps, .. } = service.catalog_response().unwrap() {
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
            if let StoreResponseData::Catalog { apps, .. } = service.catalog_response().unwrap() {
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
            let secret = [9; 32];
            let public = cp0_package::public_key(&secret);
            let key_id = cp0_store_protocol::lower_hex(&cp0_package::key_id(&public));
            fs::write(trust_root.join(format!("{key_id}.pub")), public).unwrap();
            fs::write(
                &device_policy,
                br#"{"schema_version":1,"authority":"personal","developer_mode_allowed":true,"recovery_mode_allowed":true,"store_install_allowed":true,"app_launch_policy":"allow-all","allowed_apps":[],"denied_permissions":[]}"#,
            )
            .unwrap();
            Self {
                paths: StorePaths {
                    catalog_cache: cache_root.join("catalog.json"),
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
            };
            encode_signed_catalog(&sign_catalog(catalog, &self.secret).unwrap()).unwrap()
        }
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
    fn atomically_caches_catalog_bound_icons_details_and_screenshots() {
        let fixture = Fixture::new("media-cache");
        let (catalog, resources, app) = media_catalog(&fixture, 1);
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig {
                catalog_url: Some("https://store.example.com/catalog.json".into()),
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

        let rich = service.dispatch_connection(StoreRequest {
            protocol_version: cp0_store_protocol::STORE_PROTOCOL_VERSION,
            request_id: 71,
            command: StoreCommand::Details {
                app_id: app.app_id.clone(),
            },
        });
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
            let media = service.dispatch_connection(StoreRequest {
                protocol_version: cp0_store_protocol::STORE_PROTOCOL_VERSION,
                request_id,
                command: StoreCommand::Media {
                    app_id: app.app_id.clone(),
                    media: selector,
                },
            });
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
            StoreConfig { catalog_url: None },
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
    fn resumes_verifies_stages_and_installs_catalog_package() {
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
            StoreConfig { catalog_url: None },
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
            StoreConfig { catalog_url: None },
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
            StoreConfig { catalog_url: None },
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
            StoreConfig { catalog_url: None },
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
            StoreConfig { catalog_url: None },
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
            StoreConfig { catalog_url: None },
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
            StoreConfig { catalog_url: None },
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
            StoreConfig { catalog_url: None },
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
            StoreConfig { catalog_url: None },
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
        };
        let encoded =
            encode_signed_catalog(&sign_catalog(catalog, &fixture.secret).unwrap()).unwrap();
        fs::write(&fixture.paths.catalog_cache, encoded).unwrap();
        let service = StoreService::new(
            fixture.paths.clone(),
            StoreConfig { catalog_url: None },
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
            StoreConfig { catalog_url: None },
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
            StoreConfig { catalog_url: None },
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
            StoreConfig { catalog_url: None },
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
            "catalog_url=https://store.example.com/v1/catalog.json\n",
        )
        .unwrap();
        assert!(StoreConfig::load(&config).unwrap().catalog_url.is_some());
        fs::write(
            &config,
            "catalog_url=http://store.example.com/catalog.json\n",
        )
        .unwrap();
        assert!(StoreConfig::load(&config).is_err());
    }
}
