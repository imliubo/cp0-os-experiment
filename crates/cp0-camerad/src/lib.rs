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
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use cp0_camera_protocol::{
    CAMERA_FRAME_BYTES, CAMERA_HEIGHT, CAMERA_PHOTO_HEADER_BYTES, CAMERA_WIDTH, CameraCommand,
    CameraErrorCode, CameraProtocolError, CameraResponse, MAX_CAMERA_JPEG_BYTES, encode_frame,
    encode_photo_payload, read_request, send_frame_with_fd, write_response,
};
use jpeg_encoder::{Encoder, ImageBuffer, JpegColorType, SamplingFactor};

pub const DEFAULT_RPICAM_VID: &str = "/usr/bin/rpicam-vid";
const CAMERA_STREAM_WIDTH: usize = 1280;
const CAMERA_STREAM_HEIGHT: usize = 720;
const YUV420_FRAME_BYTES: usize = CAMERA_STREAM_WIDTH * CAMERA_STREAM_HEIGHT * 3 / 2;
const RPICAM_STREAM_FPS: u16 = 30;
const JPEG_QUALITY: u8 = 90;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);
const STREAM_START_DEADLINE: Duration = Duration::from_secs(20);
const STREAM_REQUEST_TIMEOUT: Duration = Duration::from_millis(50);
const STREAM_STALL_DEADLINE: Duration = Duration::from_millis(500);
const BACKEND_IDLE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_PHOTO_PAYLOAD_BYTES: usize =
    CAMERA_PHOTO_HEADER_BYTES + CAMERA_FRAME_BYTES + MAX_CAMERA_JPEG_BYTES;

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
    fn capture_rgb565(&self) -> Result<Vec<u8>, CameraCaptureError>;
    fn capture_photo(&self) -> Result<CapturedPhoto, CameraCaptureError>;

    fn release(&self) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedPhoto {
    pub thumbnail_rgb565: Vec<u8>,
    pub jpeg: Vec<u8>,
}

#[derive(Debug)]
pub struct RpicamBackend {
    preview_program: PathBuf,
    stream: Mutex<StreamState>,
}

#[derive(Debug)]
enum StreamState {
    Idle,
    Starting {
        started_at: Instant,
        receiver: mpsc::Receiver<Result<RpicamStream, CameraCaptureError>>,
    },
    Running(RpicamStream),
}

impl Default for RpicamBackend {
    fn default() -> Self {
        Self::new(DEFAULT_RPICAM_VID)
    }
}

impl RpicamBackend {
    pub fn new(program: impl AsRef<Path>) -> Self {
        Self {
            preview_program: program.as_ref().to_path_buf(),
            stream: Mutex::new(StreamState::Idle),
        }
    }

    fn capture_yuv420(&self) -> Result<Vec<u8>, CameraCaptureError> {
        let mut state = self
            .stream
            .lock()
            .map_err(|_| CameraCaptureError::Internal)?;
        loop {
            match std::mem::replace(&mut *state, StreamState::Idle) {
                StreamState::Idle => {
                    let (sender, receiver) = mpsc::sync_channel(1);
                    let program = self.preview_program.clone();
                    thread::Builder::new()
                        .name("cp0-camera-start".into())
                        .spawn(move || {
                            let _ = sender.send(RpicamStream::start(&program));
                        })
                        .map_err(|_| CameraCaptureError::Internal)?;
                    *state = StreamState::Starting {
                        started_at: Instant::now(),
                        receiver,
                    };
                    return Err(CameraCaptureError::TimedOut);
                }
                StreamState::Starting {
                    started_at,
                    receiver,
                } => match receiver.try_recv() {
                    Ok(Ok(stream)) => {
                        *state = StreamState::Running(stream);
                    }
                    Ok(Err(error)) => return Err(error),
                    Err(mpsc::TryRecvError::Empty)
                        if started_at.elapsed() < STREAM_START_DEADLINE =>
                    {
                        *state = StreamState::Starting {
                            started_at,
                            receiver,
                        };
                        return Err(CameraCaptureError::TimedOut);
                    }
                    Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => {
                        return Err(CameraCaptureError::CaptureFailed);
                    }
                },
                StreamState::Running(mut stream) => {
                    let result = stream.capture_yuv420();
                    if !result
                        .as_ref()
                        .is_err_and(|error| *error != CameraCaptureError::TimedOut)
                    {
                        *state = StreamState::Running(stream);
                    }
                    return result;
                }
            }
        }
    }
}

