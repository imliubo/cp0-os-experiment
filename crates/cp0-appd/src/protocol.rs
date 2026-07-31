use std::fmt;
use std::io::{self, BufRead, Write};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

use crate::{
    DeviceMode, DeviceSettings, DocumentPrompt, Notification, PermissionChoice, PermissionPrompt,
};

pub const APPD_PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 8 * 1024;
pub const MAX_APP_LIST_PAGE: u8 = 8;
pub const MAX_LOG_LINES: u16 = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppdRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: AppdCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AppdCommand {
    Ping,
    List {
        offset: u16,
        limit: u8,
    },
    Start {
        app_id: String,
    },
    Stop {
        app_id: String,
    },
    Install {
        package_name: String,
    },
    StoreInstall {
        package_name: String,
        app_id: String,
        version: String,
        package_sha256: String,
        package_bytes: u64,
    },
    Rollback {
        app_id: String,
    },
    Logs {
        app_id: String,
        limit: u16,
    },
    GetPermissionPrompt,
    ResolvePermission {
        prompt_id: u64,
        choice: PermissionChoice,
    },
    ResetPermission {
        app_id: String,
        permission: cp0_manifest::Permission,
    },
    GetDeviceSettings,
    SetDeviceMode {
        mode: DeviceMode,
        enabled: bool,
    },
    TakeNotification,
    GetDocumentPrompt,
    ResolveDocument {
        prompt_id: u64,
        document_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppdResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: ResponseOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ResponseOutcome {
    Ok { data: ResponseData },
    Error { code: ErrorCode, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ResponseData {
    Pong,
    Applications {
        apps: Vec<AppSummary>,
        next_offset: Option<u16>,
    },
    Started {
        app_id: String,
        unit: String,
    },
    Stopped {
        app_id: String,
    },
    Installed {
        app_id: String,
        version: String,
        previous_version: Option<String>,
        trust: String,
    },
    RolledBack {
        app_id: String,
        version: String,
    },
    Logs {
        app_id: String,
        lines: Vec<String>,
    },
    PendingPermission {
        prompt: Option<PermissionPrompt>,
    },
    PermissionResolved {
        prompt_id: u64,
        app_id: String,
        permission: cp0_manifest::Permission,
        choice: PermissionChoice,
    },
    PermissionReset {
        app_id: String,
        permission: cp0_manifest::Permission,
    },
    DeviceSettings {
        settings: DeviceSettings,
    },
    DeviceModeChanged {
        settings: DeviceSettings,
    },
    NextNotification {
        notification: Option<Notification>,
    },
    PendingDocument {
        prompt: Option<DocumentPrompt>,
    },
    DocumentResolved {
        prompt_id: u64,
        app_id: String,
        document_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSummary {
    pub app_id: String,
    pub name: String,
    pub version: String,
    pub display: cp0_manifest::DisplayMode,
    pub running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    InvalidRequest,
    Unauthorized,
    NotFound,
    AlreadyRunning,
    NotRunning,
    ResourceExhausted,
    Untrusted,
    Conflict,
    Internal,
}

#[derive(Debug)]
pub enum ProtocolError {
    Io(io::Error),
    FrameTooLarge,
    UnterminatedFrame,
    InvalidJson(serde_json::Error),
    UnsupportedVersion(u32),
    InvalidAppId,
    InvalidPagination,
    InvalidPromptId,
    InvalidDocumentId,
    InvalidPackageName,
    InvalidStoreInstall,
    InvalidLogLimit,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "application protocol I/O error: {error}"),
            Self::FrameTooLarge => write!(
                formatter,
                "application protocol frame exceeds {MAX_FRAME_BYTES} bytes"
            ),
            Self::UnterminatedFrame => {
                formatter.write_str("application protocol frame is not newline terminated")
            }
            Self::InvalidJson(error) => {
                write!(formatter, "invalid application protocol JSON: {error}")
            }
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "unsupported application protocol version {version}"
                )
            }
            Self::InvalidAppId => formatter.write_str("invalid application ID"),
            Self::InvalidPagination => {
                formatter.write_str("application list limit must be between 1 and 8")
            }
            Self::InvalidPromptId => formatter.write_str("permission prompt ID must be non-zero"),
            Self::InvalidDocumentId => formatter.write_str("invalid document ID"),
            Self::InvalidPackageName => formatter.write_str("invalid incoming package name"),
            Self::InvalidStoreInstall => formatter.write_str("invalid store installation metadata"),
            Self::InvalidLogLimit => {
                formatter.write_str("application log limit must be between 1 and 100")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}

impl From<io::Error> for ProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::InvalidJson(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    pub pid: u32,
    pub uid: u32,
    pub gid: u32,
}

impl AppdRequest {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.protocol_version != APPD_PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion(self.protocol_version));
        }
        match &self.command {
            AppdCommand::List { limit, .. } if !(1..=MAX_APP_LIST_PAGE).contains(limit) => {
                Err(ProtocolError::InvalidPagination)
            }
            AppdCommand::Start { app_id }
            | AppdCommand::Stop { app_id }
            | AppdCommand::Rollback { app_id }
            | AppdCommand::Logs { app_id, .. }
                if !cp0_manifest::is_valid_app_id(app_id) =>
            {
                Err(ProtocolError::InvalidAppId)
            }
            AppdCommand::ResetPermission { app_id, .. }
                if !cp0_manifest::is_valid_app_id(app_id) =>
            {
                Err(ProtocolError::InvalidAppId)
            }
            AppdCommand::ResolvePermission { prompt_id: 0, .. } => {
                Err(ProtocolError::InvalidPromptId)
            }
            AppdCommand::ResolveDocument { prompt_id: 0, .. } => {
                Err(ProtocolError::InvalidPromptId)
            }
            AppdCommand::ResolveDocument {
                document_id: Some(document_id),
                ..
            } if !cp0_document_protocol::is_valid_document_id(document_id) => {
                Err(ProtocolError::InvalidDocumentId)
            }
            AppdCommand::Install { package_name } if !is_valid_package_name(package_name) => {
                Err(ProtocolError::InvalidPackageName)
            }
            AppdCommand::StoreInstall {
                package_name,
                app_id,
                version,
                package_sha256,
                package_bytes,
            } if !is_valid_package_name(package_name)
                || !cp0_manifest::is_valid_app_id(app_id)
                || !cp0_manifest::is_valid_app_version(version)
                || !cp0_store_protocol::is_lower_hex(package_sha256, 32)
                || !(1..=cp0_store_protocol::MAX_PACKAGE_BYTES).contains(package_bytes) =>
            {
                Err(ProtocolError::InvalidStoreInstall)
            }
            AppdCommand::Logs { limit, .. } if !(1..=MAX_LOG_LINES).contains(limit) => {
                Err(ProtocolError::InvalidLogLimit)
            }
            _ => Ok(()),
        }
    }
}

fn is_valid_package_name(name: &str) -> bool {
    (6..=128).contains(&name.len())
        && name.ends_with(".capp")
        && !name.starts_with('.')
        && !name.contains("..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

impl AppdResponse {
    pub fn success(request_id: u64, data: ResponseData) -> Self {
        Self {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id,
            outcome: ResponseOutcome::Ok { data },
        }
    }

    pub fn error(request_id: u64, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id,
            outcome: ResponseOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }
}

pub fn read_request(reader: &mut impl BufRead) -> Result<Option<AppdRequest>, ProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: AppdRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn read_response(reader: &mut impl BufRead) -> Result<Option<AppdResponse>, ProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: AppdResponse = serde_json::from_slice(&frame)?;
    if response.protocol_version != APPD_PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion(response.protocol_version));
    }
    Ok(Some(response))
}

