use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static REMOTE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn install(package_path: &str, device: &str) -> Result<(), String> {
    validate_device(device)?;
    let package = crate::package::read_package(package_path)?;
    crate::package::manifest_from_package(&package)?;
    package
        .verify_developer_signature()
        .map_err(|error| error.to_string())?;
    let sequence = REMOTE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let remote_path = format!("/tmp/cp0-upload-{}-{sequence}.capp", std::process::id());
    let destination = format!("{device}:{remote_path}");
    let scp = std::env::var_os("CP0_SCP").unwrap_or_else(|| "scp".into());
    let status = Command::new(scp)
        .arg("--")
        .arg(package_path)
        .arg(&destination)
        .status()
        .map_err(|error| format!("cannot execute scp: {error}"))?;
    if !status.success() {
        return Err("package upload failed".into());
    }

    let result = remote_command(device, &["sudo", "cp0ctl", "install", &remote_path]);
    if let Err(error) = remote_command(device, &["rm", "-f", "--", &remote_path]) {
        eprintln!("cp0ctl: warning: cannot remove remote upload: {error}");
    }
    result
}

pub fn logs(device: &str, app_id: &str, limit: u16) -> Result<(), String> {
    validate_device(device)?;
    if !cp0_manifest::is_valid_app_id(app_id) {
        return Err("invalid application ID".into());
    }
    remote_command(
        device,
        &["sudo", "cp0ctl", "logs", app_id, &limit.to_string()],
    )
}

fn remote_command(device: &str, arguments: &[&str]) -> Result<(), String> {
    let ssh = std::env::var_os("CP0_SSH").unwrap_or_else(|| "ssh".into());
    let status = Command::new(ssh)
        .arg("--")
        .arg(device)
        .args(arguments)
        .status()
        .map_err(|error| format!("cannot execute ssh: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("remote CardputerZero command failed".into())
    }
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
    use std::path::Path;

    #[test]
    fn accepts_hosts_but_rejects_options_and_shell_text() {
        for device in ["pi@192.168.20.146", "cardputerzero.local", "pi@[fe80::1]"] {
            validate_device(device).unwrap();
        }
        for device in ["-oProxyCommand=bad", "pi@host;bad", "pi@host name", ""] {
            assert!(validate_device(device).is_err());
        }
    }

    #[test]
    fn generated_upload_path_has_no_user_controlled_component() {
        let path = Path::new("/tmp").join(format!(
            "cp0-upload-{}-{}.capp",
            std::process::id(),
            REMOTE_SEQUENCE.load(Ordering::Relaxed)
        ));
        assert_eq!(path.parent(), Some(Path::new("/tmp")));
    }
}