impl CameraBackend for RpicamBackend {
    fn capture_rgb565(&self) -> Result<Vec<u8>, CameraCaptureError> {
        self.capture_yuv420()
            .and_then(|frame| yuv420_to_rgb565(&frame))
    }

    fn capture_photo(&self) -> Result<CapturedPhoto, CameraCaptureError> {
        let frame = self.capture_yuv420()?;
        let thumbnail_rgb565 = yuv420_to_rgb565(&frame)?;
        let jpeg = encode_yuv420_jpeg(&frame)?;
        Ok(CapturedPhoto {
            thumbnail_rgb565,
            jpeg,
        })
    }

    fn release(&self) {
        if let Ok(mut stream) = self.stream.lock() {
            *stream = StreamState::Idle;
        }
    }
}

#[derive(Debug)]
struct RpicamStream {
    child: Child,
    stdout: ChildStdout,
    started_at: Instant,
    frame: Vec<u8>,
    frame_offset: usize,
    frames_captured: u64,
    last_frame_at: Instant,
}

impl RpicamStream {
    fn start(program: &Path) -> Result<Self, CameraCaptureError> {
        let preview_fps = RPICAM_STREAM_FPS.to_string();
        let mut child = Command::new(program)
            .args([
                "--nopreview",
                "--timeout",
                "0",
                "--width",
                "1280",
                "--height",
                "720",
                "--mode",
                "1920:1080:10:P",
                "--rotation",
                "180",
                "--framerate",
            ])
            .arg(preview_fps)
            .args(["--codec", "yuv420", "--flush", "--output", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                eprintln!("cp0-camerad: cannot start rpicam-vid preview: {error}");
                match error.kind() {
                    io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied => {
                        CameraCaptureError::Unavailable
                    }
                    _ => CameraCaptureError::Internal,
                }
            })?;
        let stdout = child.stdout.take().ok_or(CameraCaptureError::Internal)?;
        let started_at = Instant::now();
        Ok(Self {
            child,
            stdout,
            started_at,
            frame: vec![0_u8; YUV420_FRAME_BYTES],
            frame_offset: 0,
            frames_captured: 0,
            last_frame_at: started_at,
        })
    }

    fn capture_yuv420(&mut self) -> Result<Vec<u8>, CameraCaptureError> {
        if self
            .child
            .try_wait()
            .map_err(|_| CameraCaptureError::Internal)?
            .is_some()
        {
            return Err(CameraCaptureError::CaptureFailed);
        }
        if self.frames_captured == 0 && self.started_at.elapsed() >= STREAM_START_DEADLINE {
            return Err(CameraCaptureError::CaptureFailed);
        }
        match read_exact_frame(
            &mut self.stdout,
            &mut self.frame,
            &mut self.frame_offset,
            STREAM_REQUEST_TIMEOUT,
        ) {
            Err(CameraCaptureError::TimedOut)
                if self.frames_captured != 0
                    && self.last_frame_at.elapsed() >= STREAM_STALL_DEADLINE =>
            {
                return Err(CameraCaptureError::CaptureFailed);
            }
            Err(error) => return Err(error),
            Ok(()) => {}
        }
        let frame = std::mem::replace(&mut self.frame, vec![0_u8; YUV420_FRAME_BYTES]);
        self.frame_offset = 0;
        self.frames_captured = self.frames_captured.saturating_add(1);
        self.last_frame_at = Instant::now();
        Ok(frame)
    }
}

impl Drop for RpicamStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_exact_frame(
    stdout: &mut ChildStdout,
    frame: &mut [u8],
    offset: &mut usize,
    timeout: Duration,
) -> Result<(), CameraCaptureError> {
    if *offset >= frame.len() {
        return Err(CameraCaptureError::InvalidFrame);
    }
    let deadline = Instant::now() + timeout;
    while *offset < frame.len() {
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
                .read(&mut frame[*offset..])
                .map_err(|_| CameraCaptureError::CaptureFailed)?;
            if count == 0 {
                return Err(CameraCaptureError::CaptureFailed);
            }
            *offset += count;
            if *offset == frame.len() {
                break;
            }
        }
        if descriptor.revents & libc::POLLHUP != 0 {
            return Err(CameraCaptureError::CaptureFailed);
        }
        if descriptor.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(CameraCaptureError::CaptureFailed);
        }
    }
    Ok(())
}

