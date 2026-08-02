use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use cp0_storage_protocol::{
    MAX_STORAGE_VALUE_BYTES, StorageCommand, StorageErrorCode, StorageProtocolError,
    StorageRequest, StorageResponse, decode_value, read_request, validate_key, write_response,
};

pub const DEFAULT_STORAGE_ROOT: &str = "/var/lib/cardputerzero/data";
pub const MAX_STORAGE_KEYS: usize = 256;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug)]
pub enum StorageServiceError {
    Io(io::Error),
    InsecureRoot,
    InsecureAppDirectory,
    InvalidEntry,
    TooManyKeys,
    QuotaExceeded,
    ValueTooLarge,
    StatePoisoned,
}

impl fmt::Display for StorageServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "private storage I/O error: {error}"),
            Self::InsecureRoot => formatter.write_str(
                "private storage root must be an owner-only directory owned by the service",
            ),
            Self::InsecureAppDirectory => formatter.write_str(
                "application storage must be an owner-only directory owned by the service",
            ),
            Self::InvalidEntry => {
                formatter.write_str("application storage contains an invalid entry")
            }
            Self::TooManyKeys => formatter.write_str("private storage key limit was reached"),
            Self::QuotaExceeded => formatter.write_str("private storage quota was exceeded"),
            Self::ValueTooLarge => formatter.write_str("stored value exceeds the operation limit"),
            Self::StatePoisoned => formatter.write_str("private storage state is unavailable"),
        }
    }
}

impl std::error::Error for StorageServiceError {}

impl From<io::Error> for StorageServiceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug)]
pub struct StorageService {
    root: PathBuf,
    owner_uid: u32,
    lock: Mutex<()>,
}

