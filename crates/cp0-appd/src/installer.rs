use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cp0_manifest::AppManifest;
use cp0_package::{CApp, key_id};
use sha2::{Digest, Sha256};

pub const DEFAULT_STORE_TRUST_DIR: &str = "/etc/cardputerzero/trust/store";
pub const DEFAULT_DEVELOPER_TRUST_DIR: &str = "/etc/cardputerzero/trust/developers";
pub const DEFAULT_REVOKED_KEYS_DIR: &str = "/etc/cardputerzero/trust/revoked";
pub const DEVICE_SDK_MAJOR: u32 = 1;
pub const DEVICE_SDK_MINOR: u32 = 1;
pub const LEGACY_SDK_VERSIONS: &[(u32, u32)] = &[(0, 1)];

static INSTALL_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustPaths {
    pub store_keys: PathBuf,
    pub developer_keys: PathBuf,
    pub revoked_keys: PathBuf,
    pub device_policy: PathBuf,
    pub developer_mode: PathBuf,
}

impl Default for TrustPaths {
    fn default() -> Self {
        Self {
            store_keys: PathBuf::from(DEFAULT_STORE_TRUST_DIR),
            developer_keys: PathBuf::from(DEFAULT_DEVELOPER_TRUST_DIR),
            revoked_keys: PathBuf::from(DEFAULT_REVOKED_KEYS_DIR),
            device_policy: PathBuf::from(crate::DEFAULT_DEVICE_POLICY_PATH),
            developer_mode: PathBuf::from(crate::DEFAULT_DEVELOPER_MODE_PATH),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustDecision {
    Store,
    DeveloperMode,
}

#[derive(Debug, Clone)]
pub struct TrustPolicy {
    paths: TrustPaths,
    enforce_root_ownership: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedInstall {
    pub manifest: AppManifest,
    pub package_dir: PathBuf,
    pub newly_extracted: bool,
    pub trust: TrustDecision,
}

#[derive(Debug)]
pub enum InstallError {
    Io(std::io::Error),
    Package(cp0_package::PackageError),
    Manifest(cp0_manifest::ManifestError),
    Invalid(String),
    Untrusted(String),
    AlreadyInstalled(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "application install I/O error: {error}"),
            Self::Package(error) => write!(formatter, "{error}"),
            Self::Manifest(error) => write!(formatter, "invalid package manifest: {error}"),
            Self::Invalid(error) => write!(formatter, "invalid application install: {error}"),
            Self::Untrusted(error) => write!(formatter, "untrusted application package: {error}"),
            Self::AlreadyInstalled(error) => write!(formatter, "install conflict: {error}"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<std::io::Error> for InstallError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<cp0_package::PackageError> for InstallError {
    fn from(error: cp0_package::PackageError) -> Self {
        Self::Package(error)
    }
}

impl From<cp0_manifest::ManifestError> for InstallError {
    fn from(error: cp0_manifest::ManifestError) -> Self {
        Self::Manifest(error)
    }
}

impl TrustPolicy {
    pub fn new(paths: TrustPaths, enforce_root_ownership: bool) -> Self {
        Self {
            paths,
            enforce_root_ownership,
        }
    }

    pub fn verify(&self, package: &CApp) -> Result<TrustDecision, InstallError> {
        package
            .verify_developer_signature()
            .map_err(|error| InstallError::Untrusted(error.to_string()))?;
        let developer_key = package
            .developer_public_key()
            .ok_or_else(|| InstallError::Untrusted("developer public key is missing".into()))?;
        let developer_id = key_id(&developer_key);
        self.reject_revoked(&developer_id, "developer")?;

        if let Some(store_id) = package.store_key_id() {
            self.reject_revoked(&store_id, "store")?;
            let store_key = self.read_trusted_key(&self.paths.store_keys, &store_id, "store")?;
            package
                .verify_store_signature(&store_key)
                .map_err(|error| InstallError::Untrusted(error.to_string()))?;
            return Ok(TrustDecision::Store);
        }

        self.require_developer_mode()?;
        let trusted_developer =
            self.read_trusted_key(&self.paths.developer_keys, &developer_id, "developer")?;
        if trusted_developer != developer_key {
            return Err(InstallError::Untrusted(
                "trusted developer key content does not match its key ID".into(),
            ));
        }
        Ok(TrustDecision::DeveloperMode)
    }

    fn require_developer_mode(&self) -> Result<(), InstallError> {
        let enabled = crate::developer_install_allowed(
            &self.paths.device_policy,
            &self.paths.developer_mode,
            self.enforce_root_ownership,
        )
        .map_err(|error| InstallError::Untrusted(error.to_string()))?;
        if !enabled {
            return Err(InstallError::Untrusted(
                "developer mode is not explicitly enabled".into(),
            ));
        }
        Ok(())
    }

    fn reject_revoked(&self, id: &[u8; 32], role: &str) -> Result<(), InstallError> {
        let path = self.paths.revoked_keys.join(hex(id));
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                validate_secure_metadata(
                    &metadata,
                    self.enforce_root_ownership,
                    "revocation marker",
                )?;
                Err(InstallError::Untrusted(format!(
                    "{role} key {} is revoked",
                    hex(id)
                )))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(InstallError::Io(error)),
        }
    }

    fn read_trusted_key(
        &self,
        directory: &Path,
        id: &[u8; 32],
        role: &str,
    ) -> Result<[u8; 32], InstallError> {
        validate_secure_directory(directory, self.enforce_root_ownership, "trust directory")?;
        let path = directory.join(format!("{}.pub", hex(id)));
        let encoded = read_secure_file(&path, self.enforce_root_ownership, "trusted public key")?;
        let key: [u8; 32] = encoded.try_into().map_err(|value: Vec<u8>| {
            InstallError::Untrusted(format!(
                "trusted {role} public key has {} bytes instead of 32",
                value.len()
            ))
        })?;
        if key_id(&key) != *id {
            return Err(InstallError::Untrusted(format!(
                "trusted {role} key filename does not match its content"
            )));
        }
        Ok(key)
    }
}

#[derive(Debug, Clone)]
pub struct PackageInstaller {
    apps_root: PathBuf,
    trust: TrustPolicy,
    enforce_root_ownership: bool,
}

impl PackageInstaller {
    pub fn new(
        apps_root: impl Into<PathBuf>,
        trust: TrustPolicy,
        enforce_root_ownership: bool,
    ) -> Self {
        Self {
            apps_root: apps_root.into(),
            trust,
            enforce_root_ownership,
        }
    }

    pub fn install(&self, package_path: impl AsRef<Path>) -> Result<PreparedInstall, InstallError> {
        self.install_inner(package_path.as_ref(), IncomingPolicy::Root)
    }

    pub fn install_store(
        &self,
        package_path: impl AsRef<Path>,
        owner_uid: u32,
        expected_app_id: &str,
        expected_version: &str,
        expected_sha256: &[u8; 32],
        expected_bytes: u64,
    ) -> Result<PreparedInstall, InstallError> {
        self.install_inner(
            package_path.as_ref(),
            IncomingPolicy::Store {
                owner_uid,
                expected_app_id,
                expected_version,
                expected_sha256,
                expected_bytes,
            },
        )
    }

    fn install_inner(
        &self,
        package_path: &Path,
        incoming: IncomingPolicy<'_>,
    ) -> Result<PreparedInstall, InstallError> {
        validate_secure_directory(
            &self.apps_root,
            self.enforce_root_ownership,
            "applications root",
        )?;
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(package_path)?;
        let metadata = file.metadata()?;
        validate_secure_metadata(
            &metadata,
            matches!(incoming, IncomingPolicy::Root) && self.enforce_root_ownership,
            "incoming package",
        )?;
        if !metadata.is_file() {
            return Err(InstallError::Invalid(
                "incoming package is not a regular file".into(),
            ));
        }
        if metadata.len() > (cp0_package::MAX_PAYLOAD_BYTES + 4096) as u64 {
            return Err(InstallError::Invalid(
                "incoming package is too large".into(),
            ));
        }
        if let IncomingPolicy::Store {
            owner_uid,
            expected_bytes,
            ..
        } = incoming
        {
            if metadata.uid() != owner_uid {
                return Err(InstallError::Invalid(
                    "store package owner does not match the requesting service".into(),
                ));
            }
            if metadata.len() != expected_bytes {
                return Err(InstallError::Invalid(
                    "store package size does not match the signed catalog".into(),
                ));
            }
        }
        let mut encoded = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut encoded)?;
        if encoded.len() as u64 != metadata.len() {
            return Err(InstallError::Invalid(
                "incoming package changed while being read".into(),
            ));
        }
        if let IncomingPolicy::Store {
            expected_sha256, ..
        } = incoming
        {
            if Sha256::digest(&encoded).as_slice() != expected_sha256 {
                return Err(InstallError::Invalid(
                    "store package hash does not match the signed catalog".into(),
                ));
            }
        }
        let package = CApp::decode(&encoded)?;
        let trust = self.trust.verify(&package)?;
        let manifest = cp0_manifest::parse_and_validate(
            package
                .entry("app.json")
                .ok_or_else(|| InstallError::Invalid("app.json is missing".into()))?,
        )?;
        require_compatible_sdk(&manifest.sdk_version)?;
        if trust == TrustDecision::DeveloperMode && !crate::is_removable_app(&manifest.id) {
            return Err(InstallError::Untrusted(format!(
                "built-in application {} accepts only Store-signed updates",
                manifest.id
            )));
        }
        if let IncomingPolicy::Store {
            expected_app_id,
            expected_version,
            ..
        } = incoming
        {
            if manifest.id != expected_app_id || manifest.version != expected_version {
                return Err(InstallError::Invalid(
                    "store package identity does not match the signed catalog".into(),
                ));
            }
        }
        if package.entry(&manifest.entrypoint).is_none() {
            return Err(InstallError::Invalid(format!(
                "entrypoint {} is missing",
                manifest.entrypoint
            )));
        }

        let app_dir = self.apps_root.join(&manifest.id);
        ensure_secure_directory(&app_dir, self.enforce_root_ownership)?;
        let final_dir = app_dir.join(&manifest.version);
        if final_dir.exists() {
            if directory_matches(&final_dir, &package, self.enforce_root_ownership)? {
                return Ok(PreparedInstall {
                    manifest,
                    package_dir: final_dir,
                    newly_extracted: false,
                    trust,
                });
            }
            return Err(InstallError::AlreadyInstalled(format!(
                "{} {} already exists with different content",
                manifest.id, manifest.version
            )));
        }

        let sequence = INSTALL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let staging = app_dir.join(format!(
            ".install-{}-{}-{sequence}",
            manifest.version,
            std::process::id()
        ));
        DirBuilder::new().mode(0o700).create(&staging)?;
        let extract_result = extract_package(&staging, &package);
        if let Err(error) = extract_result {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o755))?;
        File::open(&staging)?.sync_all()?;
        fs::rename(&staging, &final_dir)?;
        File::open(&app_dir)?.sync_all()?;
        Ok(PreparedInstall {
            manifest,
            package_dir: final_dir,
            newly_extracted: true,
            trust,
        })
    }
}

#[derive(Clone, Copy)]
enum IncomingPolicy<'a> {
    Root,
    Store {
        owner_uid: u32,
        expected_app_id: &'a str,
        expected_version: &'a str,
        expected_sha256: &'a [u8; 32],
        expected_bytes: u64,
    },
}

fn require_compatible_sdk(version: &str) -> Result<(), InstallError> {
    let (major, minor) = version.split_once('.').ok_or_else(|| {
        InstallError::Invalid("SDK version must use canonical <major>.<minor> form".into())
    })?;
    let canonical_component = |component: &str| {
        !component.is_empty()
            && component.bytes().all(|byte| byte.is_ascii_digit())
            && (component == "0" || !component.starts_with('0'))
    };
    if !canonical_component(major) || !canonical_component(minor) {
        return Err(InstallError::Invalid(
            "SDK version must use canonical <major>.<minor> form".into(),
        ));
    }
    let major = major
        .parse::<u32>()
        .map_err(|_| InstallError::Invalid("SDK version major component is out of range".into()))?;
    let minor = minor
        .parse::<u32>()
        .map_err(|_| InstallError::Invalid("SDK version minor component is out of range".into()))?;
    let is_current_line = major == DEVICE_SDK_MAJOR && minor <= DEVICE_SDK_MINOR;
    let is_supported_legacy = LEGACY_SDK_VERSIONS.contains(&(major, minor));
    if !is_current_line && !is_supported_legacy {
        return Err(InstallError::Invalid(format!(
            "SDK {version} is incompatible with device SDK {DEVICE_SDK_MAJOR}.{DEVICE_SDK_MINOR}"
        )));
    }
    Ok(())
}

fn extract_package(root: &Path, package: &CApp) -> Result<(), InstallError> {
    for entry in package.entries() {
        let relative = normalized_relative(&entry.path)?;
        let destination = root.join(&relative);
        let parent = destination
            .parent()
            .ok_or_else(|| InstallError::Invalid("entry has no parent".into()))?;
        create_directories(root, parent)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&destination)?;
        file.write_all(&entry.contents)?;
        file.set_permissions(fs::Permissions::from_mode(0o644))?;
        file.sync_all()?;
    }
    Ok(())
}

fn create_directories(root: &Path, target: &Path) -> Result<(), InstallError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| InstallError::Invalid("entry parent escaped staging directory".into()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(InstallError::Invalid(
                "entry parent is not normalized".into(),
            ));
        };
        current.push(part);
        match DirBuilder::new().mode(0o755).create(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if !fs::symlink_metadata(&current)?.is_dir() {
                    return Err(InstallError::Invalid(
                        "entry parent conflicts with a package file".into(),
                    ));
                }
            }
            Err(error) => return Err(InstallError::Io(error)),
        }
        fs::set_permissions(&current, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn directory_matches(
    root: &Path,
    package: &CApp,
    enforce_root: bool,
) -> Result<bool, InstallError> {
    let mut seen = BTreeSet::new();
    collect_installed_files(root, root, &mut seen, enforce_root)?;
    let expected: BTreeSet<_> = package
        .entries()
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    if seen != expected {
        return Ok(false);
    }
    for entry in package.entries() {
        let path = root.join(normalized_relative(&entry.path)?);
        if fs::read(path)? != entry.contents {
            return Ok(false);
        }
    }
    Ok(true)
}

fn collect_installed_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeSet<String>,
    enforce_root: bool,
) -> Result<(), InstallError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        validate_secure_metadata(&metadata, enforce_root, "installed package entry")?;
        if metadata.file_type().is_symlink() {
            return Err(InstallError::Invalid(
                "installed package contains a symbolic link".into(),
            ));
        }
        if metadata.is_dir() {
            collect_installed_files(root, &entry.path(), files, enforce_root)?;
        } else if metadata.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| InstallError::Invalid("installed entry escaped package".into()))?
                .to_path_buf();
            let path = relative
                .components()
                .map(|component| {
                    component.as_os_str().to_str().ok_or_else(|| {
                        InstallError::Invalid("installed entry path is not UTF-8".into())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("/");
            files.insert(path);
        } else {
            return Err(InstallError::Invalid(
                "installed package contains an unsupported object".into(),
            ));
        }
    }
    Ok(())
}

fn normalized_relative(path: &str) -> Result<PathBuf, InstallError> {
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(InstallError::Invalid(
            "package entry path is not normalized".into(),
        ));
    }
    Ok(path.to_path_buf())
}

