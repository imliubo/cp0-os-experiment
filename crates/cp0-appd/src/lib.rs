use std::fmt;
use std::path::{Component, Path, PathBuf};

use cp0_manifest::{AppManifest, DisplayMode};
use serde::Serialize;

mod audio_client;
mod broker;
mod camera_client;
mod document_client;
mod document_prompt;
mod gpio_client;
mod installer;
mod intent;
mod lifecycle;
mod network_client;
mod permission_prompt;
mod permissions;
mod policy;
mod protocol;
mod radio_client;
mod registry;
mod server;
mod storage_client;

pub use audio_client::{AudioClient, AudioClientError, DEFAULT_AUDIO_SOCKET};
pub use broker::{
    BROKER_PROTOCOL_VERSION, BrokerCommand, BrokerErrorCode, BrokerOutcome, BrokerProtocolError,
    BrokerRequest, BrokerResponse, MAX_BROKER_FRAME_BYTES, MAX_PENDING_NOTIFICATIONS, Notification,
    NotificationQueue, NotificationQueueError, encode_broker_response, read_broker_request,
    read_broker_response, write_broker_request, write_broker_response,
};
pub use camera_client::{
    CameraClient, CameraClientError, CapturedCameraFrame, DEFAULT_CAMERA_SOCKET,
};
pub use document_client::{
    DEFAULT_DOCUMENT_SOCKET, DocumentClient, DocumentClientError, OpenedDocument,
};
pub use document_prompt::{
    DocumentCoordinator, DocumentPrompt, DocumentPromptError, DocumentRequestResult,
};
pub use gpio_client::{DEFAULT_GPIO_SOCKET, GpioClient, GpioClientError};
pub use installer::{
    InstallError, PackageInstaller, PreparedInstall, TrustDecision, TrustPaths, TrustPolicy,
};
pub use intent::{
    IntentQueue, IntentQueueError, MAX_INTENT_PAYLOAD_BYTES, MAX_PENDING_INTENTS, PendingIntent,
};
pub use lifecycle::{
    AppManager, AppManagerError, AppUsage, InstalledApp, ManagerPaths, UninstalledApp,
    lookup_unix_account,
};
pub use network_client::{
    DEFAULT_NETWORK_SOCKET, NetworkClient, NetworkClientError, NetworkHttpResponse,
};
pub use permission_prompt::{
    PermissionCoordinator, PermissionPrompt, PermissionPromptError, PermissionRequestResult,
};
pub use permissions::{
    Authorization, DEFAULT_PERMISSION_PATH, PermissionChoice, PermissionEngine, PermissionError,
    PermissionStore,
};
pub use policy::{
    AppLaunchPolicy, DEFAULT_DEVELOPER_MODE_PATH, DEFAULT_DEVICE_POLICY_PATH,
    DEFAULT_RECOVERY_MODE_PATH, DEVICE_POLICY_SCHEMA_VERSION, DeviceMode, DeviceModePaths,
    DevicePolicy, DevicePolicyEngine, DeviceSettings, ManagementAuthority, PolicyError,
    developer_install_allowed,
};

pub use protocol::{
    APPD_PROTOCOL_VERSION, AppSummary, AppdCommand, AppdRequest, AppdResponse, ErrorCode,
    MAX_APP_LIST_PAGE, MAX_LOG_LINES, PeerCredentials, ProtocolError, ResponseData,
    ResponseOutcome, peer_credentials, read_request, read_response, write_request, write_response,
};
pub use radio_client::{DEFAULT_RADIO_SOCKET, RadioClient, RadioClientError, ReceivedRadioPacket};
pub use registry::{
    AppAccount, AppRegistry, FIRST_APP_ACCOUNT_ID, LAST_APP_ACCOUNT_ID, RegistryError,
};
pub use server::{AppdServer, CapabilityServices, ServerError};
pub use storage_client::{DEFAULT_STORAGE_SOCKET, StorageClient, StorageClientError};

