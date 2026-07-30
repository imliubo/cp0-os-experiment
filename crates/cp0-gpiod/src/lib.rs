use std::collections::BTreeSet;
use std::fmt;
use std::fs::OpenOptions;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

use cp0_gpio_protocol::{
    GpioCommand, GpioErrorCode, GpioLine, GpioProtocolError, GpioRequest, GpioResponse,
    read_request, write_response,
};

const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioDeviceError {
    Unavailable,
    Device,
}

impl fmt::Display for GpioDeviceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("GPIO line is unavailable"),
            Self::Device => formatter.write_str("GPIO operation failed"),
        }
    }
}

impl std::error::Error for GpioDeviceError {}

pub trait GpioBackend {
    fn read(&self, line: GpioLine) -> Result<bool, GpioDeviceError>;
    fn write(&self, line: GpioLine, value: bool) -> Result<(), GpioDeviceError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SysfsGpioBackend;

impl GpioBackend for SysfsGpioBackend {
    fn read(&self, line: GpioLine) -> Result<bool, GpioDeviceError> {
        let mut file = open_line(line, false)?;
        let mut value = [0_u8; 3];
        let count = file.read(&mut value).map_err(map_io_error)?;
        match &value[..count] {
            b"0\n" | b"0" => Ok(false),
            b"1\n" | b"1" => Ok(true),
            _ => Err(GpioDeviceError::Device),
        }
    }

    fn write(&self, line: GpioLine, value: bool) -> Result<(), GpioDeviceError> {
        let mut file = open_line(line, true)?;
        file.seek(SeekFrom::Start(0)).map_err(map_io_error)?;
        file.write_all(if value { b"1\n" } else { b"0\n" })
            .and_then(|()| file.flush())
            .map_err(map_io_error)
    }
}

fn line_path(line: GpioLine) -> &'static Path {
    Path::new(match line {
        GpioLine::GroveFunction => "/sys/class/leds/grove_fun/brightness",
        GpioLine::ExternalUsbFunction => "/sys/class/leds/ext_usb_gpio_fun/brightness",
        GpioLine::Grove5vPower => "/sys/class/leds/grove_5v_out/brightness",
        GpioLine::External5vPower => "/sys/class/leds/ext_5v_out/brightness",
    })
}

fn open_line(line: GpioLine, writable: bool) -> Result<std::fs::File, GpioDeviceError> {
    OpenOptions::new()
        .read(!writable)
        .write(writable)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(line_path(line))
        .map_err(map_io_error)
}

fn map_io_error(error: io::Error) -> GpioDeviceError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => GpioDeviceError::Unavailable,
        _ => GpioDeviceError::Device,
    }
}

#[derive(Debug)]
pub struct GpioServer<B> {
    backend: B,
    trusted_uids: BTreeSet<u32>,
}

impl<B: GpioBackend> GpioServer<B> {
    pub fn new(backend: B, trusted_uids: impl IntoIterator<Item = u32>) -> Self {
        Self {
            backend,
            trusted_uids: trusted_uids.into_iter().collect(),
        }
    }

    pub fn serve(&self, listener: UnixListener) -> io::Result<()> {
        loop {
            let (stream, _) = listener.accept()?;
            if let Err(error) = self.handle_connection(stream) {
                eprintln!("cp0-gpiod: rejected connection: {error}");
            }
        }
    }

    fn handle_connection(&self, mut stream: UnixStream) -> io::Result<()> {
        stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        stream.set_write_timeout(Some(CLIENT_TIMEOUT))?;
        let uid = peer_uid(&stream)?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let request = match read_request(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(error) => {
                write_response(
                    &mut stream,
                    &GpioResponse::error(
                        0,
                        GpioErrorCode::InvalidRequest,
                        "invalid GPIO service request",
                    ),
                )
                .map_err(protocol_io)?;
                eprintln!("cp0-gpiod: invalid request: {error}");
                return Ok(());
            }
        };
        if !self.trusted_uids.contains(&uid) {
            write_response(
                &mut stream,
                &GpioResponse::error(
                    request.request_id,
                    GpioErrorCode::Unauthorized,
                    "peer UID is not authorized for GPIO access",
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }
        write_response(&mut stream, &self.dispatch(request)).map_err(protocol_io)
    }

    pub fn dispatch(&self, request: GpioRequest) -> GpioResponse {
        let request_id = request.request_id;
        match request.command {
            GpioCommand::Read { line } => match self.backend.read(line) {
                Ok(value) => GpioResponse::value(request_id, line, value),
                Err(error) => device_error_response(request_id, error),
            },
            GpioCommand::Write { line, value } => match self.backend.write(line, value) {
                Ok(()) => GpioResponse::written(request_id, line, value),
                Err(error) => device_error_response(request_id, error),
            },
        }
    }
}

fn device_error_response(request_id: u64, error: GpioDeviceError) -> GpioResponse {
    let (code, message) = match error {
        GpioDeviceError::Unavailable => (GpioErrorCode::Unavailable, "GPIO line is unavailable"),
        GpioDeviceError::Device => (GpioErrorCode::Device, "GPIO operation failed"),
    };
    GpioResponse::error(request_id, code, message)
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
        "peer credentials are only implemented for the Linux target",
    ))
}

fn protocol_io(error: GpioProtocolError) -> io::Error {
    match error {
        GpioProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use cp0_gpio_protocol::GpioOutcome;

    use super::*;

    #[derive(Debug, Default)]
    struct MockGpio {
        values: RefCell<BTreeMap<u8, bool>>,
    }

    fn line_id(line: GpioLine) -> u8 {
        match line {
            GpioLine::GroveFunction => 0,
            GpioLine::ExternalUsbFunction => 1,
            GpioLine::Grove5vPower => 2,
            GpioLine::External5vPower => 3,
        }
    }

    impl GpioBackend for MockGpio {
        fn read(&self, line: GpioLine) -> Result<bool, GpioDeviceError> {
            Ok(self
                .values
                .borrow()
                .get(&line_id(line))
                .copied()
                .unwrap_or(false))
        }

        fn write(&self, line: GpioLine, value: bool) -> Result<(), GpioDeviceError> {
            self.values.borrow_mut().insert(line_id(line), value);
            Ok(())
        }
    }

    #[test]
    fn reads_and_writes_only_logical_lines() {
        let server = GpioServer::new(MockGpio::default(), [0]);
        let line = GpioLine::GroveFunction;
        assert_eq!(
            server.dispatch(GpioRequest::read(1, line)).outcome,
            GpioOutcome::Value { line, value: false }
        );
        assert_eq!(
            server.dispatch(GpioRequest::write(2, line, true)).outcome,
            GpioOutcome::Written { line, value: true }
        );
        assert_eq!(
            server.dispatch(GpioRequest::read(3, line)).outcome,
            GpioOutcome::Value { line, value: true }
        );
    }

    #[test]
    fn fixed_paths_never_derive_from_external_text() {
        assert_eq!(
            line_path(GpioLine::GroveFunction),
            Path::new("/sys/class/leds/grove_fun/brightness")
        );
        assert!(
            GpioLine::ALL
                .into_iter()
                .all(|line| line_path(line).starts_with("/sys/class/leds/"))
        );
    }
}
