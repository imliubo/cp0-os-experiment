use std::fmt;
use std::path::{Component, Path, PathBuf};

use cp0_manifest::AppManifest;

use crate::{AppLayout, AppRegistry, RegistryError, SandboxPlan, build_sandbox_plan};

pub const DEFAULT_REGISTRY_PATH: &str = "/var/lib/cardputerzero/registry/apps.json";

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
