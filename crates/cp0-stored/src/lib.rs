use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cp0_appd::{
    APPD_PROTOCOL_VERSION, AppdCommand, AppdRequest, ResponseData, ResponseOutcome,
    read_response as read_appd_response, write_request as write_appd_request,
};
use cp0_networkd::PublicResolver;
use cp0_store_metadata::validate_png_structure;
use cp0_store_protocol::{
    CatalogApp, CatalogImageResource, CatalogObjectResource, SignedCatalog, StoreAppDetails,
    StoreAppState, StoreAppSummary, StoreCommand, StoreErrorCode, StoreRequest, StoreResponse,
    StoreResponseData, decode_app_details, decode_signed_catalog, is_lower_hex, is_valid_https_url,
    read_request, verify_catalog, write_response,
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

const CLIENT_TIMEOUT: Duration = Duration::from_secs(3);
const APPD_TIMEOUT: Duration = Duration::from_secs(60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_REDIRECTS: u32 = 2;
const CLOCK_SKEW_SECONDS: u64 = 5 * 60;
pub const ICON_CACHE_BUDGET_BYTES: u64 = 4 * 1024 * 1024;
pub const DETAILS_CACHE_BUDGET_BYTES: u64 = 1024 * 1024;
pub const SCREENSHOT_CACHE_BUDGET_BYTES: u64 = 8 * 1024 * 1024;

static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
        }
    }
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
        progress: &mut dyn FnMut(u8),
    ) -> Result<(), StoreServiceError>;
}

pub trait AppInstaller: fmt::Debug + Send + Sync + 'static {
    fn install(&self, app: &CatalogApp, staged_path: &Path) -> Result<(), StoreServiceError>;
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
        progress: &mut dyn FnMut(u8),
    ) -> Result<(), StoreServiceError> {
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
            let read = reader.read(&mut buffer).map_err(StoreServiceError::Io)?;
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
            progress(((downloaded * 100) / expected_bytes) as u8);
        }
        file.sync_all()?;
        if downloaded != expected_bytes {
            return Err(StoreServiceError::Unavailable(
                "package download ended before the signed catalog size",
            ));
        }
        Ok(())
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
    state: StoreAppState,
    progress_percent: u8,
}

