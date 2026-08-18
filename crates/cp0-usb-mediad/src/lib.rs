use std::collections::BTreeSet;
use std::ffi::{CStr, OsStr};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use cp0_appd::{
    PhotoExport, PhotoExportKind, StorageClient, list_photo_exports, read_photo_export,
};
use cp0_document_protocol::{MAX_DOCUMENT_BYTES, MAX_DOCUMENTS, is_valid_document_name};
use cp0_usb_media_protocol::{
    UsbMediaCommand, UsbMediaErrorCode, UsbMediaProtocolError, UsbMediaRequest, UsbMediaResponse,
    UsbMediaState, UsbMediaStatus, read_request, write_response,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const DEFAULT_USB_MEDIA_ROOT: &str = "/var/lib/cardputerzero/usb-media";
pub const DEFAULT_DOCUMENT_ROOT: &str = "/var/lib/cardputerzero/documents";
pub const DEFAULT_CONFIGFS_ROOT: &str = "/sys/kernel/config/usb_gadget";
pub const DEFAULT_UDC_ROOT: &str = "/sys/class/udc";
pub const DEFAULT_LOOP_CONTROL: &str = "/dev/loop-control";
pub const EXCHANGE_IMAGE_NAME: &str = "exchange.img";
pub const EXCHANGE_CAPACITY_BYTES: u64 = 512 * 1024 * 1024;
const EXCHANGE_LABEL: &str = "CP0-MEDIA";
const GADGET_NAME: &str = "cardputerzero-media";
const CLIENT_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_MUSIC_IMPORTS: usize = MAX_DOCUMENTS;
const WAV_HEADER_BYTES: usize = 2048;

#[derive(Debug, Clone)]
pub struct UsbMediaPaths {
    pub root: PathBuf,
    pub document_root: PathBuf,
    pub configfs_root: PathBuf,
    pub udc_root: PathBuf,
    pub loop_control: PathBuf,
    pub storaged_socket: PathBuf,
}

impl Default for UsbMediaPaths {
    fn default() -> Self {
        Self {
            root: DEFAULT_USB_MEDIA_ROOT.into(),
            document_root: DEFAULT_DOCUMENT_ROOT.into(),
            configfs_root: DEFAULT_CONFIGFS_ROOT.into(),
            udc_root: DEFAULT_UDC_ROOT.into(),
            loop_control: DEFAULT_LOOP_CONTROL.into(),
            storaged_socket: cp0_appd::DEFAULT_STORAGE_SOCKET.into(),
        }
    }
}

impl UsbMediaPaths {
    fn image(&self) -> PathBuf {
        self.root.join(EXCHANGE_IMAGE_NAME)
    }

    fn mount(&self) -> PathBuf {
        self.root.join("mnt")
    }

    fn gadget(&self) -> PathBuf {
        self.configfs_root.join(GADGET_NAME)
    }
}

#[derive(Debug)]
pub enum UsbMediaError {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidState(&'static str),
    InvalidBacking,
    Unavailable(&'static str),
    Storage,
    Filesystem(&'static str),
    Gadget(&'static str),
}

impl fmt::Display for UsbMediaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "USB media I/O error: {error}"),
            Self::Json(error) => write!(formatter, "USB media JSON error: {error}"),
            Self::InvalidState(message) => write!(formatter, "invalid USB media state: {message}"),
            Self::InvalidBacking => formatter.write_str(
                "MSC backing must be the non-symbolic regular exchange image inside the isolated USB media directory",
            ),
            Self::Unavailable(message) => write!(formatter, "USB media unavailable: {message}"),
            Self::Storage => formatter.write_str("photo or document storage is unavailable"),
            Self::Filesystem(message) => write!(formatter, "exchange filesystem error: {message}"),
            Self::Gadget(message) => write!(formatter, "USB gadget error: {message}"),
        }
    }
}

impl std::error::Error for UsbMediaError {}

impl From<io::Error> for UsbMediaError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for UsbMediaError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone, Serialize)]
struct ExportManifest {
    schema_version: u32,
    complete: bool,
    photos: Vec<ExportedPhoto>,
}

#[derive(Debug, Clone, Serialize)]
struct ExportedPhoto {
    id: u64,
    source: &'static str,
    file: String,
    width: u16,
    height: u16,
    size_bytes: u64,
    captured_milliseconds: Option<u64>,
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
struct ImportReport {
    schema_version: u32,
    imported: Vec<ImportedMusic>,
    rejected: Vec<RejectedMusic>,
}

#[derive(Debug, Clone, Serialize)]
struct ImportedMusic {
    source: String,
    document: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
struct RejectedMusic {
    source: String,
    reason: &'static str,
}

#[derive(Debug)]
pub struct UsbMediaService {
    paths: UsbMediaPaths,
    status: UsbMediaStatus,
}

impl UsbMediaService {
    pub fn new(paths: UsbMediaPaths) -> Self {
        Self {
            paths,
            status: UsbMediaStatus::default(),
        }
    }

    pub fn recover_status(&mut self) {
        if gadget_is_bound(&self.paths.gadget()).unwrap_or(false)
            && validate_backing_image(&self.paths).is_ok()
        {
            self.status = UsbMediaStatus {
                state: UsbMediaState::Connected,
                capacity_bytes: EXCHANGE_CAPACITY_BYTES,
                ..UsbMediaStatus::default()
            };
        }
    }

