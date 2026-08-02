use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path};

use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"CP0BKUP\0";
const FORMAT_VERSION: u16 = 1;
const FIXED_HEADER_BYTES: u64 = 8 + 2 + 2 + 4 + 8 + 32;
const ENTRY_HEADER_BYTES: u64 = 1 + 3 + 4 + 4 + 4 + 2 + 2 + 8 + 32;
const MAX_ENTRIES: usize = 65_536;
const MAX_PATH_BYTES: usize = 512;
const MAX_PATH_DEPTH: usize = 32;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 64 * 1024;

const REQUIRED_DIRECTORIES: &[&str] = &[
    "cardputerzero",
    "etc-cardputerzero",
    "extrausers",
    "home",
    "network-connections",
    "network-state",
    "ssh",
];
const REQUIRED_FILES: &[&str] = &["layout-version", "machine-id", "random-seed"];

#[derive(Debug)]
pub enum BackupError {
    Io(io::Error),
    Invalid(String),
}

impl fmt::Display for BackupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "backup I/O error: {error}"),
            Self::Invalid(error) => write!(formatter, "invalid cp0 backup: {error}"),
        }
    }
}

impl std::error::Error for BackupError {}

impl From<io::Error> for BackupError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupSummary {
    pub entry_count: usize,
    pub file_count: usize,
    pub file_bytes: u64,
    pub image_profile: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryKind {
    Directory = 1,
    File = 2,
}

impl EntryKind {
    fn decode(value: u8) -> Result<Self, BackupError> {
        match value {
            1 => Ok(Self::Directory),
            2 => Ok(Self::File),
            _ => Err(BackupError::Invalid("entry type is unsupported".into())),
        }
    }
}

#[derive(Debug, Clone)]
struct EntrySource {
    path: String,
    kind: EntryKind,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    digest: [u8; 32],
    fingerprint: Fingerprint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    device: u64,
    inode: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

impl Fingerprint {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
            size: metadata.size(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        }
    }
}

#[derive(Debug)]
struct ParsedEntry {
    path: String,
    kind: EntryKind,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    digest: [u8; 32],
}

#[derive(Debug)]
struct PayloadReader<R> {
    inner: R,
    remaining: u64,
    consumed: u64,
    hasher: Sha256,
}

impl<R: Read> PayloadReader<R> {
    fn new(inner: R, length: u64) -> Self {
        Self {
            inner,
            remaining: length,
            consumed: 0,
            hasher: Sha256::new(),
        }
    }

    fn read_exact(&mut self, buffer: &mut [u8]) -> Result<(), BackupError> {
        if buffer.len() as u64 > self.remaining {
            return Err(BackupError::Invalid("payload length is truncated".into()));
        }
        self.inner.read_exact(buffer)?;
        self.hasher.update(&*buffer);
        self.remaining -= buffer.len() as u64;
        self.consumed += buffer.len() as u64;
        Ok(())
    }

    fn finish(self) -> Result<(R, [u8; 32]), BackupError> {
        if self.remaining != 0 {
            return Err(BackupError::Invalid(
                "payload has unreferenced trailing bytes".into(),
            ));
        }
        Ok((self.inner, self.hasher.finalize().into()))
    }
}

pub fn create_backup(
    source_root: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<BackupSummary, BackupError> {
    let source_root = source_root.as_ref();
    reject_root_link(source_root, "source")?;
    let source_root = fs::canonicalize(source_root)?;
    verify_source_root(&source_root)?;
    let output = output.as_ref();
    verify_output_location(&source_root, output)?;

    let mut entries = Vec::new();
    let mut total_file_bytes = 0_u64;
    collect_source_entries(&source_root, &mut entries, &mut total_file_bytes)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    validate_source_semantics(&source_root, &entries)?;

    let result = write_backup(&source_root, output, &entries);
    if let Err(error) = result {
        let _ = fs::remove_file(output);
        return Err(error);
    }
    match verify_backup(output) {
        Ok(summary) => Ok(summary),
        Err(error) => {
            let _ = fs::remove_file(output);
            Err(error)
        }
    }
}

pub fn verify_backup(bundle: impl AsRef<Path>) -> Result<BackupSummary, BackupError> {
    parse_backup(bundle.as_ref(), None)
}

#[cfg(any(test, feature = "fuzzing"))]
pub fn verify_backup_bytes(encoded: &[u8]) -> Result<BackupSummary, BackupError> {
    parse_backup_reader(std::io::Cursor::new(encoded), encoded.len() as u64, None)
}

pub fn restore_backup(
    bundle: impl AsRef<Path>,
    target_root: impl AsRef<Path>,
) -> Result<BackupSummary, BackupError> {
    let bundle = bundle.as_ref();
    let expected = verify_backup(bundle)?;
    let target_root = target_root.as_ref();
    reject_root_link(target_root, "restore target")?;
    let target_root = fs::canonicalize(target_root)?;
    verify_empty_target(&target_root)?;
    let restored = parse_backup(bundle, Some(&target_root))?;
    if restored != expected {
        return Err(BackupError::Invalid(
            "restored backup summary changed between verification passes".into(),
        ));
    }
    sync_directory(&target_root)?;
    Ok(restored)
}

fn verify_source_root(root: &Path) -> Result<(), BackupError> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BackupError::Invalid(
            "source root is not a real directory".into(),
        ));
    }
    let allowed = REQUIRED_DIRECTORIES
        .iter()
        .chain(REQUIRED_FILES.iter())
        .copied()
        .collect::<BTreeSet<_>>();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| BackupError::Invalid("source contains a non-UTF-8 name".into()))?;
        if allowed.contains(name.as_str()) {
            continue;
        }
        if name == "lost+found" {
            let metadata = fs::symlink_metadata(entry.path())?;
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || fs::read_dir(entry.path())?.next().is_some()
            {
                return Err(BackupError::Invalid(
                    "lost+found is not an empty real directory".into(),
                ));
            }
            continue;
        }
        return Err(BackupError::Invalid(format!(
            "source contains an unexpected top-level entry: {name}"
        )));
    }
    Ok(())
}

