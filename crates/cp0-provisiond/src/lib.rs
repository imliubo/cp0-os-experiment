use std::collections::BTreeSet;
use std::ffi::{CString, OsStr};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Write};
use std::mem::MaybeUninit;
use std::net::Ipv4Addr;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cp0_provision_protocol::{
    NetworkChoice, NetworkRuntimeStatus, ProvisioningCommand, ProvisioningErrorCode,
    ProvisioningOutcome, ProvisioningPhase, ProvisioningProtocolError, ProvisioningRequest,
    ProvisioningResponse, ProvisioningStatus, WifiNetwork, WifiSecurity, read_request,
    write_response,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

pub const OWNER_UID: u32 = 1000;
pub const DEVELOPER_ACCESS_GROUP_GID: u32 = 1998;
pub const SSH_GROUP_GID: u32 = 1999;
pub const STATE_SCHEMA_VERSION: u32 = 1;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);
const OWNER_SHELL: &str = "/usr/libexec/cardputerzero/owner-shell";

#[derive(Debug)]
pub enum ProvisioningError {
    Io(io::Error),
    Json(serde_json::Error),
    InvalidState(&'static str),
    InvalidValue(&'static str),
    Unavailable(&'static str),
    Operation(&'static str),
}

impl fmt::Display for ProvisioningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "provisioning I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid provisioning state: {error}"),
            Self::InvalidState(message) => {
                write!(formatter, "invalid provisioning state: {message}")
            }
            Self::InvalidValue(message) => {
                write!(formatter, "invalid provisioning value: {message}")
            }
            Self::Unavailable(message) => write!(formatter, "provisioning unavailable: {message}"),
            Self::Operation(message) => {
                write!(formatter, "provisioning operation failed: {message}")
            }
        }
    }
}

impl std::error::Error for ProvisioningError {}

impl From<io::Error> for ProvisioningError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ProvisioningError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone)]
pub struct ProvisioningPaths {
    pub state_file: PathBuf,
    pub complete_marker: PathBuf,
    pub extrausers_dir: PathBuf,
    pub owner_home_root: PathBuf,
    pub network_connections_dir: PathBuf,
    pub ssh_marker: PathBuf,
    pub hostname_file: PathBuf,
    pub locale_file: PathBuf,
    pub timezone_file: PathBuf,
    pub localtime_file: PathBuf,
    pub zoneinfo_root: PathBuf,
    pub network_country_file: PathBuf,
    pub wireless_phy_dir: PathBuf,
}