pub fn yuv420_to_rgb565(input: &[u8]) -> Result<Vec<u8>, CameraCaptureError> {
    if input.len() != YUV420_FRAME_BYTES {
        return Err(CameraCaptureError::InvalidFrame);
    }
    let width = CAMERA_STREAM_WIDTH;
    let height = CAMERA_STREAM_HEIGHT;
    let y_size = width * height;
    let chroma_width = width / 2;
    let chroma_size = chroma_width * (height / 2);
    let (luma, chroma) = input.split_at(y_size);
    let (u_plane, v_plane) = chroma.split_at(chroma_size);
    let mut output = Vec::with_capacity(CAMERA_FRAME_BYTES);
    for row in 0..CAMERA_HEIGHT as usize {
        let source_row = row * height / CAMERA_HEIGHT as usize;
        for column in 0..CAMERA_WIDTH as usize {
            let source_column = column * width / CAMERA_WIDTH as usize;
            let (red, green, blue) = yuv420_pixel(
                luma,
                u_plane,
                v_plane,
                width,
                chroma_width,
                source_column,
                source_row,
            );
            let value = (u16::from(red & 0xf8) << 8)
                | (u16::from(green & 0xfc) << 3)
                | u16::from(blue >> 3);
            output.extend_from_slice(&value.to_le_bytes());
        }
    }
    debug_assert_eq!(output.len(), CAMERA_FRAME_BYTES);
    Ok(output)
}

fn encode_yuv420_jpeg(input: &[u8]) -> Result<Vec<u8>, CameraCaptureError> {
    if input.len() != YUV420_FRAME_BYTES {
        return Err(CameraCaptureError::InvalidFrame);
    }

    #[cfg(target_os = "linux")]
    if let Ok(jpeg) = encode_yuv420_jpeg_hardware(input) {
        return Ok(jpeg);
    }

    let mut jpeg = Vec::with_capacity(256 * 1024);
    let mut encoder = Encoder::new(&mut jpeg, JPEG_QUALITY);
    encoder.set_sampling_factor(SamplingFactor::F_2_2);
    encoder
        .encode_image(Yuv420Image::new(input))
        .map_err(|_| CameraCaptureError::CaptureFailed)?;
    validate_jpeg(jpeg)
}

fn validate_jpeg(jpeg: Vec<u8>) -> Result<Vec<u8>, CameraCaptureError> {
    if jpeg.is_empty()
        || jpeg.len() > MAX_CAMERA_JPEG_BYTES
        || !jpeg.starts_with(&[0xff, 0xd8])
        || !jpeg.ends_with(&[0xff, 0xd9])
    {
        return Err(CameraCaptureError::InvalidFrame);
    }
    Ok(jpeg)
}

struct Yuv420Image<'a> {
    luma: &'a [u8],
    u_plane: &'a [u8],
    v_plane: &'a [u8],
}

impl<'a> Yuv420Image<'a> {
    fn new(input: &'a [u8]) -> Self {
        let y_size = CAMERA_STREAM_WIDTH * CAMERA_STREAM_HEIGHT;
        let chroma_size = y_size / 4;
        let (luma, chroma) = input.split_at(y_size);
        let (u_plane, v_plane) = chroma.split_at(chroma_size);
        Self {
            luma,
            u_plane,
            v_plane,
        }
    }
}

impl ImageBuffer for Yuv420Image<'_> {
    fn get_jpeg_color_type(&self) -> JpegColorType {
        JpegColorType::Ycbcr
    }

    fn width(&self) -> u16 {
        CAMERA_STREAM_WIDTH as u16
    }

    fn height(&self) -> u16 {
        CAMERA_STREAM_HEIGHT as u16
    }

    fn fill_buffers(&self, row: u16, buffers: &mut [Vec<u8>; 4]) {
        let row = usize::from(row);
        let luma = &self.luma[row * CAMERA_STREAM_WIDTH..(row + 1) * CAMERA_STREAM_WIDTH];
        let chroma_row = row / 2 * (CAMERA_STREAM_WIDTH / 2);
        for (column, &value) in luma.iter().enumerate() {
            let chroma = chroma_row + column / 2;
            buffers[0].push(value);
            buffers[1].push(self.u_plane[chroma]);
            buffers[2].push(self.v_plane[chroma]);
        }
    }
}