    pub fn dispatch(&mut self, request: UsbMediaRequest) -> UsbMediaResponse {
        let request_id = request.request_id;
        let result = match request.command {
            UsbMediaCommand::GetStatus {} => Ok(self.status.clone()),
            UsbMediaCommand::Start {} => self.start(),
            UsbMediaCommand::Stop {} => self.stop(),
        };
        result.map_or_else(
            |error| {
                eprintln!("cp0-usb-mediad: request {request_id} failed: {error}");
                UsbMediaResponse::error(request_id, error_code(&error), public_error(&error))
            },
            |status| UsbMediaResponse::state(request_id, status),
        )
    }

    fn start(&mut self) -> Result<UsbMediaStatus, UsbMediaError> {
        if self.status.state == UsbMediaState::Connected {
            return Err(UsbMediaError::InvalidState("transfer is already connected"));
        }
        self.status = UsbMediaStatus {
            state: UsbMediaState::Preparing,
            capacity_bytes: EXCHANGE_CAPACITY_BYTES,
            ..UsbMediaStatus::default()
        };
        let result = self.prepare_and_bind();
        match result {
            Ok((exported, imported, rejected)) => {
                self.status.state = UsbMediaState::Connected;
                self.status.exported_photos = exported;
                self.status.imported_music = imported;
                self.status.rejected_music = rejected;
                Ok(self.status.clone())
            }
            Err(error) => {
                let _ = self.cleanup_mount();
                let _ = unbind_gadget(&self.paths);
                self.status.state = UsbMediaState::Error;
                Err(error)
            }
        }
    }

    fn stop(&mut self) -> Result<UsbMediaStatus, UsbMediaError> {
        if self.status.state != UsbMediaState::Connected
            && !gadget_is_bound(&self.paths.gadget()).unwrap_or(false)
        {
            return Err(UsbMediaError::InvalidState("transfer is not connected"));
        }
        self.status.state = UsbMediaState::Importing;
        let result = (|| {
            unbind_gadget(&self.paths)?;
            validate_backing_image(&self.paths)?;
            check_filesystem(&self.paths.image())?;
            mount_exchange(&self.paths)?;
            let report = import_music(&self.paths)?;
            write_import_report(&self.paths.mount(), &report)?;
            unmount_exchange(&self.paths)?;
            check_filesystem(&self.paths.image())?;
            Ok::<_, UsbMediaError>(report)
        })();
        match result {
            Ok(report) => {
                self.status.state = UsbMediaState::Complete;
                self.status.imported_music = self
                    .status
                    .imported_music
                    .saturating_add(report.imported.len() as u32);
                self.status.rejected_music = self
                    .status
                    .rejected_music
                    .saturating_add(report.rejected.len() as u32);
                Ok(self.status.clone())
            }
            Err(error) => {
                let _ = self.cleanup_mount();
                let _ = unbind_gadget(&self.paths);
                self.status.state = UsbMediaState::Error;
                Err(error)
            }
        }
    }

    fn prepare_and_bind(&self) -> Result<(u32, u32, u32), UsbMediaError> {
        preflight_hardware(&self.paths)?;
        ensure_exchange_root(&self.paths)?;
        let mut recovered = ImportReport {
            schema_version: 1,
            imported: Vec::new(),
            rejected: Vec::new(),
        };
        if self.paths.image().exists() {
            validate_backing_image(&self.paths)?;
            check_filesystem(&self.paths.image())?;
            mount_exchange(&self.paths)?;
            recovered = import_music(&self.paths)?;
            unmount_exchange(&self.paths)?;
        } else {
            create_backing_image(&self.paths)?;
        }

        format_exchange(&self.paths.image())?;
        mount_exchange(&self.paths)?;
        let exported = stage_exchange(&self.paths)?;
        unmount_exchange(&self.paths)?;
        check_filesystem(&self.paths.image())?;
        validate_backing_image(&self.paths)?;
        bind_gadget(&self.paths)?;
        Ok((
            exported,
            recovered.imported.len() as u32,
            recovered.rejected.len() as u32,
        ))
    }

    fn cleanup_mount(&self) -> Result<(), UsbMediaError> {
        if is_mountpoint(&self.paths.mount())? {
            unmount_exchange(&self.paths)?;
        }
        Ok(())
    }