impl Default for ProvisioningPaths {
    fn default() -> Self {
        Self {
            state_file: "/var/lib/cardputerzero/provisioning/state.json".into(),
            complete_marker: "/var/lib/cardputerzero/provisioning/complete".into(),
            extrausers_dir: "/var/lib/extrausers".into(),
            owner_home_root: "/home".into(),
            network_connections_dir: "/etc/NetworkManager/system-connections".into(),
            ssh_marker: "/var/lib/cardputerzero/provisioning/ssh-enabled".into(),
            hostname_file: "/etc/hostname".into(),
            locale_file: "/etc/default/locale".into(),
            timezone_file: "/etc/timezone".into(),
            localtime_file: "/etc/localtime".into(),
            zoneinfo_root: "/usr/share/zoneinfo".into(),
            network_country_file: "/etc/NetworkManager/conf.d/20-cardputerzero-country.conf".into(),
            wireless_phy_dir: "/sys/class/ieee80211".into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StateDocument {
    schema_version: u32,
    state: ProvisioningStatus,
}

#[derive(Debug, Clone)]
pub struct StateStore {
    state_file: PathBuf,
    complete_marker: PathBuf,
    ssh_marker: PathBuf,
}

impl StateStore {
    pub fn new(paths: &ProvisioningPaths) -> Self {
        Self {
            state_file: paths.state_file.clone(),
            complete_marker: paths.complete_marker.clone(),
            ssh_marker: paths.ssh_marker.clone(),
        }
    }

    pub fn load(&self) -> Result<ProvisioningStatus, ProvisioningError> {
        if !self.state_file.exists() {
            return Ok(ProvisioningStatus::default());
        }
        reject_symlink(&self.state_file)?;
        let bytes = fs::read(&self.state_file)?;
        let document: StateDocument = serde_json::from_slice(&bytes)?;
        if document.schema_version != STATE_SCHEMA_VERSION {
            return Err(ProvisioningError::InvalidState(
                "unsupported schema version",
            ));
        }
        document
            .state
            .validate()
            .map_err(|_| ProvisioningError::InvalidState("state validation failed"))?;
        Ok(document.state)
    }

    pub fn save(&self, state: &ProvisioningStatus) -> Result<(), ProvisioningError> {
        state
            .validate()
            .map_err(|_| ProvisioningError::InvalidState("state validation failed"))?;
        let mut persistent_state = state.clone();
        persistent_state.network_runtime = None;
        let document = StateDocument {
            schema_version: STATE_SCHEMA_VERSION,
            state: persistent_state,
        };
        let mut bytes = serde_json::to_vec(&document)?;
        bytes.push(b'\n');
        atomic_write(&self.state_file, &bytes, 0o600)
    }

    pub fn complete_marker_exists(&self) -> bool {
        self.complete_marker.is_file() && !self.complete_marker.is_symlink()
    }

    pub fn ssh_marker_exists(&self) -> bool {
        self.ssh_marker.is_file() && !self.ssh_marker.is_symlink()
    }

    pub fn mark_complete(&self) -> Result<(), ProvisioningError> {
        atomic_write(&self.complete_marker, b"cp0-provisioned-v1\n", 0o600)
    }
}

#[derive(Debug, Clone)]
pub struct ExtrausersIdentityStore {
    root: PathBuf,
    home_root: PathBuf,
}

impl ExtrausersIdentityStore {
    pub fn new(paths: &ProvisioningPaths) -> Self {
        Self {
            root: paths.extrausers_dir.clone(),
            home_root: paths.owner_home_root.clone(),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn create_locked_owner(
        &self,
        display_name: &str,
        username: &str,
    ) -> Result<(), ProvisioningError> {
        ensure_empty_or_owner_file(&self.path("passwd"), username)?;
        fs::create_dir_all(&self.root)?;
        fs::set_permissions(&self.root, fs::Permissions::from_mode(0o755))?;
        let home = self.home_root.join(username);
        if home.is_symlink() {
            return Err(ProvisioningError::InvalidState("owner home is a symlink"));
        }
        fs::create_dir_all(&home)?;
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
        #[cfg(target_os = "linux")]
        {
            let path = std::ffi::CString::new(home.as_os_str().as_encoded_bytes())
                .map_err(|_| ProvisioningError::InvalidValue("owner home path"))?;
            if unsafe { libc::chown(path.as_ptr(), OWNER_UID, OWNER_UID) } != 0 {
                return Err(io::Error::last_os_error().into());
            }
        }
        atomic_write(
            &self.path("passwd"),
            format!(
                "{username}:x:{OWNER_UID}:{OWNER_UID}:{display_name}:/home/{username}:{OWNER_SHELL}\n"
            )
            .as_bytes(),
            0o644,
        )?;
        atomic_write(
            &self.path("shadow"),
            format!("{username}:!:0:0:99999:7:::\n").as_bytes(),
            0o640,
        )?;
        atomic_write(
            &self.path("group"),
            format!(
                "{username}:x:{OWNER_UID}:\ncp0-developer-access:x:{DEVELOPER_ACCESS_GROUP_GID}:{username}\ncp0-ssh:x:{SSH_GROUP_GID}:\n"
            )
            .as_bytes(),
            0o644,
        )?;
        atomic_write(
            &self.path("gshadow"),
            format!("{username}:!::\ncp0-developer-access:!::{username}\ncp0-ssh:!::\n").as_bytes(),
            0o640,
        )
    }

    pub fn set_password_hash(&self, username: &str, hash: &str) -> Result<(), ProvisioningError> {
        if !hash.starts_with("$y$") || hash.len() > 256 || hash.contains([':', '\n', '\r']) {
            return Err(ProvisioningError::InvalidValue("yescrypt hash"));
        }
        let existing = fs::read_to_string(self.path("passwd"))?;
        if !existing.starts_with(&format!("{username}:x:{OWNER_UID}:{OWNER_UID}:")) {
            return Err(ProvisioningError::InvalidState(
                "owner identity does not match",
            ));
        }
        let days = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 86_400;
        atomic_write(
            &self.path("shadow"),
            format!("{username}:{hash}:{days}:0:99999:7:::\n").as_bytes(),
            0o640,
        )
    }

    pub fn set_ssh_membership(
        &self,
        username: &str,
        enabled: bool,
    ) -> Result<(), ProvisioningError> {
        let members = if enabled { username } else { "" };
        atomic_write(
            &self.path("group"),
            format!(
                "{username}:x:{OWNER_UID}:\ncp0-developer-access:x:{DEVELOPER_ACCESS_GROUP_GID}:{username}\ncp0-ssh:x:{SSH_GROUP_GID}:{members}\n"
            )
            .as_bytes(),
            0o644,
        )?;
        atomic_write(
            &self.path("gshadow"),
            format!("{username}:!::\ncp0-developer-access:!::{username}\ncp0-ssh:!::{members}\n")
                .as_bytes(),
            0o640,
        )
    }

    pub fn verify(&self, state: &ProvisioningStatus) -> bool {
        let Some(username) = state.username.as_deref() else {
            return state.phase < ProvisioningPhase::PasswordReady;
        };
        let passwd = fs::read_to_string(self.path("passwd")).unwrap_or_default();
        let shadow = fs::read_to_string(self.path("shadow")).unwrap_or_default();
        let group = fs::read_to_string(self.path("group")).unwrap_or_default();
        let gshadow = fs::read_to_string(self.path("gshadow")).unwrap_or_default();
        let empty_group = format!(
            "{username}:x:{OWNER_UID}:\ncp0-developer-access:x:{DEVELOPER_ACCESS_GROUP_GID}:{username}\ncp0-ssh:x:{SSH_GROUP_GID}:\n"
        );
        let member_group = format!(
            "{username}:x:{OWNER_UID}:\ncp0-developer-access:x:{DEVELOPER_ACCESS_GROUP_GID}:{username}\ncp0-ssh:x:{SSH_GROUP_GID}:{username}\n"
        );
        let empty_gshadow =
            format!("{username}:!::\ncp0-developer-access:!::{username}\ncp0-ssh:!::\n");
        let member_gshadow =
            format!("{username}:!::\ncp0-developer-access:!::{username}\ncp0-ssh:!::{username}\n");
        let membership_valid = if !state.ssh_enabled {
            group == empty_group && gshadow == empty_gshadow
        } else if state.phase == ProvisioningPhase::Review {
            (group == empty_group && gshadow == empty_gshadow)
                || (group == member_group && gshadow == member_gshadow)
        } else if state.phase >= ProvisioningPhase::Committing {
            group == member_group && gshadow == member_gshadow
        } else {
            group == empty_group && gshadow == empty_gshadow
        };
        let home = self.home_root.join(username);
        let Ok(home_metadata) = fs::symlink_metadata(&home) else {
            return false;
        };
        let home_valid =
            home_metadata.is_dir() && home_metadata.permissions().mode() & 0o777 == 0o700;
        #[cfg(target_os = "linux")]
        let home_valid =
            home_valid && home_metadata.uid() == OWNER_UID && home_metadata.gid() == OWNER_UID;
        passwd.starts_with(&format!("{username}:x:{OWNER_UID}:{OWNER_UID}:"))
            && passwd.ends_with(&format!(":{OWNER_SHELL}\n"))
            && (!state.password_configured || shadow.starts_with(&format!("{username}:$y$")))
            && membership_valid
            && home_valid
    }
}

fn ensure_empty_or_owner_file(path: &Path, username: &str) -> Result<(), ProvisioningError> {
    if !path.exists() {
        return Ok(());
    }
    reject_symlink(path)?;
    let content = fs::read_to_string(path)?;
    if content.is_empty() || content.starts_with(&format!("{username}:")) {
        Ok(())
    } else {
        Err(ProvisioningError::InvalidState(
            "a different owner identity already exists",
        ))
    }
}

fn system_identity_uid(username: &str) -> Result<Option<u32>, ProvisioningError> {
    let name = CString::new(username).map_err(|_| ProvisioningError::InvalidValue("username"))?;
    let mut record = MaybeUninit::<libc::passwd>::uninit();
    let mut result = ptr::null_mut();
    let mut buffer = [0_u8; 16 * 1024];
    let status = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status).into());
    }
    Ok((!result.is_null()).then(|| unsafe { record.assume_init() }.pw_uid))
}

pub trait PlatformBackend {
    fn hash_password(&mut self, password: &str) -> Result<String, ProvisioningError>;
    fn list_wifi(&mut self) -> Result<Vec<WifiNetwork>, ProvisioningError>;
    fn connect_wifi(
        &mut self,
        ssid: &str,
        security: WifiSecurity,
        password: &str,
        hidden: bool,
    ) -> Result<String, ProvisioningError>;
    fn network_status(&mut self) -> NetworkRuntimeStatus;
    fn ethernet_ready(&mut self) -> Result<bool, ProvisioningError>;
    fn apply_region(&mut self, state: &ProvisioningStatus) -> Result<(), ProvisioningError>;
    fn configure_ssh(&mut self, enabled: bool) -> Result<(), ProvisioningError>;
    fn activate_ssh(&mut self, enabled: bool) -> Result<(), ProvisioningError>;
}

#[derive(Debug, Clone)]
pub struct LinuxPlatformBackend {
    paths: ProvisioningPaths,
    nmcli: PathBuf,
    mkpasswd: PathBuf,
    systemctl: PathBuf,
    hostnamectl: PathBuf,
    iw: PathBuf,
}

impl LinuxPlatformBackend {
    pub fn new(paths: ProvisioningPaths) -> Self {
        Self {
            paths,
            nmcli: "/usr/bin/nmcli".into(),
            mkpasswd: "/usr/bin/mkpasswd".into(),
            systemctl: "/usr/bin/systemctl".into(),
            hostnamectl: "/usr/bin/hostnamectl".into(),
            iw: "/usr/sbin/iw".into(),
        }
    }

    fn command_ok(
        &self,
        program: &Path,
        arguments: &[&OsStr],
        failure: &'static str,
    ) -> Result<(), ProvisioningError> {
        let output = Command::new(program)
            .args(arguments)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| {
                eprintln!(
                    "provisioning tool could not start: tool={} error={error}",
                    program.display()
                );
                ProvisioningError::Unavailable("required system tool is unavailable")
            })?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr: String = String::from_utf8_lossy(&output.stderr)
                .trim()
                .chars()
                .take(512)
                .collect();
            eprintln!(
                "provisioning command failed: tool={} status={} step={failure} stderr={stderr:?}",
                program.display(),
                output.status
            );
            Err(ProvisioningError::Operation(failure))
        }
    }

    fn device_ipv4(&self, device: &str) -> Option<String> {
        let output = Command::new(&self.nmcli)
            .args([
                "--wait",
                "5",
                "--get-values",
                "IP4.ADDRESS",
                "device",
                "show",
                device,
            ])
            .env("LC_ALL", "C")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split('/').next())
            .find_map(|address| address.parse::<Ipv4Addr>().ok())
            .map(|address| address.to_string())
    }
}

