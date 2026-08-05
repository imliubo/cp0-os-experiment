use std::fmt;
use std::net::Shutdown;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cp0_camera_protocol::{
    CAMERA_FRAME_BYTES, CAMERA_PHOTO_HEADER_BYTES, CameraErrorCode, CameraOutcome,
    CameraProtocolError, CameraRequest, MAX_CAMERA_JPEG_BYTES, decode_response, recv_frame_with_fd,
    write_request,
};

pub const DEFAULT_CAMERA_SOCKET: &str = "/run/cardputerzero-camerad/camera.sock";
// CM0 pipeline discovery and a 720p still transition can both be slow.
const CAMERA_SERVICE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug)]
pub struct CapturedCameraFrame {
    pub descriptor: OwnedFd,
}

#[derive(Debug)]
pub struct CapturedCameraPhoto {
    pub descriptor: OwnedFd,
    pub jpeg_size_bytes: u32,
}

#[derive(Debug)]
pub enum CameraClientError {
    Io(std::io::Error),
    Protocol(CameraProtocolError),
    EmptyResponse,
    MismatchedRequestId,
    MissingDescriptor,
    UnexpectedDescriptor,
    InvalidDescriptor,
    Service(CameraErrorCode),
}

impl fmt::Display for CameraClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "camera service I/O error: {error}"),
            Self::Protocol(error) => write!(formatter, "camera service protocol error: {error}"),
            Self::EmptyResponse => formatter.write_str("camera service returned no response"),
            Self::MismatchedRequestId => {
                formatter.write_str("camera service returned a mismatched request ID")
            }
            Self::MissingDescriptor => {
                formatter.write_str("camera service omitted the captured frame descriptor")
            }
            Self::UnexpectedDescriptor => {
                formatter.write_str("camera service returned an unexpected descriptor")
            }
            Self::InvalidDescriptor => formatter
                .write_str("camera frame descriptor is not sealed, read-only and exact-sized"),
            Self::Service(code) => write!(formatter, "camera service rejected request: {code:?}"),
        }
    }
}

impl std::error::Error for CameraClientError {}

impl From<std::io::Error> for CameraClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<CameraProtocolError> for CameraClientError {
    fn from(error: CameraProtocolError) -> Self {
        Self::Protocol(error)
    }
}

#[derive(Debug, Clone)]
pub struct CameraClient {
    socket_path: PathBuf,
}

impl Default for CameraClient {
    fn default() -> Self {
        Self::new(DEFAULT_CAMERA_SOCKET)
    }
}

impl CameraClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub fn capture(&self, request_id: u64) -> Result<CapturedCameraFrame, CameraClientError> {
        let stream = UnixStream::connect(&self.socket_path)?;
        let (descriptor, outcome) = Self::exchange(stream, CameraRequest::capture(request_id))?;
        if !matches!(outcome, CameraOutcome::Captured { .. }) {
            return Err(CameraClientError::InvalidDescriptor);
        }
        Ok(CapturedCameraFrame { descriptor })
    }

    pub fn capture_photo(&self, request_id: u64) -> Result<CapturedCameraPhoto, CameraClientError> {
        let stream = UnixStream::connect(&self.socket_path)?;
        let (descriptor, outcome) =
            Self::exchange(stream, CameraRequest::capture_photo(request_id))?;
        let CameraOutcome::PhotoCaptured {
            jpeg_size_bytes, ..
        } = outcome
        else {
            return Err(CameraClientError::InvalidDescriptor);
        };
        Ok(CapturedCameraPhoto {
            descriptor,
            jpeg_size_bytes,
        })
    }

    fn exchange(
        mut stream: UnixStream,
        request: CameraRequest,
    ) -> Result<(OwnedFd, CameraOutcome), CameraClientError> {
        stream.set_read_timeout(Some(CAMERA_SERVICE_TIMEOUT))?;
        stream.set_write_timeout(Some(CAMERA_SERVICE_TIMEOUT))?;
        write_request(&mut stream, &request)?;
        stream.shutdown(Shutdown::Write)?;
        let (frame, descriptor) = recv_frame_with_fd(&stream)?;
        if frame.is_empty() {
            return Err(CameraClientError::EmptyResponse);
        }
        let response = decode_response(&frame)?;
        if response.request_id != request.request_id {
            return Err(CameraClientError::MismatchedRequestId);
        }
        match response.outcome {
            outcome @ (CameraOutcome::Captured { .. } | CameraOutcome::PhotoCaptured { .. }) => {
                let descriptor = descriptor.ok_or(CameraClientError::MissingDescriptor)?;
                let expected_size = match &outcome {
                    CameraOutcome::Captured { .. } => CAMERA_FRAME_BYTES,
                    CameraOutcome::PhotoCaptured {
                        jpeg_size_bytes, ..
                    } => CAMERA_PHOTO_HEADER_BYTES + CAMERA_FRAME_BYTES + *jpeg_size_bytes as usize,
                    CameraOutcome::Error { .. } => unreachable!(),
                };
                validate_descriptor(&descriptor, expected_size)?;
                Ok((descriptor, outcome))
            }
            CameraOutcome::Error { code, .. } => {
                if descriptor.is_some() {
                    return Err(CameraClientError::UnexpectedDescriptor);
                }
                Err(CameraClientError::Service(code))
            }
        }
    }
}

fn validate_descriptor(
    descriptor: &OwnedFd,
    expected_size: usize,
) -> Result<(), CameraClientError> {
    let mut status: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(descriptor.as_raw_fd(), &raw mut status) } != 0 {
        return Err(CameraClientError::Io(std::io::Error::last_os_error()));
    }
    let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(CameraClientError::Io(std::io::Error::last_os_error()));
    }
    if status.st_mode & libc::S_IFMT != libc::S_IFREG
        || expected_size == 0
        || expected_size > CAMERA_PHOTO_HEADER_BYTES + CAMERA_FRAME_BYTES + MAX_CAMERA_JPEG_BYTES
        || status.st_size != expected_size as libc::off_t
        || flags & libc::O_ACCMODE != libc::O_RDONLY
    {
        return Err(CameraClientError::InvalidDescriptor);
    }
    validate_seals(descriptor)
}

#[cfg(target_os = "linux")]
fn validate_seals(descriptor: &OwnedFd) -> Result<(), CameraClientError> {
    let seals = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GET_SEALS) };
    let required = libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
    if seals < 0 || seals & required != required {
        Err(CameraClientError::InvalidDescriptor)
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
fn validate_seals(_descriptor: &OwnedFd) -> Result<(), CameraClientError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::fd::AsFd;
    use std::thread;

    use cp0_camera_protocol::{CameraResponse, encode_frame, read_request, send_frame_with_fd};

    use super::*;

    #[test]
    fn receives_only_an_exact_read_only_frame_descriptor() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/camera-client-frame.rgb565");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![0_u8; CAMERA_FRAME_BYTES]).unwrap();
        let file = fs::File::open(path).unwrap();
        let (client, mut service) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let mut reader = std::io::BufReader::new(service.try_clone().unwrap());
            let request = read_request(&mut reader).unwrap().unwrap();
            let frame = encode_frame(&CameraResponse::captured(request.request_id)).unwrap();
            send_frame_with_fd(&mut service, &frame, file.as_fd()).unwrap();
        });
        let result = CameraClient::exchange(client, CameraRequest::capture(11));
        worker.join().unwrap();
        #[cfg(target_os = "linux")]
        assert!(matches!(result, Err(CameraClientError::InvalidDescriptor)));
        #[cfg(not(target_os = "linux"))]
        assert!(result.is_ok());
    }
}
