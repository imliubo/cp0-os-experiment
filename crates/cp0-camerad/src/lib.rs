use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::ffi::CString;
use std::fmt;
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::Write;
use std::io::{self, BufReader, Read};
#[cfg(target_os = "linux")]
use std::os::fd::FromRawFd;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use cp0_camera_protocol::{
    CAMERA_FRAME_BYTES, CAMERA_HEIGHT, CAMERA_WIDTH, CameraErrorCode, CameraProtocolError,
    CameraResponse, encode_frame, read_request, send_frame_with_fd, write_response,
};

pub const DEFAULT_RPICAM_STILL: &str = "/usr/bin/rpicam-still";
const RGB888_PIXEL_BYTES: usize = 3;
const RGB888_FRAME_BYTES: usize =
    CAMERA_WIDTH as usize * CAMERA_HEIGHT as usize * RGB888_PIXEL_BYTES;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraCaptureError {
    Busy,
    Unavailable,
    TimedOut,
    CaptureFailed,
    InvalidFrame,
    Internal,
}

impl fmt::Display for CameraCaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => formatter.write_str("camera is busy"),
            Self::Unavailable => formatter.write_str("camera is unavailable"),
            Self::TimedOut => formatter.write_str("camera capture timed out"),
            Self::CaptureFailed => formatter.write_str("camera capture failed"),
            Self::InvalidFrame => formatter.write_str("camera returned an invalid RGB frame"),
            Self::Internal => formatter.write_str("camera service failed internally"),
        }
    }
}

impl std::error::Error for CameraCaptureError {}

pub trait CameraBackend {
    fn capture_rgb888(&self) -> Result<Vec<u8>, CameraCaptureError>;
}

#[derive(Debug, Clone)]
pub struct RpicamBackend {
    program: PathBuf,
}

impl Default for RpicamBackend {
    fn default() -> Self {
        Self::new(DEFAULT_RPICAM_STILL)
    }
}

impl RpicamBackend {
    pub fn new(program: impl AsRef<Path>) -> Self {
        Self {
            program: program.as_ref().to_path_buf(),
        }
    }
}

impl CameraBackend for RpicamBackend {
    fn capture_rgb888(&self) -> Result<Vec<u8>, CameraCaptureError> {
        let deadline = Instant::now() + CAPTURE_TIMEOUT;
        let mut child = Command::new(&self.program)
            .args([
                "--nopreview",
                "--timeout",
                "1",
                "--immediate",
                "--width",
                "320",
                "--height",
                "170",
                "--encoding",
                "rgb",
                "--output",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| match error.kind() {
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
                    CameraCaptureError::Unavailable
                }
                _ => CameraCaptureError::Internal,
            })?;
        let result = capture_child_output(&mut child);
        if result.is_err() {
            let _ = child.kill();
        }
        let status = wait_for_child(
            &mut child,
            deadline.saturating_duration_since(Instant::now()),
        )?;
        let output = result?;
        if !status.success() {
            return Err(match status.code() {
                Some(2) => CameraCaptureError::Busy,
                _ => CameraCaptureError::CaptureFailed,
            });
        }
        if output.len() != RGB888_FRAME_BYTES {
            return Err(CameraCaptureError::InvalidFrame);
        }
        Ok(output)
    }
}

