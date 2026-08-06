use std::fmt;
use std::io::{self, BufRead, Write};
use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

pub const PROVISION_PROTOCOL_VERSION: u32 = 1;
pub const MAX_PROVISION_FRAME_BYTES: usize = 16 * 1024;
pub const MAX_ERROR_CHARS: usize = 160;
pub const MAX_WIFI_NETWORKS: usize = 64;
pub const MIN_PASSWORD_CHARS: usize = 10;
pub const MAX_PASSWORD_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProvisioningPhase {
    Unprovisioned,
    Region,
    Owner,
    PasswordReady,
    Network,
    RemoteAccess,
    Review,
    Committing,
    Complete,
    RepairRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WifiSecurity {
    Open,
    Wpa2,
    Wpa3,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum NetworkChoice {
    Ethernet {},
    Wifi { profile_id: String, ssid: String },
    Offline {},
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkRuntimeStatus {
    pub network_manager_available: bool,
    pub ethernet_connected: bool,
    pub ethernet_ipv4: Option<String>,
    pub wifi_available: bool,
    pub wifi_connected: bool,
    pub wifi_ipv4: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvisioningStatus {
    pub phase: ProvisioningPhase,
    pub locale: Option<String>,
    pub country: Option<String>,
    pub timezone: Option<String>,
    pub hostname: Option<String>,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub password_configured: bool,
    pub network_choice: Option<NetworkChoice>,
    pub ssh_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_runtime: Option<NetworkRuntimeStatus>,
}

impl Default for ProvisioningStatus {
    fn default() -> Self {
        Self {
            phase: ProvisioningPhase::Unprovisioned,
            locale: None,
            country: None,
            timezone: None,
            hostname: None,
            display_name: None,
            username: None,
            password_configured: false,
            network_choice: None,
            ssh_enabled: false,
            network_runtime: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal_percent: u8,
    pub security: WifiSecurity,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvisioningRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: ProvisioningCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ProvisioningCommand {
    GetStatus {},
    SetRegion {
        locale: String,
        country: String,
        timezone: String,
        hostname: String,
    },
    SetOwner {
        display_name: String,
        username: String,
    },
    SetPassword {
        password: String,
    },
    ChangePassword {
        current_password: String,
        new_password: String,
    },
    VerifyOwnerPassword {
        current_password: String,
    },
    ListWifi {},
    ConnectWifi {
        ssid: String,
        security: WifiSecurity,
        password: String,
        hidden: bool,
    },
    UseEthernet {},
    UseOffline {},
    SetSshEnabled {
        enabled: bool,
    },
    Commit {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvisioningResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: ProvisioningOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ProvisioningOutcome {
    State {
        state: ProvisioningStatus,
    },
    WifiList {
        networks: Vec<WifiNetwork>,
    },
    Authenticated {},
    Error {
        code: ProvisioningErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProvisioningErrorCode {
    InvalidRequest,
    Unauthorized,
    InvalidState,
    InvalidValue,
    Authentication,
    Unavailable,
    Operation,
    RepairRequired,
    Internal,
}

#[derive(Debug)]
pub enum ProvisioningProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidValue(&'static str),
}

impl fmt::Display for ProvisioningProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "provisioning protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid provisioning JSON: {error}"),
            Self::FrameTooLarge => formatter.write_str("provisioning frame exceeds 16384 bytes"),
            Self::UnterminatedFrame => formatter.write_str("provisioning frame is not terminated"),
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "unsupported provisioning protocol version {version}"
                )
            }
            Self::InvalidValue(field) => write!(formatter, "invalid provisioning {field}"),
        }
    }
}

impl std::error::Error for ProvisioningProtocolError {}

impl From<io::Error> for ProvisioningProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ProvisioningProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl ProvisioningRequest {
    pub fn validate(&self) -> Result<(), ProvisioningProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.command {
            ProvisioningCommand::GetStatus {}
            | ProvisioningCommand::ListWifi {}
            | ProvisioningCommand::UseEthernet {}
            | ProvisioningCommand::UseOffline {}
            | ProvisioningCommand::Commit {} => Ok(()),
            ProvisioningCommand::SetRegion {
                locale,
                country,
                timezone,
                hostname,
            } => {
                validate_locale(locale)?;
                validate_country(country)?;
                validate_timezone(timezone)?;
                validate_hostname(hostname)
            }
            ProvisioningCommand::SetOwner {
                display_name,
                username,
            } => {
                validate_display_name(display_name)?;
                validate_username(username)
            }
            ProvisioningCommand::SetPassword { password } => validate_password(password),
            ProvisioningCommand::ChangePassword {
                current_password,
                new_password,
            } => {
                validate_current_password(current_password)?;
                validate_password(new_password)
            }
            ProvisioningCommand::VerifyOwnerPassword { current_password } => {
                validate_current_password(current_password)
            }
            ProvisioningCommand::ConnectWifi {
                ssid,
                security,
                password,
                ..
            } => {
                validate_ssid(ssid)?;
                match security {
                    WifiSecurity::Open if password.is_empty() => Ok(()),
                    WifiSecurity::Open => {
                        Err(ProvisioningProtocolError::InvalidValue("Wi-Fi password"))
                    }
                    WifiSecurity::Wpa2 | WifiSecurity::Wpa3
                        if (8..=63).contains(&password.chars().count())
                            && password.chars().all(|character| !character.is_control()) =>
                    {
                        Ok(())
                    }
                    WifiSecurity::Unsupported => {
                        Err(ProvisioningProtocolError::InvalidValue("Wi-Fi security"))
                    }
                    _ => Err(ProvisioningProtocolError::InvalidValue("Wi-Fi password")),
                }
            }
            ProvisioningCommand::SetSshEnabled { .. } => Ok(()),
        }
    }

    pub fn zeroize_secrets(&mut self) {
        match &mut self.command {
            ProvisioningCommand::SetPassword { password }
            | ProvisioningCommand::ConnectWifi { password, .. } => {
                password.zeroize();
            }
            ProvisioningCommand::ChangePassword {
                current_password,
                new_password,
            } => {
                current_password.zeroize();
                new_password.zeroize();
            }
            ProvisioningCommand::VerifyOwnerPassword { current_password } => {
                current_password.zeroize();
            }
            _ => {}
        }
    }
}

impl ProvisioningStatus {
    pub fn validate(&self) -> Result<(), ProvisioningProtocolError> {
        if self.phase == ProvisioningPhase::RepairRequired {
            return Ok(());
        }
        if let Some(locale) = &self.locale {
            validate_locale(locale)?;
        }
        if let Some(country) = &self.country {
            validate_country(country)?;
        }
        if let Some(timezone) = &self.timezone {
            validate_timezone(timezone)?;
        }
        if let Some(hostname) = &self.hostname {
            validate_hostname(hostname)?;
        }
        if let Some(display_name) = &self.display_name {
            validate_display_name(display_name)?;
        }
        if let Some(username) = &self.username {
            validate_username(username)?;
        }
        if let Some(NetworkChoice::Wifi { profile_id, ssid }) = &self.network_choice {
            if profile_id.is_empty() || profile_id.len() > 64 || !profile_id.is_ascii() {
                return Err(ProvisioningProtocolError::InvalidValue("network profile"));
            }
            validate_ssid(ssid)?;
        }
        if let Some(runtime) = &self.network_runtime {
            runtime.validate()?;
        }
        let region_complete = self.locale.is_some()
            && self.country.is_some()
            && self.timezone.is_some()
            && self.hostname.is_some();
        let owner_complete = self.display_name.is_some() && self.username.is_some();
        if self.phase >= ProvisioningPhase::Owner && !region_complete
            || self.phase >= ProvisioningPhase::PasswordReady && !owner_complete
            || self.phase >= ProvisioningPhase::Network && !self.password_configured
            || self.phase >= ProvisioningPhase::RemoteAccess && self.network_choice.is_none()
        {
            return Err(ProvisioningProtocolError::InvalidValue("state"));
        }
        Ok(())
    }
}

impl NetworkRuntimeStatus {
    pub fn validate(&self) -> Result<(), ProvisioningProtocolError> {
        for address in [&self.ethernet_ipv4, &self.wifi_ipv4].into_iter().flatten() {
            address
                .parse::<Ipv4Addr>()
                .map_err(|_| ProvisioningProtocolError::InvalidValue("network address"))?;
        }
        if !self.network_manager_available
            && (self.ethernet_connected
                || self.ethernet_ipv4.is_some()
                || self.wifi_available
                || self.wifi_connected
                || self.wifi_ipv4.is_some())
            || self.ethernet_ipv4.is_some() && !self.ethernet_connected
            || self.wifi_connected && !self.wifi_available
            || self.wifi_ipv4.is_some() && !self.wifi_connected
        {
            return Err(ProvisioningProtocolError::InvalidValue(
                "network runtime state",
            ));
        }
        Ok(())
    }
}

impl WifiNetwork {
    pub fn validate(&self) -> Result<(), ProvisioningProtocolError> {
        validate_ssid(&self.ssid)?;
        if self.signal_percent > 100 {
            return Err(ProvisioningProtocolError::InvalidValue("Wi-Fi signal"));
        }
        Ok(())
    }
}

impl ProvisioningResponse {
    pub fn state(request_id: u64, state: ProvisioningStatus) -> Self {
        Self {
            protocol_version: PROVISION_PROTOCOL_VERSION,
            request_id,
            outcome: ProvisioningOutcome::State { state },
        }
    }

    pub fn wifi_list(request_id: u64, networks: Vec<WifiNetwork>) -> Self {
        Self {
            protocol_version: PROVISION_PROTOCOL_VERSION,
            request_id,
            outcome: ProvisioningOutcome::WifiList { networks },
        }
    }

    pub fn authenticated(request_id: u64) -> Self {
        Self {
            protocol_version: PROVISION_PROTOCOL_VERSION,
            request_id,
            outcome: ProvisioningOutcome::Authenticated {},
        }
    }

    pub fn error(request_id: u64, code: ProvisioningErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: PROVISION_PROTOCOL_VERSION,
            request_id,
            outcome: ProvisioningOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), ProvisioningProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.outcome {
            ProvisioningOutcome::State { state } => state.validate(),
            ProvisioningOutcome::WifiList { networks } => {
                if networks.len() > MAX_WIFI_NETWORKS {
                    return Err(ProvisioningProtocolError::InvalidValue("Wi-Fi list"));
                }
                for network in networks {
                    network.validate()?;
                }
                Ok(())
            }
            ProvisioningOutcome::Authenticated {} => Ok(()),
            ProvisioningOutcome::Error { message, .. } => {
                if message.is_empty()
                    || message.chars().count() > MAX_ERROR_CHARS
                    || message.chars().any(char::is_control)
                {
                    Err(ProvisioningProtocolError::InvalidValue("error message"))
                } else {
                    Ok(())
                }
            }
        }
    }
}

pub fn validate_username(value: &str) -> Result<(), ProvisioningProtocolError> {
    let mut characters = value.chars();
    let first = characters
        .next()
        .ok_or(ProvisioningProtocolError::InvalidValue("username"))?;
    if value.len() > 32
        || !(first.is_ascii_lowercase() || first == '_')
        || !characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '_'
                || character == '-'
        })
        || value == "root"
        || value.starts_with("cp0-")
    {
        return Err(ProvisioningProtocolError::InvalidValue("username"));
    }
    Ok(())
}

pub fn validate_password(value: &str) -> Result<(), ProvisioningProtocolError> {
    let count = value.chars().count();
    if !(MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&count)
        || !value
            .chars()
            .all(|character| character.is_ascii() && !character.is_control())
    {
        return Err(ProvisioningProtocolError::InvalidValue("password"));
    }
    Ok(())
}

fn validate_current_password(value: &str) -> Result<(), ProvisioningProtocolError> {
    let count = value.chars().count();
    if count == 0
        || count > MAX_PASSWORD_CHARS
        || !value
            .chars()
            .all(|character| character.is_ascii() && !character.is_control())
    {
        return Err(ProvisioningProtocolError::InvalidValue("current password"));
    }
    Ok(())
}

pub fn validate_hostname(value: &str) -> Result<(), ProvisioningProtocolError> {
    if value.is_empty()
        || value.len() > 63
        || !value.is_ascii()
        || value.starts_with('-')
        || value.ends_with('-')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(ProvisioningProtocolError::InvalidValue("hostname"));
    }
    Ok(())
}

