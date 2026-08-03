use std::collections::BTreeSet;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use cp0_display_protocol::{
    DISPLAY_BRIGHTNESS_MAX_PERCENT, DISPLAY_BRIGHTNESS_MIN_PERCENT,
    DISPLAY_BRIGHTNESS_STEP_PERCENT, DisplayCommand, DisplayDirection, DisplayErrorCode,
    DisplayOutcome, DisplayProtocolError, DisplayRequest, DisplayResponse, DisplayState,
    read_request, write_response,
};

pub const DEFAULT_BACKLIGHT_ROOT: &str = "/sys/class/backlight/backlight";
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);
const ATTRIBUTE_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayDeviceError {
    Unavailable,
    Device,
}

impl fmt::Display for DisplayDeviceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("display backlight is unavailable"),
            Self::Device => formatter.write_str("display backlight operation failed"),
        }
    }
}

impl std::error::Error for DisplayDeviceError {}

pub trait DisplayBackend {
    fn read_brightness_percent(&self) -> Result<u8, DisplayDeviceError>;
    fn write_brightness_percent(&self, percent: u8) -> Result<u8, DisplayDeviceError>;
}

#[derive(Debug, Clone)]
pub struct SysfsBacklightBackend {
    root: PathBuf,
}

impl Default for SysfsBacklightBackend {
    fn default() -> Self {
        Self::new(DEFAULT_BACKLIGHT_ROOT)
    }
}

impl SysfsBacklightBackend {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn read_raw(&self, name: &str) -> Result<u32, DisplayDeviceError> {
        let path = self.root.join(name);
        let mut file = open_attribute(&path, false)?;
        let mut buffer = [0_u8; ATTRIBUTE_BYTES];
        let count = file.read(&mut buffer).map_err(map_io_error)?;
        if count == 0 || count == buffer.len() {
            return Err(DisplayDeviceError::Device);
        }
        let value = std::str::from_utf8(&buffer[..count])
            .map_err(|_| DisplayDeviceError::Device)?
            .trim();
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(DisplayDeviceError::Device);
        }
        value.parse().map_err(|_| DisplayDeviceError::Device)
    }

    fn write_raw(&self, value: u32) -> Result<(), DisplayDeviceError> {
        let path = self.root.join("brightness");
        let mut file = open_attribute(&path, true)?;
        write_sysfs_value(&mut file, value).map_err(map_io_error)
    }

    fn raw_state(&self) -> Result<(u32, u32), DisplayDeviceError> {
        let maximum = self.read_raw("max_brightness")?;
        let brightness = self.read_raw("brightness")?;
        if maximum == 0 || brightness > maximum {
            return Err(DisplayDeviceError::Device);
        }
        Ok((brightness, maximum))
    }
}

fn write_sysfs_value(writer: &mut impl Write, value: u32) -> io::Result<()> {
    let encoded = format!("{value}\n");
    let count = writer.write(encoded.as_bytes())?;
    if count != encoded.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "short sysfs attribute write",
        ));
    }
    Ok(())
}

impl DisplayBackend for SysfsBacklightBackend {
    fn read_brightness_percent(&self) -> Result<u8, DisplayDeviceError> {
        let (brightness, maximum) = self.raw_state()?;
        let percent = (u64::from(brightness) * 100 + u64::from(maximum) / 2) / u64::from(maximum);
        u8::try_from(percent.min(u64::from(DISPLAY_BRIGHTNESS_MAX_PERCENT)))
            .map_err(|_| DisplayDeviceError::Device)
    }

    fn write_brightness_percent(&self, percent: u8) -> Result<u8, DisplayDeviceError> {
        if !(DISPLAY_BRIGHTNESS_MIN_PERCENT..=DISPLAY_BRIGHTNESS_MAX_PERCENT).contains(&percent) {
            return Err(DisplayDeviceError::Device);
        }
        let (_, maximum) = self.raw_state()?;
        let raw =
            ((u64::from(maximum) * u64::from(percent) + 50) / 100).clamp(1, u64::from(maximum));
        self.write_raw(u32::try_from(raw).map_err(|_| DisplayDeviceError::Device)?)?;
        self.read_brightness_percent()
    }
}