fn reject_root_link(path: &Path, label: &str) -> Result<(), BackupError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(BackupError::Invalid(format!(
            "{label} root cannot be a symbolic link"
        )));
    }
    Ok(())
}

fn verify_output_location(source_root: &Path, output: &Path) -> Result<(), BackupError> {
    if !output.is_absolute() {
        return Err(BackupError::Invalid(
            "backup output path must be absolute".into(),
        ));
    }
    if output.exists() {
        return Err(BackupError::Invalid("backup output already exists".into()));
    }
    let parent = output
        .parent()
        .ok_or_else(|| BackupError::Invalid("backup output has no parent".into()))?;
    let parent = fs::canonicalize(parent)?;
    if parent.starts_with(source_root) {
        return Err(BackupError::Invalid(
            "backup output cannot be inside cp0-data".into(),
        ));
    }
    Ok(())
}

fn collect_source_entries(
    root: &Path,
    entries: &mut Vec<EntrySource>,
    total_file_bytes: &mut u64,
) -> Result<(), BackupError> {
    for path in REQUIRED_DIRECTORIES {
        collect_source_path(root, Path::new(path), entries, total_file_bytes)?;
    }
    for path in REQUIRED_FILES {
        collect_source_path(root, Path::new(path), entries, total_file_bytes)?;
    }
    Ok(())
}

fn collect_source_path(
    root: &Path,
    relative: &Path,
    entries: &mut Vec<EntrySource>,
    total_file_bytes: &mut u64,
) -> Result<(), BackupError> {
    if entries.len() >= MAX_ENTRIES {
        return Err(BackupError::Invalid("entry count exceeds limit".into()));
    }
    let path_text = relative
        .to_str()
        .ok_or_else(|| BackupError::Invalid("source contains a non-UTF-8 path".into()))?;
    validate_relative_path(path_text)?;
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path)?;
    let fingerprint = Fingerprint::from_metadata(&metadata);
    let mode = validate_metadata(&metadata, path_text)?;
    if metadata.is_dir() {
        entries.push(EntrySource {
            path: path_text.into(),
            kind: EntryKind::Directory,
            mode,
            uid: metadata.uid(),
            gid: metadata.gid(),
            size: 0,
            digest: [0; 32],
            fingerprint,
        });
        let mut children = fs::read_dir(&path)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let child_name = child
                .file_name()
                .into_string()
                .map_err(|_| BackupError::Invalid("source contains a non-UTF-8 name".into()))?;
            collect_source_path(root, &relative.join(child_name), entries, total_file_bytes)?;
        }
    } else if metadata.is_file() {
        if metadata.nlink() != 1 {
            return Err(BackupError::Invalid(format!(
                "hard-linked file is not supported: {path_text}"
            )));
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err(BackupError::Invalid(format!(
                "file exceeds size limit: {path_text}"
            )));
        }
        *total_file_bytes = total_file_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| BackupError::Invalid("total file size overflows".into()))?;
        if *total_file_bytes > MAX_PAYLOAD_BYTES {
            return Err(BackupError::Invalid("total file size exceeds limit".into()));
        }
        let digest = hash_source_file(&path, &fingerprint)?;
        entries.push(EntrySource {
            path: path_text.into(),
            kind: EntryKind::File,
            mode,
            uid: metadata.uid(),
            gid: metadata.gid(),
            size: metadata.len(),
            digest,
            fingerprint,
        });
    } else {
        return Err(BackupError::Invalid(format!(
            "unsupported filesystem entry: {path_text}"
        )));
    }
    Ok(())
}