impl PlatformBackend for LinuxPlatformBackend {
    fn hash_password(&mut self, password: &str) -> Result<String, ProvisioningError> {
        let mut child = Command::new(&self.mkpasswd)
            .args(["--method=yescrypt", "--stdin"])
            .env("LC_ALL", "C")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| {
                ProvisioningError::Unavailable("yescrypt password hasher is unavailable")
            })?;
        let mut secret = password.as_bytes().to_vec();
        secret.push(b'\n');
        let write_result = child
            .stdin
            .as_mut()
            .ok_or(ProvisioningError::Operation(
                "password hasher stdin is unavailable",
            ))?
            .write_all(&secret);
        secret.zeroize();
        write_result?;
        drop(child.stdin.take());
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(ProvisioningError::Operation(
                "yescrypt password hashing failed",
            ));
        }
        let hash = String::from_utf8(output.stdout)
            .map_err(|_| ProvisioningError::Operation("password hash is invalid"))?;
        let hash = hash.trim_end_matches(['\r', '\n']).to_owned();
        if !hash.starts_with("$y$") {
            return Err(ProvisioningError::Operation(
                "password hasher did not produce yescrypt",
            ));
        }
        Ok(hash)
    }

    fn list_wifi(&mut self) -> Result<Vec<WifiNetwork>, ProvisioningError> {
        let runtime = self.network_status();
        if !runtime.network_manager_available {
            return Err(ProvisioningError::Unavailable(
                "NetworkManager is unavailable",
            ));
        }
        if !runtime.wifi_available {
            return Err(ProvisioningError::Unavailable(
                "No Wi-Fi adapter is available",
            ));
        }
        let output = Command::new(&self.nmcli)
            .args([
                "--wait",
                "30",
                "--terse",
                "--escape",
                "yes",
                "--fields",
                "IN-USE,SSID,SIGNAL,SECURITY",
                "device",
                "wifi",
                "list",
                "--rescan",
                "yes",
            ])
            .env("LC_ALL", "C")
            .output()
            .map_err(|_| ProvisioningError::Unavailable("NetworkManager is unavailable"))?;
        if !output.status.success() {
            return Err(ProvisioningError::Unavailable("Wi-Fi scan is unavailable"));
        }
        parse_nmcli_wifi(&output.stdout)
    }

    fn connect_wifi(
        &mut self,
        ssid: &str,
        security: WifiSecurity,
        password: &str,
        hidden: bool,
    ) -> Result<String, ProvisioningError> {
        fs::create_dir_all(&self.paths.network_connections_dir)?;
        let uuid = Uuid::new_v4().to_string();
        let profile_id = format!("cp0-{uuid}");
        let path = self
            .paths
            .network_connections_dir
            .join(format!("{profile_id}.nmconnection"));
        let mut keyfile = format!(
            "[connection]\nid={profile_id}\nuuid={uuid}\ntype=wifi\n\n[wifi]\nmode=infrastructure\nssid={}\nhidden={}\n",
            keyfile_escape(ssid),
            if hidden { "true" } else { "false" }
        );
        match security {
            WifiSecurity::Open => {}
            WifiSecurity::Wpa2 => keyfile.push_str(&format!(
                "\n[wifi-security]\nkey-mgmt=wpa-psk\npsk={}\n",
                keyfile_escape(password)
            )),
            WifiSecurity::Wpa3 => keyfile.push_str(&format!(
                "\n[wifi-security]\nkey-mgmt=sae\npsk={}\n",
                keyfile_escape(password)
            )),
            WifiSecurity::Unsupported => {
                return Err(ProvisioningError::InvalidValue(
                    "Wi-Fi security is unsupported",
                ));
            }
        }
        keyfile.push_str("\n[ipv4]\nmethod=auto\n\n[ipv6]\nmethod=auto\n");
        let write_result = atomic_write(&path, keyfile.as_bytes(), 0o600);
        keyfile.zeroize();
        write_result?;
        let path_argument = path.as_os_str();
        let connection_result = self
            .command_ok(
                &self.nmcli,
                &[OsStr::new("connection"), OsStr::new("load"), path_argument],
                "Wi-Fi profile could not be loaded",
            )
            .and_then(|()| {
                self.command_ok(
                    &self.nmcli,
                    &[
                        OsStr::new("--wait"),
                        OsStr::new("45"),
                        OsStr::new("connection"),
                        OsStr::new("up"),
                        OsStr::new("uuid"),
                        OsStr::new(&uuid),
                    ],
                    "Wi-Fi connection could not be activated",
                )
            });
        if let Err(error) = connection_result {
            let _ = Command::new(&self.nmcli)
                .args(["connection", "delete", "uuid", &uuid])
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        Ok(profile_id)
    }

    fn network_status(&mut self) -> NetworkRuntimeStatus {
        let output = match Command::new(&self.nmcli)
            .args([
                "--wait",
                "5",
                "--terse",
                "--escape",
                "yes",
                "--fields",
                "DEVICE,TYPE,STATE",
                "device",
                "status",
            ])
            .env("LC_ALL", "C")
            .output()
        {
            Ok(output) if output.status.success() => output,
            _ => return NetworkRuntimeStatus::default(),
        };
        let devices = match parse_nmcli_devices(&output.stdout) {
            Ok(devices) => devices,
            Err(error) => {
                eprintln!("provisioning network status is invalid: {error}");
                return NetworkRuntimeStatus::default();
            }
        };
        let mut runtime = NetworkRuntimeStatus {
            network_manager_available: true,
            ..NetworkRuntimeStatus::default()
        };
        for device in devices {
            if device.kind == "wifi" {
                runtime.wifi_available = true;
                runtime.wifi_connected |= device.connected;
            } else if device.kind == "ethernet" {
                runtime.ethernet_connected |= device.connected;
            } else {
                continue;
            }
            if !device.connected {
                continue;
            }
            let address = self.device_ipv4(&device.name);
            if device.kind == "wifi" && runtime.wifi_ipv4.is_none() {
                runtime.wifi_ipv4 = address;
            } else if device.kind == "ethernet" && runtime.ethernet_ipv4.is_none() {
                runtime.ethernet_ipv4 = address;
            }
        }
        runtime
    }

    fn ethernet_ready(&mut self) -> Result<bool, ProvisioningError> {
        let runtime = self.network_status();
        if !runtime.network_manager_available {
            return Err(ProvisioningError::Unavailable(
                "NetworkManager is unavailable",
            ));
        }
        Ok(runtime.ethernet_connected && runtime.ethernet_ipv4.is_some())
    }

    fn apply_region(&mut self, state: &ProvisioningStatus) -> Result<(), ProvisioningError> {
        let hostname = state
            .hostname
            .as_deref()
            .ok_or(ProvisioningError::InvalidState("hostname is missing"))?;
        let locale = state
            .locale
            .as_deref()
            .ok_or(ProvisioningError::InvalidState("locale is missing"))?;
        let timezone = state
            .timezone
            .as_deref()
            .ok_or(ProvisioningError::InvalidState("timezone is missing"))?;
        let country = state
            .country
            .as_deref()
            .ok_or(ProvisioningError::InvalidState("country is missing"))?;
        let zone = self.paths.zoneinfo_root.join(timezone);
        if !zone.is_file() || zone.is_symlink() {
            return Err(ProvisioningError::InvalidValue(
                "timezone data is unavailable",
            ));
        }
        self.command_ok(
            &self.hostnamectl,
            &[OsStr::new("set-hostname"), OsStr::new(hostname)],
            "device name could not be configured",
        )?;
        atomic_replace(
            &self.paths.locale_file,
            format!("LANG={locale}\n").as_bytes(),
            0o644,
        )?;
        atomic_write(
            &self.paths.timezone_file,
            format!("{timezone}\n").as_bytes(),
            0o644,
        )?;
        atomic_replace(&self.paths.localtime_file, &fs::read(&zone)?, 0o644)?;
        let has_wireless_phy = match fs::read_dir(&self.paths.wireless_phy_dir) {
            Ok(mut entries) => entries.next().transpose()?.is_some(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if has_wireless_phy {
            self.command_ok(
                &self.iw,
                &[OsStr::new("reg"), OsStr::new("set"), OsStr::new(country)],
                "wireless regulatory country could not be configured",
            )?;
        }
        Ok(())
    }

    fn configure_ssh(&mut self, enabled: bool) -> Result<(), ProvisioningError> {
        if enabled {
            atomic_write(&self.paths.ssh_marker, b"password\n", 0o600)
        } else if self.paths.ssh_marker.exists() {
            reject_symlink(&self.paths.ssh_marker)?;
            fs::remove_file(&self.paths.ssh_marker).map_err(Into::into)
        } else {
            Ok(())
        }
    }

    fn activate_ssh(&mut self, enabled: bool) -> Result<(), ProvisioningError> {
        let action = if enabled { "start" } else { "stop" };
        self.command_ok(
            &self.systemctl,
            &[OsStr::new(action), OsStr::new("ssh.service")],
            "SSH service state could not be changed",
        )
    }
}

fn keyfile_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace('#', "\\#")
}

fn split_nmcli(line: &str) -> Option<Vec<String>> {
    let mut fields = vec![String::new()];
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            fields.last_mut()?.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == ':' {
            fields.push(String::new());
        } else {
            fields.last_mut()?.push(character);
        }
    }
    if escaped {
        return None;
    }
    Some(fields)
}

#[derive(Debug, PartialEq, Eq)]
struct NmcliDevice {
    name: String,
    kind: String,
    connected: bool,
}

fn parse_nmcli_devices(bytes: &[u8]) -> Result<Vec<NmcliDevice>, ProvisioningError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ProvisioningError::Operation("network status output is invalid"))?;
    let mut devices = Vec::new();
    for line in text.lines() {
        let Some(fields) = split_nmcli(line) else {
            return Err(ProvisioningError::Operation(
                "network status output is invalid",
            ));
        };
        if fields.len() != 3 || fields[0].is_empty() {
            continue;
        }
        devices.push(NmcliDevice {
            name: fields[0].clone(),
            kind: fields[1].clone(),
            connected: fields[2] == "connected" || fields[2].starts_with("connected ("),
        });
    }
    Ok(devices)
}