    pub fn emergency_stop(&self) -> Result<(), UsbMediaError> {
        unbind_gadget(&self.paths)?;
        self.cleanup_mount()?;
        if self.paths.image().exists() {
            validate_backing_image(&self.paths)?;
            check_filesystem(&self.paths.image())?;
        }
        Ok(())
    }
}

fn preflight_hardware(paths: &UsbMediaPaths) -> Result<(), UsbMediaError> {
    if !paths.configfs_root.is_dir() {
        return Err(UsbMediaError::Gadget("USB gadget ConfigFS is unavailable"));
    }
    first_udc(&paths.udc_root)?;
    let loop_metadata = fs::metadata(&paths.loop_control)
        .map_err(|_| UsbMediaError::Unavailable("loop device support is unavailable"))?;
    if !loop_metadata.file_type().is_char_device() {
        return Err(UsbMediaError::Unavailable(
            "loop device support is unavailable",
        ));
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(&paths.loop_control)
        .map_err(|_| UsbMediaError::Unavailable("loop device access is unavailable"))?;
    Ok(())
}

pub struct UsbMediaServer {
    service: UsbMediaService,
    shell_uid: u32,
}

impl UsbMediaServer {
    pub fn new(service: UsbMediaService, shell_uid: u32) -> Self {
        Self { service, shell_uid }
    }

    pub fn serve(mut self, listener: UnixListener) -> io::Result<()> {
        loop {
            let (stream, _) = listener.accept()?;
            if let Err(error) = self.handle_connection(stream) {
                eprintln!("cp0-usb-mediad: rejected connection: {error}");
            }
        }
    }

    fn handle_connection(&mut self, mut stream: UnixStream) -> io::Result<()> {
        stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
        let uid = peer_uid(&stream)?;
        let request = match read_request(&mut BufReader::new(stream.try_clone()?)) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(_) => {
                return write_response(
                    &mut stream,
                    &UsbMediaResponse::error(
                        0,
                        UsbMediaErrorCode::InvalidRequest,
                        "invalid USB media request",
                    ),
                )
                .map_err(protocol_io);
            }
        };
        let response = if uid == self.shell_uid {
            self.service.dispatch(request)
        } else {
            UsbMediaResponse::error(
                request.request_id,
                UsbMediaErrorCode::Unauthorized,
                "peer UID is not authorized for USB media transfer",
            )
        };
        write_response(&mut stream, &response).map_err(protocol_io)
    }
}

fn ensure_exchange_root(paths: &UsbMediaPaths) -> Result<(), UsbMediaError> {
    if !paths.root.is_absolute() || !paths.document_root.is_absolute() {
        return Err(UsbMediaError::InvalidBacking);
    }
    fs::create_dir_all(&paths.root)?;
    let metadata = fs::symlink_metadata(&paths.root)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(UsbMediaError::InvalidBacking);
    }
    fs::set_permissions(&paths.root, fs::Permissions::from_mode(0o700))?;
    let mount = paths.mount();
    fs::create_dir_all(&mount)?;
    let mount_metadata = fs::symlink_metadata(&mount)?;
    if !mount_metadata.file_type().is_dir() || mount_metadata.file_type().is_symlink() {
        return Err(UsbMediaError::InvalidBacking);
    }
    Ok(())
}

pub fn validate_backing_image(paths: &UsbMediaPaths) -> Result<PathBuf, UsbMediaError> {
    let expected = paths.image();
    if !paths.root.is_absolute() || expected.file_name() != Some(OsStr::new(EXCHANGE_IMAGE_NAME)) {
        return Err(UsbMediaError::InvalidBacking);
    }
    let root_metadata =
        fs::symlink_metadata(&paths.root).map_err(|_| UsbMediaError::InvalidBacking)?;
    let image_metadata =
        fs::symlink_metadata(&expected).map_err(|_| UsbMediaError::InvalidBacking)?;
    if !root_metadata.file_type().is_dir()
        || root_metadata.file_type().is_symlink()
        || !image_metadata.file_type().is_file()
        || image_metadata.file_type().is_symlink()
        || image_metadata.file_type().is_block_device()
        || image_metadata.len() != EXCHANGE_CAPACITY_BYTES
    {
        return Err(UsbMediaError::InvalidBacking);
    }
    let canonical_root =
        fs::canonicalize(&paths.root).map_err(|_| UsbMediaError::InvalidBacking)?;
    let canonical_image = fs::canonicalize(&expected).map_err(|_| UsbMediaError::InvalidBacking)?;
    if canonical_image != canonical_root.join(EXCHANGE_IMAGE_NAME)
        || !canonical_image.starts_with(&canonical_root)
    {
        return Err(UsbMediaError::InvalidBacking);
    }
    Ok(canonical_image)
}

fn create_backing_image(paths: &UsbMediaPaths) -> Result<(), UsbMediaError> {
    ensure_exchange_root(paths)?;
    let image = paths.image();
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&image)
        .map_err(|error| {
            if matches!(error.raw_os_error(), Some(libc::ELOOP) | Some(libc::EEXIST)) {
                UsbMediaError::InvalidBacking
            } else {
                UsbMediaError::Io(error)
            }
        })?;
    #[cfg(target_os = "linux")]
    {
        let result = unsafe {
            libc::fallocate(
                file.as_raw_fd(),
                0,
                0,
                EXCHANGE_CAPACITY_BYTES as libc::off_t,
            )
        };
        if result != 0 {
            return Err(UsbMediaError::Io(io::Error::last_os_error()));
        }
    }
    #[cfg(not(target_os = "linux"))]
    file.set_len(EXCHANGE_CAPACITY_BYTES)?;
    file.sync_all()?;
    validate_backing_image(paths)?;
    Ok(())
}

fn format_exchange(image: &Path) -> Result<(), UsbMediaError> {
    let status = run_command(
        "/usr/sbin/mkfs.vfat",
        ["-F", "32", "-n", EXCHANGE_LABEL, path_text(image)?],
    )?;
    if !status.success() {
        return Err(UsbMediaError::Filesystem("FAT32 formatting failed"));
    }
    Ok(())
}