#[cfg(target_os = "linux")]
fn encode_yuv420_jpeg_hardware(input: &[u8]) -> Result<Vec<u8>, CameraCaptureError> {
    unsafe extern "C" {
        fn cp0_v4l2_encode_jpeg(
            yuv420: *const u8,
            yuv420_length: usize,
            width: u32,
            height: u32,
            quality: u32,
            jpeg: *mut u8,
            jpeg_capacity: usize,
            jpeg_length: *mut usize,
        ) -> libc::c_int;
    }

    let mut jpeg = vec![0_u8; MAX_CAMERA_JPEG_BYTES];
    let mut jpeg_length = 0_usize;
    let result = unsafe {
        cp0_v4l2_encode_jpeg(
            input.as_ptr(),
            input.len(),
            CAMERA_STREAM_WIDTH as u32,
            CAMERA_STREAM_HEIGHT as u32,
            u32::from(JPEG_QUALITY),
            jpeg.as_mut_ptr(),
            jpeg.len(),
            &raw mut jpeg_length,
        )
    };
    if result != 0 || jpeg_length > jpeg.len() {
        return Err(CameraCaptureError::Unavailable);
    }
    jpeg.truncate(jpeg_length);
    validate_jpeg(jpeg)
}

fn yuv420_pixel(
    luma: &[u8],
    u_plane: &[u8],
    v_plane: &[u8],
    width: usize,
    chroma_width: usize,
    column: usize,
    row: usize,
) -> (u8, u8, u8) {
    let y = i32::from(luma[row * width + column]).saturating_sub(16);
    let chroma_index = (row / 2) * chroma_width + column / 2;
    let u = i32::from(u_plane[chroma_index]) - 128;
    let v = i32::from(v_plane[chroma_index]) - 128;
    (
        clamp_u8((298 * y + 409 * v + 128) >> 8),
        clamp_u8((298 * y - 100 * u - 208 * v + 128) >> 8),
        clamp_u8((298 * y + 516 * u + 128) >> 8),
    )
}

fn clamp_u8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
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
            if !listener_ready(&listener, BACKEND_IDLE_TIMEOUT)? {
                self.backend.release();
                continue;
            }
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

        let response = match request.command {
            CameraCommand::CaptureRgb565 => self
                .capture_descriptor()
                .map(|descriptor| (CameraResponse::captured(request.request_id), descriptor)),
            CameraCommand::CapturePhoto => {
                self.capture_photo_descriptor().map(|(descriptor, size)| {
                    (
                        CameraResponse::photo_captured(request.request_id, size),
                        descriptor,
                    )
                })
            }
        };
        let response = match response {
            Ok((response, descriptor)) => {
                let frame = encode_frame(&response).map_err(protocol_io)?;
                return send_frame_with_fd(&mut stream, &frame, descriptor.as_fd())
                    .map_err(protocol_io);
            }
            Err(error) => capture_error_response(request.request_id, error),
        };
        write_response(&mut stream, &response).map_err(protocol_io)
    }

    pub fn capture_rgb565(&self) -> Result<Vec<u8>, CameraCaptureError> {
        self.backend.capture_rgb565()
    }

    fn capture_descriptor(&self) -> Result<OwnedFd, CameraCaptureError> {
        let frame = self.capture_rgb565()?;
        sealed_payload_descriptor(&frame, CAMERA_FRAME_BYTES, CAMERA_FRAME_BYTES)
    }

    fn capture_photo_descriptor(&self) -> Result<(OwnedFd, u32), CameraCaptureError> {
        let photo = self.backend.capture_photo()?;
        let jpeg_size =
            u32::try_from(photo.jpeg.len()).map_err(|_| CameraCaptureError::InvalidFrame)?;
        let payload = encode_photo_payload(&photo.thumbnail_rgb565, &photo.jpeg)
            .map_err(|_| CameraCaptureError::InvalidFrame)?;
        let descriptor = sealed_payload_descriptor(
            &payload,
            CAMERA_PHOTO_HEADER_BYTES + CAMERA_FRAME_BYTES + 1,
            MAX_PHOTO_PAYLOAD_BYTES,
        )?;
        Ok((descriptor, jpeg_size))
    }
}

