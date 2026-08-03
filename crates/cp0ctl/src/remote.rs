use std::fs;
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use cp0_appd::ResponseOutcome;
use cp0_devd::{
    DEFAULT_DEVELOPER_SOCKET, DEVELOPER_PROTOCOL_VERSION, DeveloperCommand, DeveloperOutcome,
    DeveloperRequest, DeveloperResponse, hex, read_response, write_request,
};
use sha2::{Digest, Sha256};

static REMOTE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub fn pair(
    developer_public_path: &str,
    ssh_public_path: &str,
    host_label: &str,
    device: &str,
) -> Result<(), String> {
    let developer_public = fs::read(developer_public_path).map_err(|error| {
        format!("cannot read developer public key {developer_public_path}: {error}")
    })?;
    if developer_public.len() != 32 {
        return Err("developer public key must contain exactly 32 raw bytes".into());
    }
    let ssh_public = fs::read_to_string(ssh_public_path)
        .map_err(|error| format!("cannot read SSH public key {ssh_public_path}: {error}"))?;
    let response = transact(
        device,
        DeveloperCommand::Pair {
            host_label: host_label.into(),
            ssh_public_key: ssh_public.trim().into(),
            developer_public_key: hex(&developer_public),
        },
        &[],
    )?;
    print_response(response)
}

pub fn install(package_path: &str, device: &str) -> Result<(), String> {
    validate_device(device)?;
    let encoded = fs::read(package_path)
        .map_err(|error| format!("cannot read package {package_path}: {error}"))?;
    let package = cp0_package::CApp::decode(&encoded).map_err(|error| error.to_string())?;
    crate::package::manifest_from_package(&package)?;
    package
        .verify_developer_signature()
        .map_err(|error| error.to_string())?;
    let response = transact(
        device,
        DeveloperCommand::Install {
            package_bytes: encoded.len() as u64,
            package_sha256: hex(&Sha256::digest(&encoded)),
        },
        &encoded,
    )?;
    print_response(response)
}

pub fn logs(device: &str, app_id: &str, limit: u16) -> Result<(), String> {
    app_command(
        device,
        DeveloperCommand::Logs {
            app_id: app_id.into(),
            limit,
        },
    )
}

pub fn start(device: &str, app_id: &str) -> Result<(), String> {
    app_command(
        device,
        DeveloperCommand::Start {
            app_id: app_id.into(),
        },
    )
}

pub fn stop(device: &str, app_id: &str) -> Result<(), String> {
    app_command(
        device,
        DeveloperCommand::Stop {
            app_id: app_id.into(),
        },
    )
}

pub fn uninstall(device: &str, app_id: &str) -> Result<(), String> {
    app_command(
        device,
        DeveloperCommand::Uninstall {
            app_id: app_id.into(),
        },
    )
}

pub fn status(device: &str) -> Result<(), String> {
    app_command(device, DeveloperCommand::Status)
}

fn app_command(device: &str, command: DeveloperCommand) -> Result<(), String> {
    match &command {
        DeveloperCommand::Logs { app_id, .. }
        | DeveloperCommand::Start { app_id }
        | DeveloperCommand::Stop { app_id }
        | DeveloperCommand::Uninstall { app_id }
            if !cp0_manifest::is_valid_app_id(app_id) =>
        {
            return Err("invalid application ID".into());
        }
        _ => {}
    }
    let response = transact(device, command, &[])?;
    print_response(response)
}

fn transact(
    device: &str,
    command: DeveloperCommand,
    body: &[u8],
) -> Result<DeveloperResponse, String> {
    validate_device(device)?;
    let request_id = REMOTE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let ssh = std::env::var_os("CP0_SSH").unwrap_or_else(|| "ssh".into());
    let mut child = Command::new(ssh)
        .arg("-T")
        .arg("--")
        .arg(device)
        .arg("cp0-dev")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("cannot execute ssh: {error}"))?;
    let mut input = child
        .stdin
        .take()
        .ok_or("cannot open SSH developer input")?;
    write_request(
        &mut input,
        &DeveloperRequest {
            protocol_version: DEVELOPER_PROTOCOL_VERSION,
            request_id,
            command,
        },
    )
    .map_err(|error| error.to_string())?;
    input
        .write_all(body)
        .map_err(|error| format!("cannot send developer request body: {error}"))?;
    drop(input);

    let output = child
        .stdout
        .take()
        .ok_or("cannot open SSH developer output")?;
    let response = read_response(&mut BufReader::new(output))
        .map_err(|error| format!("cannot read developer response: {error}"))?
        .ok_or("device closed the developer session without a response")?;
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for SSH developer session: {error}"))?;
    if !status.success() {
        return Err("remote CardputerZero developer session failed".into());
    }
    if response.request_id != request_id {
        return Err("developer response request ID does not match".into());
    }
    Ok(response)
}

fn print_response(response: DeveloperResponse) -> Result<(), String> {
    match &response.outcome {
        DeveloperOutcome::Error { code, message } => {
            return Err(format!("developer service returned {code:?}: {message}"));
        }
        DeveloperOutcome::Appd { response: appd } => {
            if let ResponseOutcome::Error { code, message } = &appd.outcome {
                return Err(format!("appd returned {code:?}: {message}"));
            }
        }
        _ => {}
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&response)
            .map_err(|error| format!("cannot encode developer response: {error}"))?
    );
    Ok(())
}

pub fn session() -> Result<(), String> {
    let socket =
        std::env::var("CP0_DEVD_SOCKET").unwrap_or_else(|_| DEFAULT_DEVELOPER_SOCKET.into());
    let mut stream = UnixStream::connect(&socket)
        .map_err(|error| format!("cannot connect to developer service at {socket}: {error}"))?;
    let mut input = std::io::stdin().lock();
    std::io::copy(&mut input, &mut stream)
        .map_err(|error| format!("cannot forward developer request: {error}"))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|error| format!("cannot finish developer request: {error}"))?;
    let mut output = std::io::stdout().lock();
    std::io::copy(&mut stream, &mut output)
        .map_err(|error| format!("cannot forward developer response: {error}"))?;
    output
        .flush()
        .map_err(|error| format!("cannot finish developer response: {error}"))
}

fn validate_device(device: &str) -> Result<(), String> {
    if device.is_empty()
        || device.len() > 255
        || device.starts_with('-')
        || device.contains(char::is_whitespace)
        || device.chars().any(char::is_control)
        || !device.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '@' | '.' | ':' | '-' | '_' | '[' | ']')
        })
    {
        return Err("invalid SSH device target".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_hosts_but_rejects_options_and_shell_text() {
        for device in [
            "owner@192.168.20.146",
            "cardputerzero.local",
            "owner@[fe80::1]",
        ] {
            validate_device(device).unwrap();
        }
        for device in [
            "-oProxyCommand=bad",
            "owner@host;bad",
            "owner@host name",
            "",
        ] {
            assert!(validate_device(device).is_err());
        }
    }

    #[test]
    fn remote_transport_has_no_sudo_or_scp_dependency() {
        let source = include_str!("remote.rs");
        assert!(!source.contains(concat!("Command::new(", "scp)")));
        let forbidden = String::from_utf8(vec![b'\"', b's', b'u', b'd', b'o', b'\"']).unwrap();
        assert!(!source.contains(&forbidden));
        assert!(source.contains(".arg(\"cp0-dev\")"));
    }
}
