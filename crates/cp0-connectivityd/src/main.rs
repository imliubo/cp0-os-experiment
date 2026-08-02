use std::env;
use std::ffi::CStr;
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixListener;
use std::process::ExitCode;
use std::ptr;

use cp0_connectivityd::{ConnectivityServer, NmcliConnectivityBackend};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cp0-connectivityd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    match arguments.as_slice() {
        [command] if command == "serve" => {
            let listener = systemd_listener()?;
            let shell_uid = user_uid(c"cp0-shell").map_err(|error| error.to_string())?;
            ConnectivityServer::new(NmcliConnectivityBackend::default(), [shell_uid])
                .serve(listener)
                .map_err(|error| error.to_string())
        }
        _ => Err("usage: cp0-connectivityd serve".into()),
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
    if listen_pid != std::process::id() || listen_fds != 1 || names != "connectivity" {
        return Err("one systemd connectivity Unix listener is required".into());
    }
    let listener = unsafe { UnixListener::from_raw_fd(3) };
    listener
        .local_addr()
        .map_err(|error| format!("inherited descriptor is not a Unix listener: {error}"))?;
    Ok(listener)
}

fn user_uid(name: &CStr) -> io::Result<u32> {
    let mut record = MaybeUninit::<libc::passwd>::uninit();
    let mut result = ptr::null_mut();
    let mut buffer = [0_u8; 16 * 1024];
    let status = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &raw mut result,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    if result.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "trusted Shell account is unavailable",
        ));
    }
    let record = unsafe { record.assume_init() };
    Ok(record.pw_uid)
}