impl StorageService {
    pub fn new(root: impl AsRef<Path>, owner_uid: u32) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            owner_uid,
            lock: Mutex::new(()),
        }
    }

    pub fn dispatch(&self, request: StorageRequest) -> StorageResponse {
        let request_id = request.request_id;
        let _guard = match self.lock.lock() {
            Ok(guard) => guard,
            Err(_) => {
                return storage_error_response(request_id, StorageServiceError::StatePoisoned);
            }
        };
        let result = match request.command {
            StorageCommand::Put { key, value_base64 } => decode_value(&value_base64)
                .map_err(|_| StorageServiceError::ValueTooLarge)
                .and_then(|value| {
                    self.put(&request.app_id, request.quota_bytes, &key, &value)
                        .map(|used_bytes| StorageResponse::stored(request_id, used_bytes))
                }),
            StorageCommand::Get { key } => {
                self.get(&request.app_id, &key).map(|value| match value {
                    Some(value) => StorageResponse::value(request_id, &value),
                    None => StorageResponse::not_found(request_id),
                })
            }
            StorageCommand::Delete { key } => {
                self.delete(&request.app_id, &key)
                    .map(|(existed, used_bytes)| {
                        StorageResponse::deleted(request_id, existed, used_bytes)
                    })
            }
            StorageCommand::Usage => self
                .usage(&request.app_id)
                .map(|used_bytes| StorageResponse::usage(request_id, used_bytes)),
        };
        result.unwrap_or_else(|error| storage_error_response(request_id, error))
    }

    fn put(
        &self,
        app_id: &str,
        quota_bytes: u64,
        key: &str,
        value: &[u8],
    ) -> Result<u64, StorageServiceError> {
        validate_key(key).map_err(|_| StorageServiceError::InvalidEntry)?;
        let directory = self.ensure_app_directory(app_id, true)?;
        let (used_bytes, keys) = inspect_directory(&directory, self.owner_uid)?;
        let destination = directory.join(key);
        let existing_size = match open_value(&destination, self.owner_uid)? {
            Some(file) => file.metadata()?.len(),
            None => 0,
        };
        if existing_size == 0 && keys >= MAX_STORAGE_KEYS {
            return Err(StorageServiceError::TooManyKeys);
        }
        let projected = used_bytes
            .checked_sub(existing_size)
            .and_then(|used| used.checked_add(value.len() as u64))
            .ok_or(StorageServiceError::QuotaExceeded)?;
        if projected > quota_bytes {
            return Err(StorageServiceError::QuotaExceeded);
        }

        let temporary = directory.join(format!(
            ".cp0-tmp-{}-{}",
            std::process::id(),
            monotonic_nonce()
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
                .open(&temporary)?;
            file.write_all(value)?;
            file.sync_all()?;
            fs::rename(&temporary, &destination)?;
            sync_directory(&directory)?;
            Ok(projected)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn get(&self, app_id: &str, key: &str) -> Result<Option<Vec<u8>>, StorageServiceError> {
        validate_key(key).map_err(|_| StorageServiceError::InvalidEntry)?;
        let Some(directory) = self.app_directory(app_id)? else {
            return Ok(None);
        };
        let Some(file) = open_value(&directory.join(key), self.owner_uid)? else {
            return Ok(None);
        };
        let size = file.metadata()?.len();
        if size == 0 || size > MAX_STORAGE_VALUE_BYTES as u64 {
            return Err(StorageServiceError::ValueTooLarge);
        }
        let mut value = Vec::with_capacity(size as usize);
        file.take(MAX_STORAGE_VALUE_BYTES as u64 + 1)
            .read_to_end(&mut value)?;
        if value.len() != size as usize {
            return Err(StorageServiceError::InvalidEntry);
        }
        Ok(Some(value))
    }

    fn delete(&self, app_id: &str, key: &str) -> Result<(bool, u64), StorageServiceError> {
        validate_key(key).map_err(|_| StorageServiceError::InvalidEntry)?;
        let Some(directory) = self.app_directory(app_id)? else {
            return Ok((false, 0));
        };
        let path = directory.join(key);
        if open_value(&path, self.owner_uid)?.is_none() {
            let (used_bytes, _) = inspect_directory(&directory, self.owner_uid)?;
            return Ok((false, used_bytes));
        }
        fs::remove_file(path)?;
        sync_directory(&directory)?;
        let (used_bytes, _) = inspect_directory(&directory, self.owner_uid)?;
        Ok((true, used_bytes))
    }

    fn usage(&self, app_id: &str) -> Result<u64, StorageServiceError> {
        let Some(directory) = self.app_directory(app_id)? else {
            return Ok(0);
        };
        inspect_directory(&directory, self.owner_uid).map(|(used_bytes, _)| used_bytes)
    }

    fn ensure_app_directory(
        &self,
        app_id: &str,
        create: bool,
    ) -> Result<PathBuf, StorageServiceError> {
        self.verify_root()?;
        let directory = self.root.join(app_id);
        match fs::symlink_metadata(&directory) {
            Ok(metadata) => {
                verify_directory_metadata(&metadata, self.owner_uid)?;
                Ok(directory)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
                let mut builder = fs::DirBuilder::new();
                builder.mode(0o700);
                builder.create(&directory)?;
                verify_directory_metadata(&fs::symlink_metadata(&directory)?, self.owner_uid)?;
                sync_directory(&self.root)?;
                Ok(directory)
            }
            Err(error) => Err(StorageServiceError::Io(error)),
        }
    }

    fn app_directory(&self, app_id: &str) -> Result<Option<PathBuf>, StorageServiceError> {
        match self.ensure_app_directory(app_id, false) {
            Ok(directory) => Ok(Some(directory)),
            Err(StorageServiceError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    fn verify_root(&self) -> Result<(), StorageServiceError> {
        let metadata = fs::symlink_metadata(&self.root)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != self.owner_uid
            || metadata.mode() & 0o077 != 0
        {
            return Err(StorageServiceError::InsecureRoot);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct StorageServer {
    service: StorageService,
    trusted_uids: BTreeSet<u32>,
}

impl StorageServer {
    pub fn new(service: StorageService, trusted_uids: impl IntoIterator<Item = u32>) -> Self {
        Self {
            service,
            trusted_uids: trusted_uids.into_iter().collect(),
        }
    }

    pub fn serve(&self, listener: UnixListener) -> io::Result<()> {
        loop {
            let (stream, _) = listener.accept()?;
            if let Err(error) = self.handle_connection(stream) {
                eprintln!("cp0-storaged: rejected connection: {error}");
            }
        }
    }

    fn handle_connection(&self, mut stream: UnixStream) -> io::Result<()> {
        stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
        let uid = peer_uid(&stream)?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let request = match read_request(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(error) => {
                write_response(
                    &mut stream,
                    &StorageResponse::error(
                        0,
                        StorageErrorCode::InvalidRequest,
                        "invalid storage service request",
                    ),
                )
                .map_err(protocol_io)?;
                eprintln!("cp0-storaged: invalid request: {error}");
                return Ok(());
            }
        };
        if !self.trusted_uids.contains(&uid) {
            write_response(
                &mut stream,
                &StorageResponse::error(
                    request.request_id,
                    StorageErrorCode::Unauthorized,
                    "peer UID is not authorized for private storage access",
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }
        write_response(&mut stream, &self.service.dispatch(request)).map_err(protocol_io)
    }
}

fn inspect_directory(
    directory: &Path,
    owner_uid: u32,
) -> Result<(u64, usize), StorageServiceError> {
    let mut used_bytes = 0_u64;
    let mut keys = 0_usize;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StorageServiceError::InvalidEntry)?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.uid() != owner_uid
            || metadata.mode() & 0o077 != 0
            || metadata.len() == 0
            || metadata.len() > MAX_STORAGE_VALUE_BYTES as u64
        {
            return Err(StorageServiceError::InvalidEntry);
        }
        used_bytes = used_bytes
            .checked_add(metadata.len())
            .ok_or(StorageServiceError::QuotaExceeded)?;
        if name.starts_with(".cp0-tmp-") {
            continue;
        }
        validate_key(&name).map_err(|_| StorageServiceError::InvalidEntry)?;
        keys += 1;
        if keys > MAX_STORAGE_KEYS {
            return Err(StorageServiceError::TooManyKeys);
        }
    }
    Ok((used_bytes, keys))
}

fn verify_directory_metadata(
    metadata: &fs::Metadata,
    owner_uid: u32,
) -> Result<(), StorageServiceError> {
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != owner_uid
        || metadata.mode() & 0o077 != 0
    {
        Err(StorageServiceError::InsecureAppDirectory)
    } else {
        Ok(())
    }
}

fn open_value(path: &Path, owner_uid: u32) -> Result<Option<File>, StorageServiceError> {
    match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => {
            let metadata = file.metadata()?;
            if !metadata.is_file()
                || metadata.uid() != owner_uid
                || metadata.mode() & 0o077 != 0
                || metadata.len() == 0
                || metadata.len() > MAX_STORAGE_VALUE_BYTES as u64
            {
                return Err(StorageServiceError::InvalidEntry);
            }
            Ok(Some(file))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(StorageServiceError::Io(error)),
    }
}

fn sync_directory(directory: &Path) -> io::Result<()> {
    File::open(directory)?.sync_all()
}

fn monotonic_nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn storage_error_response(request_id: u64, error: StorageServiceError) -> StorageResponse {
    let (code, message) = match error {
        StorageServiceError::QuotaExceeded | StorageServiceError::TooManyKeys => (
            StorageErrorCode::QuotaExceeded,
            "private storage quota or key limit was reached",
        ),
        StorageServiceError::Io(ref io_error)
            if matches!(
                io_error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::NotFound
            ) =>
        {
            (
                StorageErrorCode::Unavailable,
                "private storage is unavailable",
            )
        }
        StorageServiceError::ValueTooLarge => (
            StorageErrorCode::InvalidRequest,
            "private storage value is invalid",
        ),
        StorageServiceError::Io(_)
        | StorageServiceError::InsecureRoot
        | StorageServiceError::InsecureAppDirectory
        | StorageServiceError::InvalidEntry
        | StorageServiceError::StatePoisoned => (
            StorageErrorCode::Internal,
            "private storage service failed internally",
        ),
    };
    StorageResponse::error(request_id, code, message)
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
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
        "peer credentials are only implemented for the Linux target",
    ))
}

fn protocol_io(error: StorageProtocolError) -> io::Error {
    match error {
        StorageProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    use cp0_storage_protocol::MIB;

    use super::*;

    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

    fn test_service() -> (PathBuf, StorageService) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!(
                "storaged-{}-{}",
                std::process::id(),
                NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
            ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let owner_uid = unsafe { libc::geteuid() };
        (root.clone(), StorageService::new(root, owner_uid))
    }

    #[test]
    fn atomically_replaces_and_deletes_private_values() {
        let (root, service) = test_service();
        let put = StorageRequest::put(1, "dev.cardputerzero.test", MIB, "state", b"one");
        assert!(matches!(
            service.dispatch(put).outcome,
            cp0_storage_protocol::StorageOutcome::Stored { used_bytes: 3 }
        ));
        let replace = StorageRequest::put(2, "dev.cardputerzero.test", MIB, "state", b"longer");
        assert!(matches!(
            service.dispatch(replace).outcome,
            cp0_storage_protocol::StorageOutcome::Stored { used_bytes: 6 }
        ));
        let get = StorageRequest::get(3, "dev.cardputerzero.test", MIB, "state");
        assert!(matches!(
            service.dispatch(get).outcome,
            cp0_storage_protocol::StorageOutcome::Value { .. }
        ));
        let delete = StorageRequest::delete(4, "dev.cardputerzero.test", MIB, "state");
        assert!(matches!(
            service.dispatch(delete).outcome,
            cp0_storage_protocol::StorageOutcome::Deleted {
                existed: true,
                used_bytes: 0
            }
        ));
        assert_eq!(
            service
                .dispatch(StorageRequest::usage(5, "dev.cardputerzero.test", MIB,))
                .outcome,
            cp0_storage_protocol::StorageOutcome::Usage { used_bytes: 0 }
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_symlink_entries_and_quota_overflow() {
        let (root, service) = test_service();
        let app_id = "dev.cardputerzero.test";
        for index in 0..128 {
            let request = StorageRequest::put(
                index,
                app_id,
                MIB,
                &format!("key-{index}"),
                &[0xa5; MAX_STORAGE_VALUE_BYTES],
            );
            assert!(matches!(
                service.dispatch(request).outcome,
                cp0_storage_protocol::StorageOutcome::Stored { .. }
            ));
        }
        let overflow = StorageRequest::put(200, app_id, MIB, "overflow", b"x");
        assert!(matches!(
            service.dispatch(overflow).outcome,
            cp0_storage_protocol::StorageOutcome::Error {
                code: StorageErrorCode::QuotaExceeded,
                ..
            }
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
