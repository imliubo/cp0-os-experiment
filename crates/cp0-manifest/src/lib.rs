use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_APP_MEMORY_MB: u16 = 96;
pub const MAX_APP_STORAGE_MB: u16 = 512;
pub const MAX_INTENTS_PER_MANIFEST: usize = 8;
pub const MAX_INTENT_ACTION_BYTES: usize = 96;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub sdk_version: String,
    pub runtime: Runtime,
    pub entrypoint: String,
    pub display: DisplayMode,
    pub resources: ResourceLimits,
    pub permissions: Vec<PermissionRequest>,
    #[serde(default)]
    pub intents: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Runtime {
    Wamr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DisplayMode {
    Standard,
    Immersive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLimits {
    pub memory_mb: u16,
    pub storage_mb: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRequest {
    pub name: Permission,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Permission {
    #[serde(rename = "network.client")]
    NetworkClient,
    #[serde(rename = "documents.open")]
    DocumentsOpen,
    #[serde(rename = "audio.playback")]
    AudioPlayback,
    #[serde(rename = "audio.capture")]
    AudioCapture,
    #[serde(rename = "camera.capture")]
    CameraCapture,
    #[serde(rename = "radio.lora")]
    RadioLora,
    #[serde(rename = "hardware.gpio")]
    HardwareGpio,
    #[serde(rename = "clipboard.read")]
    ClipboardRead,
    #[serde(rename = "notifications.post")]
    NotificationsPost,
}

#[derive(Debug)]
pub enum ManifestError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Invalid(Vec<String>),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "cannot read manifest: {error}"),
            Self::Json(error) => write!(f, "invalid manifest JSON: {error}"),
            Self::Invalid(errors) => write!(f, "{}", errors.join("\n")),
        }
    }
}

impl std::error::Error for ManifestError {}

pub fn load_and_validate(path: impl AsRef<Path>) -> Result<AppManifest, ManifestError> {
    let file = File::open(path).map_err(ManifestError::Io)?;
    let manifest: AppManifest =
        serde_json::from_reader(BufReader::new(file)).map_err(ManifestError::Json)?;
    validate(&manifest).map_err(ManifestError::Invalid)?;
    Ok(manifest)
}

