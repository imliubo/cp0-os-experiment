use std::ffi::CString;
use std::fmt;
use std::fs::{self, Metadata};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use cp0_manifest::AppManifest;

use crate::{AppAccount, AppLayout, AppRegistry, RegistryError, SandboxPlan, build_sandbox_plan};

pub const DEFAULT_REGISTRY_PATH: &str = "/var/lib/cardputerzero/registry/apps.json";
const SYSTEMD_RUN_PATH: &str = "/usr/bin/systemd-run";
const SYSTEMCTL_PATH: &str = "/usr/bin/systemctl";
const COMPOSITOR_USER: &str = "cp0-compositor";
const MAX_USAGE_TREE_ENTRIES: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerPaths {
    pub registry_path: PathBuf,
    pub layout: AppLayout,
}

impl Default for ManagerPaths {
    fn default() -> Self {
        Self {
            registry_path: PathBuf::from(DEFAULT_REGISTRY_PATH),
            layout: AppLayout::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    pub app_id: String,
    pub version: String,
    pub account_user: String,
    pub account_uid: u32,
    pub installed_at_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUsage {
    pub package_bytes: u64,
    pub data_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstalledApp {
    pub app_id: String,
    pub package_cleanup_pending: bool,
}

#[derive(Debug)]
pub enum AppManagerError {
    Registry(RegistryError),
    Manifest(cp0_manifest::ManifestError),
    NotInstalled(String),
    IdentityMismatch,
    InvalidPackagePath(&'static str),
    InvalidHostIdentity(String),
    CommandIo(&'static str, std::io::Error),
    AlreadyRunning(String),
    ForegroundBusy(String),
    NotRunning(String),
    NoRollback(String),
    UnitFailed(&'static str),
    PackageIo(&'static str, std::io::Error),
    Plan(crate::PlanError),
}

impl fmt::Display for AppManagerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(error) => write!(formatter, "{error}"),
            Self::Manifest(error) => write!(formatter, "{error}"),
            Self::NotInstalled(app_id) => {
                write!(formatter, "application {app_id} is not installed")
            }
            Self::IdentityMismatch => formatter.write_str(
                "installed manifest identity does not match the trusted application registry",
            ),
            Self::InvalidPackagePath(field) => {
                write!(
                    formatter,
                    "installed package {field} is missing, invalid or a symbolic link"
                )
            }
            Self::InvalidHostIdentity(error) => {
                write!(formatter, "invalid application host identity: {error}")
            }
            Self::CommandIo(command, error) => {
                write!(formatter, "cannot execute {command}: {error}")
            }
            Self::AlreadyRunning(app_id) => {
                write!(formatter, "application {app_id} is already running")
            }
            Self::ForegroundBusy(app_id) => {
                write!(
                    formatter,
                    "application {app_id} already owns the runtime slot"
                )
            }
            Self::NotRunning(app_id) => {
                write!(formatter, "application {app_id} is not running")
            }
            Self::NoRollback(app_id) => {
                write!(formatter, "application {app_id} has no rollback version")
            }
            Self::UnitFailed(action) => {
                write!(formatter, "application systemd unit failed to {action}")
            }
            Self::PackageIo(action, error) => {
                write!(formatter, "cannot {action} application package: {error}")
            }
            Self::Plan(error) => write!(formatter, "cannot construct application sandbox: {error}"),
        }
    }
}

impl std::error::Error for AppManagerError {}

impl From<RegistryError> for AppManagerError {
    fn from(error: RegistryError) -> Self {
        Self::Registry(error)
    }
}

impl From<cp0_manifest::ManifestError> for AppManagerError {
    fn from(error: cp0_manifest::ManifestError) -> Self {
        Self::Manifest(error)
    }
}

impl From<crate::PlanError> for AppManagerError {
    fn from(error: crate::PlanError) -> Self {
        Self::Plan(error)
    }
}

#[derive(Debug)]
pub struct AppManager {
    paths: ManagerPaths,
    registry: AppRegistry,
}

impl AppManager {
    pub fn load(paths: ManagerPaths) -> Result<Self, AppManagerError> {
        verify_registry_host(&paths.registry_path)?;
        let registry = AppRegistry::load(&paths.registry_path)?;
        Ok(Self { paths, registry })
    }

    pub fn from_registry(
        paths: ManagerPaths,
        registry: AppRegistry,
    ) -> Result<Self, AppManagerError> {
        registry.validate()?;
        Ok(Self { paths, registry })
    }

    pub fn registry(&self) -> &AppRegistry {
        &self.registry
    }

    pub fn mark_installed(
        &mut self,
        manifest: &AppManifest,
    ) -> Result<InstalledApp, AppManagerError> {
        let installed_manifest = self.load_package_manifest(&manifest.id, &manifest.version)?;
        if installed_manifest != *manifest {
            return Err(AppManagerError::IdentityMismatch);
        }
        let mut next_registry = self.registry.clone();
        let account = next_registry.mark_installed(manifest)?;
        next_registry.save_atomic(&self.paths.registry_path)?;
        self.registry = next_registry;
        Ok(InstalledApp {
            app_id: manifest.id.clone(),
            version: manifest.version.clone(),
            account_user: account.unix_user,
            account_uid: account.unix_uid,
            installed_at_unix_seconds: account.installed_at_unix_seconds.unwrap_or(0),
        })
    }

    pub fn prepare_account(&mut self, app_id: &str) -> Result<AppAccount, AppManagerError> {
        Ok(self.registry.assign(app_id)?)
    }

    pub fn rollback(&mut self, app_id: &str) -> Result<InstalledApp, AppManagerError> {
        let account = self
            .registry
            .account(app_id)
            .filter(|account| account.installed_version.is_some())
            .ok_or_else(|| AppManagerError::NotInstalled(app_id.into()))?;
        if unit_is_active(&format!("cardputerzero-app-{}.service", account.account_id))? {
            return Err(AppManagerError::AlreadyRunning(app_id.into()));
        }
        let version = account
            .previous_versions
            .first()
            .ok_or_else(|| AppManagerError::NoRollback(app_id.into()))?
            .clone();
        let manifest = self.load_package_manifest(app_id, &version)?;
        if manifest.id != app_id || manifest.version != version {
            return Err(AppManagerError::IdentityMismatch);
        }
        let mut next_registry = self.registry.clone();
        let account = next_registry.rollback(app_id)?;
        next_registry.save_atomic(&self.paths.registry_path)?;
        self.registry = next_registry;
        Ok(InstalledApp {
            app_id: app_id.into(),
            version,
            account_user: account.unix_user,
            account_uid: account.unix_uid,
            installed_at_unix_seconds: account.installed_at_unix_seconds.unwrap_or(0),
        })
    }

    pub fn installed_apps(&self) -> Vec<InstalledApp> {
        self.registry
            .apps
            .iter()
            .filter_map(|(app_id, account)| {
                account
                    .installed_version
                    .as_ref()
                    .map(|version| InstalledApp {
                        app_id: app_id.clone(),
                        version: version.clone(),
                        account_user: account.unix_user.clone(),
                        account_uid: account.unix_uid,
                        installed_at_unix_seconds: account.installed_at_unix_seconds.unwrap_or(0),
                    })
            })
            .collect()
    }

    pub fn installed_app_for_uid(&self, uid: u32) -> Option<InstalledApp> {
        self.registry
            .installed_app_for_uid(uid)
            .map(|(app_id, account)| InstalledApp {
                app_id: app_id.into(),
                version: account
                    .installed_version
                    .clone()
                    .expect("registry lookup only returns installed applications"),
                account_user: account.unix_user.clone(),
                account_uid: account.unix_uid,
                installed_at_unix_seconds: account.installed_at_unix_seconds.unwrap_or(0),
            })
    }

    pub fn sandbox_plan(&self, app_id: &str) -> Result<SandboxPlan, AppManagerError> {
        let account = self
            .registry
            .account(app_id)
            .ok_or_else(|| AppManagerError::NotInstalled(app_id.into()))?;
        let version = account
            .installed_version
            .as_deref()
            .ok_or_else(|| AppManagerError::NotInstalled(app_id.into()))?;
        let manifest = self.load_package_manifest(app_id, version)?;
        if manifest.id != app_id || manifest.version != version {
            return Err(AppManagerError::IdentityMismatch);
        }
        Ok(build_sandbox_plan(
            &manifest,
            &account.unix_user,
            &self.paths.layout,
        )?)
    }

    pub fn installed_manifest(&self, app_id: &str) -> Result<AppManifest, AppManagerError> {
        let account = self
            .registry
            .account(app_id)
            .ok_or_else(|| AppManagerError::NotInstalled(app_id.into()))?;
        let version = account
            .installed_version
            .as_deref()
            .ok_or_else(|| AppManagerError::NotInstalled(app_id.into()))?;
        let manifest = self.load_package_manifest(app_id, version)?;
        if manifest.id != app_id || manifest.version != version {
            return Err(AppManagerError::IdentityMismatch);
        }
        Ok(manifest)
    }

    pub fn app_usage(&self, app_id: &str) -> Result<AppUsage, AppManagerError> {
        if !cp0_manifest::is_valid_app_id(app_id) {
            return Err(AppManagerError::NotInstalled(app_id.into()));
        }
        self.registry
            .account(app_id)
            .filter(|account| account.installed_version.is_some())
            .ok_or_else(|| AppManagerError::NotInstalled(app_id.into()))?;
        let package_root = self.paths.layout.apps_root.join(app_id);
        let data_root = self.paths.layout.data_root.join(app_id);
        Ok(AppUsage {
            package_bytes: tree_bytes(&package_root)
                .map_err(|error| AppManagerError::PackageIo("measure", error))?,
            data_bytes: match fs::symlink_metadata(&data_root) {
                Ok(_) => tree_bytes(&data_root)
                    .map_err(|error| AppManagerError::PackageIo("measure data", error))?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                Err(error) => return Err(AppManagerError::PackageIo("measure data", error)),
            },
        })
    }

    pub fn uninstall(&mut self, app_id: &str) -> Result<UninstalledApp, AppManagerError> {
        if self.is_running(app_id)? {
            return Err(AppManagerError::AlreadyRunning(app_id.into()));
        }
        self.uninstall_stopped(app_id)
    }

    fn uninstall_stopped(&mut self, app_id: &str) -> Result<UninstalledApp, AppManagerError> {
        let account = self
            .registry
            .account(app_id)
            .filter(|account| account.installed_version.is_some())
            .ok_or_else(|| AppManagerError::NotInstalled(app_id.into()))?;
        let app_root = self.paths.layout.apps_root.join(app_id);
        let tombstone = self
            .paths
            .layout
            .apps_root
            .join(format!(".uninstall-{}", account.account_id));
        let metadata = fs::symlink_metadata(&app_root)
            .map_err(|error| AppManagerError::PackageIo("inspect", error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(AppManagerError::InvalidPackagePath("application directory"));
        }
        fs::rename(&app_root, &tombstone)
            .map_err(|error| AppManagerError::PackageIo("stage removal", error))?;

        let result = (|| {
            let mut next_registry = self.registry.clone();
            next_registry.uninstall(app_id)?;
            next_registry.save_atomic(&self.paths.registry_path)?;
            self.registry = next_registry;
            Ok::<(), AppManagerError>(())
        })();
        if let Err(error) = result {
            let _ = fs::rename(&tombstone, &app_root);
            return Err(error);
        }
        let package_cleanup_pending = fs::remove_dir_all(&tombstone).is_err();
        Ok(UninstalledApp {
            app_id: app_id.into(),
            package_cleanup_pending,
        })
    }

    pub fn is_running(&self, app_id: &str) -> Result<bool, AppManagerError> {
        unit_is_active(&self.unit_for_app(app_id)?)
    }

    pub fn start(&self, app_id: &str) -> Result<String, AppManagerError> {
        let plan = self.sandbox_plan(app_id)?;
        let account = self
            .registry
            .account(app_id)
            .ok_or_else(|| AppManagerError::NotInstalled(app_id.into()))?;
        self.verify_launch_host(&plan, account)?;
        if unit_is_active(&plan.unit)? {
            return Err(AppManagerError::AlreadyRunning(app_id.into()));
        }
        for installed in self.installed_apps() {
            if installed.app_id != app_id && unit_is_active(&self.unit_for_app(&installed.app_id)?)?
            {
                return Err(AppManagerError::ForegroundBusy(installed.app_id));
            }
        }

        let status = Command::new(SYSTEMD_RUN_PATH)
            .args(crate::systemd_run_arguments(&plan))
            .status()
            .map_err(|error| AppManagerError::CommandIo("systemd-run", error))?;
        if !status.success() || !unit_is_active(&plan.unit)? {
            return Err(AppManagerError::UnitFailed("start"));
        }
        Ok(plan.unit)
    }

    pub fn stop(&self, app_id: &str) -> Result<(), AppManagerError> {
        let unit = self.unit_for_app(app_id)?;
        if !unit_is_active(&unit)? {
            return Err(AppManagerError::NotRunning(app_id.into()));
        }
        let status = Command::new(SYSTEMCTL_PATH)
            .args(["stop", &unit])
            .status()
            .map_err(|error| AppManagerError::CommandIo("systemctl", error))?;
        if !status.success() || unit_is_active(&unit)? {
            return Err(AppManagerError::UnitFailed("stop"));
        }
        Ok(())
    }

    pub fn logs(&self, app_id: &str, limit: u16) -> Result<Vec<String>, AppManagerError> {
        let unit = self.unit_for_app(app_id)?;
        let limit = limit.to_string();
        let output = Command::new("/usr/bin/journalctl")
            .args([
                "--unit",
                &unit,
                "--lines",
                &limit,
                "--output",
                "cat",
                "--no-pager",
                "--quiet",
            ])
            .output()
            .map_err(|error| AppManagerError::CommandIo("journalctl", error))?;
        if !output.status.success() {
            return Err(AppManagerError::UnitFailed("read logs"));
        }
        let mut encoded_bytes = 0_usize;
        let lines = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let line: String = line
                    .chars()
                    .filter(|character| !character.is_control() || *character == '\t')
                    .take(256)
                    .collect();
                let next = encoded_bytes.saturating_add(line.len());
                if line.is_empty() || next > 3 * 1024 {
                    None
                } else {
                    encoded_bytes = next;
                    Some(line)
                }
            })
            .collect();
        Ok(lines)
    }

    pub(crate) fn unit_for_app(&self, app_id: &str) -> Result<String, AppManagerError> {
        let account = self
            .registry
            .account(app_id)
            .filter(|account| account.installed_version.is_some())
            .ok_or_else(|| AppManagerError::NotInstalled(app_id.into()))?;
        Ok(format!("cardputerzero-app-{}.service", account.account_id))
    }

    fn verify_launch_host(
        &self,
        plan: &SandboxPlan,
        account: &crate::AppAccount,
    ) -> Result<(), AppManagerError> {
        let (uid, gid) = lookup_unix_account(&account.unix_user)?;
        if uid != account.unix_uid || gid != account.unix_uid {
            return Err(AppManagerError::InvalidHostIdentity(format!(
                "{} resolves to UID/GID {uid}/{gid}, expected {}/{}",
                account.unix_user, account.unix_uid, account.unix_uid
            )));
        }

        let registry_parent = self.paths.registry_path.parent().ok_or_else(|| {
            AppManagerError::InvalidHostIdentity("registry path has no parent".into())
        })?;
        require_root_directory(registry_parent, "registry directory")?;
        require_root_directory(&self.paths.layout.apps_root, "applications root")?;
        require_root_directory(
            &self.paths.layout.apps_root.join(&plan.app_id),
            "application directory",
        )?;
        let runtime = secure_metadata(Path::new(&self.paths.layout.runtime_path), "runtime")?;
        require_owner_mode(&runtime, 0, 0o022, "runtime")?;
        if !runtime.is_file() || runtime.mode() & 0o111 == 0 {
            return Err(AppManagerError::InvalidHostIdentity(
                "runtime is not executable".into(),
            ));
        }
        let broker_path = Path::new(&self.paths.layout.broker_socket);
        let broker_parent = broker_path.parent().ok_or_else(|| {
            AppManagerError::InvalidHostIdentity("broker socket has no parent".into())
        })?;
        require_root_directory(broker_parent, "broker socket directory")?;
        let broker = secure_metadata(broker_path, "broker socket")?;
        if !broker.file_type().is_socket() || broker.uid() != 0 {
            return Err(AppManagerError::InvalidHostIdentity(
                "broker socket is not a root-owned Unix socket".into(),
            ));
        }
        let wayland_path = Path::new(&self.paths.layout.wayland_socket);
        let wayland_parent = wayland_path.parent().ok_or_else(|| {
            AppManagerError::InvalidHostIdentity("Wayland socket has no parent".into())
        })?;
        let (compositor_uid, _) = lookup_unix_account(COMPOSITOR_USER)?;
        require_controlled_directory(wayland_parent, compositor_uid, "Wayland runtime directory")?;
        let wayland = secure_metadata(wayland_path, "Wayland socket")?;
        if !wayland.file_type().is_socket() || wayland.uid() != compositor_uid {
            return Err(AppManagerError::InvalidHostIdentity(
                "Wayland endpoint is not a compositor-owned Unix socket".into(),
            ));
        }
        let package = secure_metadata(Path::new(&plan.package_dir), "package directory")?;
        require_owner_mode(&package, 0, 0o022, "package directory")?;
        let manifest = secure_metadata(&Path::new(&plan.package_dir).join("app.json"), "manifest")?;
        require_owner_mode(&manifest, 0, 0o022, "manifest")?;
        let installed_manifest = self.load_package_manifest(&plan.app_id, &plan.app_version)?;
        let entrypoint = secure_metadata(
            &Path::new(&plan.package_dir).join(installed_manifest.entrypoint),
            "entrypoint",
        )?;
        require_owner_mode(&entrypoint, 0, 0o022, "entrypoint")?;

        Ok(())
    }

    fn load_package_manifest(
        &self,
        app_id: &str,
        version: &str,
    ) -> Result<AppManifest, AppManagerError> {
        verify_directory(&self.paths.layout.apps_root, "applications root")?;
        let app_dir = self.paths.layout.apps_root.join(app_id);
        verify_directory(&app_dir, "application directory")?;
        let package_dir = app_dir.join(version);
        verify_directory(&package_dir, "version directory")?;
        let manifest_path = package_dir.join("app.json");
        verify_regular_file(&manifest_path, "manifest")?;
        let manifest = cp0_manifest::load_and_validate(&manifest_path)?;
        verify_relative_file(&package_dir, Path::new(&manifest.entrypoint), "entrypoint")?;
        Ok(manifest)
    }
}

fn verify_registry_host(path: &Path) -> Result<(), AppManagerError> {
    let metadata = secure_metadata(path, "application registry")?;
    if !metadata.is_file() {
        return Err(AppManagerError::InvalidHostIdentity(
            "application registry is not a regular file".into(),
        ));
    }
    require_owner_mode(&metadata, 0, 0o077, "application registry")?;
    let parent = path.parent().ok_or_else(|| {
        AppManagerError::InvalidHostIdentity("registry path has no parent".into())
    })?;
    require_root_directory(parent, "registry directory")
}

fn require_root_directory(path: &Path, field: &'static str) -> Result<(), AppManagerError> {
    let metadata = secure_metadata(path, field)?;
    if !metadata.is_dir() {
        return Err(AppManagerError::InvalidHostIdentity(format!(
            "{field} is not a directory"
        )));
    }
    require_owner_mode(&metadata, 0, 0o022, field)
}

fn require_controlled_directory(
    path: &Path,
    expected_uid: u32,
    field: &'static str,
) -> Result<(), AppManagerError> {
    let metadata = secure_metadata(path, field)?;
    if !metadata.is_dir() || metadata.uid() != expected_uid || metadata.mode() & 0o002 != 0 {
        return Err(AppManagerError::InvalidHostIdentity(format!(
            "{field} must be owned by UID {expected_uid} and not world-writable"
        )));
    }
    Ok(())
}

pub fn lookup_unix_account(user: &str) -> Result<(u32, u32), AppManagerError> {
    let name = CString::new(user).map_err(|_| {
        AppManagerError::InvalidHostIdentity("Unix user contains a NUL byte".into())
    })?;
    let mut entry = unsafe { std::mem::zeroed::<libc::passwd>() };
    let mut result = std::ptr::null_mut();
    let mut buffer = vec![0_u8; 16 * 1024];
    // SAFETY: all pointers refer to valid storage for the duration of the call,
    // and getpwnam_r writes at most buffer.len() bytes into the supplied buffer.
    let status = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            &mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 {
        return Err(AppManagerError::InvalidHostIdentity(format!(
            "cannot resolve {user}: {}",
            std::io::Error::from_raw_os_error(status)
        )));
    }
    if result.is_null() {
        return Err(AppManagerError::InvalidHostIdentity(format!(
            "Unix user {user} does not exist"
        )));
    }
    Ok((entry.pw_uid, entry.pw_gid))
}

fn unit_is_active(unit: &str) -> Result<bool, AppManagerError> {
    let status = Command::new(SYSTEMCTL_PATH)
        .args(["is-active", "--quiet", unit])
        .status()
        .map_err(|error| AppManagerError::CommandIo("systemctl", error))?;
    Ok(status.success())
}

fn secure_metadata(path: &Path, field: &'static str) -> Result<Metadata, AppManagerError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| AppManagerError::InvalidPackagePath(field))?;
    if metadata.file_type().is_symlink() {
        return Err(AppManagerError::InvalidPackagePath(field));
    }
    Ok(metadata)
}

fn tree_bytes(root: &Path) -> std::io::Result<u64> {
    let metadata = fs::symlink_metadata(root)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "usage root must be a directory, not a symbolic link",
        ));
    }

    let mut total = metadata.len();
    let mut visited = 1usize;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            visited = visited.saturating_add(1);
            if visited > MAX_USAGE_TREE_ENTRIES {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "usage tree exceeds the bounded entry count",
                ));
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            total = total.saturating_add(metadata.len());
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                pending.push(entry.path());
            }
        }
    }
    Ok(total)
}