fn open_attribute(path: &Path, writable: bool) -> Result<File, DisplayDeviceError> {
    OpenOptions::new()
        .read(!writable)
        .write(writable)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(map_io_error)
}

fn map_io_error(error: io::Error) -> DisplayDeviceError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
            DisplayDeviceError::Unavailable
        }
        _ => DisplayDeviceError::Device,
    }
}

#[derive(Debug)]
pub struct DisplayServer<B> {
    backend: B,
    trusted_uids: BTreeSet<u32>,
}

impl<B: DisplayBackend> DisplayServer<B> {
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
                eprintln!("cp0-displayd: rejected connection: {error}");
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
                    &DisplayResponse::error(
                        0,
                        DisplayErrorCode::InvalidRequest,
                        "invalid display service request",
                    ),
                )
                .map_err(protocol_io)?;
                eprintln!("cp0-displayd: invalid request: {error}");
                return Ok(());
            }
        };
        if !self.trusted_uids.contains(&uid) {
            write_response(
                &mut stream,
                &DisplayResponse::error(
                    request.request_id,
                    DisplayErrorCode::Unauthorized,
                    "peer UID is not authorized for display control",
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }
        let mutating = !matches!(request.command, DisplayCommand::GetState {});
        let response = self.dispatch(request);
        if mutating {
            if let DisplayOutcome::State { state } = &response.outcome {
                if let Some(percent) = state.brightness_percent {
                    eprintln!("cp0-displayd: audit uid={uid} brightness_percent={percent}");
                }
            }
        }
        write_response(&mut stream, &response).map_err(protocol_io)
    }

    pub fn dispatch(&self, request: DisplayRequest) -> DisplayResponse {
        let request_id = request.request_id;
        match request.command {
            DisplayCommand::GetState {} => match self.backend.read_brightness_percent() {
                Ok(percent) => DisplayResponse::state(request_id, DisplayState::available(percent)),
                Err(DisplayDeviceError::Unavailable) => {
                    DisplayResponse::state(request_id, DisplayState::unavailable())
                }
                Err(error) => device_error_response(request_id, error),
            },
            DisplayCommand::SetBrightness { percent } => self.write_response(request_id, percent),
            DisplayCommand::AdjustBrightness { direction } => {
                let current = match self.backend.read_brightness_percent() {
                    Ok(percent) => percent,
                    Err(error) => return device_error_response(request_id, error),
                };
                let percent = match direction {
                    DisplayDirection::Decrease => current
                        .saturating_sub(DISPLAY_BRIGHTNESS_STEP_PERCENT)
                        .max(DISPLAY_BRIGHTNESS_MIN_PERCENT),
                    DisplayDirection::Increase => current
                        .saturating_add(DISPLAY_BRIGHTNESS_STEP_PERCENT)
                        .clamp(
                            DISPLAY_BRIGHTNESS_MIN_PERCENT,
                            DISPLAY_BRIGHTNESS_MAX_PERCENT,
                        ),
                };
                self.write_response(request_id, percent)
            }
        }
    }

    fn write_response(&self, request_id: u64, percent: u8) -> DisplayResponse {
        match self.backend.write_brightness_percent(percent) {
            Ok(observed) => DisplayResponse::state(request_id, DisplayState::available(observed)),
            Err(error) => device_error_response(request_id, error),
        }
    }
}