fn validate_metadata(metadata: &fs::Metadata, path: &str) -> Result<u32, BackupError> {
    if metadata.file_type().is_symlink() {
        return Err(BackupError::Invalid(format!(
            "symbolic link is not supported: {path}"
        )));
    }
    let mode = metadata.mode() & 0o7777;
    if mode & 0o7000 != 0 {
        return Err(BackupError::Invalid(format!(
            "special permission bits are not supported: {path}"
        )));
    }
    if mode & 0o002 != 0 {
        return Err(BackupError::Invalid(format!(
            "world-writable entry is not supported: {path}"
        )));
    }
    if metadata.uid() > u16::MAX as u32 || metadata.gid() > u16::MAX as u32 {
        return Err(BackupError::Invalid(format!(
            "entry owner is outside the system range: {path}"
        )));
    }
    if metadata.is_dir() && mode & 0o500 != 0o500 {
        return Err(BackupError::Invalid(format!(
            "directory is not owner-readable and searchable: {path}"
        )));
    }
    if metadata.is_file() && mode & 0o400 == 0 {
        return Err(BackupError::Invalid(format!(
            "file is not owner-readable: {path}"
        )));
    }
    Ok(mode)
}

fn hash_source_file(path: &Path, expected: &Fingerprint) -> Result<[u8; 32], BackupError> {
    let mut file = open_read_only_no_follow(path)?;
    if Fingerprint::from_metadata(&file.metadata()?) != *expected {
        return Err(BackupError::Invalid(
            "source changed while it was inspected".into(),
        ));
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut bytes = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        hasher.update(&buffer[..read]);
    }
    if bytes != expected.size || Fingerprint::from_metadata(&file.metadata()?) != *expected {
        return Err(BackupError::Invalid(
            "source changed while it was read".into(),
        ));
    }
    Ok(hasher.finalize().into())
}

fn validate_source_semantics(root: &Path, entries: &[EntrySource]) -> Result<(), BackupError> {
    let kinds = entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry.kind))
        .collect::<BTreeMap<_, _>>();
    validate_required_entries(&kinds)?;
    if fs::read(root.join("layout-version"))? != b"cp0-data-layout-v2\n" {
        return Err(BackupError::Invalid(
            "persistent layout marker is invalid".into(),
        ));
    }
    let profile = fs::read(root.join("etc-cardputerzero/image-profile"))?;
    if profile != b"product\n" && profile != b"recovery\n" {
        return Err(BackupError::Invalid(
            "image profile marker is invalid".into(),
        ));
    }
    Ok(())
}

fn write_backup(root: &Path, output: &Path, entries: &[EntrySource]) -> Result<(), BackupError> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(output)?;
    file.write_all(&vec![0_u8; FIXED_HEADER_BYTES as usize])?;
    let mut payload_hasher = Sha256::new();
    let mut payload_bytes = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];

    for entry in entries {
        let header = encode_entry_header(entry)?;
        write_payload(&mut file, &mut payload_hasher, &mut payload_bytes, &header)?;
        write_payload(
            &mut file,
            &mut payload_hasher,
            &mut payload_bytes,
            entry.path.as_bytes(),
        )?;
        if entry.kind == EntryKind::File {
            let mut source = open_read_only_no_follow(&root.join(&entry.path))?;
            if Fingerprint::from_metadata(&source.metadata()?) != entry.fingerprint {
                return Err(BackupError::Invalid(format!(
                    "source changed before backup: {}",
                    entry.path
                )));
            }
            let mut content_hasher = Sha256::new();
            let mut content_bytes = 0_u64;
            loop {
                let read = source.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                content_bytes += read as u64;
                content_hasher.update(&buffer[..read]);
                write_payload(
                    &mut file,
                    &mut payload_hasher,
                    &mut payload_bytes,
                    &buffer[..read],
                )?;
            }
            if content_bytes != entry.size
                || <[u8; 32]>::from(content_hasher.finalize()) != entry.digest
                || Fingerprint::from_metadata(&source.metadata()?) != entry.fingerprint
            {
                return Err(BackupError::Invalid(format!(
                    "source changed during backup: {}",
                    entry.path
                )));
            }
        }
    }
    if payload_bytes > MAX_PAYLOAD_BYTES {
        return Err(BackupError::Invalid("payload exceeds size limit".into()));
    }
    let payload_digest: [u8; 32] = payload_hasher.finalize().into();
    file.seek(SeekFrom::Start(0))?;
    file.write_all(MAGIC)?;
    file.write_all(&FORMAT_VERSION.to_le_bytes())?;
    file.write_all(&0_u16.to_le_bytes())?;
    file.write_all(&(entries.len() as u32).to_le_bytes())?;
    file.write_all(&payload_bytes.to_le_bytes())?;
    file.write_all(&payload_digest)?;
    file.sync_all()?;
    Ok(())
}