fn parse_nmcli_wifi(bytes: &[u8]) -> Result<Vec<WifiNetwork>, ProvisioningError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ProvisioningError::Operation("Wi-Fi scan output is invalid"))?;
    let mut networks = Vec::new();
    let mut seen = BTreeSet::new();
    for line in text.lines() {
        let Some(fields) = split_nmcli(line) else {
            return Err(ProvisioningError::Operation("Wi-Fi scan output is invalid"));
        };
        if fields.len() != 4 || fields[1].is_empty() || !seen.insert(fields[1].clone()) {
            continue;
        }
        let signal_percent = fields[2]
            .parse::<u8>()
            .map_err(|_| ProvisioningError::Operation("Wi-Fi signal is invalid"))?;
        if signal_percent > 100 {
            return Err(ProvisioningError::Operation("Wi-Fi signal is invalid"));
        }
        let upper = fields[3].to_ascii_uppercase();
        let security = if upper.contains("802.1X") || upper.contains("EAP") || upper.contains("WEP")
        {
            WifiSecurity::Unsupported
        } else if upper.contains("SAE") || upper.contains("WPA3") {
            WifiSecurity::Wpa3
        } else if upper.contains("WPA") {
            WifiSecurity::Wpa2
        } else if upper.is_empty() || upper == "--" {
            WifiSecurity::Open
        } else {
            WifiSecurity::Unsupported
        };
        networks.push(WifiNetwork {
            ssid: fields[1].clone(),
            signal_percent,
            security,
            connected: fields[0] == "*",
        });
        if networks.len() == cp0_provision_protocol::MAX_WIFI_NETWORKS {
            break;
        }
    }
    networks.sort_by_key(|network| std::cmp::Reverse(network.signal_percent));
    Ok(networks)
}

pub struct ProvisioningService<B> {
    state_store: StateStore,
    identity_store: ExtrausersIdentityStore,
    backend: B,
}

impl<B: PlatformBackend> ProvisioningService<B> {
    pub fn new(paths: &ProvisioningPaths, backend: B) -> Self {
        Self {
            state_store: StateStore::new(paths),
            identity_store: ExtrausersIdentityStore::new(paths),
            backend,
        }
    }

    pub fn status(&self) -> Result<ProvisioningStatus, ProvisioningError> {
        let mut state = self.state_store.load()?;
        let marker = self.state_store.complete_marker_exists();
        let ssh_marker_matches = state.ssh_enabled == self.state_store.ssh_marker_exists();
        if !self.identity_store.verify(&state) {
            state.phase = ProvisioningPhase::RepairRequired;
        } else if state.phase == ProvisioningPhase::Committing {
            // All platform work is complete before Committing is persisted.
            // Recover either side of the final two-file marker transaction.
            state.phase = if marker && ssh_marker_matches {
                ProvisioningPhase::Complete
            } else if !marker {
                ProvisioningPhase::Review
            } else {
                ProvisioningPhase::RepairRequired
            };
            if state.phase != ProvisioningPhase::RepairRequired {
                self.state_store.save(&state)?;
            }
        } else if state.phase == ProvisioningPhase::Complete && !marker {
            // Recover images written by the previous Complete-then-marker order.
            state.phase = ProvisioningPhase::Review;
            self.state_store.save(&state)?;
        } else if (state.phase == ProvisioningPhase::Complete && !ssh_marker_matches)
            || (state.phase != ProvisioningPhase::Complete && marker)
        {
            state.phase = ProvisioningPhase::RepairRequired;
        }
        Ok(state)
    }

    pub fn dispatch(&mut self, request: ProvisioningRequest) -> ProvisioningResponse {
        let request_id = request.request_id;
        let command_name = command_name(&request.command);
        match self.dispatch_command(request.command) {
            Ok(ProvisioningOutcome::State { mut state }) => {
                if state.phase >= ProvisioningPhase::Network
                    && state.phase != ProvisioningPhase::RepairRequired
                {
                    state.network_runtime = Some(self.backend.network_status());
                }
                ProvisioningResponse::state(request_id, state)
            }
            Ok(ProvisioningOutcome::WifiList { networks }) => {
                ProvisioningResponse::wifi_list(request_id, networks)
            }
            Ok(ProvisioningOutcome::Error { .. }) => unreachable!(),
            Err(error) => {
                let phase = self.status().ok().map(|state| state.phase);
                eprintln!(
                    "cp0-provisiond: command failed: request_id={request_id} command={command_name} phase={phase:?} error={error}"
                );
                error_response(request_id, error)
            }
        }
    }

    fn dispatch_command(
        &mut self,
        command: ProvisioningCommand,
    ) -> Result<ProvisioningOutcome, ProvisioningError> {
        if matches!(command, ProvisioningCommand::GetStatus {}) {
            return Ok(ProvisioningOutcome::State {
                state: self.status()?,
            });
        }
        let mut state = self.status()?;
        if state.phase == ProvisioningPhase::RepairRequired {
            return Err(ProvisioningError::InvalidState("repair is required"));
        }
        if state.phase == ProvisioningPhase::Complete {
            return Err(ProvisioningError::InvalidState("setup is already complete"));
        }
        match command {
            ProvisioningCommand::GetStatus {} => unreachable!(),
            ProvisioningCommand::SetRegion {
                locale,
                country,
                timezone,
                hostname,
            } => {
                let unchanged = state.locale.as_deref() == Some(locale.as_str())
                    && state.country.as_deref() == Some(country.as_str())
                    && state.timezone.as_deref() == Some(timezone.as_str())
                    && state.hostname.as_deref() == Some(hostname.as_str());
                if state.phase > ProvisioningPhase::Owner {
                    if unchanged {
                        return Ok(ProvisioningOutcome::State { state });
                    }
                    return Err(ProvisioningError::InvalidState(
                        "regional setup cannot change after owner creation",
                    ));
                }
                if state.phase == ProvisioningPhase::Owner && unchanged {
                    return Ok(ProvisioningOutcome::State { state });
                }
                state.locale = Some(locale);
                state.country = Some(country);
                state.timezone = Some(timezone);
                state.hostname = Some(hostname);
                state.phase = ProvisioningPhase::Owner;
                self.backend.apply_region(&state)?;
                self.state_store.save(&state)?;
                Ok(ProvisioningOutcome::State { state })
            }
            ProvisioningCommand::SetOwner {
                display_name,
                username,
            } => {
                if state.phase < ProvisioningPhase::Owner {
                    return Err(ProvisioningError::InvalidState(
                        "regional setup is incomplete",
                    ));
                }
                if let Some(existing) = &state.username {
                    if existing != &username {
                        return Err(ProvisioningError::InvalidState(
                            "owner username cannot change after creation",
                        ));
                    }
                }
                if state.phase >= ProvisioningPhase::PasswordReady {
                    if state.username.as_deref() == Some(username.as_str())
                        && state.display_name.as_deref() == Some(display_name.as_str())
                    {
                        return Ok(ProvisioningOutcome::State { state });
                    }
                    return Err(ProvisioningError::InvalidState(
                        "owner setup cannot change after creation",
                    ));
                }
                if let Some(uid) = system_identity_uid(&username)? {
                    if uid != OWNER_UID {
                        return Err(ProvisioningError::InvalidValue(
                            "username is already in use",
                        ));
                    }
                }
                self.identity_store
                    .create_locked_owner(&display_name, &username)?;
                state.display_name = Some(display_name);
                state.username = Some(username);
                state.phase = ProvisioningPhase::PasswordReady;
                self.state_store.save(&state)?;
                Ok(ProvisioningOutcome::State { state })
            }
            ProvisioningCommand::SetPassword { mut password } => {
                if state.phase < ProvisioningPhase::PasswordReady {
                    password.zeroize();
                    return Err(ProvisioningError::InvalidState("owner setup is incomplete"));
                }
                if state.phase > ProvisioningPhase::PasswordReady {
                    password.zeroize();
                    return Ok(ProvisioningOutcome::State { state });
                }
                let username = state
                    .username
                    .as_deref()
                    .ok_or(ProvisioningError::InvalidState("owner username is missing"))?;
                let result = self.backend.hash_password(&password);
                password.zeroize();
                let mut hash = result?;
                let update_result = self.identity_store.set_password_hash(username, &hash);
                hash.zeroize();
                update_result?;
                state.password_configured = true;
                state.phase = ProvisioningPhase::Network;
                self.state_store.save(&state)?;
                Ok(ProvisioningOutcome::State { state })
            }
            ProvisioningCommand::ListWifi {} => Ok(ProvisioningOutcome::WifiList {
                networks: self.backend.list_wifi()?,
            }),
            ProvisioningCommand::ConnectWifi {
                ssid,
                security,
                mut password,
                hidden,
            } => {
                if state.phase < ProvisioningPhase::Network {
                    password.zeroize();
                    return Err(ProvisioningError::InvalidState(
                        "password setup is incomplete",
                    ));
                }
                let result = self
                    .backend
                    .connect_wifi(&ssid, security, &password, hidden);
                password.zeroize();
                let profile_id = result?;
                state.network_choice = Some(NetworkChoice::Wifi { profile_id, ssid });
                state.phase = ProvisioningPhase::RemoteAccess;
                self.state_store.save(&state)?;
                Ok(ProvisioningOutcome::State { state })
            }
            ProvisioningCommand::UseEthernet {} => {
                if state.phase < ProvisioningPhase::Network {
                    return Err(ProvisioningError::InvalidState(
                        "password setup is incomplete",
                    ));
                }
                if !self.backend.ethernet_ready()? {
                    return Err(ProvisioningError::Unavailable(
                        "Ethernet is not ready; wait for an IPv4 address",
                    ));
                }
                state.network_choice = Some(NetworkChoice::Ethernet {});
                state.phase = ProvisioningPhase::RemoteAccess;
                self.state_store.save(&state)?;
                Ok(ProvisioningOutcome::State { state })
            }
            ProvisioningCommand::UseOffline {} => {
                if state.phase < ProvisioningPhase::Network {
                    return Err(ProvisioningError::InvalidState(
                        "password setup is incomplete",
                    ));
                }
                state.network_choice = Some(NetworkChoice::Offline {});
                state.phase = ProvisioningPhase::RemoteAccess;
                self.state_store.save(&state)?;
                Ok(ProvisioningOutcome::State { state })
            }
            ProvisioningCommand::SetSshEnabled { enabled } => {
                if state.phase < ProvisioningPhase::RemoteAccess {
                    return Err(ProvisioningError::InvalidState(
                        "network choice is incomplete",
                    ));
                }
                state.ssh_enabled = enabled;
                state.phase = ProvisioningPhase::Review;
                self.state_store.save(&state)?;
                Ok(ProvisioningOutcome::State { state })
            }
            ProvisioningCommand::Commit {} => {
                if state.phase != ProvisioningPhase::Review {
                    return Err(ProvisioningError::InvalidState(
                        "setup is not ready to commit",
                    ));
                }
                self.backend.apply_region(&state)?;
                let username = state
                    .username
                    .as_deref()
                    .ok_or(ProvisioningError::InvalidState("owner username is missing"))?;
                self.backend.configure_ssh(state.ssh_enabled)?;
                self.identity_store
                    .set_ssh_membership(username, state.ssh_enabled)?;
                state.phase = ProvisioningPhase::Committing;
                self.state_store.save(&state)?;
                self.state_store.mark_complete()?;
                let activation = self.backend.activate_ssh(state.ssh_enabled);
                state.phase = ProvisioningPhase::Complete;
                self.state_store.save(&state)?;
                activation?;
                Ok(ProvisioningOutcome::State { state })
            }
        }
    }