fn check_filesystem(image: &Path) -> Result<(), UsbMediaError> {
    let status = run_command("/usr/sbin/fsck.vfat", ["-a", path_text(image)?])?;
    if !matches!(status.code(), Some(0) | Some(1)) {
        return Err(UsbMediaError::Filesystem(
            "FAT32 consistency check did not complete cleanly",
        ));
    }
    Ok(())
}

fn mount_exchange(paths: &UsbMediaPaths) -> Result<(), UsbMediaError> {
    validate_backing_image(paths)?;
    if is_mountpoint(&paths.mount())? {
        return Err(UsbMediaError::InvalidState(
            "exchange image is already mounted",
        ));
    }
    let image_path = paths.image();
    let mount_path = paths.mount();
    let image = path_text(&image_path)?;
    let mount = path_text(&mount_path)?;
    let status = run_command(
        "/usr/bin/mount",
        [
            "-t",
            "vfat",
            "-o",
            "loop,nodev,nosuid,noexec,flush,utf8,umask=0077",
            image,
            mount,
        ],
    )?;
    if !status.success() || !is_mountpoint(&paths.mount())? {
        return Err(UsbMediaError::Filesystem("exchange image mount failed"));
    }
    Ok(())
}

fn unmount_exchange(paths: &UsbMediaPaths) -> Result<(), UsbMediaError> {
    if !is_mountpoint(&paths.mount())? {
        return Ok(());
    }
    let status = run_command("/usr/bin/umount", [path_text(&paths.mount())?])?;
    if !status.success() || is_mountpoint(&paths.mount())? {
        return Err(UsbMediaError::Filesystem("exchange image unmount failed"));
    }
    Ok(())
}

fn is_mountpoint(path: &Path) -> Result<bool, UsbMediaError> {
    let escaped = path_text(path)?
        .replace('\\', "\\134")
        .replace(' ', "\\040")
        .replace('\t', "\\011")
        .replace('\n', "\\012");
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
    Ok(mountinfo
        .lines()
        .any(|line| line.split_whitespace().nth(4) == Some(escaped.as_str())))
}

fn stage_exchange(paths: &UsbMediaPaths) -> Result<u32, UsbMediaError> {
    let mount = paths.mount();
    if !is_mountpoint(&mount)? {
        return Err(UsbMediaError::InvalidState("exchange image is not mounted"));
    }
    let photos_root = mount.join("PHOTOS");
    let music_root = mount.join("MUSIC");
    let import_root = music_root.join("IMPORT");
    fs::create_dir(&photos_root)?;
    fs::create_dir(&music_root)?;
    fs::create_dir(&import_root)?;

    write_new_file(
        &mount.join("README.TXT"),
        b"CardputerZero isolated media exchange\r\n\r\nCopy WAV files into MUSIC/IMPORT, then eject and stop USB Media Transfer on the device.\r\nPHOTOS contains export copies; deleting them does not delete originals on the device.\r\n",
    )?;

    let storage = StorageClient::new(&paths.storaged_socket);
    let photos = list_photo_exports(&storage, 1).map_err(|_| UsbMediaError::Storage)?;
    let mut manifest = ExportManifest {
        schema_version: 1,
        complete: false,
        photos: Vec::with_capacity(photos.len()),
    };
    for photo in photos {
        manifest
            .photos
            .push(export_photo(&storage, &photos_root, &photo)?);
    }
    manifest.complete = true;
    let mut encoded = serde_json::to_vec_pretty(&manifest)?;
    encoded.push(b'\n');
    write_new_file(&mount.join("manifest.json"), &encoded)?;
    File::open(&mount)?.sync_all()?;
    Ok(manifest.photos.len() as u32)
}

fn export_photo(
    storage: &StorageClient,
    photos_root: &Path,
    photo: &PhotoExport,
) -> Result<ExportedPhoto, UsbMediaError> {
    let bytes = read_photo_export(storage, photo.id.saturating_add(2), photo)
        .map_err(|_| UsbMediaError::Storage)?;
    let (source, name, exported) = match photo.kind {
        PhotoExportKind::CameraJpeg => ("camera", format!("IMG_{:016}.JPG", photo.id), bytes),
        PhotoExportKind::ScreenshotRgb565 => (
            "screenshot",
            format!("SCREEN_{:016}.BMP", photo.id),
            encode_rgb565_bmp(photo.width, photo.height, &bytes)?,
        ),
    };
    let sha256 = digest_hex(&exported);
    let size_bytes = exported.len() as u64;
    write_new_file(&photos_root.join(&name), &exported)?;
    Ok(ExportedPhoto {
        id: photo.id,
        source,
        file: format!("PHOTOS/{name}"),
        width: photo.width,
        height: photo.height,
        size_bytes,
        captured_milliseconds: photo.captured_milliseconds,
        sha256,
    })
}

