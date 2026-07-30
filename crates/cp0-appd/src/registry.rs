use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

pub const REGISTRY_SCHEMA_VERSION: u32 = 1;
pub const FIRST_APP_ACCOUNT_ID: u32 = 20_000;
pub const LAST_APP_ACCOUNT_ID: u32 = 59_999;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppAccount {
    pub account_id: u32,
    pub unix_user: String,
    pub unix_uid: u32,
    #[serde(default)]
    pub installed_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppRegistry {
    pub schema_version: u32,
    pub next_account_id: u32,
    pub apps: BTreeMap<String, AppAccount>,
}

#[derive(Debug)]
pub enum RegistryError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Invalid(String),
    Exhausted,
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "application registry I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid application registry JSON: {error}"),
            Self::Invalid(error) => write!(formatter, "invalid application registry: {error}"),
            Self::Exhausted => formatter.write_str("application account range is exhausted"),
        }
    }
}

impl std::error::Error for RegistryError {}

impl From<std::io::Error> for RegistryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RegistryError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl Default for AppRegistry {
    fn default() -> Self {
        Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
            next_account_id: FIRST_APP_ACCOUNT_ID,
            apps: BTreeMap::new(),
        }
    }
}

impl AppRegistry {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, RegistryError> {
        let file = File::open(path)?;
        let registry: Self = serde_json::from_reader(BufReader::new(file))?;
        registry.validate()?;
        Ok(registry)
    }

    pub fn account(&self, app_id: &str) -> Option<&AppAccount> {
        self.apps.get(app_id)
    }

    pub fn installed_app_for_uid(&self, uid: u32) -> Option<(&str, &AppAccount)> {
        self.apps.iter().find_map(|(app_id, account)| {
            (account.unix_uid == uid && account.installed_version.is_some())
                .then_some((app_id.as_str(), account))
        })
    }

    pub fn assign(&mut self, app_id: &str) -> Result<AppAccount, RegistryError> {
        if !cp0_manifest::is_valid_app_id(app_id) {
            return Err(RegistryError::Invalid(format!(
                "invalid application ID {app_id:?}"
            )));
        }
        if let Some(account) = self.apps.get(app_id) {
            return Ok(account.clone());
        }
        if self.next_account_id > LAST_APP_ACCOUNT_ID {
            return Err(RegistryError::Exhausted);
        }

        let account_id = self.next_account_id;
        self.next_account_id += 1;
        let account = AppAccount {
            account_id,
            unix_user: format!("cp0-app-{account_id}"),
            unix_uid: account_id,
            installed_version: None,
        };
        self.apps.insert(app_id.to_owned(), account.clone());
        Ok(account)
    }

    pub fn mark_installed(
        &mut self,
        manifest: &cp0_manifest::AppManifest,
    ) -> Result<AppAccount, RegistryError> {
        cp0_manifest::validate(manifest).map_err(|errors| {
            RegistryError::Invalid(format!("invalid installed manifest: {}", errors.join("; ")))
        })?;
        let mut account = self.assign(&manifest.id)?;
        account.installed_version = Some(manifest.version.clone());
        self.apps.insert(manifest.id.clone(), account.clone());
        Ok(account)
    }

    pub fn validate(&self) -> Result<(), RegistryError> {
        if self.schema_version != REGISTRY_SCHEMA_VERSION {
            return Err(RegistryError::Invalid(format!(
                "schema_version must be {REGISTRY_SCHEMA_VERSION}"
            )));
        }
        if !(FIRST_APP_ACCOUNT_ID..=LAST_APP_ACCOUNT_ID + 1).contains(&self.next_account_id) {
            return Err(RegistryError::Invalid(
                "next_account_id is outside the reserved range".into(),
            ));
        }

        let mut account_ids = BTreeSet::new();
        let mut unix_users = BTreeSet::new();
        let mut highest_account_id = None;
        for (app_id, account) in &self.apps {
            if !cp0_manifest::is_valid_app_id(app_id) {
                return Err(RegistryError::Invalid(format!(
                    "registry contains invalid application ID {app_id:?}"
                )));
            }
            if !(FIRST_APP_ACCOUNT_ID..=LAST_APP_ACCOUNT_ID).contains(&account.account_id) {
                return Err(RegistryError::Invalid(format!(
                    "account {} is outside the reserved range",
                    account.account_id
                )));
            }
            let expected_user = format!("cp0-app-{}", account.account_id);
            if account.unix_user != expected_user || account.unix_uid != account.account_id {
                return Err(RegistryError::Invalid(format!(
                    "account {} has inconsistent Unix identity",
                    account.account_id
                )));
            }
            if !account_ids.insert(account.account_id) || !unix_users.insert(&account.unix_user) {
                return Err(RegistryError::Invalid(
                    "two applications share the same Unix identity".into(),
                ));
            }
            if account
                .installed_version
                .as_deref()
                .is_some_and(|version| !cp0_manifest::is_valid_app_version(version))
            {
                return Err(RegistryError::Invalid(format!(
                    "account {} has an invalid installed version",
                    account.account_id
                )));
            }
            highest_account_id = Some(
                highest_account_id.map_or(account.account_id, |highest: u32| {
                    highest.max(account.account_id)
                }),
            );
        }
        if highest_account_id.is_some_and(|highest| self.next_account_id <= highest) {
            return Err(RegistryError::Invalid(
                "next_account_id would recycle an assigned account".into(),
            ));
        }
        Ok(())
    }

