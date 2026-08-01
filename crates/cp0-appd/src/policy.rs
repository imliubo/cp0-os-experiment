use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cp0_manifest::Permission;
use serde::{Deserialize, Serialize};

pub const DEVICE_POLICY_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_DEVICE_POLICY_PATH: &str = "/etc/cardputerzero/device-policy.json";
pub const DEFAULT_DEVELOPER_MODE_PATH: &str = "/var/lib/cardputerzero/registry/developer-mode";
pub const DEFAULT_RECOVERY_MODE_PATH: &str = "/var/lib/cardputerzero/registry/recovery-mode";
const MAX_POLICY_BYTES: u64 = 16 * 1024;
const MAX_ALLOWED_APPS: usize = 64;
const MODE_CONTENTS: &[u8] = b"enabled\n";

static POLICY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagementAuthority {
    Personal,
    Parent,
    Organization,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppLaunchPolicy {
    AllowAll,
    AllowListed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceMode {
    Developer,
    Recovery,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevicePolicy {
    pub schema_version: u32,
    pub authority: ManagementAuthority,
    pub developer_mode_allowed: bool,
    pub recovery_mode_allowed: bool,
    pub store_install_allowed: bool,
    #[serde(default)]
    pub store_auto_update_allowed: bool,
    pub app_launch_policy: AppLaunchPolicy,
    pub allowed_apps: Vec<String>,
    pub denied_permissions: Vec<Permission>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceSettings {
    pub authority: ManagementAuthority,
    pub developer_mode: bool,
    pub developer_mode_allowed: bool,
    pub recovery_mode: bool,
    pub recovery_mode_allowed: bool,
    pub store_install_allowed: bool,
    pub app_launch_restricted: bool,
    pub denied_permission_count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceModePaths {
    pub developer: PathBuf,
    pub recovery: PathBuf,
}

impl Default for DeviceModePaths {
    fn default() -> Self {
        Self {
            developer: PathBuf::from(DEFAULT_DEVELOPER_MODE_PATH),
            recovery: PathBuf::from(DEFAULT_RECOVERY_MODE_PATH),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DevicePolicyEngine {
    policy: DevicePolicy,
    mode_paths: DeviceModePaths,
    enforce_root_ownership: bool,
}

#[derive(Debug)]
pub enum PolicyError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Invalid(String),
    Locked(DeviceMode),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "device policy I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid device policy JSON: {error}"),
            Self::Invalid(error) => write!(formatter, "invalid device policy: {error}"),
            Self::Locked(DeviceMode::Developer) => {
                formatter.write_str("developer mode is locked by device policy")
            }
            Self::Locked(DeviceMode::Recovery) => {
                formatter.write_str("recovery mode is locked by device policy")
            }
        }
    }
}

impl std::error::Error for PolicyError {}

impl From<std::io::Error> for PolicyError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for PolicyError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl Default for DevicePolicy {
    fn default() -> Self {
        Self {
            schema_version: DEVICE_POLICY_SCHEMA_VERSION,
            authority: ManagementAuthority::Personal,
            developer_mode_allowed: true,
            recovery_mode_allowed: true,
            store_install_allowed: true,
            store_auto_update_allowed: true,
            app_launch_policy: AppLaunchPolicy::AllowAll,
            allowed_apps: Vec::new(),
            denied_permissions: Vec::new(),
        }
    }
}

impl DevicePolicy {
    pub fn load_secure(
        path: impl AsRef<Path>,
        enforce_root_ownership: bool,
    ) -> Result<Self, PolicyError> {
        let path = path.as_ref();
        let metadata = fs::symlink_metadata(path)?;
        validate_secure_file(&metadata, enforce_root_ownership, "device policy")?;
        if metadata.len() == 0 || metadata.len() > MAX_POLICY_BYTES {
            return Err(PolicyError::Invalid(format!(
                "device policy must contain between 1 and {MAX_POLICY_BYTES} bytes"
            )));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        let opened = file.metadata()?;
        validate_secure_file(&opened, enforce_root_ownership, "device policy")?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return Err(PolicyError::Invalid(
                "device policy changed while it was opened".into(),
            ));
        }
        if opened.len() == 0 || opened.len() > MAX_POLICY_BYTES {
            return Err(PolicyError::Invalid(format!(
                "device policy must contain between 1 and {MAX_POLICY_BYTES} bytes"
            )));
        }
        if opened.len() != metadata.len() {
            return Err(PolicyError::Invalid(
                "device policy changed while it was opened".into(),
            ));
        }
        let mut encoded = Vec::with_capacity(opened.len() as usize);
        file.read_to_end(&mut encoded)?;
        if encoded.len() as u64 != opened.len() {
            return Err(PolicyError::Invalid(
                "device policy changed while it was read".into(),
            ));
        }
        let policy: Self = serde_json::from_slice(&encoded)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn allows_app(&self, app_id: &str) -> bool {
        self.app_launch_policy == AppLaunchPolicy::AllowAll
            || self
                .allowed_apps
                .binary_search_by(|candidate| candidate.as_str().cmp(app_id))
                .is_ok()
    }

    pub fn denies_permission(&self, permission: Permission) -> bool {
        self.denied_permissions.binary_search(&permission).is_ok()
    }

    fn validate(&self) -> Result<(), PolicyError> {
        if self.schema_version != DEVICE_POLICY_SCHEMA_VERSION {
            return Err(PolicyError::Invalid(format!(
                "schema_version must be {DEVICE_POLICY_SCHEMA_VERSION}"
            )));
        }
        if self.allowed_apps.len() > MAX_ALLOWED_APPS {
            return Err(PolicyError::Invalid(format!(
                "allowed_apps contains more than {MAX_ALLOWED_APPS} entries"
            )));
        }
        if self
            .allowed_apps
            .iter()
            .any(|app_id| !cp0_manifest::is_valid_app_id(app_id))
        {
            return Err(PolicyError::Invalid(
                "allowed_apps contains an invalid application ID".into(),
            ));
        }
        if self.allowed_apps.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(PolicyError::Invalid(
                "allowed_apps must be strictly sorted without duplicates".into(),
            ));
        }
        if self.app_launch_policy == AppLaunchPolicy::AllowAll && !self.allowed_apps.is_empty() {
            return Err(PolicyError::Invalid(
                "allowed_apps must be empty when app_launch_policy is allow-all".into(),
            ));
        }
        if self.denied_permissions.len() > Permission::ALL.len()
            || self
                .denied_permissions
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(PolicyError::Invalid(
                "denied_permissions must be strictly sorted without duplicates".into(),
            ));
        }
        Ok(())
    }
}

impl DevicePolicyEngine {
    pub fn load(
        policy_path: impl AsRef<Path>,
        mode_paths: DeviceModePaths,
        enforce_root_ownership: bool,
    ) -> Result<Self, PolicyError> {
        let engine = Self {
            policy: DevicePolicy::load_secure(policy_path, enforce_root_ownership)?,
            mode_paths,
            enforce_root_ownership,
        };
        if !engine.policy.developer_mode_allowed {
            set_mode_marker(
                &engine.mode_paths.developer,
                false,
                engine.enforce_root_ownership,
            )?;
        }
        if !engine.policy.recovery_mode_allowed {
            set_mode_marker(
                &engine.mode_paths.recovery,
                false,
                engine.enforce_root_ownership,
            )?;
        }
        Ok(engine)
    }

    pub fn unmanaged() -> Self {
        Self {
            policy: DevicePolicy::default(),
            mode_paths: DeviceModePaths::default(),
            enforce_root_ownership: true,
        }
    }

    pub fn settings(&self) -> Result<DeviceSettings, PolicyError> {
        let developer_mode = self.policy.developer_mode_allowed
            && mode_enabled(&self.mode_paths.developer, self.enforce_root_ownership)?;
        let recovery_mode = self.policy.recovery_mode_allowed
            && mode_enabled(&self.mode_paths.recovery, self.enforce_root_ownership)?;
        Ok(DeviceSettings {
            authority: self.policy.authority,
            developer_mode,
            developer_mode_allowed: self.policy.developer_mode_allowed,
            recovery_mode,
            recovery_mode_allowed: self.policy.recovery_mode_allowed,
            store_install_allowed: self.policy.store_install_allowed,
            app_launch_restricted: self.policy.app_launch_policy == AppLaunchPolicy::AllowListed,
            denied_permission_count: u8::try_from(self.policy.denied_permissions.len())
                .expect("permission vocabulary fits in u8"),
        })
    }

    pub fn set_mode(&self, mode: DeviceMode, enabled: bool) -> Result<(), PolicyError> {
        let allowed = match mode {
            DeviceMode::Developer => self.policy.developer_mode_allowed,
            DeviceMode::Recovery => self.policy.recovery_mode_allowed,
        };
        if enabled && !allowed {
            return Err(PolicyError::Locked(mode));
        }
        let path = match mode {
            DeviceMode::Developer => &self.mode_paths.developer,
            DeviceMode::Recovery => &self.mode_paths.recovery,
        };
        set_mode_marker(path, enabled, self.enforce_root_ownership)
    }

    pub fn allows_store_install(&self, app_id: &str) -> bool {
        self.policy.store_install_allowed && self.policy.allows_app(app_id)
    }

    pub fn allows_store_auto_update(&self, app_id: &str) -> bool {
        self.policy.store_install_allowed
            && self.policy.store_auto_update_allowed
            && self.policy.allows_app(app_id)
    }

    pub fn allows_app(&self, app_id: &str) -> bool {
        self.policy.allows_app(app_id)
    }

    pub fn denies_permission(&self, permission: Permission) -> bool {
        self.policy.denies_permission(permission)
    }
}

pub fn developer_install_allowed(
    policy_path: impl AsRef<Path>,
    marker_path: impl AsRef<Path>,
    enforce_root_ownership: bool,
) -> Result<bool, PolicyError> {
    let policy = DevicePolicy::load_secure(policy_path, enforce_root_ownership)?;
    if !policy.developer_mode_allowed {
        return Ok(false);
    }
    mode_enabled(marker_path.as_ref(), enforce_root_ownership)
}

fn mode_enabled(path: &Path, enforce_root_ownership: bool) -> Result<bool, PolicyError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    validate_secure_file(&metadata, enforce_root_ownership, "device mode marker")?;
    if metadata.len() != MODE_CONTENTS.len() as u64 {
        return Err(PolicyError::Invalid(
            "device mode marker has invalid contents".into(),
        ));
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let opened = file.metadata()?;
    validate_secure_file(&opened, enforce_root_ownership, "device mode marker")?;
    if opened.dev() != metadata.dev()
        || opened.ino() != metadata.ino()
        || opened.len() != metadata.len()
    {
        return Err(PolicyError::Invalid(
            "device mode marker changed while it was opened".into(),
        ));
    }
    let mut contents = Vec::with_capacity(MODE_CONTENTS.len());
    file.read_to_end(&mut contents)?;
    Ok(contents == MODE_CONTENTS)
}

fn set_mode_marker(
    path: &Path,
    enabled: bool,
    enforce_root_ownership: bool,
) -> Result<(), PolicyError> {
    let parent = path
        .parent()
        .ok_or_else(|| PolicyError::Invalid("device mode path has no parent".into()))?;
    validate_secure_directory(parent, enforce_root_ownership)?;
    if !enabled {
        match fs::remove_file(path) {
            Ok(()) => File::open(parent)?.sync_all()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        return Ok(());
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| PolicyError::Invalid("device mode path is not UTF-8".into()))?;
    let sequence = POLICY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{file_name}.tmp-{}-{sequence}",
        std::process::id()
    ));
    let result = (|| -> Result<(), PolicyError> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(MODE_CONTENTS)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn validate_secure_file(
    metadata: &fs::Metadata,
    enforce_root_ownership: bool,
    name: &str,
) -> Result<(), PolicyError> {
    if !metadata.file_type().is_file() {
        return Err(PolicyError::Invalid(format!(
            "{name} must be a regular file"
        )));
    }
    if (enforce_root_ownership && metadata.uid() != 0) || metadata.mode() & 0o022 != 0 {
        return Err(PolicyError::Invalid(format!(
            "{name} must be root-owned and not group/world writable"
        )));
    }
    Ok(())
}

fn validate_secure_directory(path: &Path, enforce_root_ownership: bool) -> Result<(), PolicyError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(PolicyError::Invalid(
            "device mode parent must be a directory".into(),
        ));
    }
    if (enforce_root_ownership && metadata.uid() != 0) || metadata.mode() & 0o022 != 0 {
        return Err(PolicyError::Invalid(
            "device mode parent must be root-owned and not group/world writable".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> (PathBuf, PathBuf, DeviceModePaths) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("policy-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(root.join("registry")).unwrap();
        let policy_path = root.join("device-policy.json");
        let paths = DeviceModePaths {
            developer: root.join("registry/developer-mode"),
            recovery: root.join("registry/recovery-mode"),
        };
        (root, policy_path, paths)
    }

    fn write_policy(path: &Path, policy: &DevicePolicy) {
        fs::write(path, serde_json::to_vec_pretty(policy).unwrap()).unwrap();
    }

    #[test]
    fn personal_policy_round_trips_atomic_modes() {
        let (_root, policy_path, paths) = fixture("personal");
        write_policy(&policy_path, &DevicePolicy::default());
        let engine = DevicePolicyEngine::load(&policy_path, paths.clone(), false).unwrap();
        assert!(!engine.settings().unwrap().developer_mode);
        engine.set_mode(DeviceMode::Developer, true).unwrap();
        engine.set_mode(DeviceMode::Recovery, true).unwrap();
        let settings = engine.settings().unwrap();
        assert!(settings.developer_mode && settings.recovery_mode);
        assert!(developer_install_allowed(&policy_path, &paths.developer, false).unwrap());
        engine.set_mode(DeviceMode::Developer, false).unwrap();
        assert!(!engine.settings().unwrap().developer_mode);
    }

    #[test]
    fn product_policy_is_the_strict_personal_default() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../appd/device-policy.json");
        assert_eq!(
            DevicePolicy::load_secure(path, false).unwrap(),
            DevicePolicy::default()
        );
    }

    #[test]
    fn legacy_policy_defaults_auto_updates_to_denied() {
        let (_root, policy_path, paths) = fixture("legacy-auto-update");
        fs::write(
            &policy_path,
            br#"{"schema_version":1,"authority":"personal","developer_mode_allowed":true,"recovery_mode_allowed":true,"store_install_allowed":true,"app_launch_policy":"allow-all","allowed_apps":[],"denied_permissions":[]}"#,
        )
        .unwrap();
        let policy = DevicePolicy::load_secure(&policy_path, false).unwrap();
        assert!(!policy.store_auto_update_allowed);
        let engine = DevicePolicyEngine::load(&policy_path, paths, false).unwrap();
        assert!(engine.allows_store_install("dev.cardputerzero.example"));
        assert!(!engine.allows_store_auto_update("dev.cardputerzero.example"));
    }

    #[test]
    fn managed_policy_locks_modes_apps_store_and_permissions() {
        let (_root, policy_path, paths) = fixture("managed");
        set_mode_marker(&paths.developer, true, false).unwrap();
        set_mode_marker(&paths.recovery, true, false).unwrap();
        let policy = DevicePolicy {
            authority: ManagementAuthority::Organization,
            developer_mode_allowed: false,
            recovery_mode_allowed: false,
            store_install_allowed: true,
            app_launch_policy: AppLaunchPolicy::AllowListed,
            allowed_apps: vec!["dev.cardputerzero.allowed".into()],
            denied_permissions: vec![Permission::CameraCapture],
            ..DevicePolicy::default()
        };
        write_policy(&policy_path, &policy);
        let engine = DevicePolicyEngine::load(&policy_path, paths, false).unwrap();
        assert!(!engine.settings().unwrap().developer_mode);
        assert!(!engine.settings().unwrap().recovery_mode);
        assert!(matches!(
            engine.set_mode(DeviceMode::Developer, true),
            Err(PolicyError::Locked(DeviceMode::Developer))
        ));
        assert!(engine.allows_store_install("dev.cardputerzero.allowed"));
        assert!(engine.allows_store_auto_update("dev.cardputerzero.allowed"));
        assert!(!engine.allows_store_install("dev.cardputerzero.blocked"));
        assert!(engine.denies_permission(Permission::CameraCapture));
        engine.set_mode(DeviceMode::Developer, false).unwrap();
    }

    #[test]
    fn rejects_unknown_unsorted_duplicate_and_oversized_policy() {
        let (_root, policy_path, paths) = fixture("invalid");
        fs::write(
            &policy_path,
            br#"{"schema_version":1,"authority":"personal","developer_mode_allowed":true,"recovery_mode_allowed":true,"store_install_allowed":true,"app_launch_policy":"allow-all","allowed_apps":[],"denied_permissions":[],"extra":true}"#,
        )
        .unwrap();
        assert!(DevicePolicyEngine::load(&policy_path, paths.clone(), false).is_err());

        let mut policy = DevicePolicy {
            app_launch_policy: AppLaunchPolicy::AllowListed,
            allowed_apps: vec![
                "dev.cardputerzero.zed".into(),
                "dev.cardputerzero.alpha".into(),
            ],
            ..DevicePolicy::default()
        };
        write_policy(&policy_path, &policy);
        assert!(DevicePolicyEngine::load(&policy_path, paths.clone(), false).is_err());
        policy.allowed_apps.clear();
        policy.denied_permissions = vec![Permission::CameraCapture, Permission::CameraCapture];
        write_policy(&policy_path, &policy);
        assert!(DevicePolicyEngine::load(&policy_path, paths.clone(), false).is_err());
        fs::write(&policy_path, vec![b' '; MAX_POLICY_BYTES as usize + 1]).unwrap();
        assert!(DevicePolicyEngine::load(&policy_path, paths, false).is_err());
    }
}