fn encode_rgb565_bmp(width: u16, height: u16, pixels: &[u8]) -> Result<Vec<u8>, UsbMediaError> {
    let row_bytes = usize::from(width) * 2;
    let stride = (row_bytes + 3) & !3;
    let pixel_bytes = stride
        .checked_mul(usize::from(height))
        .ok_or(UsbMediaError::Filesystem("screenshot is too large"))?;
    if pixels.len() != row_bytes * usize::from(height) {
        return Err(UsbMediaError::Storage);
    }
    let offset = 66_usize;
    let file_bytes = offset + pixel_bytes;
    let mut bmp = vec![0_u8; file_bytes];
    bmp[..2].copy_from_slice(b"BM");
    bmp[2..6].copy_from_slice(&(file_bytes as u32).to_le_bytes());
    bmp[10..14].copy_from_slice(&(offset as u32).to_le_bytes());
    bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&u32::from(width).to_le_bytes());
    bmp[22..26].copy_from_slice(&u32::from(height).to_le_bytes());
    bmp[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&16_u16.to_le_bytes());
    bmp[30..34].copy_from_slice(&3_u32.to_le_bytes());
    bmp[34..38].copy_from_slice(&(pixel_bytes as u32).to_le_bytes());
    bmp[54..58].copy_from_slice(&0x0000_f800_u32.to_le_bytes());
    bmp[58..62].copy_from_slice(&0x0000_07e0_u32.to_le_bytes());
    bmp[62..66].copy_from_slice(&0x0000_001f_u32.to_le_bytes());
    for destination_row in 0..usize::from(height) {
        let source_row = usize::from(height) - destination_row - 1;
        let source = &pixels[source_row * row_bytes..(source_row + 1) * row_bytes];
        let start = offset + destination_row * stride;
        bmp[start..start + row_bytes].copy_from_slice(source);
    }
    Ok(bmp)
}

fn import_music(paths: &UsbMediaPaths) -> Result<ImportReport, UsbMediaError> {
    let mut report = ImportReport {
        schema_version: 1,
        imported: Vec::new(),
        rejected: Vec::new(),
    };
    let import_root = paths.mount().join("MUSIC/IMPORT");
    if !import_root.exists() {
        return Ok(report);
    }
    let metadata = fs::symlink_metadata(&import_root)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(UsbMediaError::Filesystem(
            "music import directory is invalid",
        ));
    }
    validate_document_root(&paths.document_root)?;
    let (document_uid, document_gid) = lookup_user(c"cp0-document")?;
    let mut used_names = document_names(&paths.document_root)?;
    let mut entries = fs::read_dir(&import_root)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries.into_iter().take(MAX_MUSIC_IMPORTS) {
        let Some(source_name) = entry.file_name().into_string().ok() else {
            report.rejected.push(RejectedMusic {
                source: "INVALID-NAME".into(),
                reason: "invalid-name",
            });
            continue;
        };
        if used_names.len() >= MAX_DOCUMENTS {
            report.rejected.push(RejectedMusic {
                source: source_name,
                reason: "document-limit",
            });
            continue;
        }
        if !valid_wav_name(&source_name) {
            report.rejected.push(RejectedMusic {
                source: source_name,
                reason: "unsupported-name",
            });
            continue;
        }
        let source_path = entry.path();
        let mut source = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&source_path)
        {
            Ok(file) => file,
            Err(_) => {
                report.rejected.push(RejectedMusic {
                    source: source_name,
                    reason: "invalid-file",
                });
                continue;
            }
        };
        let before = source.metadata()?;
        if !before.file_type().is_file()
            || before.len() == 0
            || before.len() > MAX_DOCUMENT_BYTES
            || validate_wav(&mut source, before.len()).is_err()
        {
            report.rejected.push(RejectedMusic {
                source: source_name,
                reason: "unsupported-wav",
            });
            continue;
        }
        let document_name = choose_document_name(&source_name, &used_names)?;
        match publish_document(
            &mut source,
            &before,
            &source_path,
            &paths.document_root,
            &document_name,
            document_uid,
            document_gid,
        ) {
            Ok(sha256) => {
                used_names.insert(document_name.clone());
                report.imported.push(ImportedMusic {
                    source: source_name,
                    document: document_name,
                    size_bytes: before.len(),
                    sha256,
                });
            }
            Err(_) => report.rejected.push(RejectedMusic {
                source: source_name,
                reason: "publish-failed",
            }),
        }
    }
    Ok(report)
}

fn validate_document_root(root: &Path) -> Result<(), UsbMediaError> {
    if !root.is_absolute() {
        return Err(UsbMediaError::Storage);
    }
    let metadata = fs::symlink_metadata(root).map_err(|_| UsbMediaError::Storage)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(UsbMediaError::Storage);
    }
    Ok(())
}

fn document_names(root: &Path) -> Result<BTreeSet<String>, UsbMediaError> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let Some(name) = entry.file_name().into_string().ok() else {
            continue;
        };
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_file() && is_valid_document_name(&name) {
            names.insert(name);
        }
    }
    Ok(names)
}

fn valid_wav_name(name: &str) -> bool {
    is_valid_document_name(name)
        && name != "."
        && name != ".."
        && name
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("wav"))
        && !name.contains('\\')
}