fn validate_display_name(value: &str) -> Result<(), ProvisioningProtocolError> {
    let count = value.chars().count();
    if !(1..=64).contains(&count) || value.contains(':') || value.chars().any(char::is_control) {
        Err(ProvisioningProtocolError::InvalidValue("display name"))
    } else {
        Ok(())
    }
}

fn validate_locale(value: &str) -> Result<(), ProvisioningProtocolError> {
    if matches!(value, "en_US.UTF-8" | "zh_CN.UTF-8") {
        Ok(())
    } else {
        Err(ProvisioningProtocolError::InvalidValue("locale"))
    }
}

fn validate_country(value: &str) -> Result<(), ProvisioningProtocolError> {
    if value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_uppercase()) {
        Ok(())
    } else {
        Err(ProvisioningProtocolError::InvalidValue("country"))
    }
}

fn validate_timezone(value: &str) -> Result<(), ProvisioningProtocolError> {
    if value.is_empty()
        || value.len() > 64
        || !value.is_ascii()
        || value.starts_with('/')
        || value.contains("..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'+'))
    {
        Err(ProvisioningProtocolError::InvalidValue("timezone"))
    } else {
        Ok(())
    }
}

fn validate_ssid(value: &str) -> Result<(), ProvisioningProtocolError> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 32 || value.chars().any(char::is_control) {
        Err(ProvisioningProtocolError::InvalidValue("SSID"))
    } else {
        Ok(())
    }
}