fn capture_child_output(child: &mut Child) -> Result<Vec<u8>, CameraCaptureError> {
    let mut stdout = child.stdout.take().ok_or(CameraCaptureError::Internal)?;
    let deadline = Instant::now() + CAPTURE_TIMEOUT;
    let mut output = Vec::with_capacity(RGB888_FRAME_BYTES);
    let mut chunk = [0_u8; 4096];
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(CameraCaptureError::TimedOut);
        }
        let remaining = deadline.saturating_duration_since(now);
        let timeout = i32::try_from(remaining.as_millis().min(i32::MAX as u128))
            .map_err(|_| CameraCaptureError::Internal)?;
        let mut descriptor = libc::pollfd {
            fd: stdout.as_raw_fd(),
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        };
        let result = unsafe { libc::poll(&raw mut descriptor, 1, timeout) };
        if result < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(CameraCaptureError::Internal);
        }
        if result == 0 {
            return Err(CameraCaptureError::TimedOut);
        }
        if descriptor.revents & libc::POLLIN != 0 {
            let count = stdout
                .read(&mut chunk)
                .map_err(|_| CameraCaptureError::CaptureFailed)?;
            if count == 0 {
                break;
            }
            if output.len() + count > RGB888_FRAME_BYTES {
                return Err(CameraCaptureError::InvalidFrame);
            }
            output.extend_from_slice(&chunk[..count]);
        }
        if descriptor.revents & libc::POLLHUP != 0 {
            let count = stdout
                .read(&mut chunk)
                .map_err(|_| CameraCaptureError::CaptureFailed)?;
            if count > 0 {
                if output.len() + count > RGB888_FRAME_BYTES {
                    return Err(CameraCaptureError::InvalidFrame);
                }
                output.extend_from_slice(&chunk[..count]);
            }
            if count == 0 {
                break;
            }
        }
        if descriptor.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(CameraCaptureError::CaptureFailed);
        }
    }
    Ok(output)
}

fn wait_for_child(
    child: &mut Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, CameraCaptureError> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CameraCaptureError::TimedOut);
            }
            Err(_) => return Err(CameraCaptureError::Internal),
        }
    }
}

pub fn rgb888_to_rgb565(input: &[u8]) -> Result<Vec<u8>, CameraCaptureError> {
    if input.len() != RGB888_FRAME_BYTES {
        return Err(CameraCaptureError::InvalidFrame);
    }
    let mut output = Vec::with_capacity(CAMERA_FRAME_BYTES);
    for pixel in input.chunks_exact(RGB888_PIXEL_BYTES) {
        let value = (u16::from(pixel[0] & 0xf8) << 8)
            | (u16::from(pixel[1] & 0xfc) << 3)
            | u16::from(pixel[2] >> 3);
        output.extend_from_slice(&value.to_le_bytes());
    }
    debug_assert_eq!(output.len(), CAMERA_FRAME_BYTES);
    Ok(output)
}

#[derive(Debug)]
pub struct CameraServer<B> {
    backend: B,
    trusted_uids: BTreeSet<u32>,
}

impl<B: CameraBackend> CameraServer<B> {
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
                eprintln!("cp0-camerad: rejected connection: {error}");
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
                    &CameraResponse::error(
                        0,
                        CameraErrorCode::InvalidRequest,
                        "invalid camera service request",
                    ),
                )
                .map_err(protocol_io)?;
                eprintln!("cp0-camerad: invalid request: {error}");
                return Ok(());
            }
        };
        if !self.trusted_uids.contains(&uid) {
            write_response(
                &mut stream,
                &CameraResponse::error(
                    request.request_id,
                    CameraErrorCode::Unauthorized,
                    "peer UID is not authorized for camera access",
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }

        let response = match self.capture_descriptor() {
            Ok(descriptor) => {
                let response = CameraResponse::captured(request.request_id);
                let frame = encode_frame(&response).map_err(protocol_io)?;
                return send_frame_with_fd(&mut stream, &frame, descriptor.as_fd())
                    .map_err(protocol_io);
            }
            Err(error) => capture_error_response(request.request_id, error),
        };
        write_response(&mut stream, &response).map_err(protocol_io)
    }

    pub fn capture_rgb565(&self) -> Result<Vec<u8>, CameraCaptureError> {
        rgb888_to_rgb565(&self.backend.capture_rgb888()?)
    }

    fn capture_descriptor(&self) -> Result<OwnedFd, CameraCaptureError> {
        let frame = self.capture_rgb565()?;
        sealed_frame_descriptor(&frame)
    }
}

