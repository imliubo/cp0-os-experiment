use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::FromRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use cp0_appd::{
    APPD_PROTOCOL_VERSION, AppdCommand, AppdRequest, AppdResponse, developer_install_allowed,
    lookup_unix_account, peer_credentials, read_response as read_appd_response,
    write_request as write_appd_request,
};
use cp0_devd::{
    DEVELOPER_PROTOCOL_VERSION, DeveloperCommand, DeveloperErrorCode, DeveloperOutcome,
    DeveloperRequest, DeveloperResponse, MAX_PAIRED_HOSTS, PairedHostSummary, decode_hex_32, hex,
    read_request, write_response,
};
use cp0_package::{CApp, key_id};
use cp0_provision_protocol::{ProvisioningPhase, ProvisioningStatus};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const OWNER_UID: u32 = 1000;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
struct DeveloperPaths {
    provisioning_state: PathBuf,
    provisioning_complete: PathBuf,
    device_policy: PathBuf,
    developer_mode: PathBuf,
    developer_trust: PathBuf,
    authorized_keys: PathBuf,
    pair_store: PathBuf,
    pairing_window: PathBuf,
    appd_staging: PathBuf,
    appd_socket: PathBuf,
}

impl Default for DeveloperPaths {
    fn default() -> Self {
        Self {
            provisioning_state: "/var/lib/cardputerzero/provisioning/state.json".into(),
            provisioning_complete: "/var/lib/cardputerzero/provisioning/complete".into(),
            device_policy: "/etc/cardputerzero/device-policy.json".into(),
            developer_mode: "/var/lib/cardputerzero/registry/developer-mode".into(),
            developer_trust: "/etc/cardputerzero/trust/developers".into(),
            authorized_keys: "/etc/cardputerzero/authorized_keys".into(),
            pair_store: "/var/lib/cardputerzero/developer/paired-hosts.json".into(),
            pairing_window: "/run/cardputerzero-devd/pairing-window.json".into(),
            appd_staging: "/run/cardputerzero-appd".into(),
            appd_socket: "/run/cardputerzero-appd/control.sock".into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvisioningDocument {
    schema_version: u32,
    state: ProvisioningStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairedHost {
    label: String,
    ssh_public_key: String,
    ssh_fingerprint: String,
    developer_key_id: String,
    paired_at_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairStore {
    schema_version: u32,
    hosts: BTreeMap<String, PairedHost>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PairingWindow {
    schema_version: u32,
    expires_at_boottime_milliseconds: u64,
}

impl Default for PairStore {
    fn default() -> Self {
        Self {
            schema_version: 1,
            hosts: BTreeMap::new(),
        }
    }
}

fn main() -> ExitCode {
    let result = match env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [command] if command == "serve" => systemd_listener().and_then(|listener| {
            let (shell_uid, _) =
                lookup_unix_account("cp0-shell").map_err(|error| error.to_string())?;
            serve(listener, shell_uid)
        }),
        _ => Err("usage: cp0-devd serve".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cp0-devd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn systemd_listener() -> Result<UnixListener, String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("serve requires root".into());
    }
    let listen_pid = env::var("LISTEN_PID")
        .map_err(|_| "LISTEN_PID is not set")?
        .parse::<u32>()
        .map_err(|_| "LISTEN_PID is invalid")?;
    let listen_fds = env::var("LISTEN_FDS")
        .map_err(|_| "LISTEN_FDS is not set")?
        .parse::<u32>()
        .map_err(|_| "LISTEN_FDS is invalid")?;
    let names = env::var("LISTEN_FDNAMES").map_err(|_| "LISTEN_FDNAMES is not set")?;
    if listen_pid != std::process::id() || listen_fds != 1 || names != "control" {
        return Err("one systemd developer control socket is required".into());
    }
    // SAFETY: systemd supplies the single declared listener as descriptor 3.
    Ok(unsafe { UnixListener::from_raw_fd(3) })
}

fn serve(listener: UnixListener, shell_uid: u32) -> Result<(), String> {
    let paths = DeveloperPaths::default();
    let owner = load_owner(&paths, true)?;
    reconcile_pair_state(&paths, &owner, true)?;
    loop {
        let (stream, _) = listener
            .accept()
            .map_err(|error| format!("cannot accept developer connection: {error}"))?;
        if let Err(error) = handle_connection(stream, &paths, shell_uid, true) {
            eprintln!("cp0-devd: rejected developer connection: {error}");
        }
    }
}

fn handle_connection(
    mut stream: UnixStream,
    paths: &DeveloperPaths,
    shell_uid: u32,
    enforce_root: bool,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(CLIENT_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(CLIENT_TIMEOUT)))
        .map_err(|error| format!("cannot set developer session timeout: {error}"))?;
    let credentials = peer_credentials(&stream)
        .map_err(|error| format!("cannot authenticate developer session: {error}"))?;
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|error| format!("cannot clone developer stream: {error}"))?,
    );
    let request = match read_request(&mut reader) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(error) => {
            write_response(
                &mut stream,
                &DeveloperResponse::error(0, DeveloperErrorCode::InvalidRequest, error.to_string()),
            )
            .map_err(|write_error| write_error.to_string())?;
            return Ok(());
        }
    };
    if !authorized_peer(credentials.uid, shell_uid, &request.command) {
        return respond_error(
            &mut stream,
            request.request_id,
            DeveloperErrorCode::Unauthorized,
            "developer access command is not authorized for this local identity",
        );
    }
    let owner = match load_owner(paths, enforce_root) {
        Ok(owner) => owner,
        Err(error) => {
            return respond_error(
                &mut stream,
                request.request_id,
                DeveloperErrorCode::Unavailable,
                error,
            );
        }
    };
    let developer_mode =
        developer_install_allowed(&paths.device_policy, &paths.developer_mode, enforce_root)
            .unwrap_or(false);
    if !developer_mode && !allowed_when_developer_mode_off(&request.command) {
        return respond_error(
            &mut stream,
            request.request_id,
            DeveloperErrorCode::DeveloperModeOff,
            "Developer Mode must be enabled on the device",
        );
    }

    let response = match dispatch(
        request,
        &mut reader,
        paths,
        &owner,
        developer_mode,
        enforce_root,
    ) {
        Ok(response) => response,
        Err((request_id, code, message)) => DeveloperResponse::error(request_id, code, message),
    };
    write_response(&mut stream, &response).map_err(|error| error.to_string())
}

fn authorized_peer(uid: u32, shell_uid: u32, command: &DeveloperCommand) -> bool {
    (uid == shell_uid && management_command(command))
        || (uid == OWNER_UID && !management_command(command))
}

fn management_command(command: &DeveloperCommand) -> bool {
    matches!(
        command,
        DeveloperCommand::OpenPairing { .. }
            | DeveloperCommand::ListPaired
            | DeveloperCommand::Unpair { .. }
            | DeveloperCommand::UnpairAll
    )
}

fn allowed_when_developer_mode_off(command: &DeveloperCommand) -> bool {
    matches!(
        command,
        DeveloperCommand::Status
            | DeveloperCommand::ListPaired
            | DeveloperCommand::Unpair { .. }
            | DeveloperCommand::UnpairAll
    )
}

fn dispatch(
    request: DeveloperRequest,
    reader: &mut impl Read,
    paths: &DeveloperPaths,
    owner: &str,
    developer_mode: bool,
    enforce_root: bool,
) -> Result<DeveloperResponse, (u64, DeveloperErrorCode, String)> {
    let request_id = request.request_id;
    let outcome = match request.command {
        DeveloperCommand::Pair {
            host_label,
            ssh_public_key,
            developer_public_key,
        } => pair(
            paths,
            owner,
            request_id,
            &host_label,
            &ssh_public_key,
            &developer_public_key,
            enforce_root,
        )?,
        DeveloperCommand::Install {
            package_bytes,
            package_sha256,
        } => {
            let response = install(
                reader,
                paths,
                request_id,
                package_bytes,
                &package_sha256,
                enforce_root,
            )?;
            DeveloperOutcome::Appd { response }
        }
        DeveloperCommand::Logs { app_id, limit } => DeveloperOutcome::Appd {
            response: send_appd(paths, request_id, AppdCommand::Logs { app_id, limit })?,
        },
        DeveloperCommand::Start { app_id } => DeveloperOutcome::Appd {
            response: send_appd(paths, request_id, AppdCommand::Start { app_id })?,
        },
        DeveloperCommand::Stop { app_id } => DeveloperOutcome::Appd {
            response: send_appd(paths, request_id, AppdCommand::Stop { app_id })?,
        },
        DeveloperCommand::Uninstall { app_id } => DeveloperOutcome::Appd {
            response: send_appd(paths, request_id, AppdCommand::Uninstall { app_id })?,
        },
        DeveloperCommand::OpenPairing { duration_seconds } => {
            open_pairing(paths, request_id, duration_seconds, enforce_root)?
        }
        DeveloperCommand::ListPaired => list_paired(paths, request_id, enforce_root)?,
        DeveloperCommand::Unpair { host_fingerprint } => unpair(
            paths,
            owner,
            request_id,
            Some(&host_fingerprint),
            enforce_root,
        )?,
        DeveloperCommand::UnpairAll => unpair(paths, owner, request_id, None, enforce_root)?,
        DeveloperCommand::Status => {
            let count = load_pair_store(paths, enforce_root)
                .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?
                .hosts
                .len();
            DeveloperOutcome::Device {
                developer_mode,
                paired_hosts: u8::try_from(count).unwrap_or(u8::MAX),
            }
        }
    };
    Ok(DeveloperResponse {
        protocol_version: DEVELOPER_PROTOCOL_VERSION,
        request_id,
        outcome,
    })
}

fn pair(
    paths: &DeveloperPaths,
    owner: &str,
    request_id: u64,
    label: &str,
    ssh_public_key: &str,
    developer_public_key: &str,
    enforce_root: bool,
) -> Result<DeveloperOutcome, (u64, DeveloperErrorCode, String)> {
    if active_pairing_remaining(paths, enforce_root)
        .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?
        .is_none()
    {
        return Err((
            request_id,
            DeveloperErrorCode::PairingClosed,
            "open PAIR NEW COMPUTER on the device before pairing".into(),
        ));
    }
    let normalized_ssh = parse_ssh_ed25519(ssh_public_key)
        .map_err(|message| (request_id, DeveloperErrorCode::InvalidRequest, message))?;
    let ssh_blob = STANDARD
        .decode(normalized_ssh.split_once(' ').unwrap().1)
        .map_err(|_| {
            (
                request_id,
                DeveloperErrorCode::InvalidRequest,
                "SSH public key encoding is invalid".into(),
            )
        })?;
    let host_fingerprint = format!(
        "SHA256:{}",
        STANDARD_NO_PAD.encode(Sha256::digest(&ssh_blob))
    );
    let developer_key = decode_hex_32(developer_public_key).map_err(|_| {
        (
            request_id,
            DeveloperErrorCode::InvalidRequest,
            "developer public key must be 32 raw bytes encoded as lowercase hex".into(),
        )
    })?;
    let developer_key_id = hex(&key_id(&developer_key));
    let mut store = load_pair_store(paths, enforce_root)
        .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?;
    if !store.hosts.contains_key(&host_fingerprint) && store.hosts.len() >= MAX_PAIRED_HOSTS {
        return Err((
            request_id,
            DeveloperErrorCode::Conflict,
            "the device already has eight paired computers".into(),
        ));
    }
    store.hosts.insert(
        host_fingerprint.clone(),
        PairedHost {
            label: label.into(),
            ssh_public_key: normalized_ssh,
            ssh_fingerprint: host_fingerprint.clone(),
            developer_key_id: developer_key_id.clone(),
            paired_at_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        },
    );
    save_pair_state(paths, owner, &store, &developer_key, &developer_key_id)
        .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?;
    Ok(DeveloperOutcome::Paired {
        host_fingerprint,
        developer_key_id,
    })
}

fn open_pairing(
    paths: &DeveloperPaths,
    request_id: u64,
    duration_seconds: u16,
    enforce_root: bool,
) -> Result<DeveloperOutcome, (u64, DeveloperErrorCode, String)> {
    let now = boot_time_milliseconds()
        .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?;
    let duration_milliseconds = u64::from(duration_seconds) * 1000;
    let expires_at_boottime_milliseconds =
        now.checked_add(duration_milliseconds).ok_or_else(|| {
            (
                request_id,
                DeveloperErrorCode::Internal,
                "pairing window expiry overflows the boot clock".into(),
            )
        })?;
    let parent = paths.pairing_window.parent().ok_or_else(|| {
        (
            request_id,
            DeveloperErrorCode::Internal,
            "pairing window path has no parent directory".into(),
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        (
            request_id,
            DeveloperErrorCode::Internal,
            format!("cannot create pairing runtime directory: {error}"),
        )
    })?;
    if enforce_root {
        let metadata = fs::symlink_metadata(parent).map_err(|error| {
            (
                request_id,
                DeveloperErrorCode::Internal,
                format!("cannot inspect pairing runtime directory: {error}"),
            )
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.uid() != 0 {
            return Err((
                request_id,
                DeveloperErrorCode::Internal,
                "pairing runtime directory has unsafe ownership".into(),
            ));
        }
    }
    let window = PairingWindow {
        schema_version: 2,
        expires_at_boottime_milliseconds,
    };
    atomic_write(
        &paths.pairing_window,
        &serde_json::to_vec(&window)
            .map_err(|error| (request_id, DeveloperErrorCode::Internal, error.to_string()))?,
        0o600,
    )
    .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?;
    Ok(DeveloperOutcome::PairingWindow {
        remaining_seconds: duration_seconds,
    })
}

fn list_paired(
    paths: &DeveloperPaths,
    request_id: u64,
    enforce_root: bool,
) -> Result<DeveloperOutcome, (u64, DeveloperErrorCode, String)> {
    let store = load_pair_store(paths, enforce_root)
        .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?;
    let hosts = store
        .hosts
        .into_values()
        .map(|host| PairedHostSummary {
            label: host.label,
            ssh_fingerprint: host.ssh_fingerprint,
            developer_key_id: host.developer_key_id,
            paired_at_unix_seconds: host.paired_at_unix_seconds,
        })
        .collect();
    let pairing_remaining_seconds = active_pairing_remaining(paths, enforce_root)
        .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?;
    Ok(DeveloperOutcome::PairedHosts {
        pairing_remaining_seconds,
        hosts,
    })
}

fn unpair(
    paths: &DeveloperPaths,
    owner: &str,
    request_id: u64,
    fingerprint: Option<&str>,
    enforce_root: bool,
) -> Result<DeveloperOutcome, (u64, DeveloperErrorCode, String)> {
    let mut store = load_pair_store(paths, enforce_root)
        .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?;
    let removed_hosts: Vec<_> = match fingerprint {
        Some(fingerprint) => store.hosts.remove(fingerprint).into_iter().collect(),
        None => std::mem::take(&mut store.hosts).into_values().collect(),
    };
    if removed_hosts.is_empty() && fingerprint.is_some() {
        return Err((
            request_id,
            DeveloperErrorCode::NotFound,
            "paired computer was not found".into(),
        ));
    }
    save_existing_pair_state(paths, owner, &store)
        .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?;
    for developer_key_id in removed_hosts
        .iter()
        .map(|host| &host.developer_key_id)
        .collect::<std::collections::BTreeSet<_>>()
    {
        if !store
            .hosts
            .values()
            .any(|host| &host.developer_key_id == developer_key_id)
        {
            let trust_path = paths
                .developer_trust
                .join(format!("{developer_key_id}.pub"));
            match fs::remove_file(&trust_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err((
                        request_id,
                        DeveloperErrorCode::Internal,
                        format!("cannot remove revoked developer key: {error}"),
                    ));
                }
            }
        }
    }
    Ok(DeveloperOutcome::Unpaired {
        removed: u8::try_from(removed_hosts.len()).unwrap_or(u8::MAX),
        paired_hosts: u8::try_from(store.hosts.len()).unwrap_or(u8::MAX),
    })
}

fn active_pairing_remaining(
    paths: &DeveloperPaths,
    enforce_root: bool,
) -> Result<Option<u16>, String> {
    let encoded = match read_secure_file(&paths.pairing_window, enforce_root) {
        Ok(encoded) => encoded,
        Err(_error) if !paths.pairing_window.exists() => return Ok(None),
        Err(error) => return Err(error),
    };
    let window: PairingWindow = serde_json::from_slice(&encoded)
        .map_err(|error| format!("invalid pairing window: {error}"))?;
    if window.schema_version != 2 {
        return Err("invalid pairing window schema".into());
    }
    pairing_remaining_seconds_at(&window, boot_time_milliseconds()?)
}

fn pairing_remaining_seconds_at(
    window: &PairingWindow,
    now_milliseconds: u64,
) -> Result<Option<u16>, String> {
    let Some(remaining_milliseconds) = window
        .expires_at_boottime_milliseconds
        .checked_sub(now_milliseconds)
    else {
        return Ok(None);
    };
    if remaining_milliseconds == 0 {
        return Ok(None);
    }
    let maximum = u64::from(cp0_devd::MAX_PAIRING_WINDOW_SECONDS) * 1000;
    if remaining_milliseconds > maximum {
        return Err("pairing window exceeds the maximum duration".into());
    }
    let rounded_up = remaining_milliseconds.div_ceil(1000);
    Ok(Some(u16::try_from(rounded_up).map_err(|_| {
        "pairing window remaining duration is invalid".to_string()
    })?))
}

fn boot_time_milliseconds() -> Result<u64, String> {
    #[cfg(target_os = "linux")]
    let clock_id = libc::CLOCK_BOOTTIME;
    #[cfg(not(target_os = "linux"))]
    let clock_id = libc::CLOCK_MONOTONIC;

    let mut value = MaybeUninit::<libc::timespec>::uninit();
    // SAFETY: clock_gettime initializes the supplied timespec on success.
    if unsafe { libc::clock_gettime(clock_id, value.as_mut_ptr()) } != 0 {
        return Err(format!(
            "cannot read the boot clock: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: the successful clock_gettime call initialized value.
    let value = unsafe { value.assume_init() };
    if value.tv_sec < 0 || value.tv_nsec < 0 || value.tv_nsec >= 1_000_000_000 {
        return Err("boot clock returned an invalid duration".into());
    }
    let seconds = u64::try_from(value.tv_sec)
        .map_err(|_| "boot clock seconds cannot be represented".to_string())?;
    let nanoseconds = u64::try_from(value.tv_nsec)
        .map_err(|_| "boot clock nanoseconds cannot be represented".to_string())?;
    seconds
        .checked_mul(1000)
        .and_then(|milliseconds| milliseconds.checked_add(nanoseconds / 1_000_000))
        .ok_or_else(|| "boot clock duration overflows milliseconds".into())
}

fn install(
    reader: &mut impl Read,
    paths: &DeveloperPaths,
    request_id: u64,
    package_bytes: u64,
    expected_sha256: &str,
    enforce_root: bool,
) -> Result<AppdResponse, (u64, DeveloperErrorCode, String)> {
    let mut encoded = vec![0_u8; package_bytes as usize];
    reader.read_exact(&mut encoded).map_err(|_| {
        (
            request_id,
            DeveloperErrorCode::InvalidRequest,
            "developer package body is truncated".into(),
        )
    })?;
    if hex(&Sha256::digest(&encoded)) != expected_sha256 {
        return Err((
            request_id,
            DeveloperErrorCode::InvalidRequest,
            "developer package digest does not match its header".into(),
        ));
    }
    let package = CApp::decode(&encoded).map_err(|error| {
        (
            request_id,
            DeveloperErrorCode::InvalidRequest,
            error.to_string(),
        )
    })?;
    package.verify_developer_signature().map_err(|error| {
        (
            request_id,
            DeveloperErrorCode::InvalidRequest,
            error.to_string(),
        )
    })?;
    let developer_key = package.developer_public_key().ok_or_else(|| {
        (
            request_id,
            DeveloperErrorCode::InvalidRequest,
            "developer signature is missing".into(),
        )
    })?;
    let developer_id = hex(&key_id(&developer_key));
    let paired = load_pair_store(paths, enforce_root)
        .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?
        .hosts
        .values()
        .any(|host| host.developer_key_id == developer_id);
    if !paired {
        return Err((
            request_id,
            DeveloperErrorCode::UnpairedKey,
            "package developer key is not paired with this device".into(),
        ));
    }
    let trusted_path = paths.developer_trust.join(format!("{developer_id}.pub"));
    let trusted = read_secure_file(&trusted_path, enforce_root).map_err(|_| {
        (
            request_id,
            DeveloperErrorCode::UnpairedKey,
            "package developer key is not paired with this device".into(),
        )
    })?;
    if trusted != developer_key {
        return Err((
            request_id,
            DeveloperErrorCode::UnpairedKey,
            "paired developer key does not match the package".into(),
        ));
    }
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = format!("incoming-dev-{}-{sequence}.capp", std::process::id());
    let staged = paths.appd_staging.join(&name);
    write_new(&staged, &encoded, 0o600)
        .map_err(|message| (request_id, DeveloperErrorCode::Internal, message))?;
    let result = send_appd(
        paths,
        request_id,
        AppdCommand::Install { package_name: name },
    );
    let _ = fs::remove_file(&staged);
    result
}

fn send_appd(
    paths: &DeveloperPaths,
    request_id: u64,
    command: AppdCommand,
) -> Result<AppdResponse, (u64, DeveloperErrorCode, String)> {
    let mut stream = UnixStream::connect(&paths.appd_socket).map_err(|error| {
        (
            request_id,
            DeveloperErrorCode::Unavailable,
            format!("cannot connect to appd: {error}"),
        )
    })?;
    stream
        .set_read_timeout(Some(CLIENT_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(CLIENT_TIMEOUT)))
        .map_err(|error| {
            (
                request_id,
                DeveloperErrorCode::Unavailable,
                format!("cannot set appd timeout: {error}"),
            )
        })?;
    write_appd_request(
        &mut stream,
        &AppdRequest {
            protocol_version: APPD_PROTOCOL_VERSION,
            request_id,
            command,
        },
    )
    .map_err(|error| {
        (
            request_id,
            DeveloperErrorCode::Unavailable,
            error.to_string(),
        )
    })?;
    read_appd_response(&mut BufReader::new(stream))
        .map_err(|error| {
            (
                request_id,
                DeveloperErrorCode::Unavailable,
                error.to_string(),
            )
        })?
        .ok_or_else(|| {
            (
                request_id,
                DeveloperErrorCode::Unavailable,
                "appd closed without a response".into(),
            )
        })
}

fn load_owner(paths: &DeveloperPaths, enforce_root: bool) -> Result<String, String> {
    require_secure_file(&paths.provisioning_complete, enforce_root)?;
    let encoded = read_secure_file(&paths.provisioning_state, enforce_root)?;
    let document: ProvisioningDocument = serde_json::from_slice(&encoded)
        .map_err(|error| format!("invalid provisioning state: {error}"))?;
    if document.schema_version != 1 || document.state.phase != ProvisioningPhase::Complete {
        return Err("device provisioning is incomplete".into());
    }
    document
        .state
        .username
        .filter(|username| cp0_provision_protocol::validate_username(username).is_ok())
        .ok_or_else(|| "provisioned owner is unavailable".into())
}

fn load_pair_store(paths: &DeveloperPaths, enforce_root: bool) -> Result<PairStore, String> {
    match read_secure_file(&paths.pair_store, enforce_root) {
        Ok(encoded) => {
            let store: PairStore = serde_json::from_slice(&encoded)
                .map_err(|error| format!("invalid paired computer state: {error}"))?;
            if store.schema_version != 1 || store.hosts.len() > MAX_PAIRED_HOSTS {
                return Err("invalid paired computer state".into());
            }
            for (fingerprint, host) in &store.hosts {
                let normalized = parse_ssh_ed25519(&host.ssh_public_key)?;
                let ssh_blob = STANDARD
                    .decode(normalized.split_once(' ').unwrap().1)
                    .map_err(|_| "paired SSH public key encoding is invalid")?;
                let expected = format!(
                    "SHA256:{}",
                    STANDARD_NO_PAD.encode(Sha256::digest(&ssh_blob))
                );
                if fingerprint != &host.ssh_fingerprint
                    || fingerprint != &expected
                    || host.label.is_empty()
                    || host.label.len() > cp0_devd::MAX_HOST_LABEL_BYTES
                    || !host.label.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
                    })
                    || decode_hex_32(&host.developer_key_id).is_err()
                {
                    return Err("invalid paired computer state".into());
                }
            }
            Ok(store)
        }
        Err(_error) if !paths.pair_store.exists() => Ok(PairStore::default()),
        Err(error) => Err(error),
    }
}

fn save_pair_state(
    paths: &DeveloperPaths,
    owner: &str,
    store: &PairStore,
    developer_key: &[u8; 32],
    developer_key_id: &str,
) -> Result<(), String> {
    fs::create_dir_all(&paths.developer_trust)
        .map_err(|error| format!("cannot create developer trust directory: {error}"))?;
    fs::set_permissions(&paths.developer_trust, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot secure developer trust directory: {error}"))?;
    let trust_path = paths
        .developer_trust
        .join(format!("{developer_key_id}.pub"));
    if trust_path.exists() {
        if fs::read(&trust_path).map_err(|error| error.to_string())? != developer_key {
            return Err("developer trust key ID already has different content".into());
        }
    } else {
        write_new(&trust_path, developer_key, 0o644)?;
    }
    save_pair_store(paths, store)?;
    write_authorized_keys(paths, owner, store)
}

fn save_pair_store(paths: &DeveloperPaths, store: &PairStore) -> Result<(), String> {
    let pair_parent = paths
        .pair_store
        .parent()
        .ok_or("paired computer state has no parent directory")?;
    fs::create_dir_all(pair_parent)
        .map_err(|error| format!("cannot create paired computer directory: {error}"))?;
    fs::set_permissions(pair_parent, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("cannot secure paired computer directory: {error}"))?;
    atomic_write(
        &paths.pair_store,
        &serde_json::to_vec_pretty(store).map_err(|error| error.to_string())?,
        0o600,
    )
}

fn write_authorized_keys(
    paths: &DeveloperPaths,
    owner: &str,
    store: &PairStore,
) -> Result<(), String> {
    fs::create_dir_all(&paths.authorized_keys)
        .map_err(|error| format!("cannot create authorized key directory: {error}"))?;
    fs::set_permissions(&paths.authorized_keys, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot secure authorized key directory: {error}"))?;
    let mut authorized = String::new();
    for host in store.hosts.values() {
        authorized.push_str("restrict,command=\"/usr/bin/cp0ctl dev-session\" ");
        authorized.push_str(&host.ssh_public_key);
        authorized.push_str(" cp0-");
        authorized.push_str(&host.label);
        authorized.push('\n');
    }
    atomic_write(
        &paths.authorized_keys.join(owner),
        authorized.as_bytes(),
        0o644,
    )
}

fn save_existing_pair_state(
    paths: &DeveloperPaths,
    owner: &str,
    store: &PairStore,
) -> Result<(), String> {
    // Revoke SSH authorization before publishing the smaller trust set.
    write_authorized_keys(paths, owner, store)?;
    save_pair_store(paths, store)
}

fn reconcile_pair_state(
    paths: &DeveloperPaths,
    owner: &str,
    enforce_root: bool,
) -> Result<(), String> {
    let store = load_pair_store(paths, enforce_root)?;
    write_authorized_keys(paths, owner, &store)
}

fn parse_ssh_ed25519(value: &str) -> Result<String, String> {
    let mut fields = value.split_ascii_whitespace();
    let key_type = fields.next().ok_or("SSH public key type is missing")?;
    let encoded = fields.next().ok_or("SSH public key body is missing")?;
    if key_type != "ssh-ed25519" || encoded.len() > 256 {
        return Err("only bounded Ed25519 SSH public keys are accepted".into());
    }
    let blob = STANDARD
        .decode(encoded)
        .map_err(|_| "SSH public key body is invalid")?;
    let mut offset = 0;
    let algorithm = take_ssh_field(&blob, &mut offset)?;
    let key = take_ssh_field(&blob, &mut offset)?;
    if algorithm != b"ssh-ed25519" || key.len() != 32 || offset != blob.len() {
        return Err("SSH public key is not a canonical Ed25519 key".into());
    }
    Ok(format!("ssh-ed25519 {encoded}"))
}

fn take_ssh_field<'a>(encoded: &'a [u8], offset: &mut usize) -> Result<&'a [u8], String> {
    let length_bytes: [u8; 4] = encoded
        .get(*offset..*offset + 4)
        .ok_or("SSH public key is truncated")?
        .try_into()
        .unwrap();
    *offset += 4;
    let length = u32::from_be_bytes(length_bytes) as usize;
    let field = encoded
        .get(*offset..*offset + length)
        .ok_or("SSH public key field is truncated")?;
    *offset += length;
    Ok(field)
}

fn read_secure_file(path: &Path, enforce_root: bool) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.mode() & 0o022 != 0
        || enforce_root && metadata.uid() != 0
    {
        return Err(format!("{} has unsafe ownership or mode", path.display()));
    }
    fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn require_secure_file(path: &Path, enforce_root: bool) -> Result<(), String> {
    read_secure_file(path, enforce_root).map(|_| ())
}

fn write_new(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot finish {}: {error}", path.display()))
}

fn atomic_write(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
    let parent = path.parent().ok_or("atomic path has no parent directory")?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("atomic path has no UTF-8 file name")?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.tmp-{}-{sequence}", std::process::id()));
    let result = (|| {
        write_new(&temporary, contents, mode)?;
        fs::rename(&temporary, path)
            .map_err(|error| format!("cannot publish {}: {error}", path.display()))?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("cannot sync {}: {error}", parent.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn respond_error(
    stream: &mut UnixStream,
    request_id: u64,
    code: DeveloperErrorCode,
    message: impl Into<String>,
) -> Result<(), String> {
    write_response(stream, &DeveloperResponse::error(request_id, code, message))
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths(name: &str) -> (PathBuf, DeveloperPaths) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("devd-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let paths = DeveloperPaths {
            provisioning_state: root.join("provisioning/state.json"),
            provisioning_complete: root.join("provisioning/complete"),
            device_policy: root.join("device-policy.json"),
            developer_mode: root.join("developer-mode"),
            developer_trust: root.join("trust"),
            authorized_keys: root.join("authorized"),
            pair_store: root.join("state/paired.json"),
            pairing_window: root.join("run/pairing-window.json"),
            appd_staging: root.join("run"),
            appd_socket: root.join("appd.sock"),
        };
        (root, paths)
    }

    fn test_ssh_key(fill: u8) -> (String, String) {
        let mut blob = Vec::new();
        blob.extend_from_slice(&(11_u32.to_be_bytes()));
        blob.extend_from_slice(b"ssh-ed25519");
        blob.extend_from_slice(&(32_u32.to_be_bytes()));
        blob.extend_from_slice(&[fill; 32]);
        let fingerprint = format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(&blob)));
        (
            format!("ssh-ed25519 {}", STANDARD.encode(blob)),
            fingerprint,
        )
    }

    #[test]
    fn validates_and_normalizes_ed25519_key() {
        let mut blob = Vec::new();
        blob.extend_from_slice(&(11_u32.to_be_bytes()));
        blob.extend_from_slice(b"ssh-ed25519");
        blob.extend_from_slice(&(32_u32.to_be_bytes()));
        blob.extend_from_slice(&[7_u8; 32]);
        let encoded = STANDARD.encode(blob);
        assert_eq!(
            parse_ssh_ed25519(&format!("ssh-ed25519 {encoded} host comment")).unwrap(),
            format!("ssh-ed25519 {encoded}")
        );
        assert!(parse_ssh_ed25519("ssh-rsa bad").is_err());
    }

    #[test]
    fn forced_authorized_keys_never_grant_a_shell() {
        let (root, paths) = test_paths("forced-key");
        fs::create_dir_all(&paths.developer_trust).unwrap();
        let (key, fingerprint) = test_ssh_key(9);
        let mut store = PairStore::default();
        store.hosts.insert(
            fingerprint.clone(),
            PairedHost {
                label: "workstation".into(),
                ssh_public_key: key,
                ssh_fingerprint: fingerprint,
                developer_key_id: "00".repeat(32),
                paired_at_unix_seconds: 1,
            },
        );
        save_pair_state(&paths, "owner", &store, &[0_u8; 32], &"00".repeat(32)).unwrap();
        let authorized = fs::read_to_string(paths.authorized_keys.join("owner")).unwrap();
        assert!(
            authorized.starts_with("restrict,command=\"/usr/bin/cp0ctl dev-session\" ssh-ed25519 ")
        );
        assert!(!authorized.contains("/bin/sh"));
        assert_eq!(
            fs::metadata(paths.authorized_keys.join("owner"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn management_commands_are_reserved_for_the_system_shell() {
        assert!(management_command(&DeveloperCommand::OpenPairing {
            duration_seconds: 600
        }));
        assert!(management_command(&DeveloperCommand::ListPaired));
        assert!(management_command(&DeveloperCommand::UnpairAll));
        assert!(!management_command(&DeveloperCommand::Pair {
            host_label: "workstation".into(),
            ssh_public_key: "ssh-ed25519 placeholder".into(),
            developer_public_key: "00".repeat(32),
        }));
        assert!(!management_command(&DeveloperCommand::Install {
            package_bytes: 1,
            package_sha256: "00".repeat(32),
        }));
        let pair = DeveloperCommand::Pair {
            host_label: "workstation".into(),
            ssh_public_key: "ssh-ed25519 placeholder".into(),
            developer_public_key: "00".repeat(32),
        };
        assert!(authorized_peer(OWNER_UID, 900, &pair));
        assert!(!authorized_peer(
            OWNER_UID,
            900,
            &DeveloperCommand::UnpairAll
        ));
        assert!(authorized_peer(900, 900, &DeveloperCommand::UnpairAll));
        assert!(!authorized_peer(900, 900, &pair));
        assert!(!authorized_peer(1234, 900, &pair));
        assert!(!authorized_peer(1234, 900, &DeveloperCommand::UnpairAll));
        assert!(allowed_when_developer_mode_off(&DeveloperCommand::Status));
        assert!(allowed_when_developer_mode_off(
            &DeveloperCommand::UnpairAll
        ));
        assert!(!allowed_when_developer_mode_off(&pair));
        assert!(!allowed_when_developer_mode_off(
            &DeveloperCommand::OpenPairing {
                duration_seconds: 600
            }
        ));
    }

    #[test]
    fn pairing_window_is_explicit_and_revocation_removes_trust() {
        let (root, paths) = test_paths("pairing-window");
        fs::create_dir_all(&paths.developer_trust).unwrap();
        assert_eq!(active_pairing_remaining(&paths, false).unwrap(), None);
        let outcome = open_pairing(&paths, 1, 600, false).unwrap();
        assert!(matches!(outcome, DeveloperOutcome::PairingWindow { .. }));
        assert!(active_pairing_remaining(&paths, false).unwrap().is_some());

        let (key, fingerprint) = test_ssh_key(11);
        let developer_key_id = "11".repeat(32);
        let mut store = PairStore::default();
        store.hosts.insert(
            fingerprint.clone(),
            PairedHost {
                label: "laptop".into(),
                ssh_public_key: key,
                ssh_fingerprint: fingerprint.clone(),
                developer_key_id: developer_key_id.clone(),
                paired_at_unix_seconds: 1,
            },
        );
        save_pair_state(&paths, "owner", &store, &[0x11_u8; 32], &developer_key_id).unwrap();
        assert!(
            paths
                .developer_trust
                .join(format!("{developer_key_id}.pub"))
                .exists()
        );
        let outcome = unpair(&paths, "owner", 2, Some(&fingerprint), false).unwrap();
        assert_eq!(
            outcome,
            DeveloperOutcome::Unpaired {
                removed: 1,
                paired_hosts: 0,
            }
        );
        assert_eq!(
            fs::read_to_string(paths.authorized_keys.join("owner")).unwrap(),
            ""
        );
        assert!(
            !paths
                .developer_trust
                .join(format!("{developer_key_id}.pub"))
                .exists()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pairing_window_uses_a_bounded_monotonic_duration() {
        let window = PairingWindow {
            schema_version: 2,
            expires_at_boottime_milliseconds: 610_000,
        };
        assert_eq!(
            pairing_remaining_seconds_at(&window, 10_000).unwrap(),
            Some(600)
        );
        assert_eq!(
            pairing_remaining_seconds_at(&window, 609_001).unwrap(),
            Some(1)
        );
        assert_eq!(
            pairing_remaining_seconds_at(&window, 610_000).unwrap(),
            None
        );
        assert_eq!(
            pairing_remaining_seconds_at(&window, 610_001).unwrap(),
            None
        );
        assert!(pairing_remaining_seconds_at(&window, 9_999).is_err());
    }

    #[test]
    fn pairing_rejects_a_ninth_computer() {
        let (root, paths) = test_paths("pairing-cap");
        fs::create_dir_all(paths.pair_store.parent().unwrap()).unwrap();
        let mut store = PairStore::default();
        for index in 0..MAX_PAIRED_HOSTS {
            let (key, fingerprint) = test_ssh_key(index as u8);
            store.hosts.insert(
                fingerprint.clone(),
                PairedHost {
                    label: format!("host-{index}"),
                    ssh_public_key: key,
                    ssh_fingerprint: fingerprint,
                    developer_key_id: "00".repeat(32),
                    paired_at_unix_seconds: index as u64,
                },
            );
        }
        save_pair_store(&paths, &store).unwrap();
        open_pairing(&paths, 1, 600, false).unwrap();
        let (ssh_public_key, _) = test_ssh_key(100);
        let error = pair(
            &paths,
            "owner",
            2,
            "ninth",
            &ssh_public_key,
            &"11".repeat(32),
            false,
        )
        .unwrap_err();
        assert_eq!(error.1, DeveloperErrorCode::Conflict);
        fs::remove_dir_all(root).unwrap();
    }
}