fn encode_entry_header(entry: &EntrySource) -> Result<Vec<u8>, BackupError> {
    let path_length = u16::try_from(entry.path.len())
        .map_err(|_| BackupError::Invalid("entry path is too long".into()))?;
    let mut encoded = Vec::with_capacity(ENTRY_HEADER_BYTES as usize);
    encoded.push(entry.kind as u8);
    encoded.extend_from_slice(&[0; 3]);
    encoded.extend_from_slice(&entry.mode.to_le_bytes());
    encoded.extend_from_slice(&entry.uid.to_le_bytes());
    encoded.extend_from_slice(&entry.gid.to_le_bytes());
    encoded.extend_from_slice(&path_length.to_le_bytes());
    encoded.extend_from_slice(&0_u16.to_le_bytes());
    encoded.extend_from_slice(&entry.size.to_le_bytes());
    encoded.extend_from_slice(&entry.digest);
    Ok(encoded)
}

fn write_payload(
    file: &mut File,
    hasher: &mut Sha256,
    count: &mut u64,
    bytes: &[u8],
) -> Result<(), BackupError> {
    *count = count
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| BackupError::Invalid("payload size overflows".into()))?;
    if *count > MAX_PAYLOAD_BYTES {
        return Err(BackupError::Invalid("payload exceeds size limit".into()));
    }
    file.write_all(bytes)?;
    hasher.update(bytes);
    Ok(())
}

fn parse_backup(bundle: &Path, target: Option<&Path>) -> Result<BackupSummary, BackupError> {
    let file = open_read_only_no_follow(bundle)?;
    let file_length = file.metadata()?.len();
    parse_backup_reader(file, file_length, target)
}

fn parse_backup_reader<R: Read>(
    mut file: R,
    file_length: u64,
    target: Option<&Path>,
) -> Result<BackupSummary, BackupError> {
    if !(FIXED_HEADER_BYTES..=FIXED_HEADER_BYTES + MAX_PAYLOAD_BYTES).contains(&file_length) {
        return Err(BackupError::Invalid(
            "backup file size is outside limits".into(),
        ));
    }
    let mut magic = [0_u8; 8];
    file.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(BackupError::Invalid("backup magic does not match".into()));
    }
    if read_u16(&mut file)? != FORMAT_VERSION {
        return Err(BackupError::Invalid(
            "backup format version is unsupported".into(),
        ));
    }
    if read_u16(&mut file)? != 0 {
        return Err(BackupError::Invalid("unknown backup flags are set".into()));
    }
    let entry_count = read_u32(&mut file)? as usize;
    if entry_count == 0 || entry_count > MAX_ENTRIES {
        return Err(BackupError::Invalid("entry count is outside limits".into()));
    }
    let payload_length = read_u64(&mut file)?;
    if payload_length > MAX_PAYLOAD_BYTES || FIXED_HEADER_BYTES + payload_length != file_length {
        return Err(BackupError::Invalid(
            "payload length does not match file size".into(),
        ));
    }
    let mut expected_payload_digest = [0_u8; 32];
    file.read_exact(&mut expected_payload_digest)?;
    let mut payload = PayloadReader::new(file, payload_length);
    let mut previous_path: Option<String> = None;
    let mut kinds = BTreeMap::new();
    let mut file_count = 0_usize;
    let mut file_bytes = 0_u64;
    let mut image_profile = None;
    let mut directory_metadata = Vec::new();

    for _ in 0..entry_count {
        let entry = read_entry(&mut payload)?;
        if let Some(previous) = &previous_path {
            if entry.path <= *previous {
                return Err(BackupError::Invalid(
                    "entry paths are not strictly sorted".into(),
                ));
            }
        }
        previous_path = Some(entry.path.clone());
        if kinds.insert(entry.path.clone(), entry.kind).is_some() {
            return Err(BackupError::Invalid("entry path is duplicated".into()));
        }
        let mut special_contents = Vec::new();
        let special =
            entry.path == "layout-version" || entry.path == "etc-cardputerzero/image-profile";
        if special && entry.size > 64 {
            return Err(BackupError::Invalid(
                "required marker is unexpectedly large".into(),
            ));
        }

        match entry.kind {
            EntryKind::Directory => {
                if entry.size != 0 || entry.digest != [0; 32] {
                    return Err(BackupError::Invalid(
                        "directory entry carries file contents".into(),
                    ));
                }
                if let Some(root) = target {
                    create_restore_directory(root, &entry)?;
                    directory_metadata.push((
                        root.join(&entry.path),
                        entry.uid,
                        entry.gid,
                        entry.mode,
                    ));
                }
            }
            EntryKind::File => {
                file_count += 1;
                file_bytes = file_bytes
                    .checked_add(entry.size)
                    .ok_or_else(|| BackupError::Invalid("file byte count overflows".into()))?;
                if file_bytes > MAX_PAYLOAD_BYTES {
                    return Err(BackupError::Invalid(
                        "file contents exceed total size limit".into(),
                    ));
                }
                let mut destination = match target {
                    Some(root) => Some(create_restore_file(root, &entry)?),
                    None => None,
                };
                let mut content_hasher = Sha256::new();
                let mut remaining = entry.size;
                let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
                while remaining != 0 {
                    let chunk = usize::try_from(remaining.min(buffer.len() as u64))
                        .map_err(|_| BackupError::Invalid("file size overflows".into()))?;
                    payload.read_exact(&mut buffer[..chunk])?;
                    content_hasher.update(&buffer[..chunk]);
                    if special {
                        special_contents.extend_from_slice(&buffer[..chunk]);
                    }
                    if let Some(file) = &mut destination {
                        file.write_all(&buffer[..chunk])?;
                    }
                    remaining -= chunk as u64;
                }
                if <[u8; 32]>::from(content_hasher.finalize()) != entry.digest {
                    return Err(BackupError::Invalid(format!(
                        "file digest does not match: {}",
                        entry.path
                    )));
                }
                if let Some(file) = destination {
                    finish_restore_file(file, &entry)?;
                }
            }
        }
        if entry.path == "layout-version" && special_contents != b"cp0-data-layout-v2\n" {
            return Err(BackupError::Invalid(
                "persistent layout marker is invalid".into(),
            ));
        }
        if entry.path == "etc-cardputerzero/image-profile" {
            image_profile = match special_contents.as_slice() {
                b"product\n" => Some("product".to_owned()),
                b"recovery\n" => Some("recovery".to_owned()),
                _ => {
                    return Err(BackupError::Invalid(
                        "image profile marker is invalid".into(),
                    ));
                }
            };
        }
    }
    let (mut file, payload_digest) = payload.finish()?;
    if payload_digest != expected_payload_digest {
        return Err(BackupError::Invalid("payload digest does not match".into()));
    }
    let mut trailing = [0_u8; 1];
    if file.read(&mut trailing)? != 0 {
        return Err(BackupError::Invalid("backup has trailing bytes".into()));
    }
    validate_required_entries(
        &kinds
            .iter()
            .map(|(path, kind)| (path.as_str(), *kind))
            .collect(),
    )?;
    let image_profile = image_profile
        .ok_or_else(|| BackupError::Invalid("image profile marker is missing".into()))?;
    if target.is_some() {
        for (path, uid, gid, mode) in directory_metadata.into_iter().rev() {
            set_path_metadata(&path, uid, gid, mode)?;
            sync_directory(&path)?;
        }
    }
    Ok(BackupSummary {
        entry_count,
        file_count,
        file_bytes,
        image_profile,
    })
}