#[derive(Debug, Default)]
struct MutableState {
    catalog: Option<SignedCatalog>,
    operations: BTreeMap<String, OperationState>,
    active_job: bool,
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
    trusted_uids: BTreeSet<u32>,
    state: Mutex<MutableState>,
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
        let response = if !self.trusted_uids.contains(&uid) {
            StoreResponse::error(
                request.request_id,
                StoreErrorCode::Unauthorized,
                "peer UID is not authorized to use the store",
            )
        } else if let Err(error) = request.validate() {
            StoreResponse::error(
                request.request_id,
                StoreErrorCode::InvalidRequest,
                error.to_string(),
            )
        } else {
            self.dispatch(request)
        };
        write_response(&mut stream, &response).map_err(protocol_io)
    }

    pub fn dispatch(self: &Arc<Self>, request: StoreRequest) -> StoreResponse {
        let request_id = request.request_id;
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
            StoreCommand::Install { app_id } => self
                .start_install(&app_id)
                .map(|version| StoreResponseData::InstallAccepted { app_id, version }),
        };
        match result {
            Ok(data) => StoreResponse::success(request_id, data),
            Err(error) => service_error_response(request_id, &error),
        }
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

    fn start_install(self: &Arc<Self>, app_id: &str) -> Result<String, StoreServiceError> {
        let app = {
            let state = self
                .state
                .lock()
                .map_err(|_| StoreServiceError::Unavailable("store state lock is unavailable"))?;
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
            catalog
                .catalog
                .apps
                .iter()
                .find(|app| app.app_id == app_id)
                .cloned()
                .ok_or(StoreServiceError::NotFound)?
        };
        self.reserve_job()?;
        self.set_operation(&app.app_id, StoreAppState::Queued, 0);
        let version = app.version.clone();
        let service = Arc::clone(self);
        thread::Builder::new()
            .name("cp0-store-install".into())
            .spawn(move || {
                if let Err(error) = service.install_now(&app) {
                    service.set_operation(&app.app_id, StoreAppState::Failed, 0);
                    eprintln!("cp0-stored: {} installation failed: {error}", app.app_id);
                }
                service.release_job();
            })
            .map_err(|error| {
                self.release_job();
                StoreServiceError::Io(error)
            })?;
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

    fn set_operation(&self, app_id: &str, state_value: StoreAppState, progress_percent: u8) {
        if let Ok(mut state) = self.state.lock() {
            state.operations.insert(
                app_id.into(),
                OperationState {
                    state: state_value,
                    progress_percent: progress_percent.min(100),
                },
            );
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
            let catalog_app_ids = signed
                .catalog
                .apps
                .iter()
                .map(|app| app.app_id.clone())
                .collect::<BTreeSet<_>>();
            state.catalog = Some(signed.clone());
            state
                .operations
                .retain(|app_id, _| catalog_app_ids.contains(app_id));
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

    fn install_now(&self, app: &CatalogApp) -> Result<(), StoreServiceError> {
        let packages = self.paths.cache_root.join("packages");
        let partial = packages.join(format!("{}.part", app.package_sha256));
        self.set_operation(&app.app_id, StoreAppState::Downloading, 0);
        self.network.download_package(
            &app.package_url,
            &partial,
            app.package_bytes,
            &mut |progress| {
                self.set_operation(&app.app_id, StoreAppState::Downloading, progress);
            },
        )?;
        verify_package_file(&partial, app)?;
        let staged = stage_for_appd(&partial, &self.paths.appd_inbox)?;
        self.set_operation(&app.app_id, StoreAppState::Installing, 100);
        let install_result = self.installer.install(app, &staged);
        let cleanup_result = fs::remove_file(&staged);
        install_result?;
        cleanup_result?;
        self.set_operation(&app.app_id, StoreAppState::Installed, 100);
        Ok(())
    }
}

fn store_app_summary(app: &CatalogApp, operation: Option<&OperationState>) -> StoreAppSummary {
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
    }
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
    use std::os::unix::fs::{PermissionsExt, symlink};

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
            progress: &mut dyn FnMut(u8),
        ) -> Result<(), StoreServiceError> {
            let mut file = open_resume_file(destination)?;
            let offset = file.metadata()?.len() as usize;
            file.seek(SeekFrom::End(0))?;
            file.write_all(&self.package[offset..])?;
            file.sync_all()?;
            assert_eq!(self.package.len() as u64, expected_bytes);
            progress(100);
            Ok(())
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
            progress: &mut dyn FnMut(u8),
        ) -> Result<(), StoreServiceError> {
            let mut file = open_resume_file(destination)?;
            let offset = file.metadata()?.len() as usize;
            file.seek(SeekFrom::End(0))?;
            file.write_all(&self.package[offset..])?;
            file.sync_all()?;
            assert_eq!(self.package.len() as u64, expected_bytes);
            progress(100);
            Ok(())
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
            for directory in [&cache_root, &trust_root, &inbox] {
                fs::create_dir_all(directory).unwrap();
            }
            let secret = [9; 32];
            let public = cp0_package::public_key(&secret);
            let key_id = cp0_store_protocol::lower_hex(&cp0_package::key_id(&public));
            fs::write(trust_root.join(format!("{key_id}.pub")), public).unwrap();
            Self {
                paths: StorePaths {
                    catalog_cache: cache_root.join("catalog.json"),
                    cache_root,
                    trust_root,
                    appd_inbox: inbox,
                    appd_socket: root.join("appd.sock"),
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

        service.install_now(&app).unwrap();
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

        service.install_now(&app).unwrap();
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
