use std::env;
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixListener;
use std::process::ExitCode;

use cp0_networkd::{NetworkServer, UreqFetcher};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cp0-networkd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    match arguments.as_slice() {
        [command] if command == "serve" => {
            let listener = systemd_listener()?;
            NetworkServer::new(UreqFetcher::default(), [0])
                .serve(listener)
                .map_err(|error| error.to_string())
        }
        _ => Err("usage: cp0-networkd serve".into()),
    }
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
    if listen_pid != std::process::id() || listen_fds != 1 || names != "network" {
        return Err("one systemd network Unix listener is required".into());
    }
    // SAFETY: systemd assigns the single activated listener to descriptor 3,
    // which is consumed exactly once here.
    let listener = unsafe { UnixListener::from_raw_fd(3) };
    listener
        .local_addr()
        .map_err(|error| format!("inherited descriptor is not a Unix listener: {error}"))?;
    Ok(listener)
}
