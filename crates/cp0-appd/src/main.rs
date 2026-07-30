use std::env;
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixListener;
use std::process::ExitCode;

use cp0_appd::{
    AppLayout, AppManager, AppRegistry, AppdServer, ManagerPaths, RegistryError,
    build_sandbox_plan, lookup_unix_account,
};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cp0-appd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    match arguments.as_slice() {
        [command, manifest_path, app_user] if command == "plan" => {
            print_plan(manifest_path, app_user)
        }
        [command, app_id, version] if command == "register-installed" => {
            register_installed(app_id, version)
        }
        [command] if command == "serve" => serve(),
        _ => Err(
            "usage: cp0-appd plan <app.json> <cp0-app-N> | register-installed <app-id> <version> | serve"
                .into(),
        ),
    }
}

fn print_plan(manifest_path: &str, app_user: &str) -> Result<(), String> {
    let manifest = cp0_manifest::load_and_validate(manifest_path)
        .map_err(|error| format!("cannot plan application sandbox: {error}"))?;
    let plan = build_sandbox_plan(&manifest, app_user, &AppLayout::default())
        .map_err(|error| format!("cannot plan application sandbox: {error}"))?;
    serde_json::to_writer_pretty(std::io::stdout().lock(), &plan)
        .map_err(|error| format!("cannot write application sandbox plan: {error}"))?;
    println!();
    Ok(())
}

fn register_installed(app_id: &str, version: &str) -> Result<(), String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("register-installed requires root".into());
    }
    if !cp0_manifest::is_valid_app_id(app_id) || !cp0_manifest::is_valid_app_version(version) {
        return Err("invalid application ID or version".into());
    }

    let paths = ManagerPaths::default();
    let manifest_path = paths
        .layout
        .apps_root
        .join(app_id)
        .join(version)
        .join("app.json");
    let manifest = cp0_manifest::load_and_validate(&manifest_path)
        .map_err(|error| format!("cannot load installed manifest: {error}"))?;
    if manifest.id != app_id || manifest.version != version {
        return Err("installed manifest identity does not match command arguments".into());
    }

    let registry = match AppRegistry::load(&paths.registry_path) {
        Ok(registry) => registry,
        Err(RegistryError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = paths
                .registry_path
                .parent()
                .ok_or("registry path has no parent")?;
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create registry directory: {error}"))?;
            AppRegistry::default()
        }
        Err(error) => return Err(error.to_string()),
    };
    let mut registry_with_identity = registry;
    let account = registry_with_identity
        .assign(app_id)
        .map_err(|error| error.to_string())?;
    let (uid, gid) = lookup_unix_account(&account.unix_user).map_err(|error| error.to_string())?;
    if uid != account.unix_uid || gid != account.unix_uid {
        return Err(format!(
            "{} must resolve to UID/GID {}/{}",
            account.unix_user, account.unix_uid, account.unix_uid
        ));
    }
    let mut manager = AppManager::from_registry(paths, registry_with_identity)
        .map_err(|error| error.to_string())?;
    let installed = manager
        .mark_installed(&manifest)
        .map_err(|error| error.to_string())?;
    println!(
        "registered {} {} as {} (UID {})",
        installed.app_id, installed.version, installed.account_user, installed.account_uid
    );
    Ok(())
}

fn serve() -> Result<(), String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("serve requires root".into());
    }
    let manager = AppManager::load(ManagerPaths::default()).map_err(|error| error.to_string())?;
    let (shell_uid, _) = lookup_unix_account("cp0-shell").map_err(|error| error.to_string())?;
    let listener = systemd_listener()?;
    AppdServer::new(manager, [0, shell_uid])
        .serve(listener)
        .map_err(|error| error.to_string())
}

fn systemd_listener() -> Result<UnixListener, String> {
    let listen_pid = env::var("LISTEN_PID")
        .map_err(|_| "LISTEN_PID is not set")?
        .parse::<u32>()
        .map_err(|_| "LISTEN_PID is invalid")?;
    let listen_fds = env::var("LISTEN_FDS")
        .map_err(|_| "LISTEN_FDS is not set")?
        .parse::<u32>()
        .map_err(|_| "LISTEN_FDS is invalid")?;
    if listen_pid != std::process::id() || listen_fds != 1 {
        return Err("exactly one systemd-owned listening socket is required".into());
    }
    if env::var("LISTEN_FDNAMES").is_ok_and(|names| names != "control") {
        return Err("systemd listening socket must be named control".into());
    }

    // SAFETY: systemd's socket activation contract assigns the first inherited
    // descriptor to FD 3 when LISTEN_PID matches this process and LISTEN_FDS=1.
    let listener = unsafe { UnixListener::from_raw_fd(3) };
    listener
        .local_addr()
        .map_err(|error| format!("inherited descriptor is not a Unix listener: {error}"))?;
    Ok(listener)
}