    pub fn apply_boot_configuration(&mut self) -> Result<(), ProvisioningError> {
        let state = self.status()?;
        if state.phase != ProvisioningPhase::Complete {
            return Ok(());
        }
        self.backend.apply_region(&state)?;
        self.backend.configure_ssh(state.ssh_enabled)
    }
}

fn command_name(command: &ProvisioningCommand) -> &'static str {
    match command {
        ProvisioningCommand::GetStatus {} => "get-status",
        ProvisioningCommand::SetRegion { .. } => "set-region",
        ProvisioningCommand::SetOwner { .. } => "set-owner",
        ProvisioningCommand::SetPassword { .. } => "set-password",
        ProvisioningCommand::ListWifi {} => "list-wifi",
        ProvisioningCommand::ConnectWifi { .. } => "connect-wifi",
        ProvisioningCommand::UseEthernet {} => "use-ethernet",
        ProvisioningCommand::UseOffline {} => "use-offline",
        ProvisioningCommand::SetSshEnabled { .. } => "set-ssh-enabled",
        ProvisioningCommand::Commit {} => "commit",
    }
}

pub struct ProvisioningServer<B> {
    service: ProvisioningService<B>,
    shell_uid: u32,
}

impl<B: PlatformBackend> ProvisioningServer<B> {
    pub fn new(service: ProvisioningService<B>, shell_uid: u32) -> Self {
        Self { service, shell_uid }
    }

    pub fn serve(&mut self, listener: UnixListener) -> io::Result<()> {
        loop {
            let (stream, _) = listener.accept()?;
            if let Err(error) = self.handle_connection(stream) {
                eprintln!("cp0-provisiond: rejected connection: {error}");
            }
        }
    }

    fn handle_connection(&mut self, mut stream: UnixStream) -> io::Result<()> {
        stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
        let uid = peer_uid(&stream)?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let request = match read_request(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(_) => {
                write_response(
                    &mut stream,
                    &ProvisioningResponse::error(
                        0,
                        ProvisioningErrorCode::InvalidRequest,
                        "invalid provisioning request",
                    ),
                )
                .map_err(protocol_io)?;
                return Ok(());
            }
        };
        let response = if uid == self.shell_uid {
            self.service.dispatch(request)
        } else {
            ProvisioningResponse::error(
                request.request_id,
                ProvisioningErrorCode::Unauthorized,
                "peer UID is not authorized for provisioning",
            )
        };
        write_response(&mut stream, &response).map_err(protocol_io)
    }
}

fn error_response(request_id: u64, error: ProvisioningError) -> ProvisioningResponse {
    let (code, message) = match error {
        ProvisioningError::InvalidState("repair is required") => (
            ProvisioningErrorCode::RepairRequired,
            "provisioning state requires repair",
        ),
        ProvisioningError::InvalidState(_) => (
            ProvisioningErrorCode::InvalidState,
            "provisioning command is not valid in the current state",
        ),
        ProvisioningError::InvalidValue(_) => (
            ProvisioningErrorCode::InvalidValue,
            "provisioning value is invalid",
        ),
        ProvisioningError::Unavailable(_) => (
            ProvisioningErrorCode::Unavailable,
            "required hardware or service is unavailable",
        ),
        ProvisioningError::Operation(message) => (ProvisioningErrorCode::Operation, message),
        ProvisioningError::Io(_) | ProvisioningError::Json(_) => (
            ProvisioningErrorCode::Internal,
            "provisioning state could not be updated",
        ),
    };
    ProvisioningResponse::error(request_id, code, message)
}

fn atomic_write(path: &Path, contents: &[u8], mode: u32) -> Result<(), ProvisioningError> {
    atomic_write_impl(path, contents, mode, true)
}

fn atomic_replace(path: &Path, contents: &[u8], mode: u32) -> Result<(), ProvisioningError> {
    atomic_write_impl(path, contents, mode, false)
}

fn atomic_write_impl(
    path: &Path,
    contents: &[u8],
    mode: u32,
    reject_destination_symlink: bool,
) -> Result<(), ProvisioningError> {
    let parent = path
        .parent()
        .ok_or(ProvisioningError::InvalidValue("path has no parent"))?;
    fs::create_dir_all(parent)?;
    if parent.is_symlink() {
        return Err(ProvisioningError::InvalidState(
            "persistent parent is a symlink",
        ));
    }
    if reject_destination_symlink && path.exists() {
        reject_symlink(path)?;
    }
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(OsStr::to_str).unwrap_or("state"),
        std::process::id()
    ));
    if temporary.exists() {
        reject_symlink(&temporary)?;
        fs::remove_file(&temporary)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temporary)?;
    file.write_all(contents)?;
    file.sync_all()?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))?;
    fs::rename(&temporary, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), ProvisioningError> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        Err(ProvisioningError::InvalidState(
            "symbolic link is not allowed",
        ))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SO_PEERCRED returned an unexpected size",
        ));
    }
    Ok(credentials.uid)
}

#[cfg(not(target_os = "linux"))]
fn peer_uid(_stream: &UnixStream) -> io::Result<u32> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "peer credentials require Linux",
    ))
}