pub fn validate(manifest: &AppManifest) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    if manifest.schema_version != SCHEMA_VERSION {
        errors.push(format!(
            "schema_version must be {SCHEMA_VERSION}, got {}",
            manifest.schema_version
        ));
    }
    if !is_valid_app_id(&manifest.id) {
        errors.push("id must be a lowercase reverse-domain name with at least three parts".into());
    }
    let name_len = manifest.name.chars().count();
    if !(1..=32).contains(&name_len) {
        errors.push("name must contain between 1 and 32 characters".into());
    }
    if !is_valid_app_version(&manifest.version) {
        errors.push("version must be a valid three-part semantic version".into());
    }
    if !valid_sdk_version(&manifest.sdk_version) {
        errors.push("sdk_version must use the <major>.<minor> format".into());
    }
    if !valid_entrypoint(&manifest.entrypoint) {
        errors.push("entrypoint must be a relative .wasm or .aot path inside the package".into());
    }
    if !(8..=MAX_APP_MEMORY_MB).contains(&manifest.resources.memory_mb) {
        errors.push(format!(
            "resources.memory_mb must be between 8 and {MAX_APP_MEMORY_MB}"
        ));
    }
    if !(1..=MAX_APP_STORAGE_MB).contains(&manifest.resources.storage_mb) {
        errors.push(format!(
            "resources.storage_mb must be between 1 and {MAX_APP_STORAGE_MB}"
        ));
    }

    let mut permissions = HashSet::new();
    for permission in &manifest.permissions {
        if !permissions.insert(permission.name) {
            errors.push(format!(
                "permission {:?} is declared more than once",
                permission.name
            ));
        }
        let reason_len = permission.reason.chars().count();
        if !(5..=160).contains(&reason_len) {
            errors.push(format!(
                "permission {:?} reason must contain between 5 and 160 characters",
                permission.name
            ));
        }
    }

    if manifest.intents.len() > MAX_INTENTS_PER_MANIFEST {
        errors.push(format!(
            "intents must contain at most {MAX_INTENTS_PER_MANIFEST} actions"
        ));
    }
    let mut intents = HashSet::new();
    for action in &manifest.intents {
        if !is_valid_intent_action(action) {
            errors.push(format!(
                "intent action {action:?} must be a lowercase reverse-domain name no longer than {MAX_INTENT_ACTION_BYTES} bytes"
            ));
        }
        if !intents.insert(action) {
            errors.push(format!(
                "intent action {action:?} is declared more than once"
            ));
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn is_valid_app_id(id: &str) -> bool {
    if id.len() > 128 {
        return false;
    }

    let parts: Vec<_> = id.split('.').collect();
    parts.len() >= 3
        && parts.iter().all(|part| {
            part.len() <= 32
                && part.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                && !part.ends_with('-')
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

pub fn is_valid_intent_action(action: &str) -> bool {
    if action.is_empty() || action.len() > MAX_INTENT_ACTION_BYTES {
        return false;
    }
    let parts: Vec<_> = action.split('.').collect();
    parts.len() >= 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 32
                && part.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                && !part.ends_with('-')
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

pub fn is_valid_app_version(version: &str) -> bool {
    if version.is_empty() || version.len() > 64 {
        return false;
    }
    let (without_build, build) = version
        .split_once('+')
        .map_or((version, None), |(left, right)| (left, Some(right)));
    let (core, prerelease) = without_build
        .split_once('-')
        .map_or((without_build, None), |(left, right)| (left, Some(right)));

    let core_parts: Vec<_> = core.split('.').collect();
    core_parts.len() == 3
        && core_parts.iter().all(|part| valid_numeric_identifier(part))
        && prerelease.is_none_or(valid_prerelease_identifiers)
        && build.is_none_or(valid_semver_identifiers)
}

fn valid_numeric_identifier(identifier: &str) -> bool {
    !identifier.is_empty()
        && identifier.bytes().all(|byte| byte.is_ascii_digit())
        && (identifier == "0" || !identifier.starts_with('0'))
}

fn valid_semver_identifiers(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

fn valid_prerelease_identifiers(value: &str) -> bool {
    valid_semver_identifiers(value)
        && value.split('.').all(|part| {
            !part.bytes().all(|byte| byte.is_ascii_digit()) || valid_numeric_identifier(part)
        })
}

fn valid_sdk_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 2 && parts.iter().all(|part| valid_numeric_identifier(part))
}

fn valid_entrypoint(entrypoint: &str) -> bool {
    let path = Path::new(entrypoint);
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("wasm" | "aot")
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest() -> AppManifest {
        AppManifest {
            schema_version: 1,
            id: "dev.cardputerzero.example".into(),
            name: "Example".into(),
            version: "1.2.3-beta.1+build.7".into(),
            sdk_version: "0.1".into(),
            runtime: Runtime::Wamr,
            entrypoint: "bin/example.wasm".into(),
            display: DisplayMode::Standard,
            resources: ResourceLimits {
                memory_mb: 32,
                storage_mb: 16,
            },
            permissions: vec![PermissionRequest {
                name: Permission::NotificationsPost,
                reason: "Notify the user when the operation is complete".into(),
            }],
            intents: vec!["dev.cardputerzero.example.open".into()],
        }
    }

    #[test]
    fn accepts_valid_manifest() {
        assert_eq!(validate(&valid_manifest()), Ok(()));
    }

    #[test]
    fn rejects_path_escape_and_excess_memory() {
        let mut manifest = valid_manifest();
        manifest.entrypoint = "../escape.wasm".into();
        manifest.resources.memory_mb = 128;

        let errors = validate(&manifest).expect_err("manifest should be rejected");
        assert!(errors.iter().any(|error| error.contains("entrypoint")));
        assert!(errors.iter().any(|error| error.contains("memory_mb")));
    }

    #[test]
    fn rejects_malicious_path_escape_fixture() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/malicious/path-escape-app.json");
        let error = load_and_validate(path).expect_err("path escape fixture should be rejected");
        assert!(error.to_string().contains("entrypoint"));
    }

    #[test]
    fn rejects_duplicate_permissions() {
        let mut manifest = valid_manifest();
        manifest.permissions.push(manifest.permissions[0].clone());

        let errors = validate(&manifest).expect_err("manifest should be rejected");
        assert!(errors.iter().any(|error| error.contains("more than once")));
    }

    #[test]
    fn validates_bounded_unique_intent_actions() {
        let mut manifest = valid_manifest();
        manifest.intents = vec!["dev.cardputerzero.example.open".into(); 8];
        manifest.intents.push("Open".into());

        let errors = validate(&manifest).expect_err("manifest should be rejected");
        assert!(errors.iter().any(|error| error.contains("reverse-domain")));
        assert!(errors.iter().any(|error| error.contains("at most")));
        assert!(errors.iter().any(|error| error.contains("more than once")));
        assert!(is_valid_intent_action("dev.cardputerzero.documents.open"));
        assert!(!is_valid_intent_action("dev.cardputerzero.bad_action"));
    }

    #[test]
    fn rejects_semver_numeric_prerelease_with_leading_zero() {
        let mut manifest = valid_manifest();
        manifest.version = "1.2.3-beta.01".into();

        let errors = validate(&manifest).expect_err("manifest should be rejected");
        assert!(
            errors
                .iter()
                .any(|error| error.contains("semantic version"))
        );
    }

    #[test]
    fn rejects_excessively_long_version() {
        let mut app = valid_manifest();
        app.version = format!("1.0.0+{}", "a".repeat(64));
        assert!(validate(&app).is_err());
    }

    #[test]
    fn rejects_unknown_json_fields() {
        let json = r#"{
            "schema_version": 1,
            "id": "dev.cardputerzero.example",
            "name": "Example",
            "version": "1.0.0",
            "sdk_version": "0.1",
            "runtime": "wamr",
            "entrypoint": "app.wasm",
            "display": "standard",
            "resources": { "memory_mb": 32, "storage_mb": 8 },
            "permissions": [],
            "unexpected": true
        }"#;

        assert!(serde_json::from_str::<AppManifest>(json).is_err());
    }
}