fn choose_document_name(source: &str, used: &BTreeSet<String>) -> Result<String, UsbMediaError> {
    if !used.contains(source) {
        return Ok(source.into());
    }
    let (stem, extension) = source.rsplit_once('.').ok_or(UsbMediaError::Storage)?;
    for suffix in 1..=9999 {
        let candidate = format!("{stem} ({suffix}).{extension}");
        if is_valid_document_name(&candidate) && !used.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(UsbMediaError::Storage)
}

fn validate_wav(file: &mut File, file_bytes: u64) -> Result<(), UsbMediaError> {
    file.seek(SeekFrom::Start(0))?;
    let mut header = vec![0_u8; WAV_HEADER_BYTES.min(file_bytes as usize)];
    file.read_exact(&mut header)?;
    file.seek(SeekFrom::Start(0))?;
    if header.len() < 44 || &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(UsbMediaError::Storage);
    }
    let declared = u32::from_le_bytes(header[4..8].try_into().unwrap()) as u64 + 8;
    if declared > file_bytes {
        return Err(UsbMediaError::Storage);
    }
    let mut cursor = 12_usize;
    let mut format_valid = false;
    while cursor.checked_add(8).is_some_and(|end| end <= header.len()) {
        let chunk = &header[cursor..cursor + 4];
        let size = u32::from_le_bytes(header[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        let data = cursor + 8;
        if chunk == b"fmt " {
            if size < 16 || data.checked_add(16).is_none_or(|end| end > header.len()) {
                return Err(UsbMediaError::Storage);
            }
            format_valid = u16::from_le_bytes(header[data..data + 2].try_into().unwrap()) == 1
                && u16::from_le_bytes(header[data + 2..data + 4].try_into().unwrap()) == 2
                && u32::from_le_bytes(header[data + 4..data + 8].try_into().unwrap()) == 48_000
                && u32::from_le_bytes(header[data + 8..data + 12].try_into().unwrap()) == 192_000
                && u16::from_le_bytes(header[data + 12..data + 14].try_into().unwrap()) == 4
                && u16::from_le_bytes(header[data + 14..data + 16].try_into().unwrap()) == 16;
        } else if chunk == b"data" {
            let data_end = (data as u64)
                .checked_add(size as u64)
                .ok_or(UsbMediaError::Storage)?;
            if !format_valid || size == 0 || size % 4 != 0 || data_end > file_bytes {
                return Err(UsbMediaError::Storage);
            }
            return Ok(());
        }
        cursor = data
            .checked_add(size)
            .and_then(|next| next.checked_add(size & 1))
            .ok_or(UsbMediaError::Storage)?;
    }
    Err(UsbMediaError::Storage)
}

fn publish_document(
    source: &mut File,
    before: &fs::Metadata,
    source_path: &Path,
    document_root: &Path,
    document_name: &str,
    document_uid: u32,
    document_gid: u32,
) -> Result<String, UsbMediaError> {
    source.seek(SeekFrom::Start(0))?;
    let temporary_name = format!(
        ".cp0-usb-import-{}-{}.tmp",
        std::process::id(),
        before.ino()
    );
    let temporary_path = document_root.join(&temporary_name);
    let destination = document_root.join(document_name);
    let mut temporary = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temporary_path)?;
    let copy_result = (|| {
        let mut digest = Sha256::new();
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            temporary.write_all(&buffer[..count])?;
            digest.update(&buffer[..count]);
            copied = copied.saturating_add(count as u64);
        }
        let after = source.metadata()?;
        if copied != before.len()
            || after.len() != before.len()
            || after.dev() != before.dev()
            || after.ino() != before.ino()
            || after.mtime() != before.mtime()
            || after.mtime_nsec() != before.mtime_nsec()
        {
            return Err(UsbMediaError::Storage);
        }
        temporary.flush()?;
        let chown = unsafe {
            libc::fchown(
                temporary.as_raw_fd(),
                document_uid as libc::uid_t,
                document_gid as libc::gid_t,
            )
        };
        if chown != 0 {
            return Err(UsbMediaError::Io(io::Error::last_os_error()));
        }
        temporary.set_permissions(fs::Permissions::from_mode(0o640))?;
        temporary.sync_all()?;
        fs::hard_link(&temporary_path, &destination)?;
        fs::remove_file(&temporary_path)?;
        File::open(document_root)?.sync_all()?;
        fs::remove_file(source_path)?;
        Ok(digest_hex(&digest.finalize()))
    })();
    if copy_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    copy_result
}