fn validate_version(version: u32) -> Result<(), ProvisioningProtocolError> {
    if version == PROVISION_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ProvisioningProtocolError::UnsupportedVersion(version))
    }
}

pub fn read_request(
    reader: &mut impl BufRead,
) -> Result<Option<ProvisioningRequest>, ProvisioningProtocolError> {
    let Some(mut frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let parsed = serde_json::from_slice(&frame);
    frame.fill(0);
    let mut request: ProvisioningRequest = parsed?;
    if let Err(error) = request.validate() {
        request.zeroize_secrets();
        return Err(error);
    }
    Ok(Some(request))
}

pub fn write_request(
    writer: &mut impl Write,
    request: &ProvisioningRequest,
) -> Result<(), ProvisioningProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<ProvisioningResponse>, ProvisioningProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: ProvisioningResponse = serde_json::from_slice(&frame)?;
    response.validate()?;
    Ok(Some(response))
}

pub fn write_response(
    writer: &mut impl Write,
    response: &ProvisioningResponse,
) -> Result<(), ProvisioningProtocolError> {
    response.validate()?;
    write_frame(writer, response)
}

fn write_frame(
    writer: &mut impl Write,
    value: &impl Serialize,
) -> Result<(), ProvisioningProtocolError> {
    let mut encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_PROVISION_FRAME_BYTES {
        return Err(ProvisioningProtocolError::FrameTooLarge);
    }
    encoded.push(b'\n');
    writer.write_all(&encoded)?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, ProvisioningProtocolError> {
    let mut frame = Vec::with_capacity(256);
    let mut terminated = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let consumed = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if frame.len() + consumed > MAX_PROVISION_FRAME_BYTES {
            return Err(ProvisioningProtocolError::FrameTooLarge);
        }
        terminated = available[consumed - 1] == b'\n';
        frame.extend_from_slice(&available[..consumed]);
        reader.consume(consumed);
        if terminated {
            break;
        }
    }
    if frame.is_empty() {
        return Ok(None);
    }
    if !terminated {
        return Err(ProvisioningProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    fn request(command: ProvisioningCommand) -> ProvisioningRequest {
        ProvisioningRequest {
            protocol_version: 1,
            request_id: 7,
            command,
        }
    }

    #[test]
    fn round_trips_bounded_commands() {
        let commands = [
            ProvisioningCommand::GetStatus {},
            ProvisioningCommand::SetOwner {
                display_name: "Owner".into(),
                username: "owner_1".into(),
            },
            ProvisioningCommand::SetPassword {
                password: "correct horse".into(),
            },
            ProvisioningCommand::ChangePassword {
                current_password: "correct horse".into(),
                new_password: "new password".into(),
            },
            ProvisioningCommand::VerifyOwnerPassword {
                current_password: "correct horse".into(),
            },
            ProvisioningCommand::UseOffline {},
            ProvisioningCommand::SetSshEnabled { enabled: true },
        ];
        for command in commands {
            let expected = request(command);
            let mut bytes = Vec::new();
            write_request(&mut bytes, &expected).unwrap();
            assert_eq!(
                read_request(&mut BufReader::new(Cursor::new(bytes))).unwrap(),
                Some(expected)
            );
        }
    }

    #[test]
    fn enforces_identity_and_secret_bounds() {
        assert!(validate_username("owner").is_ok());
        assert!(validate_username("root").is_err());
        assert!(validate_username("cp0-shell").is_err());
        assert!(validate_username("Owner").is_err());
        assert!(validate_password("short").is_err());
        assert!(validate_password("ten-chars!").is_ok());
        let mut secrets = request(ProvisioningCommand::ChangePassword {
            current_password: "current password".into(),
            new_password: "replacement password".into(),
        });
        secrets.zeroize_secrets();
        assert!(matches!(
            secrets.command,
            ProvisioningCommand::ChangePassword {
                current_password,
                new_password,
            } if current_password.is_empty() && new_password.is_empty()
        ));
        let mut verification = request(ProvisioningCommand::VerifyOwnerPassword {
            current_password: "correct horse".into(),
        });
        verification.zeroize_secrets();
        assert!(matches!(
            verification.command,
            ProvisioningCommand::VerifyOwnerPassword { current_password }
                if current_password.is_empty()
        ));
        assert!(
            request(ProvisioningCommand::ChangePassword {
                current_password: String::new(),
                new_password: "new password".into(),
            })
            .validate()
            .is_err()
        );
        assert!(
            request(ProvisioningCommand::ConnectWifi {
                ssid: "Corp".into(),
                security: WifiSecurity::Unsupported,
                password: String::new(),
                hidden: false,
            })
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_fields_and_inconsistent_state() {
        let document = b"{\"protocol_version\":1,\"request_id\":1,\"command\":{\"name\":\"get-status\",\"extra\":true}}\n";
        assert!(read_request(&mut BufReader::new(&document[..])).is_err());
        let status = ProvisioningStatus {
            phase: ProvisioningPhase::Complete,
            ..Default::default()
        };
        assert!(status.validate().is_err());
    }

    #[test]
    fn never_serializes_passwords_in_status_responses() {
        let response = ProvisioningResponse::state(
            1,
            ProvisioningStatus {
                network_runtime: Some(NetworkRuntimeStatus {
                    network_manager_available: true,
                    ethernet_connected: true,
                    ethernet_ipv4: Some("192.168.20.146".into()),
                    wifi_available: true,
                    ..NetworkRuntimeStatus::default()
                }),
                ..ProvisioningStatus::default()
            },
        );
        let encoded = serde_json::to_string(&response).unwrap();
        assert!(!encoded.contains("password\":"));
        assert!(encoded.contains("password_configured"));
        assert!(encoded.contains("192.168.20.146"));
    }

    #[test]
    fn validates_runtime_network_consistency() {
        let valid = NetworkRuntimeStatus {
            network_manager_available: true,
            ethernet_connected: true,
            ethernet_ipv4: Some("192.168.31.121".into()),
            wifi_available: true,
            wifi_connected: true,
            wifi_ipv4: Some("192.168.31.122".into()),
        };
        assert!(valid.validate().is_ok());
        assert!(
            NetworkRuntimeStatus {
                network_manager_available: true,
                ethernet_connected: false,
                ethernet_ipv4: Some("192.168.31.121".into()),
                ..NetworkRuntimeStatus::default()
            }
            .validate()
            .is_err()
        );
    }
}
