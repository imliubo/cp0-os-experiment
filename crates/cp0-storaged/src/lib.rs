use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, Write};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use cp0_storage_protocol::{
    MAX_STORAGE_BLOB_BYTES, MAX_STORAGE_VALUE_BYTES, SYSTEM_PHOTO_LIBRARY_ID, StorageCommand,
    StorageErrorCode, StorageProtocolError, StorageRequest, StorageResponse, decode_value,
    encode_response_frame, read_request, validate_key, write_response,
};

pub const DEFAULT_STORAGE_ROOT: &str = "/var/lib/cardputerzero/data";
pub const MAX_STORAGE_KEYS: usize = 256;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(4);
#[cfg(target_os = "linux")]
const SYSTEM_FREE_SPACE_RESERVE_BYTES: u64 = 64 * 1024 * 1024;

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

#[derive(Debug)]
struct StorageDispatch {
    response: StorageResponse,
    descriptor: Option<OwnedFd>,
}

impl StorageDispatch {
    const fn response(response: StorageResponse) -> Self {
        Self {
            response,
            descriptor: None,
        }
    }
}

impl StorageService {
    pub fn new(root: impl AsRef<Path>, owner_uid: u32) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            owner_uid,
            lock: Mutex::new(()),
        }
    }

    pub fn initialize(&self) -> Result<(), StorageServiceError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| StorageServiceError::StatePoisoned)?;
        self.verify_root()?;
        let Some(directory) = self.app_directory(SYSTEM_PHOTO_LIBRARY_ID)? else {
            return Ok(());
        };
        cleanup_staging_blobs(&directory, self.owner_uid)
    }

    pub fn dispatch(&self, request: StorageRequest) -> StorageResponse {
        self.dispatch_with_descriptor(request).response
    }

    fn dispatch_with_descriptor(&self, request: StorageRequest) -> StorageDispatch {
        let request_id = request.request_id;
        let _guard = match self.lock.lock() {
            Ok(guard) => guard,
            Err(_) => {
                return StorageDispatch::response(storage_error_response(
                    request_id,
                    StorageServiceError::StatePoisoned,
                ));
            }
        };
        let result = match request.command {
            StorageCommand::Put { key, value_base64 } => decode_value(&value_base64)
                .map_err(|_| StorageServiceError::ValueTooLarge)
                .and_then(|value| {
                    self.put(&request.app_id, request.quota_bytes, &key, &value)
                        .map(|used_bytes| StorageResponse::stored(request_id, used_bytes))
                })
                .map(StorageDispatch::response),
            StorageCommand::Get { key } => self
                .get(&request.app_id, &key)
                .map(|value| match value {
                    Some(value) => StorageResponse::value(request_id, &value),
                    None => StorageResponse::not_found(request_id),
                })
                .map(StorageDispatch::response),
            StorageCommand::Delete { key } => self
                .delete(&request.app_id, &key)
                .map(|(existed, used_bytes)| {
                    StorageResponse::deleted(request_id, existed, used_bytes)
                })
                .map(StorageDispatch::response),
            StorageCommand::PutBlobChunk {
                key,
                offset,
                total_bytes,
                value_base64,
            } => decode_value(&value_base64)
                .map_err(|_| StorageServiceError::ValueTooLarge)
                .and_then(|value| {
                    self.put_blob_chunk(
                        &request.app_id,
                        request.quota_bytes,
                        &key,
                        offset,
                        total_bytes,
                        &value,
                    )
                    .map(|used_bytes| StorageResponse::stored(request_id, used_bytes))
                })
                .map(StorageDispatch::response),
            StorageCommand::GetBlobChunk {
                key,
                offset,
                length,
            } => self
                .get_blob_chunk(&request.app_id, &key, offset, length)
                .map(|value| match value {
                    Some(value) => StorageResponse::value(request_id, &value),
                    None => StorageResponse::not_found(request_id),
                })
                .map(StorageDispatch::response),
            StorageCommand::OpenBlob {
                key,
                expected_bytes,
            } => self
                .open_blob_descriptor(&request.app_id, &key, expected_bytes)
                .map(|descriptor| match descriptor {
                    Some(descriptor) => StorageDispatch {
                        response: StorageResponse::blob_opened(request_id, expected_bytes),
                        descriptor: Some(descriptor),
                    },
                    None => StorageDispatch::response(StorageResponse::not_found(request_id)),
                }),
            StorageCommand::DeleteBlob { key } => self
                .delete_blob(&request.app_id, &key)
                .map(|(existed, used_bytes)| {
                    StorageResponse::deleted(request_id, existed, used_bytes)
                })
                .map(StorageDispatch::response),
            StorageCommand::Usage => self
                .usage(&request.app_id)
                .map(|used_bytes| StorageResponse::usage(request_id, used_bytes))
                .map(StorageDispatch::response),
        };
        result.unwrap_or_else(|error| {
            StorageDispatch::response(storage_error_response(request_id, error))
        })
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
        let system_library = app_id == SYSTEM_PHOTO_LIBRARY_ID;
        let (used_bytes, keys) = inspect_directory(&directory, self.owner_uid, system_library)?;
        let destination = directory.join(key);
        let existing_size = match open_value(&destination, self.owner_uid)? {
            Some(file) => file.metadata()?.len(),
            None => 0,
        };
        if !system_library && existing_size == 0 && keys >= MAX_STORAGE_KEYS {
            return Err(StorageServiceError::TooManyKeys);
        }
        let projected = used_bytes
            .checked_sub(existing_size)
            .and_then(|used| used.checked_add(value.len() as u64))
            .ok_or(StorageServiceError::QuotaExceeded)?;
        if projected > quota_bytes {
            return Err(StorageServiceError::QuotaExceeded);
        }
        require_free_space(&directory, value.len() as u64)?;

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
            let (used_bytes, _) = inspect_directory(
                &directory,
                self.owner_uid,
                app_id == SYSTEM_PHOTO_LIBRARY_ID,
            )?;
            return Ok((false, used_bytes));
        }
        fs::remove_file(path)?;
        sync_directory(&directory)?;
        let (used_bytes, _) = inspect_directory(
            &directory,
            self.owner_uid,
            app_id == SYSTEM_PHOTO_LIBRARY_ID,
        )?;
        Ok((true, used_bytes))
    }

    fn usage(&self, app_id: &str) -> Result<u64, StorageServiceError> {
        let Some(directory) = self.app_directory(app_id)? else {
            return Ok(0);
        };
        inspect_directory(
            &directory,
            self.owner_uid,
            app_id == SYSTEM_PHOTO_LIBRARY_ID,
        )
        .map(|(used_bytes, _)| used_bytes)
    }

    fn put_blob_chunk(
        &self,
        app_id: &str,
        quota_bytes: u64,
        key: &str,
        offset: u32,
        total_bytes: u32,
        value: &[u8],
    ) -> Result<u64, StorageServiceError> {
        if app_id != SYSTEM_PHOTO_LIBRARY_ID
            || value.is_empty()
            || total_bytes == 0
            || total_bytes as usize > MAX_STORAGE_BLOB_BYTES
            || u64::from(offset) + value.len() as u64 > u64::from(total_bytes)
        {
            return Err(StorageServiceError::InvalidEntry);
        }
        validate_key(key).map_err(|_| StorageServiceError::InvalidEntry)?;
        let directory = self.ensure_app_directory(app_id, true)?;
        let destination = directory.join(key);
        let temporary = directory.join(format!(".cp0-blob-{key}"));

        let projected = if offset == 0 {
            remove_stale_blob(&temporary, self.owner_uid)?;
            let (used_bytes, _) = inspect_directory(&directory, self.owner_uid, true)?;
            let existing_size = match open_blob(&destination, self.owner_uid)? {
                Some(file) => file.metadata()?.len(),
                None => 0,
            };
            let projected = used_bytes
                .checked_sub(existing_size)
                .and_then(|used| used.checked_add(u64::from(total_bytes)))
                .ok_or(StorageServiceError::QuotaExceeded)?;
            if projected > quota_bytes {
                return Err(StorageServiceError::QuotaExceeded);
            }
            require_free_space(&directory, u64::from(total_bytes))?;
            projected
        } else {
            quota_bytes.min(u64::from(total_bytes))
        };

        let mut options = OpenOptions::new();
        options
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        if offset == 0 {
            options.create_new(true);
        }
        let mut file = options.open(&temporary)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != self.owner_uid
            || metadata.mode() & 0o077 != 0
            || metadata.len() != u64::from(offset)
        {
            return Err(StorageServiceError::InvalidEntry);
        }
        file.seek(std::io::SeekFrom::Start(u64::from(offset)))?;
        file.write_all(value)?;
        let final_length = u64::from(offset) + value.len() as u64;
        if final_length == u64::from(total_bytes) {
            // The staging name is never visible to readers. Sync once after
            // the complete blob is present, then publish it atomically.
            file.sync_all()?;
            fs::rename(&temporary, &destination)?;
            sync_directory(&directory)?;
            self.usage(app_id)
        } else {
            Ok(projected)
        }
    }

    fn get_blob_chunk(
        &self,
        app_id: &str,
        key: &str,
        offset: u32,
        length: u32,
    ) -> Result<Option<Vec<u8>>, StorageServiceError> {
        if app_id != SYSTEM_PHOTO_LIBRARY_ID
            || length == 0
            || length as usize > MAX_STORAGE_VALUE_BYTES
        {
            return Err(StorageServiceError::InvalidEntry);
        }
        validate_key(key).map_err(|_| StorageServiceError::InvalidEntry)?;
        let Some(directory) = self.app_directory(app_id)? else {
            return Ok(None);
        };
        let Some(mut file) = open_blob(&directory.join(key), self.owner_uid)? else {
            return Ok(None);
        };
        if u64::from(offset) + u64::from(length) > file.metadata()?.len() {
            return Err(StorageServiceError::InvalidEntry);
        }
        file.seek(std::io::SeekFrom::Start(u64::from(offset)))?;
        let mut value = vec![0_u8; length as usize];
        file.read_exact(&mut value)?;
        Ok(Some(value))
    }

    fn open_blob_descriptor(
        &self,
        app_id: &str,
        key: &str,
        expected_bytes: u32,
    ) -> Result<Option<OwnedFd>, StorageServiceError> {
        if app_id != SYSTEM_PHOTO_LIBRARY_ID
            || expected_bytes == 0
            || expected_bytes as usize > MAX_STORAGE_BLOB_BYTES
        {
            return Err(StorageServiceError::InvalidEntry);
        }
        validate_key(key).map_err(|_| StorageServiceError::InvalidEntry)?;
        let Some(directory) = self.app_directory(app_id)? else {
            return Ok(None);
        };
        let Some(file) = open_blob(&directory.join(key), self.owner_uid)? else {
            return Ok(None);
        };
        if file.metadata()?.len() != u64::from(expected_bytes) {
            return Err(StorageServiceError::InvalidEntry);
        }
        Ok(Some(file.into()))
    }

    fn delete_blob(&self, app_id: &str, key: &str) -> Result<(bool, u64), StorageServiceError> {
        if app_id != SYSTEM_PHOTO_LIBRARY_ID {
            return Err(StorageServiceError::InvalidEntry);
        }
        validate_key(key).map_err(|_| StorageServiceError::InvalidEntry)?;
        let Some(directory) = self.app_directory(app_id)? else {
            return Ok((false, 0));
        };
        let destination = directory.join(key);
        let temporary = directory.join(format!(".cp0-blob-{key}"));
        let mut existed = false;
        if open_blob(&destination, self.owner_uid)?.is_some() {
            fs::remove_file(&destination)?;
            existed = true;
        }
        if fs::symlink_metadata(&temporary).is_ok() {
            remove_stale_blob(&temporary, self.owner_uid)?;
        }
        if existed {
            sync_directory(&directory)?;
        }
        self.usage(app_id).map(|used_bytes| (existed, used_bytes))
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
        let dispatch = self.service.dispatch_with_descriptor(request);
        if let Some(descriptor) = dispatch.descriptor.as_ref() {
            let frame = encode_response_frame(&dispatch.response).map_err(protocol_io)?;
            cp0_document_protocol::send_frame_with_fd(&mut stream, &frame, descriptor.as_fd())
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
        } else {
            write_response(&mut stream, &dispatch.response).map_err(protocol_io)
        }
    }
}