fn write_import_report(mount: &Path, report: &ImportReport) -> Result<(), UsbMediaError> {
    let path = mount.join("MUSIC/IMPORT-RESULTS.JSON");
    let metadata = fs::symlink_metadata(&path).ok();
    if metadata.as_ref().is_some_and(|metadata| {
        !metadata.file_type().is_file() || metadata.file_type().is_symlink()
    }) {
        return Err(UsbMediaError::Filesystem("import report path is invalid"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    serde_json::to_writer_pretty(&mut file, report)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn bind_gadget(paths: &UsbMediaPaths) -> Result<(), UsbMediaError> {
    validate_backing_image(paths)?;
    if !paths.configfs_root.is_dir() {
        return Err(UsbMediaError::Gadget("USB gadget ConfigFS is unavailable"));
    }
    unbind_gadget(paths)?;
    let gadget = paths.gadget();
    let result = (|| {
        fs::create_dir(&gadget)?;
        write_attribute(&gadget.join("idVendor"), b"0x1d6b")?;
        write_attribute(&gadget.join("idProduct"), b"0x0104")?;
        write_attribute(&gadget.join("bcdDevice"), b"0x0100")?;
        write_attribute(&gadget.join("bcdUSB"), b"0x0200")?;

        let strings = gadget.join("strings/0x409");
        fs::create_dir_all(&strings)?;
        write_attribute(&strings.join("serialnumber"), b"CP0-MEDIA")?;
        write_attribute(&strings.join("manufacturer"), b"CardputerZero")?;
        write_attribute(&strings.join("product"), b"Isolated Media Exchange")?;

        let config = gadget.join("configs/c.1");
        fs::create_dir_all(config.join("strings/0x409"))?;
        write_attribute(&config.join("MaxPower"), b"250")?;
        write_attribute(&config.join("bmAttributes"), b"0x80")?;
        write_attribute(
            &config.join("strings/0x409/configuration"),
            b"Media Transfer",
        )?;

        let function = gadget.join("functions/mass_storage.0");
        fs::create_dir_all(&function)?;
        write_attribute(&function.join("stall"), b"1")?;
        write_attribute(&function.join("lun.0/cdrom"), b"0")?;
        write_attribute(&function.join("lun.0/ro"), b"0")?;
        write_attribute(&function.join("lun.0/removable"), b"1")?;
        let canonical_image = validate_backing_image(paths)?;
        write_attribute(
            &function.join("lun.0/file"),
            canonical_image.as_os_str().as_bytes(),
        )?;
        std::os::unix::fs::symlink(&function, config.join("mass_storage.0"))?;

        let udc = first_udc(&paths.udc_root)?;
        write_attribute(&gadget.join("UDC"), udc.as_os_str().as_bytes())?;
        if !gadget_is_bound(&gadget)? {
            return Err(UsbMediaError::Gadget("USB device controller did not bind"));
        }
        Ok::<_, UsbMediaError>(())
    })();
    if result.is_err() {
        let _ = unbind_gadget(paths);
    }
    result
}

fn unbind_gadget(paths: &UsbMediaPaths) -> Result<(), UsbMediaError> {
    let gadget = paths.gadget();
    if !gadget.exists() {
        return Ok(());
    }
    if !gadget.starts_with(&paths.configfs_root)
        || gadget.file_name() != Some(OsStr::new(GADGET_NAME))
    {
        return Err(UsbMediaError::Gadget(
            "refusing to remove an unknown gadget",
        ));
    }
    let udc = gadget.join("UDC");
    if udc.exists() {
        write_attribute(&udc, b"")?;
    }
    let link = gadget.join("configs/c.1/mass_storage.0");
    if fs::symlink_metadata(&link).is_ok() {
        fs::remove_file(&link)?;
    }
    let lun_file = gadget.join("functions/mass_storage.0/lun.0/file");
    if lun_file.exists() {
        write_attribute(&lun_file, b"")?;
    }
    for directory in [
        "functions/mass_storage.0",
        "configs/c.1/strings/0x409",
        "configs/c.1",
        "strings/0x409",
    ] {
        let path = gadget.join(directory);
        if path.exists() {
            fs::remove_dir(path)?;
        }
    }
    fs::remove_dir(&gadget)?;
    Ok(())
}

fn gadget_is_bound(gadget: &Path) -> Result<bool, UsbMediaError> {
    let path = gadget.join("UDC");
    if !path.exists() {
        return Ok(false);
    }
    Ok(!fs::read_to_string(path)?.trim().is_empty())
}

fn first_udc(root: &Path) -> Result<PathBuf, UsbMediaError> {
    let mut devices = fs::read_dir(root)
        .map_err(|_| UsbMediaError::Unavailable("USB device controller is unavailable"))?
        .collect::<Result<Vec<_>, _>>()?;
    devices.sort_by_key(|entry| entry.file_name());
    devices
        .first()
        .map(|entry| PathBuf::from(entry.file_name()))
        .ok_or(UsbMediaError::Unavailable(
            "USB device controller is unavailable",
        ))
}

fn write_attribute(path: &Path, value: &[u8]) -> Result<(), UsbMediaError> {
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.write_all(value)?;
    file.flush()?;
    Ok(())
}

fn write_new_file(path: &Path, contents: &[u8]) -> Result<(), UsbMediaError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

fn digest_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        write!(&mut output, "{byte:02x}")
            .expect("writing hexadecimal digest into String cannot fail");
    }
    output
}

fn lookup_user(name: &CStr) -> Result<(u32, u32), UsbMediaError> {
    let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let mut buffer = [0_u8; 16 * 1024];
    let status = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 {
        return Err(UsbMediaError::Io(io::Error::from_raw_os_error(status)));
    }
    if result.is_null() {
        return Err(UsbMediaError::Storage);
    }
    let record = unsafe { record.assume_init() };
    Ok((record.pw_uid, record.pw_gid))
}

fn run_command<const N: usize>(
    program: &str,
    arguments: [&str; N],
) -> Result<ExitStatus, UsbMediaError> {
    Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .status()
        .map_err(|_| UsbMediaError::Unavailable("required media utility is unavailable"))
}

fn path_text(path: &Path) -> Result<&str, UsbMediaError> {
    path.to_str().ok_or(UsbMediaError::InvalidBacking)
}

fn error_code(error: &UsbMediaError) -> UsbMediaErrorCode {
    match error {
        UsbMediaError::InvalidState(_) => UsbMediaErrorCode::InvalidState,
        UsbMediaError::Unavailable(_) => UsbMediaErrorCode::Unavailable,
        UsbMediaError::Storage => UsbMediaErrorCode::Storage,
        UsbMediaError::InvalidBacking | UsbMediaError::Filesystem(_) => {
            UsbMediaErrorCode::Filesystem
        }
        UsbMediaError::Gadget(_) => UsbMediaErrorCode::Gadget,
        UsbMediaError::Io(_) | UsbMediaError::Json(_) => UsbMediaErrorCode::Internal,
    }
}

fn public_error(error: &UsbMediaError) -> &'static str {
    match error {
        UsbMediaError::InvalidState(message)
        | UsbMediaError::Unavailable(message)
        | UsbMediaError::Filesystem(message)
        | UsbMediaError::Gadget(message) => message,
        UsbMediaError::Storage => "photo or music storage is unavailable",
        UsbMediaError::InvalidBacking => "isolated USB exchange storage failed validation",
        UsbMediaError::Io(_) | UsbMediaError::Json(_) => "USB media transfer failed",
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
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(credentials.uid)
}

#[cfg(not(target_os = "linux"))]
fn peer_uid(_stream: &UnixStream) -> io::Result<u32> {
    Ok(0)
}

fn protocol_io(error: UsbMediaProtocolError) -> io::Error {
    match error {
        UsbMediaProtocolError::Io(error) => error,
        error => io::Error::new(io::ErrorKind::InvalidData, error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        PathBuf::from("target/test-tmp").join(format!(
            "cp0-usb-media-{name}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ))
    }

    #[test]
    fn backing_validation_rejects_symlinks_and_wrong_sizes() {
        let root = temp_root("backing");
        fs::create_dir_all(&root).unwrap();
        let outside = root.with_extension("outside");
        File::create(&outside)
            .unwrap()
            .set_len(EXCHANGE_CAPACITY_BYTES)
            .unwrap();
        std::os::unix::fs::symlink(&outside, root.join(EXCHANGE_IMAGE_NAME)).unwrap();
        let paths = UsbMediaPaths {
            root: root.clone(),
            ..UsbMediaPaths::default()
        };
        assert!(matches!(
            validate_backing_image(&paths),
            Err(UsbMediaError::InvalidBacking)
        ));
        fs::remove_file(root.join(EXCHANGE_IMAGE_NAME)).unwrap();
        File::create(root.join(EXCHANGE_IMAGE_NAME))
            .unwrap()
            .set_len(4096)
            .unwrap();
        assert!(matches!(
            validate_backing_image(&paths),
            Err(UsbMediaError::InvalidBacking)
        ));
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn preflight_reports_missing_configfs_before_allocating_storage() {
        let root = temp_root("preflight");
        let paths = UsbMediaPaths {
            root: root.clone(),
            configfs_root: root.join("missing-configfs"),
            ..UsbMediaPaths::default()
        };
        assert!(matches!(
            preflight_hardware(&paths),
            Err(UsbMediaError::Gadget("USB gadget ConfigFS is unavailable"))
        ));
        assert!(!paths.image().exists());
    }

    #[test]
    fn rgb565_bmp_is_bottom_up_and_has_bitmasks() {
        let pixels = [0x00, 0xf8, 0xe0, 0x07, 0x1f, 0x00, 0xff, 0xff];
        let bmp = encode_rgb565_bmp(2, 2, &pixels).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 66);
        assert_eq!(&bmp[66..70], &pixels[4..8]);
        assert_eq!(&bmp[70..74], &pixels[..4]);
    }

    #[test]
    fn wav_validation_matches_music_app_contract() {
        let root = temp_root("wav");
        fs::create_dir_all(root.parent().unwrap()).unwrap();
        let mut bytes = vec![0_u8; 48];
        bytes[..4].copy_from_slice(b"RIFF");
        bytes[4..8].copy_from_slice(&40_u32.to_le_bytes());
        bytes[8..12].copy_from_slice(b"WAVE");
        bytes[12..16].copy_from_slice(b"fmt ");
        bytes[16..20].copy_from_slice(&16_u32.to_le_bytes());
        bytes[20..22].copy_from_slice(&1_u16.to_le_bytes());
        bytes[22..24].copy_from_slice(&2_u16.to_le_bytes());
        bytes[24..28].copy_from_slice(&48_000_u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&192_000_u32.to_le_bytes());
        bytes[32..34].copy_from_slice(&4_u16.to_le_bytes());
        bytes[34..36].copy_from_slice(&16_u16.to_le_bytes());
        bytes[36..40].copy_from_slice(b"data");
        bytes[40..44].copy_from_slice(&4_u32.to_le_bytes());
        fs::write(&root, &bytes).unwrap();
        let mut file = File::open(&root).unwrap();
        assert!(validate_wav(&mut file, bytes.len() as u64).is_ok());
        bytes[24] = 0;
        fs::write(&root, &bytes).unwrap();
        let mut file = File::open(&root).unwrap();
        assert!(validate_wav(&mut file, bytes.len() as u64).is_err());
        fs::remove_file(root).unwrap();
    }

    #[test]
    fn collision_names_never_overwrite_documents() {
        let used = BTreeSet::from(["song.wav".to_owned(), "song (1).wav".to_owned()]);
        assert_eq!(
            choose_document_name("song.wav", &used).unwrap(),
            "song (2).wav"
        );
    }
}
