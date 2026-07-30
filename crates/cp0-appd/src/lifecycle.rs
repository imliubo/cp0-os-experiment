use std::ffi::CString;
use std::fmt;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use cp0_manifest::AppManifest;

use crate::{AppLayout, AppRegistry, RegistryError, SandboxPlan, build_sandbox_plan};

pub const DEFAULT_REGISTRY_PATH: &str = "/var/lib/cardputerzero/registry/apps.json";
const SYSTEMD_RUN_PATH: &str = "/usr/bin/systemd-run";
const SYSTEMCTL_PATH: &str = "/usr/bin/systemctl";

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
    UnitFailed(&'static str),
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
            Self::UnitFailed(action) => {
                write!(formatter, "application systemd unit failed to {action}")
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
        let account = self.registry.mark_installed(manifest)?;
        self.registry.save_atomic(&self.paths.registry_path)?;
        Ok(InstalledApp {
            app_id: manifest.id.clone(),
            version: manifest.version.clone(),
            account_user: account.unix_user,
            account_uid: account.unix_uid,
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
                    })
            })
            .collect()
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

    fn unit_for_app(&self, app_id: &str) -> Result<String, AppManagerError> {
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
        require_root_directory(&self.paths.layout.data_root, "data root")?;
        let runtime = secure_metadata(Path::new(&self.paths.layout.runtime_path), "runtime")?;
        require_owner_mode(&runtime, 0, 0o022, "runtime")?;
        if !runtime.is_file() || runtime.mode() & 0o111 == 0 {
            return Err(AppManagerError::InvalidHostIdentity(
                "runtime is not executable".into(),
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

        let data = secure_metadata(Path::new(&plan.data_dir), "data directory")?;
        require_owner_mode(&data, account.unix_uid, 0o077, "data directory")?;
        if !data.is_dir() {
            return Err(AppManagerError::InvalidHostIdentity(
                "data path is not a directory".into(),
            ));
        }
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
}