fn inspect_directory(
    directory: &Path,
    owner_uid: u32,
    allow_blobs: bool,
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
        let staging_blob = name.starts_with(".cp0-blob-");
        let temporary_value = name.starts_with(".cp0-tmp-");
        let size_limit =
            if allow_blobs && (staging_blob || metadata.len() > MAX_STORAGE_VALUE_BYTES as u64) {
                MAX_STORAGE_BLOB_BYTES as u64
            } else {
                MAX_STORAGE_VALUE_BYTES as u64
            };
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.uid() != owner_uid
            || metadata.mode() & 0o077 != 0
            || metadata.len() == 0
            || metadata.len() > size_limit
        {
            return Err(StorageServiceError::InvalidEntry);
        }
        used_bytes = used_bytes
            .checked_add(metadata.len())
            .ok_or(StorageServiceError::QuotaExceeded)?;
        if temporary_value || staging_blob {
            continue;
        }
        validate_key(&name).map_err(|_| StorageServiceError::InvalidEntry)?;
        keys += 1;
        if !allow_blobs && keys > MAX_STORAGE_KEYS {
            return Err(StorageServiceError::TooManyKeys);
        }
    }
    Ok((used_bytes, keys))
}

fn cleanup_staging_blobs(directory: &Path, owner_uid: u32) -> Result<(), StorageServiceError> {
    let mut removed = false;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| StorageServiceError::InvalidEntry)?;
        let Some(key) = name.strip_prefix(".cp0-blob-") else {
            continue;
        };
        validate_key(key).map_err(|_| StorageServiceError::InvalidEntry)?;
        remove_stale_blob(&entry.path(), owner_uid)?;
        removed = true;
    }
    if removed {
        sync_directory(directory)?;
    }
    Ok(())
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