fn read_entry<R: Read>(payload: &mut PayloadReader<R>) -> Result<ParsedEntry, BackupError> {
    let mut encoded = vec![0_u8; ENTRY_HEADER_BYTES as usize];
    payload.read_exact(&mut encoded)?;
    let kind = EntryKind::decode(encoded[0])?;
    if encoded[1..4] != [0; 3] {
        return Err(BackupError::Invalid(
            "entry reserved bytes are nonzero".into(),
        ));
    }
    let mode = u32::from_le_bytes(encoded[4..8].try_into().expect("fixed slice"));
    let uid = u32::from_le_bytes(encoded[8..12].try_into().expect("fixed slice"));
    let gid = u32::from_le_bytes(encoded[12..16].try_into().expect("fixed slice"));
    let path_length = u16::from_le_bytes(encoded[16..18].try_into().expect("fixed slice")) as usize;
    if encoded[18..20] != [0; 2] {
        return Err(BackupError::Invalid(
            "entry reserved field is nonzero".into(),
        ));
    }
    let size = u64::from_le_bytes(encoded[20..28].try_into().expect("fixed slice"));
    let digest = encoded[28..60].try_into().expect("fixed slice");
    if path_length == 0 || path_length > MAX_PATH_BYTES || size > MAX_FILE_BYTES {
        return Err(BackupError::Invalid(
            "entry path or size is outside limits".into(),
        ));
    }
    validate_encoded_mode(mode)?;
    if uid > u16::MAX as u32 || gid > u16::MAX as u32 {
        return Err(BackupError::Invalid(
            "entry owner is outside the system range".into(),
        ));
    }
    if kind == EntryKind::Directory && mode & 0o500 != 0o500 {
        return Err(BackupError::Invalid(
            "directory is not owner-readable and searchable".into(),
        ));
    }
    if kind == EntryKind::File && mode & 0o400 == 0 {
        return Err(BackupError::Invalid("file is not owner-readable".into()));
    }
    let mut path_bytes = vec![0_u8; path_length];
    payload.read_exact(&mut path_bytes)?;
    let path = String::from_utf8(path_bytes)
        .map_err(|_| BackupError::Invalid("entry path is not UTF-8".into()))?;
    validate_relative_path(&path)?;
    Ok(ParsedEntry {
        path,
        kind,
        mode,
        uid,
        gid,
        size,
        digest,
    })
}

