use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cp0_manifest::{AppManifest, Permission};
use serde::{Deserialize, Serialize};

pub const PERMISSION_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_PERMISSION_PATH: &str = "/var/lib/cardputerzero/registry/permissions.json";

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StoredDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppDecisions {
    decisions: BTreeMap<Permission, StoredDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionStore {
    schema_version: u32,
    apps: BTreeMap<String, AppDecisions>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Authorization {
    Allow,
    Deny,
    Prompt,
    Undeclared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionChoice {
    AllowOnce,
    AllowAlways,
    Deny,
}

#[derive(Debug)]
pub enum PermissionError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Invalid(String),
    Undeclared,
}

impl fmt::Display for PermissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "permission database I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid permission database JSON: {error}"),
            Self::Invalid(error) => write!(formatter, "invalid permission database: {error}"),
            Self::Undeclared => {
                formatter.write_str("application did not declare the requested permission")
            }
        }
    }
}

impl std::error::Error for PermissionError {}

impl From<std::io::Error> for PermissionError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for PermissionError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl Default for PermissionStore {
    fn default() -> Self {
        Self {
            schema_version: PERMISSION_SCHEMA_VERSION,
            apps: BTreeMap::new(),
        }
    }
}

impl PermissionStore {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PermissionError> {
        let file = File::open(path)?;
        let store: Self = serde_json::from_reader(BufReader::new(file))?;
        store.validate()?;
        Ok(store)
    }