fn capture_error_response(request_id: u64, error: CameraCaptureError) -> CameraResponse {
    let (code, message) = match error {
        CameraCaptureError::Busy => (CameraErrorCode::Busy, "camera is busy"),
        CameraCaptureError::Unavailable => (CameraErrorCode::Unavailable, "camera is unavailable"),
        CameraCaptureError::TimedOut => {
            (CameraErrorCode::CaptureFailed, "camera capture timed out")
        }
        CameraCaptureError::CaptureFailed | CameraCaptureError::InvalidFrame => (
            CameraErrorCode::CaptureFailed,
            "camera could not produce a valid frame",
        ),
        CameraCaptureError::Internal => (
            CameraErrorCode::Internal,
            "camera service failed internally",
        ),
    };
    CameraResponse::error(request_id, code, message)
}

#[cfg(target_os = "linux")]
fn sealed_frame_descriptor(frame: &[u8]) -> Result<OwnedFd, CameraCaptureError> {
    if frame.len() != CAMERA_FRAME_BYTES {
        return Err(CameraCaptureError::InvalidFrame);
    }
    let name = c"cp0-camera-frame";
    let raw =
        unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if raw < 0 {
        return Err(CameraCaptureError::Internal);
    }
    let mut file = unsafe { File::from_raw_fd(raw) };
    file.write_all(frame)
        .and_then(|()| file.flush())
        .map_err(|_| CameraCaptureError::Internal)?;
    let seals = libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } != 0 {
        return Err(CameraCaptureError::Internal);
    }
    let path = CString::new(format!("/proc/self/fd/{}", file.as_raw_fd()))
        .map_err(|_| CameraCaptureError::Internal)?;
    let read_only = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if read_only < 0 {
        return Err(CameraCaptureError::Internal);
    }
    Ok(unsafe { OwnedFd::from_raw_fd(read_only) })
}

#[cfg(not(target_os = "linux"))]
fn sealed_frame_descriptor(_frame: &[u8]) -> Result<OwnedFd, CameraCaptureError> {
    Err(CameraCaptureError::Unavailable)
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

fn protocol_io(error: CameraProtocolError) -> io::Error {
    match error {
        CameraProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct MockCamera {
        frame: Vec<u8>,
    }

    impl CameraBackend for MockCamera {
        fn capture_rgb888(&self) -> Result<Vec<u8>, CameraCaptureError> {
            Ok(self.frame.clone())
        }
    }

    #[test]
    fn converts_rgb888_to_little_endian_rgb565() {
        let mut input = vec![0_u8; RGB888_FRAME_BYTES];
        input[..12].copy_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
        let converted = rgb888_to_rgb565(&input).unwrap();
        assert_eq!(
            &converted[..8],
            &[0x00, 0xf8, 0xe0, 0x07, 0x1f, 0x00, 0xff, 0xff]
        );
        assert_eq!(converted.len(), CAMERA_FRAME_BYTES);
    }

    #[test]
    fn rejects_any_non_exact_source_frame() {
        for size in [0, RGB888_FRAME_BYTES - 1, RGB888_FRAME_BYTES + 1] {
            assert_eq!(
                rgb888_to_rgb565(&vec![0; size]),
                Err(CameraCaptureError::InvalidFrame)
            );
        }
    }

    #[test]
    fn server_returns_only_the_fixed_rgb565_frame() {
        let server = CameraServer::new(
            MockCamera {
                frame: vec![0x80; RGB888_FRAME_BYTES],
            },
            [0],
        );
        let frame = server.capture_rgb565().unwrap();
        assert_eq!(frame.len(), CAMERA_FRAME_BYTES);
        assert!(
            frame
                .chunks_exact(cp0_camera_protocol::CAMERA_PIXEL_BYTES)
                .all(|pixel| pixel == [0x10, 0x84])
        );
    }
}
