use std::fmt;
use std::path::{Component, Path, PathBuf};

use cp0_manifest::AppManifest;
use serde::Serialize;

pub const DEFAULT_APPS_ROOT: &str = "/var/lib/cardputerzero/apps";
pub const DEFAULT_DATA_ROOT: &str = "/var/lib/cardputerzero/data";
pub const DEFAULT_RUNTIME: &str = "/usr/libexec/cardputerzero/app-runtime";
pub const BWRAP_PATH: &str = "/usr/bin/bwrap";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLayout {
    pub apps_root: PathBuf,
    pub data_root: PathBuf,
    pub runtime_path: PathBuf,
}

impl Default for AppLayout {
    fn default() -> Self {
        Self {
            apps_root: PathBuf::from(DEFAULT_APPS_ROOT),
            data_root: PathBuf::from(DEFAULT_DATA_ROOT),
            runtime_path: PathBuf::from(DEFAULT_RUNTIME),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SandboxPlan {
    pub app_id: String,
    pub app_version: String,
    pub user: String,
    pub unit: String,
    pub memory_max_bytes: u64,
    pub package_dir: String,
    pub data_dir: String,
    pub program: String,
    pub arguments: Vec<String>,
    pub systemd_properties: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    InvalidAppUser,
    InvalidLayout(&'static str),
    NonUtf8Path(&'static str),
}

impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAppUser => formatter
                .write_str("app user must use the cp0-app-<numeric-id> system account format"),
            Self::InvalidLayout(field) => {
                write!(formatter, "{field} must be an absolute normalized path")
            }
            Self::NonUtf8Path(field) => write!(formatter, "{field} must be valid UTF-8"),
        }
    }
}

impl std::error::Error for PlanError {}

pub fn build_sandbox_plan(
    manifest: &AppManifest,
    app_user: &str,
    layout: &AppLayout,
) -> Result<SandboxPlan, PlanError> {
    if !valid_app_user(app_user) {
        return Err(PlanError::InvalidAppUser);
    }
    validate_root(&layout.apps_root, "apps_root")?;
    validate_root(&layout.data_root, "data_root")?;
    validate_root(&layout.runtime_path, "runtime_path")?;

    let package_dir = layout.apps_root.join(&manifest.id).join(&manifest.version);
    let data_dir = layout.data_root.join(&manifest.id);
    let package = path_text(&package_dir, "package_dir")?;
    let data = path_text(&data_dir, "data_dir")?;
    let runtime = path_text(&layout.runtime_path, "runtime_path")?;
    let entrypoint = format!("/app/{}", manifest.entrypoint);
    let unit = format!("cardputerzero-app-{}.service", &app_user[8..]);
    let memory_max_bytes = u64::from(manifest.resources.memory_mb) * 1024 * 1024;

    let arguments = vec![
        "--unshare-all".into(),
        "--die-with-parent".into(),
        "--new-session".into(),
        "--clearenv".into(),
        "--dir".into(),
        "/runtime".into(),
        "--dir".into(),
        "/app".into(),
        "--dir".into(),
        "/data".into(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        "--ro-bind".into(),
        runtime,
        "/runtime/cardputerzero-app-runtime".into(),
        "--ro-bind".into(),
        package.clone(),
        "/app".into(),
        "--bind".into(),
        data.clone(),
        "/data".into(),
        "--chdir".into(),
        "/app".into(),
        "--setenv".into(),
        "HOME".into(),
        "/data".into(),
        "--setenv".into(),
        "XDG_DATA_HOME".into(),
        "/data".into(),
        "--setenv".into(),
        "TMPDIR".into(),
        "/tmp".into(),
        "--".into(),
        "/runtime/cardputerzero-app-runtime".into(),
        entrypoint,
    ];

    let systemd_properties = vec![
        format!("User={app_user}"),
        format!("Group={app_user}"),
        format!("MemoryMax={memory_max_bytes}"),
        "TasksMax=32".into(),
        "UMask=0077".into(),
        "NoNewPrivileges=yes".into(),
        "PrivateDevices=yes".into(),
        "PrivateTmp=yes".into(),
        "ProtectSystem=strict".into(),
        "ProtectHome=yes".into(),
        "ProtectKernelTunables=yes".into(),
        "ProtectKernelModules=yes".into(),
        "ProtectControlGroups=yes".into(),
        "RestrictAddressFamilies=AF_UNIX".into(),
        "CapabilityBoundingSet=".into(),
        "AmbientCapabilities=".into(),
        "LockPersonality=yes".into(),
        "RestrictRealtime=yes".into(),
        "SystemCallArchitectures=native".into(),
    ];

    Ok(SandboxPlan {
        app_id: manifest.id.clone(),
        app_version: manifest.version.clone(),
        user: app_user.into(),
        unit,
        memory_max_bytes,
        package_dir: package,
        data_dir: data,
        program: BWRAP_PATH.into(),
        arguments,
        systemd_properties,
    })
}

pub fn systemd_run_arguments(plan: &SandboxPlan) -> Vec<String> {
    let mut arguments = vec![
        "--quiet".into(),
        "--collect".into(),
        format!("--unit={}", plan.unit),
    ];
    for property in &plan.systemd_properties {
        arguments.push(format!("--property={property}"));
    }
    arguments.push("--".into());
    arguments.push(plan.program.clone());
    arguments.extend(plan.arguments.iter().cloned());
    arguments
}

fn valid_app_user(user: &str) -> bool {
    let Some(identifier) = user.strip_prefix("cp0-app-") else {
        return false;
    };
    !identifier.is_empty()
        && identifier.len() <= 5
        && identifier.bytes().all(|byte| byte.is_ascii_digit())
        && !identifier.starts_with('0')
}

fn validate_root(path: &Path, field: &'static str) -> Result<(), PlanError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
    {
        return Err(PlanError::InvalidLayout(field));
    }
    Ok(())
}

fn path_text(path: &Path, field: &'static str) -> Result<String, PlanError> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or(PlanError::NonUtf8Path(field))
}

#[cfg(test)]
mod tests {
    use cp0_manifest::{DisplayMode, Permission, PermissionRequest, ResourceLimits, Runtime};

    use super::*;

    fn manifest() -> AppManifest {
        AppManifest {
            schema_version: 1,
            id: "dev.cardputerzero.hello".into(),
            name: "Hello".into(),
            version: "1.2.3".into(),
            sdk_version: "0.1".into(),
            runtime: Runtime::Wamr,
            entrypoint: "bin/hello.wasm".into(),
            display: DisplayMode::Standard,
            resources: ResourceLimits {
                memory_mb: 24,
                storage_mb: 8,
            },
            permissions: vec![PermissionRequest {
                name: Permission::NotificationsPost,
                reason: "Show a completed notification".into(),
            }],
        }
    }

    #[test]
    fn creates_closed_default_sandbox() {
        let plan = build_sandbox_plan(&manifest(), "cp0-app-1001", &AppLayout::default())
            .expect("plan should be valid");

        assert_eq!(plan.unit, "cardputerzero-app-1001.service");
        assert_eq!(plan.memory_max_bytes, 24 * 1024 * 1024);
        assert_eq!(
            plan.package_dir,
            "/var/lib/cardputerzero/apps/dev.cardputerzero.hello/1.2.3"
        );
        assert!(
            plan.arguments
                .windows(2)
                .any(|pair| pair == ["--unshare-all", "--die-with-parent"])
        );
        assert!(plan.arguments.windows(3).any(|values| values
            == [
                "--ro-bind",
                "/var/lib/cardputerzero/apps/dev.cardputerzero.hello/1.2.3",
                "/app"
            ]));
        assert!(plan.arguments.windows(3).any(|values| values
            == [
                "--bind",
                "/var/lib/cardputerzero/data/dev.cardputerzero.hello",
                "/data"
            ]));
        for directory in ["/runtime", "/app", "/data"] {
            assert!(
                plan.arguments
                    .windows(2)
                    .any(|values| values == ["--dir", directory])
            );
        }
        assert!(!plan.arguments.iter().any(|value| value == "/usr"));
        assert!(
            plan.systemd_properties
                .contains(&"RestrictAddressFamilies=AF_UNIX".into())
        );
        assert!(
            plan.systemd_properties
                .contains(&"CapabilityBoundingSet=".into())
        );
        assert!(
            plan.systemd_properties
                .contains(&"PrivateDevices=yes".into())
        );
    }

    #[test]
    fn renders_systemd_run_without_shell_parsing() {
        let plan = build_sandbox_plan(&manifest(), "cp0-app-42", &AppLayout::default())
            .expect("plan should be valid");
        let arguments = systemd_run_arguments(&plan);

        assert_eq!(arguments[0], "--quiet");
        assert!(arguments.contains(&"--unit=cardputerzero-app-42.service".into()));
        assert!(arguments.contains(&"--property=MemoryMax=25165824".into()));
        assert!(arguments.contains(&BWRAP_PATH.into()));
    }

    #[test]
    fn rejects_untrusted_user_and_layout_values() {
        assert_eq!(
            build_sandbox_plan(&manifest(), "root", &AppLayout::default()),
            Err(PlanError::InvalidAppUser)
        );
        let mut layout = AppLayout::default();
        layout.data_root = PathBuf::from("relative/data");
        assert_eq!(
            build_sandbox_plan(&manifest(), "cp0-app-1", &layout),
            Err(PlanError::InvalidLayout("data_root"))
        );
    }
}