fn require_owner_mode(
    metadata: &Metadata,
    expected_uid: u32,
    forbidden_mode: u32,
    field: &'static str,
) -> Result<(), AppManagerError> {
    if metadata.uid() != expected_uid || metadata.mode() & forbidden_mode != 0 {
        return Err(AppManagerError::InvalidHostIdentity(format!(
            "{field} must be owned by UID {expected_uid} and mode must exclude {forbidden_mode:#o}"
        )));
    }
    Ok(())
}

fn verify_directory(path: &Path, field: &'static str) -> Result<(), AppManagerError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| AppManagerError::InvalidPackagePath(field))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AppManagerError::InvalidPackagePath(field));
    }
    Ok(())
}

fn verify_regular_file(path: &Path, field: &'static str) -> Result<(), AppManagerError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| AppManagerError::InvalidPackagePath(field))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppManagerError::InvalidPackagePath(field));
    }
    Ok(())
}

fn verify_relative_file(
    root: &Path,
    relative: &Path,
    field: &'static str,
) -> Result<(), AppManagerError> {
    let mut current = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(value) = component else {
            return Err(AppManagerError::InvalidPackagePath(field));
        };
        current.push(value);
        if index + 1 == components.len() {
            verify_regular_file(&current, field)?;
        } else {
            verify_directory(&current, field)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    use super::*;

    fn fixture(name: &str) -> (ManagerPaths, AppManifest) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("manager-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove previous repository fixture");
        }
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let manifest = crate::tests::manifest();
        let apps_root = root.join("apps");
        let package = apps_root.join(&manifest.id).join(&manifest.version);
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        fs::create_dir_all(root.join("registry")).unwrap();
        fs::write(
            package.join("app.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        fs::write(package.join(&manifest.entrypoint), b"wasm").unwrap();
        let runtime = root.join("app-runtime");
        fs::write(&runtime, b"runtime").unwrap();
        (
            ManagerPaths {
                registry_path: root.join("registry/apps.json"),
                layout: AppLayout {
                    apps_root,
                    data_root: root.join("data"),
                    runtime_path: runtime,
                    broker_socket: root.join("broker.sock"),
                    wayland_socket: root.join("wayland.sock"),
                },
            },
            manifest,
        )
    }

    #[test]
    fn derives_plan_only_from_registered_install() {
        let (paths, manifest) = fixture("registered");
        let mut manager = AppManager::from_registry(paths, AppRegistry::default()).unwrap();
        assert!(matches!(
            manager.sandbox_plan(&manifest.id),
            Err(AppManagerError::NotInstalled(_))
        ));
        let installed = manager.mark_installed(&manifest).unwrap();
        let plan = manager.sandbox_plan(&manifest.id).unwrap();

        assert_eq!(installed.account_uid, crate::FIRST_APP_ACCOUNT_ID);
        assert_eq!(plan.user, "cp0-app-20000");
        assert!(
            plan.package_dir
                .ends_with(&format!("/{}/{}", manifest.id, manifest.version))
        );
    }

    #[test]
    fn rejects_manifest_identity_change() {
        let (paths, manifest) = fixture("identity");
        let mut manager = AppManager::from_registry(paths.clone(), AppRegistry::default()).unwrap();
        manager.mark_installed(&manifest).unwrap();
        let package = paths
            .layout
            .apps_root
            .join(&manifest.id)
            .join(&manifest.version);
        let mut changed = manifest.clone();
        changed.id = "dev.cardputerzero.changed".into();
        fs::write(
            package.join("app.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();

        assert!(matches!(
            manager.sandbox_plan(&manifest.id),
            Err(AppManagerError::IdentityMismatch)
        ));
        assert_eq!(
            manager.unit_for_app(&manifest.id).unwrap(),
            "cardputerzero-app-20000.service"
        );
    }

    #[test]
    fn rejects_symlink_in_entrypoint_path() {
        let (paths, mut manifest) = fixture("symlink");
        let package = paths
            .layout
            .apps_root
            .join(&manifest.id)
            .join(&manifest.version);
        manifest.entrypoint = "linked/module.wasm".into();
        fs::write(
            package.join("app.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        symlink(package.join("bin"), package.join("linked")).unwrap();
        let mut manager = AppManager::from_registry(paths, AppRegistry::default()).unwrap();

        assert!(matches!(
            manager.mark_installed(&manifest),
            Err(AppManagerError::InvalidPackagePath("entrypoint"))
        ));
    }

    #[test]
    fn measures_package_and_private_data_without_following_links() {
        let (paths, manifest) = fixture("usage");
        let mut manager = AppManager::from_registry(paths.clone(), AppRegistry::default()).unwrap();
        manager.mark_installed(&manifest).unwrap();
        let data = paths.layout.data_root.join(&manifest.id);
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("state.bin"), vec![7; 123]).unwrap();
        symlink("/dev/null", data.join("outside")).unwrap();

        let usage = manager.app_usage(&manifest.id).unwrap();
        assert!(usage.package_bytes >= 4);
        assert!(usage.data_bytes >= 123);
        assert!(usage.data_bytes < 4096);
    }

    #[test]
    fn uninstall_removes_packages_but_preserves_private_data_and_identity() {
        let (paths, manifest) = fixture("uninstall");
        let mut manager = AppManager::from_registry(paths.clone(), AppRegistry::default()).unwrap();
        let installed = manager.mark_installed(&manifest).unwrap();
        let data = paths.layout.data_root.join(&manifest.id);
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("state.bin"), b"private").unwrap();

        let removed = manager.uninstall_stopped(&manifest.id).unwrap();
        assert_eq!(removed.app_id, manifest.id);
        assert!(!removed.package_cleanup_pending);
        assert!(!paths.layout.apps_root.join(&manifest.id).exists());
        assert_eq!(fs::read(data.join("state.bin")).unwrap(), b"private");
        assert_eq!(
            manager.registry().account(&manifest.id).unwrap().unix_uid,
            installed.account_uid
        );
        assert!(manager.installed_apps().is_empty());
    }
}