fn validate_relative_path(path: &str) -> Result<(), BackupError> {
    if path.is_empty() || path.len() > MAX_PATH_BYTES {
        return Err(BackupError::Invalid("entry path length is invalid".into()));
    }
    if path.chars().any(char::is_control) {
        return Err(BackupError::Invalid(
            "entry path contains a control character".into(),
        ));
    }
    let parsed = Path::new(path);
    if parsed.is_absolute() {
        return Err(BackupError::Invalid(
            "absolute entry path is forbidden".into(),
        ));
    }
    let components = parsed.components().collect::<Vec<_>>();
    if components.is_empty()
        || components.len() > MAX_PATH_DEPTH
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(BackupError::Invalid(
            "entry path contains an unsafe component".into(),
        ));
    }
    let top = components[0].as_os_str().to_string_lossy();
    if !REQUIRED_DIRECTORIES.contains(&top.as_ref()) && !REQUIRED_FILES.contains(&top.as_ref()) {
        return Err(BackupError::Invalid(
            "entry path is outside the persistent allowlist".into(),
        ));
    }
    if REQUIRED_FILES.contains(&top.as_ref()) && components.len() != 1 {
        return Err(BackupError::Invalid(
            "file allowlist root cannot contain children".into(),
        ));
    }
    Ok(())
}

fn validate_encoded_mode(mode: u32) -> Result<(), BackupError> {
    if mode & !0o7777 != 0 || mode & 0o7000 != 0 || mode & 0o002 != 0 {
        return Err(BackupError::Invalid(
            "entry permissions are outside policy".into(),
        ));
    }
    Ok(())
}

fn validate_required_entries(kinds: &BTreeMap<&str, EntryKind>) -> Result<(), BackupError> {
    for path in REQUIRED_DIRECTORIES {
        if kinds.get(path) != Some(&EntryKind::Directory) {
            return Err(BackupError::Invalid(format!(
                "required directory is missing or invalid: {path}"
            )));
        }
    }
    for path in REQUIRED_FILES {
        if kinds.get(path) != Some(&EntryKind::File) {
            return Err(BackupError::Invalid(format!(
                "required file is missing or invalid: {path}"
            )));
        }
    }
    if kinds.get("etc-cardputerzero/image-profile") != Some(&EntryKind::File) {
        return Err(BackupError::Invalid(
            "image profile marker is missing or invalid".into(),
        ));
    }
    Ok(())
}

fn verify_empty_target(root: &Path) -> Result<(), BackupError> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.mode() & 0o077 != 0 {
        return Err(BackupError::Invalid(
            "restore target must be an owner-only real directory".into(),
        ));
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_name() != "lost+found" {
            return Err(BackupError::Invalid("restore target is not empty".into()));
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || fs::read_dir(entry.path())?.next().is_some()
        {
            return Err(BackupError::Invalid(
                "restore target lost+found is not empty".into(),
            ));
        }
    }
    Ok(())
}

fn create_restore_directory(root: &Path, entry: &ParsedEntry) -> Result<(), BackupError> {
    let path = root.join(&entry.path);
    let parent = path
        .parent()
        .ok_or_else(|| BackupError::Invalid("restore path has no parent".into()))?;
    verify_restore_parent(root, parent)?;
    fs::create_dir(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn create_restore_file(root: &Path, entry: &ParsedEntry) -> Result<File, BackupError> {
    let path = root.join(&entry.path);
    let parent = path
        .parent()
        .ok_or_else(|| BackupError::Invalid("restore path has no parent".into()))?;
    verify_restore_parent(root, parent)?;
    Ok(OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?)
}

fn verify_restore_parent(root: &Path, parent: &Path) -> Result<(), BackupError> {
    if !parent.starts_with(root) {
        return Err(BackupError::Invalid(
            "restore parent escapes target root".into(),
        ));
    }
    let metadata = fs::symlink_metadata(parent)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BackupError::Invalid(
            "restore parent is not a real directory".into(),
        ));
    }
    Ok(())
}

fn finish_restore_file(file: File, entry: &ParsedEntry) -> Result<(), BackupError> {
    file.sync_all()?;
    set_fd_metadata(&file, entry.uid, entry.gid, entry.mode)?;
    file.sync_all()?;
    Ok(())
}

fn set_path_metadata(path: &Path, uid: u32, gid: u32, mode: u32) -> Result<(), BackupError> {
    let file = open_read_only_no_follow(path)?;
    set_fd_metadata(&file, uid, gid, mode)
}