pub fn write_request(writer: &mut impl Write, request: &AppdRequest) -> Result<(), ProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn write_response(
    writer: &mut impl Write,
    response: &AppdResponse,
) -> Result<(), ProtocolError> {
    write_frame(writer, response)
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, ProtocolError> {
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
        if frame.len() + consumed > MAX_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge);
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
        return Err(ProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<(), ProtocolError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn peer_credentials(stream: &UnixStream) -> io::Result<PeerCredentials> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: credentials and length point to valid writable storage of the
    // exact type and size required by SO_PEERCRED for the lifetime of the call.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of::<libc::ucred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SO_PEERCRED returned an unexpected credential size",
        ));
    }
    Ok(PeerCredentials {
        pid: credentials
            .pid
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "peer PID is negative"))?,
        uid: credentials.uid,
        gid: credentials.gid,
    })
}

#[cfg(not(target_os = "linux"))]
pub fn peer_credentials(_stream: &UnixStream) -> io::Result<PeerCredentials> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "peer credentials are only implemented for the Linux target",
    ))
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use super::*;

    fn start_request() -> AppdRequest {
        AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 42,
            command: AppdCommand::Start {
                app_id: "dev.cardputerzero.hello".into(),
            },
        }
    }

    #[test]
    fn round_trips_strict_request_and_response() {
        let request_json = serde_json::to_string(&start_request()).unwrap() + "\n";
        let mut reader = BufReader::new(Cursor::new(request_json));
        assert_eq!(read_request(&mut reader).unwrap(), Some(start_request()));

        let response = AppdResponse::success(
            42,
            ResponseData::Started {
                app_id: "dev.cardputerzero.hello".into(),
                unit: "cardputerzero-app-20000.service".into(),
            },
        );
        let mut encoded = Vec::new();
        write_response(&mut encoded, &response).unwrap();
        assert_eq!(encoded.last(), Some(&b'\n'));
        let decoded: AppdResponse = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, response);

        let mut encoded_request = Vec::new();
        write_request(&mut encoded_request, &start_request()).unwrap();
        let mut request_reader = Cursor::new(encoded_request);
        assert_eq!(
            read_request(&mut request_reader).unwrap(),
            Some(start_request())
        );
        let mut response_reader = Cursor::new(encoded);
        assert_eq!(read_response(&mut response_reader).unwrap(), Some(response));
    }

    #[test]
    fn round_trips_permission_resolution() {
        let request = AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 77,
            command: AppdCommand::ResolvePermission {
                prompt_id: 9,
                choice: PermissionChoice::AllowOnce,
            },
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &request).unwrap();
        assert_eq!(
            read_request(&mut Cursor::new(encoded)).unwrap(),
            Some(request)
        );

        let invalid = AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 78,
            command: AppdCommand::ResolvePermission {
                prompt_id: 0,
                choice: PermissionChoice::Deny,
            },
        };
        assert!(matches!(
            invalid.validate(),
            Err(ProtocolError::InvalidPromptId)
        ));
    }

    #[test]
    fn round_trips_device_mode_controls() {
        let request = AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 78,
            command: AppdCommand::SetDeviceMode {
                mode: DeviceMode::Recovery,
                enabled: true,
            },
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &request).unwrap();
        assert_eq!(
            read_request(&mut Cursor::new(encoded)).unwrap(),
            Some(request)
        );

        let response = AppdResponse::success(
            78,
            ResponseData::DeviceSettings {
                settings: DeviceSettings {
                    authority: crate::ManagementAuthority::Organization,
                    developer_mode: false,
                    developer_mode_allowed: false,
                    recovery_mode: true,
                    recovery_mode_allowed: true,
                    store_install_allowed: false,
                    app_launch_restricted: true,
                    denied_permission_count: 3,
                },
            },
        );
        let mut encoded = Vec::new();
        write_response(&mut encoded, &response).unwrap();
        assert_eq!(
            read_response(&mut Cursor::new(encoded)).unwrap(),
            Some(response)
        );
    }

    #[test]
    fn validates_document_resolution_against_opaque_ids() {
        let request = AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 79,
            command: AppdCommand::ResolveDocument {
                prompt_id: 3,
                document_id: Some("00000000000000010000000000000002".into()),
            },
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &request).unwrap();
        assert_eq!(
            read_request(&mut Cursor::new(encoded)).unwrap(),
            Some(request)
        );

        let invalid = AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 80,
            command: AppdCommand::ResolveDocument {
                prompt_id: 3,
                document_id: Some("../../etc/passwd".into()),
            },
        };
        assert!(matches!(
            invalid.validate(),
            Err(ProtocolError::InvalidDocumentId)
        ));
    }

    #[test]
    fn round_trips_launcher_metadata() {
        let response = AppdResponse::success(
            81,
            ResponseData::Applications {
                apps: vec![AppSummary {
                    app_id: "dev.cardputerzero.hello".into(),
                    name: "Hello Card".into(),
                    version: "0.1.0".into(),
                    display: cp0_manifest::DisplayMode::Standard,
                    running: true,
                }],
                next_offset: None,
            },
        );
        let mut encoded = Vec::new();
        write_response(&mut encoded, &response).unwrap();
        assert_eq!(
            read_response(&mut Cursor::new(encoded)).unwrap(),
            Some(response)
        );
    }

    #[test]
    fn rejects_unknown_fields_version_and_invalid_app_id() {
        let mut unknown = Cursor::new(
            b"{\"protocol_version\":1,\"request_id\":1,\"command\":{\"name\":\"ping\"},\"extra\":true}\n",
        );
        assert!(matches!(
            read_request(&mut unknown),
            Err(ProtocolError::InvalidJson(_))
        ));

        let mut wrong_version = start_request();
        wrong_version.protocol_version = 2;
        assert!(matches!(
            wrong_version.validate(),
            Err(ProtocolError::UnsupportedVersion(2))
        ));

        let mut invalid_app = start_request();
        invalid_app.command = AppdCommand::Start {
            app_id: "../../etc".into(),
        };
        assert!(matches!(
            invalid_app.validate(),
            Err(ProtocolError::InvalidAppId)
        ));

        let invalid_page = AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 3,
            command: AppdCommand::List {
                offset: 0,
                limit: 0,
            },
        };
        assert!(matches!(
            invalid_page.validate(),
            Err(ProtocolError::InvalidPagination)
        ));
        let oversized_page = AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 4,
            command: AppdCommand::List {
                offset: 0,
                limit: MAX_APP_LIST_PAGE + 1,
            },
        };
        assert!(matches!(
            oversized_page.validate(),
            Err(ProtocolError::InvalidPagination)
        ));

        for package_name in ["../escape.capp", ".hidden.capp", "nested/app.capp", "x"] {
            let request = AppdRequest {
                protocol_version: APPD_PROTOCOL_VERSION,
                request_id: 5,
                command: AppdCommand::Install {
                    package_name: package_name.into(),
                },
            };
            assert!(matches!(
                request.validate(),
                Err(ProtocolError::InvalidPackageName)
            ));
        }

        let store_install = AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id: 6,
            command: AppdCommand::StoreInstall {
                package_name: "store-package.capp".into(),
                app_id: "dev.cardputerzero.example".into(),
                version: "1.0.0".into(),
                package_sha256: "11".repeat(32),
                package_bytes: 4096,
            },
        };
        assert!(store_install.validate().is_ok());
        let mut invalid_store = store_install.clone();
        if let AppdCommand::StoreInstall { package_sha256, .. } = &mut invalid_store.command {
            *package_sha256 = "AA".repeat(32);
        }
        assert!(matches!(
            invalid_store.validate(),
            Err(ProtocolError::InvalidStoreInstall)
        ));
    }

    #[test]
    fn bounds_and_terminates_frames() {
        let mut oversized = Cursor::new(vec![b'x'; MAX_FRAME_BYTES + 1]);
        assert!(matches!(
            read_request(&mut oversized),
            Err(ProtocolError::FrameTooLarge)
        ));

        let encoded = serde_json::to_vec(&start_request()).unwrap();
        let mut unterminated = Cursor::new(encoded);
        assert!(matches!(
            read_request(&mut unterminated),
            Err(ProtocolError::UnterminatedFrame)
        ));

        let response = AppdResponse::error(1, ErrorCode::Internal, "x".repeat(MAX_FRAME_BYTES));
        assert!(matches!(
            write_response(&mut Vec::new(), &response),
            Err(ProtocolError::FrameTooLarge)
        ));
    }

    #[test]
    fn maximum_launcher_page_fits_the_bounded_frame() {
        let part = "a".repeat(31);
        let app_id = format!("{part}.{part}.{part}.{part}");
        let version = format!("1.0.0+{}", "a".repeat(58));
        assert_eq!(app_id.len(), 127);
        assert_eq!(version.len(), 64);
        let app = AppSummary {
            app_id,
            name: "\u{1}".repeat(32),
            version,
            display: cp0_manifest::DisplayMode::Immersive,
            running: true,
        };
        let response = AppdResponse::success(
            91,
            ResponseData::Applications {
                apps: vec![app; usize::from(MAX_APP_LIST_PAGE)],
                next_offset: Some(u16::from(MAX_APP_LIST_PAGE)),
            },
        );
        let mut encoded = Vec::new();
        write_response(&mut encoded, &response).unwrap();
        assert!(encoded.len() <= MAX_FRAME_BYTES);
    }
}