fn open_blob(path: &Path, owner_uid: u32) -> Result<Option<File>, StorageServiceError> {
    match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => {
            let metadata = file.metadata()?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.uid() != owner_uid
                || metadata.mode() & 0o077 != 0
                || metadata.len() == 0
                || metadata.len() > MAX_STORAGE_BLOB_BYTES as u64
            {
                return Err(StorageServiceError::InvalidEntry);
            }
            Ok(Some(file))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(StorageServiceError::Io(error)),
    }
}

fn remove_stale_blob(path: &Path, owner_uid: u32) -> Result<(), StorageServiceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.uid() != owner_uid
                || metadata.mode() & 0o077 != 0
                || metadata.len() > MAX_STORAGE_BLOB_BYTES as u64
            {
                return Err(StorageServiceError::InvalidEntry);
            }
            fs::remove_file(path)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn require_free_space(path: &Path, write_bytes: u64) -> Result<(), StorageServiceError> {
    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| StorageServiceError::InvalidEntry)?;
        let mut status: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(path.as_ptr(), &raw mut status) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let available = u64::from(status.f_bavail).saturating_mul(u64::from(status.f_frsize));
        if available < write_bytes.saturating_add(SYSTEM_FREE_SPACE_RESERVE_BYTES) {
            return Err(StorageServiceError::QuotaExceeded);
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (path, write_bytes);
    Ok(())
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

    use cp0_storage_protocol::{
        MIB, SYSTEM_PHOTO_LIBRARY_ID, SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES, decode_value,
    };

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

    #[test]
    fn photo_blob_is_published_only_after_the_final_chunk() {
        let (root, service) = test_service();
        let frame = vec![0x5a_u8; 320 * 170 * 2];
        let key = "p000000000000002a.rgb565";
        for (chunk, value) in frame.chunks(MAX_STORAGE_VALUE_BYTES).enumerate() {
            let request = StorageRequest::put_blob_chunk(
                10 + chunk as u64,
                SYSTEM_PHOTO_LIBRARY_ID,
                SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES,
                key,
                (chunk * MAX_STORAGE_VALUE_BYTES) as u32,
                frame.len() as u32,
                value,
            );
            assert!(matches!(
                service.dispatch(request).outcome,
                cp0_storage_protocol::StorageOutcome::Stored { .. }
            ));
            if chunk + 1 != frame.len().div_ceil(MAX_STORAGE_VALUE_BYTES) {
                assert!(!root.join(SYSTEM_PHOTO_LIBRARY_ID).join(key).exists());
            }
        }
        assert_eq!(
            fs::metadata(root.join(SYSTEM_PHOTO_LIBRARY_ID).join(key))
                .unwrap()
                .len(),
            frame.len() as u64
        );

        let response = service.dispatch(StorageRequest::get_blob_chunk(
            30,
            SYSTEM_PHOTO_LIBRARY_ID,
            SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES,
            key,
            MAX_STORAGE_VALUE_BYTES as u32,
            MAX_STORAGE_VALUE_BYTES as u32,
        ));
        let cp0_storage_protocol::StorageOutcome::Value { value_base64 } = response.outcome else {
            panic!("photo blob chunk was not returned")
        };
        assert_eq!(
            decode_value(&value_base64).unwrap(),
            vec![0x5a; MAX_STORAGE_VALUE_BYTES]
        );

        let opened = service.dispatch_with_descriptor(StorageRequest::open_blob(
            31,
            SYSTEM_PHOTO_LIBRARY_ID,
            SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES,
            key,
            frame.len() as u32,
        ));
        assert_eq!(
            opened.response.outcome,
            cp0_storage_protocol::StorageOutcome::BlobOpened {
                size_bytes: frame.len() as u32
            }
        );
        let mut opened_file = File::from(opened.descriptor.unwrap());
        let mut opened_frame = Vec::new();
        opened_file.read_to_end(&mut opened_frame).unwrap();
        assert_eq!(opened_frame, frame);

        assert!(matches!(
            service
                .dispatch(StorageRequest::delete_blob(
                    32,
                    SYSTEM_PHOTO_LIBRARY_ID,
                    SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES,
                    key,
                ))
                .outcome,
            cp0_storage_protocol::StorageOutcome::Deleted { existed: true, .. }
        ));
        assert!(!root.join(SYSTEM_PHOTO_LIBRARY_ID).join(key).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn startup_removes_incomplete_photo_blobs_without_touching_committed_values() {
        let (root, service) = test_service();
        let key = "p000000000000002a.rgb565";
        let first = StorageRequest::put_blob_chunk(
            40,
            SYSTEM_PHOTO_LIBRARY_ID,
            SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES,
            key,
            0,
            (320 * 170 * 2) as u32,
            &[0x7b; MAX_STORAGE_VALUE_BYTES],
        );
        assert!(matches!(
            service.dispatch(first).outcome,
            cp0_storage_protocol::StorageOutcome::Stored { .. }
        ));
        let directory = root.join(SYSTEM_PHOTO_LIBRARY_ID);
        let staging = directory.join(format!(".cp0-blob-{key}"));
        let retained = directory.join("head.v2");
        fs::write(&retained, b"retained").unwrap();
        fs::set_permissions(&retained, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(staging.exists());

        StorageService::new(&root, unsafe { libc::geteuid() })
            .initialize()
            .unwrap();
        assert!(!staging.exists());
        assert_eq!(fs::read(retained).unwrap(), b"retained");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn startup_refuses_a_symlink_disguised_as_photo_staging() {
        use std::os::unix::fs::symlink;

        let (root, service) = test_service();
        let create = StorageRequest::put(
            50,
            SYSTEM_PHOTO_LIBRARY_ID,
            SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES,
            "head.v2",
            b"retained",
        );
        assert!(matches!(
            service.dispatch(create).outcome,
            cp0_storage_protocol::StorageOutcome::Stored { .. }
        ));
        let directory = root.join(SYSTEM_PHOTO_LIBRARY_ID);
        let retained = directory.join("head.v2");
        let staging = directory.join(".cp0-blob-p000000000000002a.rgb565");
        symlink(&retained, &staging).unwrap();

        assert!(matches!(
            StorageService::new(&root, unsafe { libc::geteuid() }).initialize(),
            Err(StorageServiceError::InvalidEntry)
        ));
        assert!(staging.is_symlink());
        assert_eq!(fs::read(retained).unwrap(), b"retained");
        fs::remove_dir_all(root).unwrap();
    }
}