fn protocol_io(error: ProvisioningProtocolError) -> io::Error {
    match error {
        ProvisioningProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[cfg(target_os = "linux")]
    use cp0_provision_protocol::MAX_PROVISION_FRAME_BYTES;
    #[cfg(target_os = "linux")]
    use std::io::{Read, Write};
    #[cfg(target_os = "linux")]
    use std::os::fd::FromRawFd;

    static NEXT: AtomicUsize = AtomicUsize::new(1);

    #[derive(Default)]
    struct MockBackend {
        ssh: bool,
        hash_calls: usize,
        fail_hash_once: bool,
        fail_configure_once: bool,
        fail_activate_once: bool,
    }

    impl PlatformBackend for MockBackend {
        fn hash_password(&mut self, _password: &str) -> Result<String, ProvisioningError> {
            self.hash_calls += 1;
            if self.fail_hash_once {
                self.fail_hash_once = false;
                return Err(ProvisioningError::Operation(
                    "injected password hashing failure",
                ));
            }
            Ok("$y$j9T$testsalt$testhash".into())
        }
        fn list_wifi(&mut self) -> Result<Vec<WifiNetwork>, ProvisioningError> {
            Ok(vec![WifiNetwork {
                ssid: "CP0-NET".into(),
                signal_percent: 80,
                security: WifiSecurity::Wpa2,
                connected: false,
            }])
        }
        fn connect_wifi(
            &mut self,
            _ssid: &str,
            _security: WifiSecurity,
            _password: &str,
            _hidden: bool,
        ) -> Result<String, ProvisioningError> {
            Ok("cp0-test-profile".into())
        }
        fn network_status(&mut self) -> NetworkRuntimeStatus {
            NetworkRuntimeStatus {
                network_manager_available: true,
                ethernet_connected: true,
                ethernet_ipv4: Some("192.168.20.146".into()),
                wifi_available: true,
                wifi_connected: false,
                wifi_ipv4: None,
            }
        }
        fn ethernet_ready(&mut self) -> Result<bool, ProvisioningError> {
            Ok(true)
        }
        fn apply_region(&mut self, _state: &ProvisioningStatus) -> Result<(), ProvisioningError> {
            Ok(())
        }
        fn configure_ssh(&mut self, enabled: bool) -> Result<(), ProvisioningError> {
            if self.fail_configure_once {
                self.fail_configure_once = false;
                return Err(ProvisioningError::Operation("injected failure"));
            }
            self.ssh = enabled;
            Ok(())
        }
        fn activate_ssh(&mut self, _enabled: bool) -> Result<(), ProvisioningError> {
            if self.fail_activate_once {
                self.fail_activate_once = false;
                return Err(ProvisioningError::Operation(
                    "injected SSH activation failure",
                ));
            }
            Ok(())
        }
    }

    fn fixture() -> (PathBuf, ProvisioningPaths) {
        let root = std::env::temp_dir().join(format!(
            "cp0-provisiond-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let paths = ProvisioningPaths {
            state_file: root.join("state/state.json"),
            complete_marker: root.join("state/complete"),
            extrausers_dir: root.join("extrausers"),
            owner_home_root: root.join("home"),
            network_connections_dir: root.join("network"),
            ssh_marker: root.join("state/ssh-enabled"),
            hostname_file: root.join("etc/hostname"),
            locale_file: root.join("etc/default/locale"),
            timezone_file: root.join("etc/timezone"),
            localtime_file: root.join("etc/localtime"),
            zoneinfo_root: root.join("zoneinfo"),
            network_country_file: root.join("etc/NetworkManager/country.conf"),
            wireless_phy_dir: root.join("sys/class/ieee80211"),
        };
        (root, paths)
    }

    fn request(id: u64, command: ProvisioningCommand) -> ProvisioningRequest {
        ProvisioningRequest {
            protocol_version: 1,
            request_id: id,
            command,
        }
    }

    fn state(response: ProvisioningResponse) -> ProvisioningStatus {
        match response.outcome {
            ProvisioningOutcome::State { state } => state,
            other => panic!("expected state, got {other:?}"),
        }
    }

    fn region_state() -> ProvisioningStatus {
        ProvisioningStatus {
            locale: Some("en_US.UTF-8".into()),
            country: Some("CN".into()),
            timezone: Some("Asia/Shanghai".into()),
            hostname: Some("cp0-test".into()),
            ..ProvisioningStatus::default()
        }
    }

    fn make_tool(path: &Path, script: &[u8]) {
        atomic_write(path, script, 0o755).unwrap();
    }

    fn region_backend(paths: ProvisioningPaths, tool: &Path) -> LinuxPlatformBackend {
        let mut backend = LinuxPlatformBackend::new(paths);
        backend.hostnamectl = tool.into();
        backend.iw = tool.into();
        backend
    }

    fn advance_to_review(service: &mut ProvisioningService<MockBackend>, ssh_enabled: bool) {
        state(service.dispatch(request(
            1,
            ProvisioningCommand::SetRegion {
                locale: "en_US.UTF-8".into(),
                country: "CN".into(),
                timezone: "Asia/Shanghai".into(),
                hostname: "cp0-test".into(),
            },
        )));
        state(service.dispatch(request(
            2,
            ProvisioningCommand::SetOwner {
                display_name: "Owner".into(),
                username: "owner".into(),
            },
        )));
        state(service.dispatch(request(
            3,
            ProvisioningCommand::SetPassword {
                password: "correct horse".into(),
            },
        )));
        state(service.dispatch(request(4, ProvisioningCommand::UseOffline {})));
        state(service.dispatch(request(
            5,
            ProvisioningCommand::SetSshEnabled {
                enabled: ssh_enabled,
            },
        )));
    }

    #[test]
    fn applies_region_without_wireless_phy() {
        let (root, paths) = fixture();
        let zone = paths.zoneinfo_root.join("Asia/Shanghai");
        fs::create_dir_all(zone.parent().unwrap()).unwrap();
        fs::write(&zone, b"test zone").unwrap();
        let original_locale = root.join("etc/locale.conf");
        fs::create_dir_all(original_locale.parent().unwrap()).unwrap();
        fs::write(&original_locale, b"LANG=C.UTF-8\n").unwrap();
        fs::create_dir_all(paths.locale_file.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink("../locale.conf", &paths.locale_file).unwrap();
        let original_zone = root.join("original-zone");
        fs::write(&original_zone, b"original zone").unwrap();
        fs::create_dir_all(paths.localtime_file.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&original_zone, &paths.localtime_file).unwrap();
        let success = root.join("bin/success");
        make_tool(&success, b"#!/bin/sh\nexit 0\n");
        let mut backend = region_backend(paths.clone(), &success);
        backend.iw = root.join("bin/iw-must-not-run");

        backend.apply_region(&region_state()).unwrap();
        assert_eq!(
            fs::read_to_string(&paths.locale_file).unwrap(),
            "LANG=en_US.UTF-8\n"
        );
        assert!(
            !fs::symlink_metadata(&paths.locale_file)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(&original_locale).unwrap(),
            "LANG=C.UTF-8\n"
        );
        assert_eq!(
            fs::read_to_string(&paths.timezone_file).unwrap(),
            "Asia/Shanghai\n"
        );
        assert_eq!(fs::read(&paths.localtime_file).unwrap(), b"test zone");
        assert!(
            !fs::symlink_metadata(&paths.localtime_file)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&original_zone).unwrap(), b"original zone");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_existing_system_identity_as_owner() {
        assert_eq!(system_identity_uid("root").unwrap(), Some(0));
        let (root, paths) = fixture();
        let mut service = ProvisioningService::new(&paths, MockBackend::default());
        state(service.dispatch(request(
            1,
            ProvisioningCommand::SetRegion {
                locale: "en_US.UTF-8".into(),
                country: "CN".into(),
                timezone: "Asia/Shanghai".into(),
                hostname: "cp0-test".into(),
            },
        )));
        assert!(matches!(
            service
                .dispatch(request(
                    2,
                    ProvisioningCommand::SetOwner {
                        display_name: "System User".into(),
                        username: "nobody".into(),
                    },
                ))
                .outcome,
            ProvisioningOutcome::Error {
                code: ProvisioningErrorCode::InvalidValue,
                ..
            }
        ));
        assert_eq!(service.status().unwrap().phase, ProvisioningPhase::Owner);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_wireless_regulatory_failure_when_phy_exists() {
        let (root, paths) = fixture();
        let zone = paths.zoneinfo_root.join("Asia/Shanghai");
        fs::create_dir_all(zone.parent().unwrap()).unwrap();
        fs::write(&zone, b"test zone").unwrap();
        fs::create_dir_all(paths.wireless_phy_dir.join("phy0")).unwrap();
        let success = root.join("bin/success");
        let failure = root.join("bin/failure");
        make_tool(&success, b"#!/bin/sh\nexit 0\n");
        make_tool(
            &failure,
            b"#!/bin/sh\necho 'test regulatory failure' >&2\nexit 1\n",
        );
        let mut backend = region_backend(paths, &success);
        backend.iw = failure;

        assert!(matches!(
            backend.apply_region(&region_state()),
            Err(ProvisioningError::Operation(
                "wireless regulatory country could not be configured"
            ))
        ));
        let response = error_response(
            7,
            ProvisioningError::Operation("wireless regulatory country could not be configured"),
        );
        assert!(matches!(
            response.outcome,
            ProvisioningOutcome::Error {
                code: ProvisioningErrorCode::Operation,
                message,
            } if message == "wireless regulatory country could not be configured"
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completes_offline_setup_without_persisting_secrets() {
        let (root, paths) = fixture();
        let mut service = ProvisioningService::new(&paths, MockBackend::default());
        state(service.dispatch(request(
            1,
            ProvisioningCommand::SetRegion {
                locale: "en_US.UTF-8".into(),
                country: "CN".into(),
                timezone: "Asia/Shanghai".into(),
                hostname: "cardputer-zero".into(),
            },
        )));
        state(service.dispatch(request(
            2,
            ProvisioningCommand::SetOwner {
                display_name: "Local Owner".into(),
                username: "owner".into(),
            },
        )));
        state(service.dispatch(request(
            3,
            ProvisioningCommand::SetPassword {
                password: "correct horse".into(),
            },
        )));
        state(service.dispatch(request(4, ProvisioningCommand::UseOffline {})));
        state(service.dispatch(request(
            5,
            ProvisioningCommand::SetSshEnabled { enabled: true },
        )));
        let complete = state(service.dispatch(request(6, ProvisioningCommand::Commit {})));
        assert_eq!(complete.phase, ProvisioningPhase::Complete);
        assert!(paths.complete_marker.is_file());
        let persisted = fs::read_to_string(&paths.state_file).unwrap();
        assert!(!persisted.contains("correct horse"));
        assert!(!persisted.contains("$y$"));
        assert!(!persisted.contains("network_runtime"));
        assert!(
            fs::read_to_string(paths.extrausers_dir.join("shadow"))
                .unwrap()
                .contains("$y$")
        );
        assert!(
            fs::read_to_string(paths.extrausers_dir.join("group"))
                .unwrap()
                .contains("cp0-ssh:x:1999:owner")
        );
        atomic_write(&paths.ssh_marker, b"password\n", 0o600).unwrap();
        assert_eq!(service.status().unwrap().phase, ProvisioningPhase::Complete);
        fs::remove_file(&paths.ssh_marker).unwrap();
        assert_eq!(
            service.status().unwrap().phase,
            ProvisioningPhase::RepairRequired
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replays_completed_steps_without_rewinding_or_reapplying_secrets() {
        let (root, paths) = fixture();
        let mut service = ProvisioningService::new(&paths, MockBackend::default());
        let region = ProvisioningCommand::SetRegion {
            locale: "en_US.UTF-8".into(),
            country: "CN".into(),
            timezone: "Asia/Shanghai".into(),
            hostname: "cp0-replay".into(),
        };
        let owner = ProvisioningCommand::SetOwner {
            display_name: "Owner".into(),
            username: "owner".into(),
        };

        assert_eq!(
            state(service.dispatch(request(1, region.clone()))).phase,
            ProvisioningPhase::Owner
        );
        assert_eq!(
            state(service.dispatch(request(2, owner.clone()))).phase,
            ProvisioningPhase::PasswordReady
        );
        assert_eq!(
            state(service.dispatch(request(
                3,
                ProvisioningCommand::SetPassword {
                    password: "original password".into(),
                },
            )))
            .phase,
            ProvisioningPhase::Network
        );
        assert_eq!(service.backend.hash_calls, 1);

        assert_eq!(
            state(service.dispatch(request(4, region))).phase,
            ProvisioningPhase::Network
        );
        assert_eq!(
            state(service.dispatch(request(5, owner))).phase,
            ProvisioningPhase::Network
        );
        assert_eq!(
            state(service.dispatch(request(
                6,
                ProvisioningCommand::SetPassword {
                    password: "must not replace original".into(),
                },
            )))
            .phase,
            ProvisioningPhase::Network
        );
        assert_eq!(service.backend.hash_calls, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resumes_and_completes_every_network_and_ssh_choice() {
        for network in 0..3 {
            for ssh_enabled in [false, true] {
                let (root, paths) = fixture();
                let mut service = ProvisioningService::new(&paths, MockBackend::default());
                assert_eq!(
                    state(service.dispatch(request(
                        1,
                        ProvisioningCommand::SetRegion {
                            locale: "en_US.UTF-8".into(),
                            country: "CN".into(),
                            timezone: "Asia/Shanghai".into(),
                            hostname: format!("cp0-{network}-{}", u8::from(ssh_enabled)),
                        },
                    )))
                    .phase,
                    ProvisioningPhase::Owner
                );
                service = ProvisioningService::new(&paths, MockBackend::default());
                assert_eq!(service.status().unwrap().phase, ProvisioningPhase::Owner);
                assert_eq!(
                    state(service.dispatch(request(
                        2,
                        ProvisioningCommand::SetOwner {
                            display_name: "Owner".into(),
                            username: "owner".into(),
                        },
                    )))
                    .phase,
                    ProvisioningPhase::PasswordReady
                );
                service = ProvisioningService::new(&paths, MockBackend::default());
                assert_eq!(
                    state(service.dispatch(request(
                        3,
                        ProvisioningCommand::SetPassword {
                            password: "matrix password".into(),
                        },
                    )))
                    .phase,
                    ProvisioningPhase::Network
                );
                service = ProvisioningService::new(&paths, MockBackend::default());
                let response = match network {
                    0 => service.dispatch(request(4, ProvisioningCommand::UseEthernet {})),
                    1 => service.dispatch(request(
                        4,
                        ProvisioningCommand::ConnectWifi {
                            ssid: "CP0-NET".into(),
                            security: WifiSecurity::Wpa2,
                            password: "wifi password".into(),
                            hidden: false,
                        },
                    )),
                    _ => service.dispatch(request(4, ProvisioningCommand::UseOffline {})),
                };
                assert_eq!(state(response).phase, ProvisioningPhase::RemoteAccess);
                service = ProvisioningService::new(&paths, MockBackend::default());
                assert_eq!(
                    state(service.dispatch(request(
                        5,
                        ProvisioningCommand::SetSshEnabled {
                            enabled: ssh_enabled,
                        },
                    )))
                    .phase,
                    ProvisioningPhase::Review
                );
                service = ProvisioningService::new(&paths, MockBackend::default());
                assert_eq!(
                    state(service.dispatch(request(6, ProvisioningCommand::Commit {}))).phase,
                    ProvisioningPhase::Complete
                );
                if ssh_enabled {
                    // MockBackend records SSH only in memory; emulate the Linux
                    // backend's durable consent marker before process restart.
                    atomic_write(&paths.ssh_marker, b"password\n", 0o600).unwrap();
                }
                service = ProvisioningService::new(&paths, MockBackend::default());
                assert_eq!(service.status().unwrap().phase, ProvisioningPhase::Complete);
                let persisted = fs::read_to_string(&paths.state_file).unwrap();
                assert!(!persisted.contains("matrix password"));
                assert!(!persisted.contains("wifi password"));
                fs::remove_dir_all(root).unwrap();
            }
        }
    }

    #[test]
    fn rejects_commands_before_their_prerequisites() {
        let (root, paths) = fixture();
        let mut service = ProvisioningService::new(&paths, MockBackend::default());
        for (id, command) in [
            (
                1,
                ProvisioningCommand::SetOwner {
                    display_name: "Owner".into(),
                    username: "owner".into(),
                },
            ),
            (
                2,
                ProvisioningCommand::SetPassword {
                    password: "early password".into(),
                },
            ),
            (3, ProvisioningCommand::UseEthernet {}),
            (4, ProvisioningCommand::UseOffline {}),
            (5, ProvisioningCommand::SetSshEnabled { enabled: true }),
            (6, ProvisioningCommand::Commit {}),
        ] {
            assert!(matches!(
                service.dispatch(request(id, command)).outcome,
                ProvisioningOutcome::Error {
                    code: ProvisioningErrorCode::InvalidState,
                    ..
                }
            ));
            assert_eq!(
                service.status().unwrap().phase,
                ProvisioningPhase::Unprovisioned
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_password_hash_remains_retryable_without_persisting_secret() {
        let (root, paths) = fixture();
        let backend = MockBackend {
            fail_hash_once: true,
            ..MockBackend::default()
        };
        let mut service = ProvisioningService::new(&paths, backend);
        state(service.dispatch(request(
            1,
            ProvisioningCommand::SetRegion {
                locale: "en_US.UTF-8".into(),
                country: "CN".into(),
                timezone: "Asia/Shanghai".into(),
                hostname: "cp0-test".into(),
            },
        )));
        state(service.dispatch(request(
            2,
            ProvisioningCommand::SetOwner {
                display_name: "Owner".into(),
                username: "owner".into(),
            },
        )));
        assert!(matches!(
            service
                .dispatch(request(
                    3,
                    ProvisioningCommand::SetPassword {
                        password: "first password".into(),
                    },
                ))
                .outcome,
            ProvisioningOutcome::Error {
                code: ProvisioningErrorCode::Operation,
                ..
            }
        ));
        assert_eq!(
            service.status().unwrap().phase,
            ProvisioningPhase::PasswordReady
        );
        assert!(
            !fs::read_to_string(&paths.state_file)
                .unwrap()
                .contains("first password")
        );
        assert_eq!(
            state(service.dispatch(request(
                4,
                ProvisioningCommand::SetPassword {
                    password: "second password".into(),
                },
            )))
            .phase,
            ProvisioningPhase::Network
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detects_torn_completion_and_refuses_mutation() {
        let (root, paths) = fixture();
        let status = ProvisioningStatus {
            phase: ProvisioningPhase::Complete,
            locale: Some("en_US.UTF-8".into()),
            country: Some("CN".into()),
            timezone: Some("Asia/Shanghai".into()),
            hostname: Some("cp0".into()),
            display_name: Some("Owner".into()),
            username: Some("owner".into()),
            password_configured: true,
            network_choice: Some(NetworkChoice::Offline {}),
            ssh_enabled: false,
            network_runtime: None,
        };
        let store = StateStore::new(&paths);
        store.save(&status).unwrap();
        let mut service = ProvisioningService::new(&paths, MockBackend::default());
        assert_eq!(
            service.status().unwrap().phase,
            ProvisioningPhase::RepairRequired
        );
        assert!(matches!(
            service
                .dispatch(request(1, ProvisioningCommand::UseOffline {}))
                .outcome,
            ProvisioningOutcome::Error {
                code: ProvisioningErrorCode::RepairRequired,
                ..
            }
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_commit_remains_reviewable_and_can_be_retried() {
        let (root, paths) = fixture();
        let backend = MockBackend {
            fail_configure_once: true,
            ..MockBackend::default()
        };
        let mut service = ProvisioningService::new(&paths, backend);
        state(service.dispatch(request(
            1,
            ProvisioningCommand::SetRegion {
                locale: "en_US.UTF-8".into(),
                country: "CN".into(),
                timezone: "Asia/Shanghai".into(),
                hostname: "cp0-retry".into(),
            },
        )));
        state(service.dispatch(request(
            2,
            ProvisioningCommand::SetOwner {
                display_name: "Owner".into(),
                username: "owner".into(),
            },
        )));
        state(service.dispatch(request(
            3,
            ProvisioningCommand::SetPassword {
                password: "retry-password".into(),
            },
        )));
        state(service.dispatch(request(4, ProvisioningCommand::UseOffline {})));
        state(service.dispatch(request(
            5,
            ProvisioningCommand::SetSshEnabled { enabled: true },
        )));
        assert!(matches!(
            service
                .dispatch(request(6, ProvisioningCommand::Commit {}))
                .outcome,
            ProvisioningOutcome::Error {
                code: ProvisioningErrorCode::Operation,
                ..
            }
        ));
        assert_eq!(service.status().unwrap().phase, ProvisioningPhase::Review);
        assert_eq!(
            state(service.dispatch(request(7, ProvisioningCommand::Commit {}))).phase,
            ProvisioningPhase::Complete
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_live_ssh_activation_keeps_durable_setup_complete() {
        let (root, paths) = fixture();
        let backend = MockBackend {
            fail_activate_once: true,
            ..MockBackend::default()
        };
        let mut service = ProvisioningService::new(&paths, backend);
        advance_to_review(&mut service, true);
        // MockBackend tracks consent in memory; emulate the Linux backend's
        // durable marker so post-error reconciliation checks the real layout.
        atomic_write(&paths.ssh_marker, b"password\n", 0o600).unwrap();
        assert!(matches!(
            service
                .dispatch(request(6, ProvisioningCommand::Commit {}))
                .outcome,
            ProvisioningOutcome::Error {
                code: ProvisioningErrorCode::Operation,
                ..
            }
        ));
        assert_eq!(service.status().unwrap().phase, ProvisioningPhase::Complete);
        assert!(paths.complete_marker.is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovers_both_sides_of_completion_marker_transaction() {
        let (root, paths) = fixture();
        let mut service = ProvisioningService::new(&paths, MockBackend::default());
        advance_to_review(&mut service, true);

        let mut interrupted = service.status().unwrap();
        service.backend.configure_ssh(true).unwrap();
        service
            .identity_store
            .set_ssh_membership("owner", true)
            .unwrap();
        interrupted.phase = ProvisioningPhase::Committing;
        service.state_store.save(&interrupted).unwrap();
        assert_eq!(service.status().unwrap().phase, ProvisioningPhase::Review);

        atomic_write(&paths.ssh_marker, b"password\n", 0o600).unwrap();
        interrupted.phase = ProvisioningPhase::Committing;
        service.state_store.save(&interrupted).unwrap();
        service.state_store.mark_complete().unwrap();
        assert_eq!(service.status().unwrap().phase, ProvisioningPhase::Complete);
        assert_eq!(
            service.state_store.load().unwrap().phase,
            ProvisioningPhase::Complete
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_and_bounds_nmcli_wifi_results() {
        let networks = parse_nmcli_wifi(
            b"*:Office\\:5G:92:WPA2\n:Corp:70:WPA2 802.1X\n:Legacy:50:WEP\n:Open:31:--\n:Office\\:5G:40:WPA2\n",
        )
        .unwrap();
        assert_eq!(networks.len(), 4);
        assert_eq!(networks[0].ssid, "Office:5G");
        assert!(networks[0].connected);
        assert_eq!(networks[1].security, WifiSecurity::Unsupported);
        assert_eq!(networks[2].security, WifiSecurity::Unsupported);
        assert_eq!(networks[3].security, WifiSecurity::Open);
        assert_eq!(
            parse_nmcli_devices(
                b"eth0:ethernet:connected\nwlan0:wifi:disconnected\nusb\\:0:ethernet:connected (externally)\n"
            )
            .unwrap(),
            vec![
                NmcliDevice {
                    name: "eth0".into(),
                    kind: "ethernet".into(),
                    connected: true,
                },
                NmcliDevice {
                    name: "wlan0".into(),
                    kind: "wifi".into(),
                    connected: false,
                },
                NmcliDevice {
                    name: "usb:0".into(),
                    kind: "ethernet".into(),
                    connected: true,
                },
            ]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn serves_a_complete_request_over_a_seqpacket_socket() {
        let (root, paths) = fixture();
        let service = ProvisioningService::new(&paths, MockBackend::default());
        let mut descriptors = [-1; 2];
        let result = unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                descriptors.as_mut_ptr(),
            )
        };
        assert_eq!(result, 0);
        let server_stream = unsafe { UnixStream::from_raw_fd(descriptors[0]) };
        let mut client_stream = unsafe { UnixStream::from_raw_fd(descriptors[1]) };
        let mut server = ProvisioningServer::new(service, unsafe { libc::geteuid() });
        client_stream
            .write_all(
                b"{\"protocol_version\":1,\"request_id\":17,\"command\":{\"name\":\"get-status\"}}\n",
            )
            .unwrap();
        server.handle_connection(server_stream).unwrap();
        let mut response = [0_u8; MAX_PROVISION_FRAME_BYTES];
        let length = client_stream.read(&mut response).unwrap();
        let document: ProvisioningResponse =
            serde_json::from_slice(&response[..length - 1]).unwrap();
        assert_eq!(document.request_id, 17);
        assert!(matches!(
            document.outcome,
            ProvisioningOutcome::State {
                state: ProvisioningStatus {
                    phase: ProvisioningPhase::Unprovisioned,
                    ..
                }
            }
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