fn device_error_response(request_id: u64, error: DisplayDeviceError) -> DisplayResponse {
    match error {
        DisplayDeviceError::Unavailable => DisplayResponse::error(
            request_id,
            DisplayErrorCode::Unavailable,
            "display backlight is unavailable",
        ),
        DisplayDeviceError::Device => DisplayResponse::error(
            request_id,
            DisplayErrorCode::Device,
            "display backlight operation failed",
        ),
    }
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

fn protocol_io(error: DisplayProtocolError) -> io::Error {
    match error {
        DisplayProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[derive(Debug, Default)]
    struct SingleWriteRecorder {
        calls: usize,
        bytes: Vec<u8>,
    }

    impl Write for SingleWriteRecorder {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            if self.calls > 1 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "sysfs rejected a second write",
                ));
            }
            self.bytes.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct MockDisplay {
        percent: Cell<u8>,
        unavailable: bool,
    }

    impl MockDisplay {
        fn available(percent: u8) -> Self {
            Self {
                percent: Cell::new(percent),
                unavailable: false,
            }
        }

        fn unavailable() -> Self {
            Self {
                percent: Cell::new(0),
                unavailable: true,
            }
        }
    }

    impl DisplayBackend for MockDisplay {
        fn read_brightness_percent(&self) -> Result<u8, DisplayDeviceError> {
            if self.unavailable {
                Err(DisplayDeviceError::Unavailable)
            } else {
                Ok(self.percent.get())
            }
        }

        fn write_brightness_percent(&self, percent: u8) -> Result<u8, DisplayDeviceError> {
            if self.unavailable {
                return Err(DisplayDeviceError::Unavailable);
            }
            self.percent.set(percent);
            Ok(self.percent.get())
        }
    }

    fn state(response: DisplayResponse) -> DisplayState {
        match response.outcome {
            DisplayOutcome::State { state } => state,
            outcome => panic!("expected state response, got {outcome:?}"),
        }
    }

    #[test]
    fn encodes_brightness_as_one_sysfs_write() {
        let mut writer = SingleWriteRecorder::default();
        write_sysfs_value(&mut writer, 65).expect("write brightness");
        assert_eq!(writer.calls, 1);
        assert_eq!(writer.bytes, b"65\n");
    }

    #[test]
    fn reports_and_adjusts_observed_brightness() {
        let server = DisplayServer::new(MockDisplay::available(70), [0]);
        assert_eq!(
            state(server.dispatch(DisplayRequest::get_state(1))),
            DisplayState::available(70)
        );
        assert_eq!(
            state(server.dispatch(DisplayRequest::adjust_brightness(
                2,
                DisplayDirection::Decrease,
            ))),
            DisplayState::available(60)
        );
        assert_eq!(
            state(server.dispatch(DisplayRequest::set_brightness(3, 95))),
            DisplayState::available(95)
        );
    }

    #[test]
    fn clamps_adjustments_to_safe_bounds() {
        let low = DisplayServer::new(MockDisplay::available(6), [0]);
        assert_eq!(
            state(low.dispatch(DisplayRequest::adjust_brightness(
                1,
                DisplayDirection::Decrease,
            ))),
            DisplayState::available(DISPLAY_BRIGHTNESS_MIN_PERCENT)
        );
        let high = DisplayServer::new(MockDisplay::available(99), [0]);
        assert_eq!(
            state(high.dispatch(DisplayRequest::adjust_brightness(
                2,
                DisplayDirection::Increase,
            ))),
            DisplayState::available(DISPLAY_BRIGHTNESS_MAX_PERCENT)
        );
    }

    #[test]
    fn fails_closed_when_backlight_is_absent() {
        let server = DisplayServer::new(MockDisplay::unavailable(), [0]);
        assert_eq!(
            state(server.dispatch(DisplayRequest::get_state(1))),
            DisplayState::unavailable()
        );
        let response = server.dispatch(DisplayRequest::adjust_brightness(
            2,
            DisplayDirection::Increase,
        ));
        assert!(matches!(
            response.outcome,
            DisplayOutcome::Error {
                code: DisplayErrorCode::Unavailable,
                ..
            }
        ));
    }
}