fn ensure_secure_directory(path: &Path, enforce_root: bool) -> Result<(), InstallError> {
    match DirBuilder::new().mode(0o755).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(InstallError::Io(error)),
    }
    validate_secure_directory(path, enforce_root, "application directory")?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn validate_secure_directory(
    path: &Path,
    enforce_root: bool,
    name: &str,
) -> Result<(), InstallError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(InstallError::Invalid(format!("{name} is not a directory")));
    }
    validate_secure_metadata(&metadata, enforce_root, name)
}

fn validate_secure_metadata(
    metadata: &fs::Metadata,
    enforce_root: bool,
    name: &str,
) -> Result<(), InstallError> {
    if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
        return Err(InstallError::Invalid(format!(
            "{name} is not a regular file or directory"
        )));
    }
    if metadata.mode() & 0o022 != 0 {
        return Err(InstallError::Invalid(format!(
            "{name} is writable by group or other users"
        )));
    }
    if enforce_root && metadata.uid() != 0 {
        return Err(InstallError::Invalid(format!(
            "{name} is not owned by root"
        )));
    }
    Ok(())
}

fn read_secure_file(path: &Path, enforce_root: bool, name: &str) -> Result<Vec<u8>, InstallError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            InstallError::Untrusted(format!("{name} is missing"))
        } else {
            InstallError::Io(error)
        }
    })?;
    if !metadata.is_file() {
        return Err(InstallError::Untrusted(format!(
            "{name} is not a regular file"
        )));
    }
    validate_secure_metadata(&metadata, enforce_root, name)?;
    Ok(fs::read(path)?)
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp0_manifest::{AppManifest, DisplayMode, ResourceLimits, Runtime, SCHEMA_VERSION};
    use cp0_package::{PackageEntry, public_key};

    struct Fixture {
        root: PathBuf,
        apps: PathBuf,
        incoming: PathBuf,
        trust_paths: TrustPaths,
        developer_key: [u8; 32],
        store_key: [u8; 32],
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/test-tmp")
                .join(format!("installer-{name}-{}", std::process::id()));
            if root.exists() {
                fs::remove_dir_all(&root).unwrap();
            }
            let apps = root.join("apps");
            let incoming = root.join("incoming");
            let trust_paths = TrustPaths {
                store_keys: root.join("trust/store"),
                developer_keys: root.join("trust/developers"),
                revoked_keys: root.join("trust/revoked"),
                device_policy: root.join("device-policy.json"),
                developer_mode: root.join("developer-mode"),
            };
            for directory in [
                &apps,
                &incoming,
                &trust_paths.store_keys,
                &trust_paths.developer_keys,
                &trust_paths.revoked_keys,
            ] {
                fs::create_dir_all(directory).unwrap();
            }
            let fixture = Self {
                root,
                apps,
                incoming,
                trust_paths,
                developer_key: [7; 32],
                store_key: [11; 32],
            };
            fs::write(
                &fixture.trust_paths.device_policy,
                serde_json::to_vec_pretty(&crate::DevicePolicy::default()).unwrap(),
            )
            .unwrap();
            let store_public = public_key(&fixture.store_key);
            fs::write(
                fixture
                    .trust_paths
                    .store_keys
                    .join(format!("{}.pub", hex(&key_id(&store_public)))),
                store_public,
            )
            .unwrap();
            fixture
        }

        fn manifest(&self) -> AppManifest {
            AppManifest {
                schema_version: SCHEMA_VERSION,
                id: "dev.cardputerzero.installer".into(),
                name: "Installer Test".into(),
                version: "1.0.0".into(),
                sdk_version: "1.0".into(),
                runtime: Runtime::Wamr,
                entrypoint: "bin/app.wasm".into(),
                display: DisplayMode::Standard,
                resources: ResourceLimits {
                    memory_mb: 24,
                    storage_mb: 8,
                },
                permissions: Vec::new(),
                intents: Vec::new(),
            }
        }

        fn signed_package(&self, wasm: &[u8], store_signed: bool) -> CApp {
            let mut package = CApp::new(vec![
                PackageEntry {
                    path: "app.json".into(),
                    contents: serde_json::to_vec_pretty(&self.manifest()).unwrap(),
                },
                PackageEntry {
                    path: "bin/app.wasm".into(),
                    contents: wasm.to_vec(),
                },
            ])
            .unwrap();
            package.sign_developer(&self.developer_key).unwrap();
            if store_signed {
                package.sign_store(&self.store_key).unwrap();
            }
            package
        }

        fn write_package(&self, name: &str, package: &CApp) -> PathBuf {
            let path = self.incoming.join(name);
            fs::write(&path, package.encode().unwrap()).unwrap();
            path
        }

        fn installer(&self) -> PackageInstaller {
            PackageInstaller::new(
                &self.apps,
                TrustPolicy::new(self.trust_paths.clone(), false),
                false,
            )
        }
    }

    #[test]
    fn verifies_store_signature_and_atomically_extracts() {
        let fixture = Fixture::new("store");
        let path =
            fixture.write_package("app.capp", &fixture.signed_package(b"trusted wasm", true));
        let first = fixture.installer().install(&path).unwrap();
        assert_eq!(first.trust, TrustDecision::Store);
        assert!(first.newly_extracted);
        assert_eq!(
            fs::read(first.package_dir.join("bin/app.wasm")).unwrap(),
            b"trusted wasm"
        );

        let recovered = fixture.installer().install(&path).unwrap();
        assert!(!recovered.newly_extracted);
        assert_eq!(recovered.package_dir, first.package_dir);
    }

    #[test]
    fn installed_tree_modes_ignore_restrictive_umask() {
        const CHILD_MARKER: &str = "CP0_INSTALLER_RESTRICTIVE_UMASK_CHILD";

        if std::env::var_os(CHILD_MARKER).is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("installed_tree_modes_ignore_restrictive_umask")
                .arg("--nocapture")
                .env(CHILD_MARKER, "1")
                .status()
                .unwrap();
            assert!(status.success(), "restrictive umask child test failed");
            return;
        }

        // umask is process-global, so exercise it only in the isolated child process.
        unsafe {
            libc::umask(0o077);
        }
        let fixture = Fixture::new("restrictive-umask");
        let path =
            fixture.write_package("app.capp", &fixture.signed_package(b"trusted wasm", true));
        let installed = fixture.installer().install(&path).unwrap();
        let app_dir = installed.package_dir.parent().unwrap();

        for directory in [
            app_dir.to_path_buf(),
            installed.package_dir.clone(),
            installed.package_dir.join("bin"),
        ] {
            assert_eq!(
                fs::symlink_metadata(&directory).unwrap().mode() & 0o777,
                0o755,
                "wrong installed directory mode for {}",
                directory.display()
            );
        }
        for file in [
            installed.package_dir.join("app.json"),
            installed.package_dir.join("bin/app.wasm"),
        ] {
            assert_eq!(
                fs::symlink_metadata(&file).unwrap().mode() & 0o777,
                0o644,
                "wrong installed file mode for {}",
                file.display()
            );
        }
    }

    #[test]
    fn developer_signature_requires_explicit_mode_and_trusted_key() {
        let fixture = Fixture::new("developer");
        let package = fixture.signed_package(b"developer wasm", false);
        let path = fixture.write_package("developer.capp", &package);
        assert!(matches!(
            fixture.installer().install(&path),
            Err(InstallError::Untrusted(_))
        ));

        fs::write(&fixture.trust_paths.developer_mode, b"enabled\n").unwrap();
        let developer_public = public_key(&fixture.developer_key);
        fs::write(
            fixture
                .trust_paths
                .developer_keys
                .join(format!("{}.pub", hex(&key_id(&developer_public)))),
            developer_public,
        )
        .unwrap();
        assert_eq!(
            fixture.installer().install(&path).unwrap().trust,
            TrustDecision::DeveloperMode
        );

        let locked_policy = crate::DevicePolicy {
            developer_mode_allowed: false,
            ..crate::DevicePolicy::default()
        };
        fs::write(
            &fixture.trust_paths.device_policy,
            serde_json::to_vec_pretty(&locked_policy).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            fixture.installer().install(&path),
            Err(InstallError::Untrusted(_))
        ));
    }

    #[test]
    fn builtins_reject_developer_replacement_but_accept_store_updates() {
        let fixture = Fixture::new("builtin-trust");
        fs::write(&fixture.trust_paths.developer_mode, b"enabled\n").unwrap();
        let developer_public = public_key(&fixture.developer_key);
        fs::write(
            fixture
                .trust_paths
                .developer_keys
                .join(format!("{}.pub", hex(&key_id(&developer_public)))),
            developer_public,
        )
        .unwrap();

        let mut manifest = fixture.manifest();
        manifest.id = "dev.cardputerzero.camera".into();
        manifest.name = "Camera".into();
        let package = |store_signed| {
            let mut package = CApp::new(vec![
                PackageEntry {
                    path: "app.json".into(),
                    contents: serde_json::to_vec_pretty(&manifest).unwrap(),
                },
                PackageEntry {
                    path: "bin/app.wasm".into(),
                    contents: b"camera wasm".to_vec(),
                },
            ])
            .unwrap();
            package.sign_developer(&fixture.developer_key).unwrap();
            if store_signed {
                package.sign_store(&fixture.store_key).unwrap();
            }
            package
        };

        let developer = fixture.write_package("camera-developer.capp", &package(false));
        assert!(matches!(
            fixture.installer().install(&developer),
            Err(InstallError::Untrusted(_))
        ));
        assert!(!fixture.apps.join("dev.cardputerzero.camera").exists());

        let store = fixture.write_package("camera-store.capp", &package(true));
        assert_eq!(
            fixture.installer().install(&store).unwrap().trust,
            TrustDecision::Store
        );
    }

    #[test]
    fn revocation_and_same_version_content_conflicts_are_rejected() {
        let fixture = Fixture::new("revoked");
        let first_path =
            fixture.write_package("first.capp", &fixture.signed_package(b"first wasm", true));
        fixture.installer().install(&first_path).unwrap();

        let conflict_path = fixture.write_package(
            "conflict.capp",
            &fixture.signed_package(b"different wasm", true),
        );
        assert!(matches!(
            fixture.installer().install(&conflict_path),
            Err(InstallError::AlreadyInstalled(_))
        ));

        let developer_id = key_id(&public_key(&fixture.developer_key));
        fs::write(
            fixture.trust_paths.revoked_keys.join(hex(&developer_id)),
            b"",
        )
        .unwrap();
        assert!(matches!(
            fixture.installer().install(&first_path),
            Err(InstallError::Untrusted(_))
        ));
    }

    #[test]
    fn rejects_incompatible_sdk_before_extraction() {
        let fixture = Fixture::new("sdk");
        let mut manifest = fixture.manifest();
        manifest.sdk_version = "1.2".into();
        let mut package = CApp::new(vec![
            PackageEntry {
                path: "app.json".into(),
                contents: serde_json::to_vec_pretty(&manifest).unwrap(),
            },
            PackageEntry {
                path: "bin/app.wasm".into(),
                contents: b"wasm".to_vec(),
            },
        ])
        .unwrap();
        package.sign_developer(&fixture.developer_key).unwrap();
        package.sign_store(&fixture.store_key).unwrap();
        let path = fixture.write_package("future.capp", &package);
        assert!(matches!(
            fixture.installer().install(path),
            Err(InstallError::Invalid(_))
        ));
        assert!(
            !fixture
                .apps
                .join(&manifest.id)
                .join(&manifest.version)
                .exists()
        );
    }

    #[test]
    fn store_install_is_bound_to_owner_hash_size_and_manifest_identity() {
        let fixture = Fixture::new("store-bound");
        let package = fixture.signed_package(b"store wasm", true);
        let encoded = package.encode().unwrap();
        let digest: [u8; 32] = Sha256::digest(&encoded).into();
        let path = fixture.write_package("store.capp", &package);
        let manifest = fixture.manifest();
        let owner_uid = fs::metadata(&path).unwrap().uid();

        fixture
            .installer()
            .install_store(
                &path,
                owner_uid,
                &manifest.id,
                &manifest.version,
                &digest,
                encoded.len() as u64,
            )
            .unwrap();

        let wrong_hash = [0; 32];
        for result in [
            fixture.installer().install_store(
                &path,
                owner_uid.wrapping_add(1),
                &manifest.id,
                &manifest.version,
                &digest,
                encoded.len() as u64,
            ),
            fixture.installer().install_store(
                &path,
                owner_uid,
                &manifest.id,
                &manifest.version,
                &wrong_hash,
                encoded.len() as u64,
            ),
            fixture.installer().install_store(
                &path,
                owner_uid,
                &manifest.id,
                &manifest.version,
                &digest,
                encoded.len() as u64 + 1,
            ),
            fixture.installer().install_store(
                &path,
                owner_uid,
                "dev.cardputerzero.other",
                &manifest.version,
                &digest,
                encoded.len() as u64,
            ),
        ] {
            assert!(matches!(result, Err(InstallError::Invalid(_))));
        }
    }

    #[test]
    fn accepts_current_and_exact_legacy_sdk_versions() {
        for version in ["1.1", "1.0", "0.1"] {
            require_compatible_sdk(version).unwrap();
        }
    }

    #[test]
    fn rejects_unknown_and_noncanonical_sdk_versions() {
        for version in [
            "0.0", "0.2", "1.2", "2.0", "01.0", "1.00", "1", "1.0.0", "1.x", "",
        ] {
            assert!(
                matches!(
                    require_compatible_sdk(version),
                    Err(InstallError::Invalid(_))
                ),
                "SDK {version} should be rejected"
            );
        }
    }

    #[test]
    fn fixture_stays_inside_repository_target() {
        let fixture = Fixture::new("scope");
        assert!(
            fixture.root.starts_with(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp")
            )
        );
    }
}