pub const DEFAULT_APPS_ROOT: &str = "/var/lib/cardputerzero/apps";
pub const DEFAULT_DATA_ROOT: &str = "/var/lib/cardputerzero/data";
pub const DEFAULT_RUNTIME: &str = "/usr/libexec/cardputerzero/app-runtime";
pub const DEFAULT_BROKER_SOCKET: &str = "/run/cardputerzero-broker/runtime.sock";
pub const DEFAULT_WAYLAND_SOCKET: &str = "/run/cardputerzero/wayland-0";
pub const BWRAP_PATH: &str = "/usr/bin/bwrap";
pub const STABILITY_ACCEPTANCE_UNIT: &str = "cardputerzero-stability-acceptance.service";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppLayout {
    pub apps_root: PathBuf,
    pub data_root: PathBuf,
    pub runtime_path: PathBuf,
    pub broker_socket: PathBuf,
    pub wayland_socket: PathBuf,
}

impl Default for AppLayout {
    fn default() -> Self {
        Self {
            apps_root: PathBuf::from(DEFAULT_APPS_ROOT),
            data_root: PathBuf::from(DEFAULT_DATA_ROOT),
            runtime_path: PathBuf::from(DEFAULT_RUNTIME),
            broker_socket: PathBuf::from(DEFAULT_BROKER_SOCKET),
            wayland_socket: PathBuf::from(DEFAULT_WAYLAND_SOCKET),
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
    validate_root(&layout.broker_socket, "broker_socket")?;
    validate_root(&layout.wayland_socket, "wayland_socket")?;

    let package_dir = layout.apps_root.join(&manifest.id).join(&manifest.version);
    let data_dir = layout.data_root.join(&manifest.id);
    let package = path_text(&package_dir, "package_dir")?;
    let data = path_text(&data_dir, "data_dir")?;
    let runtime = path_text(&layout.runtime_path, "runtime_path")?;
    let broker_socket = path_text(&layout.broker_socket, "broker_socket")?;
    let wayland_socket = path_text(&layout.wayland_socket, "wayland_socket")?;
    let display_mode = match manifest.display {
        DisplayMode::Standard => "standard",
        DisplayMode::Immersive => "immersive",
    };
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
        "--dir".into(),
        "/run".into(),
        "--dir".into(),
        "/run/cardputerzero".into(),
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
        "--ro-bind".into(),
        broker_socket,
        "/run/cardputerzero/broker.sock".into(),
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
        "--setenv".into(),
        "CP0_BROKER_SOCKET".into(),
        "/run/cardputerzero/broker.sock".into(),
        "--setenv".into(),
        "WAYLAND_SOCKET".into(),
        "3".into(),
        "--setenv".into(),
        "CP0_APP_ID".into(),
        manifest.id.clone(),
        "--setenv".into(),
        "CP0_DISPLAY_MODE".into(),
        display_mode.into(),
        "--".into(),
        "/runtime/cardputerzero-app-runtime".into(),
        entrypoint,
    ];

    let systemd_properties = vec![
        format!("User={app_user}"),
        format!("Group={app_user}"),
        format!("MemoryMax={memory_max_bytes}"),
        "MemorySwapMax=0".into(),
        "CPUQuota=60%".into(),
        "CPUWeight=50".into(),
        "TasksMax=32".into(),
        format!("Conflicts={STABILITY_ACCEPTANCE_UNIT}"),
        "UMask=0077".into(),
        "NoNewPrivileges=yes".into(),
        "PrivateDevices=yes".into(),
        "PrivateTmp=yes".into(),
        "ProtectSystem=strict".into(),
        "ProtectHome=yes".into(),
        "ProtectKernelModules=yes".into(),
        "ProtectControlGroups=yes".into(),
        "RestrictAddressFamilies=AF_UNIX AF_NETLINK".into(),
        "CapabilityBoundingSet=".into(),
        "AmbientCapabilities=".into(),
        "LockPersonality=yes".into(),
        "RestrictRealtime=yes".into(),
        "SystemCallArchitectures=native".into(),
        "Environment=WAYLAND_SOCKET=3".into(),
        format!("OpenFile={wayland_socket}:wayland"),
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
        "--service-type=exec".into(),
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

    pub(crate) fn manifest() -> AppManifest {
        AppManifest {
            schema_version: 1,
            id: "dev.cardputerzero.hello".into(),
            name: "Hello".into(),
            version: "1.2.3".into(),
            sdk_version: "1.0".into(),
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
            intents: Vec::new(),
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
        assert!(!plan.arguments.windows(3).any(|values| values
            == [
                "--bind",
                "/var/lib/cardputerzero/data/dev.cardputerzero.hello",
                "/data"
            ]));
        assert!(!plan.arguments.iter().any(|value| value == "--keep-fd"));
        assert!(
            plan.arguments
                .windows(3)
                .any(|values| values == ["--setenv", "CP0_APP_ID", "dev.cardputerzero.hello"])
        );
        assert!(
            plan.arguments
                .windows(3)
                .any(|values| values == ["--setenv", "CP0_DISPLAY_MODE", "standard"])
        );
        for directory in ["/runtime", "/app", "/data"] {
            assert!(
                plan.arguments
                    .windows(2)
                    .any(|values| values == ["--dir", directory])
            );
        }
        assert!(!plan.arguments.iter().any(|value| value == "/usr"));
        let read_only_sources: Vec<&str> = plan
            .arguments
            .windows(3)
            .filter(|values| values[0] == "--ro-bind")
            .map(|values| values[1].as_str())
            .collect();
        assert_eq!(
            read_only_sources,
            [
                DEFAULT_RUNTIME,
                "/var/lib/cardputerzero/apps/dev.cardputerzero.hello/1.2.3",
                DEFAULT_BROKER_SOCKET,
            ]
        );
        for forbidden in [
            "--bind",
            "--bind-try",
            "--dev-bind",
            "/run/dbus",
            "/dev/dri",
            "/dev/input",
            "/dev/gpiochip0",
            "/dev/snd",
            "/proc/self/root",
        ] {
            assert!(
                !plan.arguments.iter().any(|value| value == forbidden),
                "sandbox exposes forbidden argument {forbidden}"
            );
        }
        assert!(
            plan.arguments
                .windows(2)
                .any(|values| values == ["--dev", "/dev"])
        );
        assert!(
            plan.systemd_properties
                .contains(&"RestrictAddressFamilies=AF_UNIX AF_NETLINK".into())
        );
        assert!(
            plan.systemd_properties
                .contains(&"CapabilityBoundingSet=".into())
        );
        assert!(plan.systemd_properties.contains(&"MemorySwapMax=0".into()));
        assert!(plan.systemd_properties.contains(&"CPUQuota=60%".into()));
        assert!(plan.systemd_properties.contains(&"CPUWeight=50".into()));
        assert!(
            plan.systemd_properties
                .contains(&format!("Conflicts={STABILITY_ACCEPTANCE_UNIT}"))
        );
        assert!(
            plan.systemd_properties
                .contains(&"PrivateDevices=yes".into())
        );
        assert_eq!(
            plan.systemd_properties
                .iter()
                .filter(|property| property.starts_with("OpenFile="))
                .count(),
            1
        );
        assert!(
            plan.systemd_properties
                .contains(&"PrivateDevices=yes".into())
        );
        assert!(
            !plan
                .systemd_properties
                .iter()
                .any(|property| property.starts_with("ReadWritePaths="))
        );
        assert!(
            plan.systemd_properties
                .contains(&"OpenFile=/run/cardputerzero/wayland-0:wayland".into())
        );
    }

    #[test]
    fn renders_systemd_run_without_shell_parsing() {
        let plan = build_sandbox_plan(&manifest(), "cp0-app-42", &AppLayout::default())
            .expect("plan should be valid");
        let arguments = systemd_run_arguments(&plan);

        assert_eq!(arguments[0], "--quiet");
        assert!(arguments.contains(&"--service-type=exec".into()));
        assert!(arguments.contains(&"--unit=cardputerzero-app-42.service".into()));
        assert!(arguments.contains(&"--property=MemoryMax=25165824".into()));
        assert!(arguments.contains(&"--property=CPUQuota=60%".into()));
        assert!(arguments.contains(&"--property=CPUWeight=50".into()));
        assert!(arguments.contains(&format!("--property=Conflicts={STABILITY_ACCEPTANCE_UNIT}")));
        assert!(arguments.contains(&BWRAP_PATH.into()));
    }

    #[test]
    fn rejects_untrusted_user_and_layout_values() {
        assert_eq!(
            build_sandbox_plan(&manifest(), "root", &AppLayout::default()),
            Err(PlanError::InvalidAppUser)
        );
        let layout = AppLayout {
            data_root: PathBuf::from("relative/data"),
            ..AppLayout::default()
        };
        assert_eq!(
            build_sandbox_plan(&manifest(), "cp0-app-1", &layout),
            Err(PlanError::InvalidLayout("data_root"))
        );
    }
}