fn set_fd_metadata(file: &File, uid: u32, gid: u32, mode: u32) -> Result<(), BackupError> {
    let metadata = file.metadata()?;
    if metadata.uid() != uid || metadata.gid() != gid {
        let result = unsafe { libc::fchown(file.as_raw_fd(), uid, gid) };
        if result != 0 {
            return Err(io::Error::last_os_error().into());
        }
    }
    let result = unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) };
    if result != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

fn open_read_only_no_follow(path: &Path) -> Result<File, BackupError> {
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?)
}

fn sync_directory(path: &Path) -> Result<(), BackupError> {
    #[cfg(target_os = "linux")]
    File::open(path)?.sync_all()?;
    #[cfg(not(target_os = "linux"))]
    let _ = path;
    Ok(())
}

fn read_u16(reader: &mut impl Read) -> Result<u16, BackupError> {
    let mut bytes = [0_u8; 2];
    reader.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(reader: &mut impl Read) -> Result<u32, BackupError> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(reader: &mut impl Read) -> Result<u64, BackupError> {
    let mut bytes = [0_u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "cp0-recovery-{name}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("create fixture");
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("protect fixture");
            Self { root }
        }

        fn data_root(&self) -> PathBuf {
            self.root.join("data")
        }

        fn backup(&self) -> PathBuf {
            self.root.join("backup.cp0backup")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn create_data_root(path: &Path) {
        fs::create_dir(path).expect("create data root");
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("protect data root");
        for directory in REQUIRED_DIRECTORIES {
            fs::create_dir(path.join(directory)).expect("create required directory");
            fs::set_permissions(path.join(directory), fs::Permissions::from_mode(0o700))
                .expect("protect required directory");
        }
        fs::write(path.join("layout-version"), b"cp0-data-layout-v2\n").expect("write layout");
        fs::write(
            path.join("machine-id"),
            b"0123456789abcdef0123456789abcdef\n",
        )
        .expect("write machine id");
        fs::write(path.join("random-seed"), b"seed").expect("write seed");
        fs::write(path.join("etc-cardputerzero/image-profile"), b"product\n")
            .expect("write profile");
        fs::create_dir_all(path.join("cardputerzero/data/dev.example")).expect("create app data");
        fs::write(
            path.join("cardputerzero/data/dev.example/value"),
            b"private-value",
        )
        .expect("write app data");
        protect_tree(path);
    }

    fn protect_tree(root: &Path) {
        for entry in fs::read_dir(root).expect("read tree") {
            let entry = entry.expect("entry");
            let metadata = entry.metadata().expect("metadata");
            if metadata.is_dir() {
                protect_tree(&entry.path());
                fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o700))
                    .expect("protect directory");
            } else {
                fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o600))
                    .expect("protect file");
            }
        }
    }

    fn refresh_payload_digest(encoded: &mut [u8]) {
        let payload_start = FIXED_HEADER_BYTES as usize;
        let digest: [u8; 32] = Sha256::digest(&encoded[payload_start..]).into();
        encoded[24..56].copy_from_slice(&digest);
    }

    #[test]
    fn backup_is_deterministic_and_restores_exact_contents() {
        let fixture = Fixture::new("round-trip");
        let source = fixture.data_root();
        create_data_root(&source);
        let first = fixture.backup();
        let second = fixture.root.join("second.cp0backup");
        let summary = create_backup(&source, &first).expect("create backup");
        create_backup(&source, &second).expect("create second backup");
        assert_eq!(
            fs::read(&first).expect("first"),
            fs::read(&second).expect("second")
        );
        assert_eq!(summary.image_profile, "product");
        assert!(summary.entry_count >= 12);

        let restored = fixture.root.join("restored");
        fs::create_dir(&restored).expect("create restore root");
        fs::set_permissions(&restored, fs::Permissions::from_mode(0o700))
            .expect("protect restore root");
        let restored_summary = restore_backup(&first, &restored).expect("restore backup");
        assert_eq!(restored_summary, summary);
        assert_eq!(
            fs::read(restored.join("cardputerzero/data/dev.example/value"))
                .expect("restored value"),
            b"private-value"
        );
        assert_eq!(
            fs::read(restored.join("etc-cardputerzero/image-profile")).expect("profile"),
            b"product\n"
        );
    }

    #[test]
    fn corruption_and_trailing_data_are_rejected_before_restore() {
        let fixture = Fixture::new("corrupt");
        let source = fixture.data_root();
        create_data_root(&source);
        let backup = fixture.backup();
        create_backup(&source, &backup).expect("create backup");

        let mut bytes = fs::read(&backup).expect("read backup");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x80;
        let corrupt = fixture.root.join("corrupt.cp0backup");
        fs::write(&corrupt, &bytes).expect("write corrupt backup");
        assert!(verify_backup(&corrupt).is_err());

        let mut trailing = fs::read(&backup).expect("read backup");
        trailing.push(0);
        let trailing_path = fixture.root.join("trailing.cp0backup");
        fs::write(&trailing_path, trailing).expect("write trailing backup");
        assert!(verify_backup(&trailing_path).is_err());
    }

    #[test]
    fn byte_slice_verifier_matches_files_and_rejects_truncation() {
        let fixture = Fixture::new("byte-slice");
        let source = fixture.data_root();
        create_data_root(&source);
        let backup = fixture.backup();
        let expected = create_backup(&source, &backup).expect("create backup");
        let encoded = fs::read(&backup).expect("read backup");

        assert_eq!(
            verify_backup_bytes(&encoded).expect("verify bytes"),
            expected
        );

        let cut_points = [
            0,
            1,
            MAGIC.len() - 1,
            MAGIC.len(),
            FIXED_HEADER_BYTES as usize - 1,
            FIXED_HEADER_BYTES as usize,
            FIXED_HEADER_BYTES as usize + ENTRY_HEADER_BYTES as usize - 1,
            encoded.len() - 1,
        ];
        for cut in cut_points {
            assert!(
                verify_backup_bytes(&encoded[..cut]).is_err(),
                "truncated backup unexpectedly passed at byte {cut}"
            );
        }

        let mut corrupt = encoded;
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0x80;
        assert!(verify_backup_bytes(&corrupt).is_err());
    }

    #[test]
    fn source_links_unsafe_modes_and_unknown_roots_are_rejected() {
        let fixture = Fixture::new("unsafe");
        let source = fixture.data_root();
        create_data_root(&source);
        let source_link = fixture.root.join("data-link");
        symlink(&source, &source_link).expect("create source root link");
        assert!(create_backup(&source_link, fixture.backup()).is_err());

        symlink(
            source.join("machine-id"),
            source.join("cardputerzero/escaped"),
        )
        .expect("create symlink");
        assert!(create_backup(&source, fixture.backup()).is_err());

        fs::remove_file(source.join("cardputerzero/escaped")).expect("remove test link");
        fs::write(source.join("unexpected"), b"data").expect("write unexpected entry");
        assert!(create_backup(&source, fixture.backup()).is_err());
        fs::remove_file(source.join("unexpected")).expect("remove unexpected entry");

        fs::set_permissions(source.join("machine-id"), fs::Permissions::from_mode(0o602))
            .expect("make unsafe mode");
        assert!(create_backup(&source, fixture.backup()).is_err());
    }

    #[test]
    fn restore_requires_an_empty_owner_only_target() {
        let fixture = Fixture::new("target");
        let source = fixture.data_root();
        create_data_root(&source);
        let backup = fixture.backup();
        create_backup(&source, &backup).expect("create backup");

        let target = fixture.root.join("target");
        fs::create_dir(&target).expect("create target");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).expect("protect target");
        fs::write(target.join("existing"), b"do not overwrite").expect("write existing");
        assert!(restore_backup(&backup, &target).is_err());
        assert_eq!(
            fs::read(target.join("existing")).expect("existing remains"),
            b"do not overwrite"
        );

        let empty_target = fixture.root.join("empty-target");
        fs::create_dir(&empty_target).expect("create empty target");
        fs::set_permissions(&empty_target, fs::Permissions::from_mode(0o700))
            .expect("protect empty target");
        let target_link = fixture.root.join("target-link");
        symlink(&empty_target, &target_link).expect("create target link");
        assert!(restore_backup(&backup, &target_link).is_err());
    }

    #[test]
    fn structurally_unsafe_entries_fail_with_a_matching_payload_digest() {
        let fixture = Fixture::new("mutated-structure");
        let source = fixture.data_root();
        create_data_root(&source);
        let backup = fixture.backup();
        create_backup(&source, &backup).expect("create backup");

        let mut escaped = fs::read(&backup).expect("read backup");
        let first_path = FIXED_HEADER_BYTES as usize + ENTRY_HEADER_BYTES as usize;
        assert_eq!(&escaped[first_path..first_path + 13], b"cardputerzero");
        escaped[first_path..first_path + 13].copy_from_slice(b"../escape-dir");
        refresh_payload_digest(&mut escaped);
        let escaped_path = fixture.root.join("escaped.cp0backup");
        fs::write(&escaped_path, escaped).expect("write escaped backup");
        assert!(verify_backup(&escaped_path).is_err());

        let mut privileged = fs::read(&backup).expect("read backup");
        let first_mode = FIXED_HEADER_BYTES as usize + 4;
        privileged[first_mode..first_mode + 4].copy_from_slice(&0o4700_u32.to_le_bytes());
        refresh_payload_digest(&mut privileged);
        let privileged_path = fixture.root.join("privileged.cp0backup");
        fs::write(&privileged_path, privileged).expect("write privileged backup");
        assert!(verify_backup(&privileged_path).is_err());
    }
}