fn listener_ready(listener: &UnixListener, timeout: Duration) -> io::Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout_ms = i32::try_from(remaining.as_millis().min(i32::MAX as u128))
            .map_err(|_| io::Error::other("camera listener timeout is invalid"))?;
        let mut descriptor = libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(&raw mut descriptor, 1, timeout_ms) };
        if result < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(io::Error::last_os_error());
        }
        if result == 0 {
            return Ok(false);
        }
        if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::other("camera listener poll failed"));
        }
        return Ok(descriptor.revents & libc::POLLIN != 0);
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
fn sealed_payload_descriptor(
    payload: &[u8],
    minimum: usize,
    maximum: usize,
) -> Result<OwnedFd, CameraCaptureError> {
    if payload.len() < minimum || payload.len() > maximum {
        return Err(CameraCaptureError::InvalidFrame);
    }
    let name = c"cp0-camera-frame";
    let raw =
        unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if raw < 0 {
        return Err(CameraCaptureError::Internal);
    }
    let mut file = unsafe { File::from_raw_fd(raw) };
    file.write_all(payload)
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
fn sealed_payload_descriptor(
    _payload: &[u8],
    _minimum: usize,
    _maximum: usize,
) -> Result<OwnedFd, CameraCaptureError> {
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
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[derive(Debug)]
    struct MockCamera {
        frame: Vec<u8>,
    }

    impl CameraBackend for MockCamera {
        fn capture_rgb565(&self) -> Result<Vec<u8>, CameraCaptureError> {
            Ok(self.frame.clone())
        }

        fn capture_photo(&self) -> Result<CapturedPhoto, CameraCaptureError> {
            Ok(CapturedPhoto {
                thumbnail_rgb565: self.frame.clone(),
                jpeg: vec![0xff, 0xd8, 0x42, 0xff, 0xd9],
            })
        }
    }

    fn wait_for_preview(backend: &RpicamBackend) -> Vec<u8> {
        for _ in 0..100 {
            match backend.capture_rgb565() {
                Ok(frame) => return frame,
                Err(CameraCaptureError::TimedOut) => thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("camera preview failed while warming: {error}"),
            }
        }
        panic!("camera preview did not become ready")
    }

    #[test]
    fn converts_yuv420_to_little_endian_rgb565() {
        let y_size = CAMERA_WIDTH as usize * CAMERA_HEIGHT as usize;
        let mut input = vec![128_u8; YUV420_FRAME_BYTES];
        input[..y_size].fill(16);
        input[0] = 235;
        let converted = yuv420_to_rgb565(&input).unwrap();
        assert_eq!(&converted[..4], &[0xff, 0xff, 0x00, 0x00]);
        assert_eq!(converted.len(), CAMERA_FRAME_BYTES);
    }

    #[test]
    fn rejects_any_non_exact_source_frame() {
        for size in [0, YUV420_FRAME_BYTES - 1, YUV420_FRAME_BYTES + 1] {
            assert_eq!(
                yuv420_to_rgb565(&vec![0; size]),
                Err(CameraCaptureError::InvalidFrame)
            );
        }
    }

    #[test]
    fn reuses_the_fixed_preview_process_until_release() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("camerad-stream-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let program = directory.join("fake-rpicam-vid");
        let arguments = directory.join("arguments");
        let starts = directory.join("starts");
        fs::write(&starts, b"").unwrap();
        fs::write(
            &program,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$$\" >> '{}'\nprintf '%s\\n' \"$@\" > '{}'\nhead -c {} /dev/zero\nsleep 30\n",
                starts.display(),
                arguments.display(),
                YUV420_FRAME_BYTES * 2
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();

        let backend = RpicamBackend::new(&program);
        assert_eq!(wait_for_preview(&backend).len(), CAMERA_FRAME_BYTES);
        assert_eq!(backend.capture_rgb565().unwrap().len(), CAMERA_FRAME_BYTES);
        assert_eq!(fs::read_to_string(&starts).unwrap().lines().count(), 1);
        let fixed_arguments = fs::read_to_string(&arguments).unwrap();
        assert!(fixed_arguments.contains("--width\n1280\n"));
        assert!(fixed_arguments.contains("--height\n720\n"));
        assert!(fixed_arguments.contains("--mode\n1920:1080:10:P\n"));
        assert!(fixed_arguments.contains("--rotation\n180\n"));
        assert!(fixed_arguments.contains("--framerate\n30\n"));
        assert!(fixed_arguments.contains("--codec\nyuv420\n"));

        backend.release();
        assert_eq!(wait_for_preview(&backend).len(), CAMERA_FRAME_BYTES);
        assert_eq!(fs::read_to_string(&starts).unwrap().lines().count(), 2);
    }

    #[test]
    fn photo_capture_encodes_the_stream_frame_without_restarting_preview() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("camerad-photo-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let preview = directory.join("fake-rpicam-vid");
        let preview_arguments = directory.join("preview-arguments");
        let starts = directory.join("starts");
        fs::write(&starts, b"").unwrap();
        fs::write(
            &preview,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$$\" >> '{}'\nprintf '%s\\n' \"$@\" > '{}'\nhead -c {} /dev/zero\nsleep 30\n",
                starts.display(),
                preview_arguments.display(),
                YUV420_FRAME_BYTES * 2
            ),
        )
        .unwrap();
        fs::set_permissions(&preview, fs::Permissions::from_mode(0o755)).unwrap();

        let backend = RpicamBackend::new(&preview);
        assert_eq!(wait_for_preview(&backend).len(), CAMERA_FRAME_BYTES);
        let photo = backend.capture_photo().unwrap();
        assert_eq!(photo.thumbnail_rgb565.len(), CAMERA_FRAME_BYTES);
        assert!(photo.jpeg.starts_with(&[0xff, 0xd8]));
        assert!(photo.jpeg.ends_with(&[0xff, 0xd9]));
        assert_eq!(fs::read_to_string(&starts).unwrap().lines().count(), 1);
        let arguments = fs::read_to_string(preview_arguments).unwrap();
        assert!(arguments.contains("--width\n1280\n"));
        assert!(arguments.contains("--height\n720\n"));
        assert!(arguments.contains("--timeout\n0\n"));
        assert!(arguments.contains("--rotation\n180\n"));
    }

    #[test]
    fn a_stalled_warm_stream_does_not_block_for_the_cold_start_timeout() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("camerad-stall-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let program = directory.join("fake-rpicam-vid");
        fs::write(
            &program,
            format!(
                "#!/bin/sh\nhead -c {} /dev/zero\nsleep 30\n",
                YUV420_FRAME_BYTES
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();

        let backend = RpicamBackend::new(&program);
        assert_eq!(wait_for_preview(&backend).len(), CAMERA_FRAME_BYTES);
        let started = Instant::now();
        loop {
            match backend.capture_rgb565() {
                Err(CameraCaptureError::TimedOut) => {}
                Err(CameraCaptureError::CaptureFailed) => break,
                other => panic!("unexpected stalled stream result: {other:?}"),
            }
        }
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn cold_start_timeouts_keep_the_same_process_and_partial_frame() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("camerad-cold-start-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let program = directory.join("fake-rpicam-vid");
        let starts = directory.join("starts");
        fs::write(&starts, b"").unwrap();
        fs::write(
            &program,
            format!(
                "#!/bin/sh\nprintf x >> '{}'\nhead -c {} /dev/zero\nsleep 0.7\nhead -c {} /dev/zero\nsleep 30\n",
                starts.display(),
                YUV420_FRAME_BYTES / 2,
                YUV420_FRAME_BYTES - YUV420_FRAME_BYTES / 2
            ),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();

        let backend = RpicamBackend::new(&program);
        let started = Instant::now();
        assert_eq!(backend.capture_rgb565(), Err(CameraCaptureError::TimedOut));
        assert!(started.elapsed() < Duration::from_millis(100));
        assert_eq!(wait_for_preview(&backend).len(), CAMERA_FRAME_BYTES);
        assert_eq!(fs::read(&starts).unwrap(), b"x");
    }

    #[test]
    fn server_returns_only_the_fixed_rgb565_frame() {
        let server = CameraServer::new(
            MockCamera {
                frame: [0x10, 0x84].repeat(CAMERA_FRAME_BYTES / 2),
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

    #[cfg(target_os = "linux")]
    #[test]
    fn server_packages_a_bounded_720p_photo_with_its_thumbnail() {
        let server = CameraServer::new(
            MockCamera {
                frame: vec![0x34, 0x12].repeat(CAMERA_FRAME_BYTES / 2),
            },
            [0],
        );
        let (descriptor, jpeg_size) = server.capture_photo_descriptor().unwrap();
        assert_eq!(jpeg_size, 5);
        let mut file = File::from(descriptor);
        let mut payload = Vec::new();
        file.read_to_end(&mut payload).unwrap();
        let (thumbnail, jpeg) = cp0_camera_protocol::decode_photo_payload(&payload).unwrap();
        assert_eq!(thumbnail.len(), CAMERA_FRAME_BYTES);
        assert_eq!(jpeg, [0xff, 0xd8, 0x42, 0xff, 0xd9]);
    }
}
