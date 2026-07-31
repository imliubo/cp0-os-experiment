use std::env;
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixListener;
use std::process::ExitCode;
use std::sync::Arc;

use cp0_appd::lookup_unix_account;
use cp0_stored::{
    AppdInstaller, DEFAULT_CONFIG_PATH, StoreConfig, StorePaths, StoreService, UreqStoreNetwork,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cp0-stored: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let config = StoreConfig::load(
        env::var_os("CP0_STORE_CONFIG").unwrap_or_else(|| DEFAULT_CONFIG_PATH.into()),
    )
    .map_err(|error| error.to_string())?;
    let paths = StorePaths::default();
    let (shell_uid, _) = lookup_unix_account("cp0-shell").map_err(|error| error.to_string())?;
    let service = StoreService::new(
        paths.clone(),
        config,
        Arc::new(UreqStoreNetwork::default()),
        Arc::new(AppdInstaller::new(paths.appd_socket)),
        [0, shell_uid],
    )
    .map_err(|error| error.to_string())?;
    service
        .serve(systemd_listener()?)
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
    let names = env::var("LISTEN_FDNAMES").map_err(|_| "LISTEN_FDNAMES is not set")?;
    if listen_pid != std::process::id() || listen_fds != 1 || names != "control" {
        return Err("exactly one named control socket is required".into());
    }
    // SAFETY: systemd supplies the one declared listener as descriptor 3,
    // and ownership is transferred exactly once here.
    let listener = unsafe { UnixListener::from_raw_fd(3) };
    listener
        .local_addr()
        .map_err(|error| format!("descriptor 3 is not a Unix listener: {error}"))?;
    Ok(listener)
}
