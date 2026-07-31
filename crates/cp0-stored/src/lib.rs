use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
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
use cp0_store_protocol::{
    CatalogApp, SignedCatalog, StoreAppState, StoreAppSummary, StoreCommand, StoreErrorCode,
    StoreRequest, StoreResponse, StoreResponseData, decode_signed_catalog, is_valid_https_url,
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
        let catalog = match fs::read(&paths.catalog_cache) {
            Ok(encoded) => Some(load_trusted_catalog(&encoded, &paths)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Arc::new(Self {
            paths,
            config,
            network,
            installer,
            trusted_uids: trusted_uids.into_iter().collect(),
            state: Mutex::new(MutableState {
                catalog,
                ..MutableState::default()
            }),
        }))
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
            .map(|app| {
                let operation = state.operations.get(&app.app_id);
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
            })
            .collect();
        Ok(StoreResponseData::Catalog {
            sequence: catalog.catalog.sequence,
            expires_unix_seconds: catalog.catalog.expires_unix_seconds,
            stale: now >= catalog.catalog.expires_unix_seconds,
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
        state.catalog = Some(signed);
        state
            .operations
            .retain(|app_id, _| catalog_app_ids.contains(app_id));
        Ok(())
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
    use cp0_store_protocol::{
        CATALOG_SCHEMA_VERSION, Catalog, encode_signed_catalog, sign_catalog,
    };
    use std::os::unix::fs::PermissionsExt;

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
            let digest = cp0_store_protocol::lower_hex(&Sha256::digest(&self.package));
            let catalog = Catalog {
                schema_version: CATALOG_SCHEMA_VERSION,
                sequence,
                published_unix_seconds: now.saturating_sub(10),
                expires_unix_seconds: now + 3600,
                apps: vec![CatalogApp {
                    app_id: "dev.cardputerzero.storetest".into(),
                    name: "Store Test".into(),
                    version: "1.0.0".into(),
                    sdk_version: "1.0".into(),
                    summary: "Tests trusted resumable installation".into(),
                    package_url: "https://store.example.com/storetest.capp".into(),
                    package_sha256: digest,
                    package_bytes: self.package.len() as u64,
                    permissions: Vec::new(),
                }],
            };
            encode_signed_catalog(&sign_catalog(catalog, &self.secret).unwrap()).unwrap()
        }
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