    pub fn save_atomic(&self, path: impl AsRef<Path>) -> Result<(), RegistryError> {
        self.validate()?;
        let path = path.as_ref();
        let parent = path.parent().ok_or_else(|| {
            RegistryError::Invalid("registry path must have a parent directory".into())
        })?;
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                RegistryError::Invalid("registry path must have a UTF-8 file name".into())
            })?;
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary_path = parent.join(format!(
            ".{file_name}.tmp-{}-{sequence}",
            std::process::id()
        ));

        let result = (|| -> Result<(), RegistryError> {
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
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use super::*;

    fn test_directory(name: &str) -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("registry-{name}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).expect("remove previous repository test directory");
        }
        fs::create_dir_all(&path).expect("create repository test directory");
        path
    }

    #[test]
    fn assigns_stable_non_recycled_accounts() {
        let mut registry = AppRegistry::default();
        let first = registry
            .assign("dev.cardputerzero.alpha")
            .expect("assign first account");
        let second = registry
            .assign("dev.cardputerzero.beta")
            .expect("assign second account");

        assert_eq!(first.unix_user, "cp0-app-20000");
        assert_eq!(first.unix_uid, 20_000);
        assert_eq!(second.unix_user, "cp0-app-20001");
        assert_eq!(registry.assign("dev.cardputerzero.alpha").unwrap(), first);
        assert_eq!(registry.next_account_id, 20_002);
    }

    #[test]
    fn records_installed_version_without_changing_identity() {
        let mut registry = AppRegistry::default();
        let first = registry
            .mark_installed(&crate::tests::manifest())
            .expect("register first version");
        let mut upgraded = crate::tests::manifest();
        upgraded.version = "1.2.4".into();
        let second = registry
            .mark_installed(&upgraded)
            .expect("register upgraded version");

        assert_eq!(first.account_id, second.account_id);
        assert_eq!(second.installed_version.as_deref(), Some("1.2.4"));
        assert_eq!(
            registry
                .installed_app_for_uid(second.unix_uid)
                .map(|(app_id, _)| app_id),
            Some("dev.cardputerzero.hello")
        );
        assert!(registry.installed_app_for_uid(0).is_none());
    }

    #[test]
    fn atomically_round_trips_registry() {
        let directory = test_directory("round-trip");
        let path = directory.join("apps.json");
        let mut registry = AppRegistry::default();
        registry
            .assign("dev.cardputerzero.hello")
            .expect("assign account");
        registry.save_atomic(&path).expect("save registry");

        assert_eq!(AppRegistry::load(&path).unwrap(), registry);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn rejects_corrupt_or_recycled_identity() {
        let mut registry = AppRegistry::default();
        let account = registry
            .assign("dev.cardputerzero.hello")
            .expect("assign account");
        registry.next_account_id = account.account_id;
        assert!(matches!(
            registry.validate(),
            Err(RegistryError::Invalid(_))
        ));

        registry.next_account_id = account.account_id + 1;
        registry
            .apps
            .get_mut("dev.cardputerzero.hello")
            .unwrap()
            .unix_uid = 0;
        assert!(matches!(
            registry.validate(),
            Err(RegistryError::Invalid(_))
        ));
    }
}