    pub fn load_secure(path: impl AsRef<Path>) -> Result<Self, PermissionError> {
        let path = path.as_ref();
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_file() {
            return Err(PermissionError::Invalid(
                "permission database must be a regular file, not a symbolic link".into(),
            ));
        }
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(PermissionError::Invalid(
                "permission database must be root-owned and not group/world writable".into(),
            ));
        }
        Self::load(path)
    }

    pub fn save_atomic(&self, path: impl AsRef<Path>) -> Result<(), PermissionError> {
        self.validate()?;
        let path = path.as_ref();
        let parent = path.parent().ok_or_else(|| {
            PermissionError::Invalid("permission path must have a parent directory".into())
        })?;
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                PermissionError::Invalid("permission path must have a UTF-8 file name".into())
            })?;
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary_path = parent.join(format!(
            ".{file_name}.tmp-{}-{sequence}",
            std::process::id()
        ));
        let result = (|| -> Result<(), PermissionError> {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary_path)?;
            let mut writer = BufWriter::new(file);
            serde_json::to_writer_pretty(&mut writer, self)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            writer.get_ref().sync_all()?;
            fs::rename(&temporary_path, path)?;
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result
    }

    fn decision(&self, app_id: &str, permission: Permission) -> Option<StoredDecision> {
        self.apps
            .get(app_id)
            .and_then(|app| app.decisions.get(&permission))
            .copied()
    }

    fn set(&mut self, app_id: &str, permission: Permission, decision: StoredDecision) {
        self.apps
            .entry(app_id.into())
            .or_default()
            .decisions
            .insert(permission, decision);
    }

    fn validate(&self) -> Result<(), PermissionError> {
        if self.schema_version != PERMISSION_SCHEMA_VERSION {
            return Err(PermissionError::Invalid(format!(
                "schema_version must be {PERMISSION_SCHEMA_VERSION}"
            )));
        }
        for app_id in self.apps.keys() {
            if !cp0_manifest::is_valid_app_id(app_id) {
                return Err(PermissionError::Invalid(format!(
                    "invalid application ID {app_id:?}"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct PermissionEngine {
    path: PathBuf,
    store: PermissionStore,
    allow_once: BTreeSet<(String, Permission)>,
}

impl PermissionEngine {
    pub fn new(path: impl Into<PathBuf>, store: PermissionStore) -> Result<Self, PermissionError> {
        store.validate()?;
        Ok(Self {
            path: path.into(),
            store,
            allow_once: BTreeSet::new(),
        })
    }

    pub fn authorize(&self, manifest: &AppManifest, permission: Permission) -> Authorization {
        if !declares(manifest, permission) {
            return Authorization::Undeclared;
        }
        if self.allow_once.contains(&(manifest.id.clone(), permission)) {
            return Authorization::Allow;
        }
        match self.store.decision(&manifest.id, permission) {
            Some(StoredDecision::Allow) => Authorization::Allow,
            Some(StoredDecision::Deny) => Authorization::Deny,
            None => Authorization::Prompt,
        }
    }

    pub fn resolve(
        &mut self,
        manifest: &AppManifest,
        permission: Permission,
        choice: PermissionChoice,
    ) -> Result<(), PermissionError> {
        if !declares(manifest, permission) {
            return Err(PermissionError::Undeclared);
        }
        match choice {
            PermissionChoice::AllowOnce => {
                self.allow_once.insert((manifest.id.clone(), permission));
                Ok(())
            }
            PermissionChoice::AllowAlways => {
                self.store
                    .set(&manifest.id, permission, StoredDecision::Allow);
                self.store.save_atomic(&self.path)
            }
            PermissionChoice::Deny => {
                self.allow_once.remove(&(manifest.id.clone(), permission));
                self.store
                    .set(&manifest.id, permission, StoredDecision::Deny);
                self.store.save_atomic(&self.path)
            }
        }
    }

    pub fn clear_session(&mut self, app_id: &str) {
        self.allow_once.retain(|(id, _)| id != app_id);
    }
}

fn declares(manifest: &AppManifest, permission: Permission) -> bool {
    manifest
        .permissions
        .iter()
        .any(|request| request.name == permission)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("permissions-{name}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir_all(&path).unwrap();
        fs::canonicalize(path).unwrap().join("permissions.json")
    }

    #[test]
    fn defaults_to_prompt_and_rejects_undeclared_capability() {
        let engine = PermissionEngine::new(fixture("default"), PermissionStore::default()).unwrap();
        let manifest = crate::tests::manifest();

        assert_eq!(
            engine.authorize(&manifest, Permission::NotificationsPost),
            Authorization::Prompt
        );
        assert_eq!(
            engine.authorize(&manifest, Permission::CameraCapture),
            Authorization::Undeclared
        );
    }

    #[test]
    fn allow_once_is_session_only_and_deny_is_persistent() {
        let path = fixture("decisions");
        let manifest = crate::tests::manifest();
        let mut engine = PermissionEngine::new(&path, PermissionStore::default()).unwrap();
        engine
            .resolve(
                &manifest,
                Permission::NotificationsPost,
                PermissionChoice::AllowOnce,
            )
            .unwrap();
        assert_eq!(
            engine.authorize(&manifest, Permission::NotificationsPost),
            Authorization::Allow
        );
        assert!(!path.exists());
        engine.clear_session(&manifest.id);
        assert_eq!(
            engine.authorize(&manifest, Permission::NotificationsPost),
            Authorization::Prompt
        );

        engine
            .resolve(
                &manifest,
                Permission::NotificationsPost,
                PermissionChoice::Deny,
            )
            .unwrap();
        let loaded = PermissionStore::load(&path).unwrap();
        let reloaded = PermissionEngine::new(&path, loaded).unwrap();
        assert_eq!(
            reloaded.authorize(&manifest, Permission::NotificationsPost),
            Authorization::Deny
        );
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn never_resolves_undeclared_permission() {
        let mut engine =
            PermissionEngine::new(fixture("undeclared"), PermissionStore::default()).unwrap();
        assert!(matches!(
            engine.resolve(
                &crate::tests::manifest(),
                Permission::CameraCapture,
                PermissionChoice::AllowAlways,
            ),
            Err(PermissionError::Undeclared)
        ));
    }

    #[test]
    fn allow_always_survives_engine_restart() {
        let path = fixture("allow-always");
        let manifest = crate::tests::manifest();
        let mut engine = PermissionEngine::new(&path, PermissionStore::default()).unwrap();
        engine
            .resolve(
                &manifest,
                Permission::NotificationsPost,
                PermissionChoice::AllowAlways,
            )
            .unwrap();

        let reloaded = PermissionEngine::new(&path, PermissionStore::load(&path).unwrap()).unwrap();
        assert_eq!(
            reloaded.authorize(&manifest, Permission::NotificationsPost),
            Authorization::Allow
        );
    }

    #[cfg(unix)]
    #[test]
    fn secure_load_rejects_symbolic_links() {
        use std::os::unix::fs::symlink;

        let path = fixture("symlink");
        let target = path.with_file_name("target.json");
        PermissionStore::default().save_atomic(&target).unwrap();
        symlink(&target, &path).unwrap();

        assert!(matches!(
            PermissionStore::load_secure(path),
            Err(PermissionError::Invalid(_))
        ));
    }
}
