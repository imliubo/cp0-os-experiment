use std::fmt;
use std::io::{self, BufRead, Write};
use std::mem;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

pub const CAMERA_PROTOCOL_VERSION: u32 = 1;
pub const CAMERA_WIDTH: u16 = 320;
pub const CAMERA_HEIGHT: u16 = 170;
pub const CAMERA_PIXEL_BYTES: usize = 2;
pub const CAMERA_FRAME_BYTES: usize =
    CAMERA_WIDTH as usize * CAMERA_HEIGHT as usize * CAMERA_PIXEL_BYTES;
pub const CAMERA_PREVIEW_FPS: u16 = 30;
pub const CAMERA_PHOTO_WIDTH: u16 = 1280;
pub const CAMERA_PHOTO_HEIGHT: u16 = 720;
pub const MAX_CAMERA_JPEG_BYTES: usize = 4 * 1024 * 1024;
pub const CAMERA_PHOTO_HEADER_BYTES: usize = 16;
pub const CAMERA_PHOTO_MAGIC: [u8; 4] = *b"CP0J";
pub const MAX_CAMERA_PROTOCOL_FRAME_BYTES: usize = 2 * 1024;
pub const MAX_CAMERA_ERROR_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: CameraCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CameraCommand {
    CaptureRgb565,
    CapturePhoto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: CameraOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CameraOutcome {
    Captured {
        width: u16,
        height: u16,
        pixel_format: CameraPixelFormat,
        size_bytes: u32,
    },
    PhotoCaptured {
        width: u16,
        height: u16,
        thumbnail_size_bytes: u32,
        jpeg_size_bytes: u32,
    },
    Error {
        code: CameraErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraPixelFormat {
    Rgb565Le,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraErrorCode {
    InvalidRequest,
    Unauthorized,
    Busy,
    Unavailable,
    CaptureFailed,
    Internal,
}

#[derive(Debug)]
pub enum CameraProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnexpectedTrailingData,
    UnsupportedVersion(u32),
    InvalidFrameMetadata,
    InvalidErrorMessage,
    InvalidDescriptor,
}

impl fmt::Display for CameraProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "camera protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid camera protocol JSON: {error}"),
            Self::FrameTooLarge => write!(
                formatter,
                "camera protocol frame exceeds {MAX_CAMERA_PROTOCOL_FRAME_BYTES} bytes"
            ),
            Self::UnterminatedFrame => {
                formatter.write_str("camera protocol frame is not newline terminated")
            }
            Self::UnexpectedTrailingData => {
                formatter.write_str("camera protocol frame has trailing data")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported camera protocol version {version}")
            }
            Self::InvalidFrameMetadata => {
                formatter.write_str("camera response does not describe the fixed RGB565 frame")
            }
            Self::InvalidErrorMessage => {
                formatter.write_str("camera error message is empty, too long or contains controls")
            }
            Self::InvalidDescriptor => {
                formatter.write_str("invalid camera frame descriptor transfer")
            }
        }
    }
}

impl std::error::Error for CameraProtocolError {}

impl From<io::Error> for CameraProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for CameraProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl CameraRequest {
    pub const fn capture(request_id: u64) -> Self {
        Self {
            protocol_version: CAMERA_PROTOCOL_VERSION,
            request_id,
            command: CameraCommand::CaptureRgb565,
        }
    }

    pub const fn capture_photo(request_id: u64) -> Self {
        Self {
            protocol_version: CAMERA_PROTOCOL_VERSION,
            request_id,
            command: CameraCommand::CapturePhoto,
        }
    }

    pub fn validate(&self) -> Result<(), CameraProtocolError> {
        validate_version(self.protocol_version)
    }
}

impl CameraResponse {
    pub const fn captured(request_id: u64) -> Self {
        Self {
            protocol_version: CAMERA_PROTOCOL_VERSION,
            request_id,
            outcome: CameraOutcome::Captured {
                width: CAMERA_WIDTH,
                height: CAMERA_HEIGHT,
                pixel_format: CameraPixelFormat::Rgb565Le,
                size_bytes: CAMERA_FRAME_BYTES as u32,
            },
        }
    }

    pub const fn photo_captured(request_id: u64, jpeg_size_bytes: u32) -> Self {
        Self {
            protocol_version: CAMERA_PROTOCOL_VERSION,
            request_id,
            outcome: CameraOutcome::PhotoCaptured {
                width: CAMERA_PHOTO_WIDTH,
                height: CAMERA_PHOTO_HEIGHT,
                thumbnail_size_bytes: CAMERA_FRAME_BYTES as u32,
                jpeg_size_bytes,
            },
        }
    }

    pub fn error(request_id: u64, code: CameraErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: CAMERA_PROTOCOL_VERSION,
            request_id,
            outcome: CameraOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), CameraProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.outcome {
            CameraOutcome::Captured {
                width,
                height,
                pixel_format,
                size_bytes,
            } if *width != CAMERA_WIDTH
                || *height != CAMERA_HEIGHT
                || *pixel_format != CameraPixelFormat::Rgb565Le
                || *size_bytes != CAMERA_FRAME_BYTES as u32 =>
            {
                Err(CameraProtocolError::InvalidFrameMetadata)
            }
            CameraOutcome::PhotoCaptured {
                width,
                height,
                thumbnail_size_bytes,
                jpeg_size_bytes,
            } if *width != CAMERA_PHOTO_WIDTH
                || *height != CAMERA_PHOTO_HEIGHT
                || *thumbnail_size_bytes != CAMERA_FRAME_BYTES as u32
                || *jpeg_size_bytes == 0
                || *jpeg_size_bytes as usize > MAX_CAMERA_JPEG_BYTES =>
            {
                Err(CameraProtocolError::InvalidFrameMetadata)
            }
            CameraOutcome::Error { message, .. }
                if message.is_empty()
                    || message.chars().count() > MAX_CAMERA_ERROR_CHARS
                    || message.chars().any(char::is_control) =>
            {
                Err(CameraProtocolError::InvalidErrorMessage)
            }
            _ => Ok(()),
        }
    }
}

pub fn encode_photo_payload(thumbnail: &[u8], jpeg: &[u8]) -> Result<Vec<u8>, CameraProtocolError> {
    if thumbnail.len() != CAMERA_FRAME_BYTES
        || jpeg.is_empty()
        || jpeg.len() > MAX_CAMERA_JPEG_BYTES
        || !jpeg.starts_with(&[0xff, 0xd8])
        || !jpeg.ends_with(&[0xff, 0xd9])
    {
        return Err(CameraProtocolError::InvalidFrameMetadata);
    }
    let jpeg_size =
        u32::try_from(jpeg.len()).map_err(|_| CameraProtocolError::InvalidFrameMetadata)?;
    let mut payload = Vec::with_capacity(CAMERA_PHOTO_HEADER_BYTES + thumbnail.len() + jpeg.len());
    payload.extend_from_slice(&CAMERA_PHOTO_MAGIC);
    payload.extend_from_slice(&1_u16.to_le_bytes());
    payload.extend_from_slice(&(CAMERA_PHOTO_HEADER_BYTES as u16).to_le_bytes());
    payload.extend_from_slice(&(CAMERA_FRAME_BYTES as u32).to_le_bytes());
    payload.extend_from_slice(&jpeg_size.to_le_bytes());
    payload.extend_from_slice(thumbnail);
    payload.extend_from_slice(jpeg);
    Ok(payload)
}

pub fn decode_photo_payload(payload: &[u8]) -> Result<(&[u8], &[u8]), CameraProtocolError> {
    if payload.len() < CAMERA_PHOTO_HEADER_BYTES
        || payload[..4] != CAMERA_PHOTO_MAGIC
        || u16::from_le_bytes(payload[4..6].try_into().unwrap()) != 1
        || usize::from(u16::from_le_bytes(payload[6..8].try_into().unwrap()))
            != CAMERA_PHOTO_HEADER_BYTES
    {
        return Err(CameraProtocolError::InvalidFrameMetadata);
    }
    let thumbnail_size = u32::from_le_bytes(payload[8..12].try_into().unwrap()) as usize;
    let jpeg_size = u32::from_le_bytes(payload[12..16].try_into().unwrap()) as usize;
    let expected = CAMERA_PHOTO_HEADER_BYTES
        .checked_add(thumbnail_size)
        .and_then(|size| size.checked_add(jpeg_size))
        .ok_or(CameraProtocolError::InvalidFrameMetadata)?;
    if thumbnail_size != CAMERA_FRAME_BYTES
        || jpeg_size == 0
        || jpeg_size > MAX_CAMERA_JPEG_BYTES
        || payload.len() != expected
    {
        return Err(CameraProtocolError::InvalidFrameMetadata);
    }
    let thumbnail_end = CAMERA_PHOTO_HEADER_BYTES + thumbnail_size;
    let thumbnail = &payload[CAMERA_PHOTO_HEADER_BYTES..thumbnail_end];
    let jpeg = &payload[thumbnail_end..];
    if !jpeg.starts_with(&[0xff, 0xd8]) || !jpeg.ends_with(&[0xff, 0xd9]) {
        return Err(CameraProtocolError::InvalidFrameMetadata);
    }
    Ok((thumbnail, jpeg))
}

pub fn write_request(
    writer: &mut impl Write,
    request: &CameraRequest,
) -> Result<(), CameraProtocolError> {
    request.validate()?;
    write_value(writer, request)
}

pub fn read_request(
    reader: &mut impl BufRead,
) -> Result<Option<CameraRequest>, CameraProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: CameraRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn write_response(
    writer: &mut impl Write,
    response: &CameraResponse,
) -> Result<(), CameraProtocolError> {
    response.validate()?;
    write_value(writer, response)
}

pub fn decode_response(frame: &[u8]) -> Result<CameraResponse, CameraProtocolError> {
    let response: CameraResponse = serde_json::from_slice(frame)?;
    response.validate()?;
    Ok(response)
}

pub fn encode_frame(value: &impl Serialize) -> Result<Vec<u8>, CameraProtocolError> {
    let mut encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_CAMERA_PROTOCOL_FRAME_BYTES {
        return Err(CameraProtocolError::FrameTooLarge);
    }
    encoded.push(b'\n');
    Ok(encoded)
}

pub fn send_frame_with_fd(
    stream: &mut UnixStream,
    frame: &[u8],
    descriptor: BorrowedFd<'_>,
) -> Result<(), CameraProtocolError> {
    if frame.is_empty() || frame.len() > MAX_CAMERA_PROTOCOL_FRAME_BYTES || !frame.ends_with(b"\n")
    {
        return Err(CameraProtocolError::UnterminatedFrame);
    }
    let mut io_vector = libc::iovec {
        iov_base: frame.as_ptr().cast_mut().cast(),
        iov_len: frame.len(),
    };
    let control_length = unsafe { libc::CMSG_SPACE(mem::size_of::<RawFd>() as u32) } as usize;
    let control_words = control_length.div_ceil(mem::size_of::<usize>());
    let mut control = vec![0_usize; control_words];
    let mut message: libc::msghdr = unsafe { mem::zeroed() };
    message.msg_iov = &raw mut io_vector;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = control_length
        .try_into()
        .map_err(|_| CameraProtocolError::InvalidDescriptor)?;

    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        if header.is_null() {
            return Err(CameraProtocolError::InvalidDescriptor);
        }
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(mem::size_of::<RawFd>() as u32) as _;
        libc::CMSG_DATA(header)
            .cast::<RawFd>()
            .write(descriptor.as_raw_fd());
    }
    let count = loop {
        let result = unsafe { libc::sendmsg(stream.as_raw_fd(), &message, libc::MSG_NOSIGNAL) };
        if result < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if result < 0 {
            return Err(CameraProtocolError::Io(io::Error::last_os_error()));
        }
        break result as usize;
    };
    if count == 0 || count > frame.len() {
        return Err(CameraProtocolError::InvalidDescriptor);
    }
    stream.write_all(&frame[count..])?;
    stream.flush()?;
    Ok(())
}

pub fn recv_frame_with_fd(
    stream: &UnixStream,
) -> Result<(Vec<u8>, Option<OwnedFd>), CameraProtocolError> {
    let mut frame = [0_u8; MAX_CAMERA_PROTOCOL_FRAME_BYTES];
    let mut length = 0_usize;
    let mut received_fd = None;

    loop {
        if length == frame.len() {
            return Err(CameraProtocolError::FrameTooLarge);
        }
        let mut io_vector = libc::iovec {
            iov_base: frame[length..].as_mut_ptr().cast(),
            iov_len: frame.len() - length,
        };
        let control_length = unsafe { libc::CMSG_SPACE(mem::size_of::<RawFd>() as u32) } as usize;
        let control_words = control_length.div_ceil(mem::size_of::<usize>());
        let mut control = vec![0_usize; control_words];
        let mut message: libc::msghdr = unsafe { mem::zeroed() };
        message.msg_iov = &raw mut io_vector;
        message.msg_iovlen = 1;
        message.msg_control = control.as_mut_ptr().cast();
        message.msg_controllen = control_length
            .try_into()
            .map_err(|_| CameraProtocolError::InvalidDescriptor)?;

        let count = loop {
            let result = unsafe { libc::recvmsg(stream.as_raw_fd(), &raw mut message, 0) };
            if result < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if result < 0 {
                return Err(CameraProtocolError::Io(io::Error::last_os_error()));
            }
            break result as usize;
        };
        if count == 0 {
            return Err(CameraProtocolError::UnterminatedFrame);
        }
        unsafe {
            let mut header = libc::CMSG_FIRSTHDR(&message);
            while !header.is_null() {
                if (*header).cmsg_level == libc::SOL_SOCKET
                    && (*header).cmsg_type == libc::SCM_RIGHTS
                {
                    let header_bytes = libc::CMSG_LEN(0) as usize;
                    if ((*header).cmsg_len as usize) < header_bytes {
                        return Err(CameraProtocolError::InvalidDescriptor);
                    }
                    let data_bytes = (*header).cmsg_len as usize - header_bytes;
                    if data_bytes != mem::size_of::<RawFd>() || received_fd.is_some() {
                        close_control_descriptors(header);
                        return Err(CameraProtocolError::InvalidDescriptor);
                    }
                    let raw_fd = libc::CMSG_DATA(header).cast::<RawFd>().read();
                    if libc::fcntl(raw_fd, libc::F_SETFD, libc::FD_CLOEXEC) != 0 {
                        libc::close(raw_fd);
                        return Err(CameraProtocolError::Io(io::Error::last_os_error()));
                    }
                    received_fd = Some(OwnedFd::from_raw_fd(raw_fd));
                }
                header = libc::CMSG_NXTHDR(&message, header);
            }
        }
        if message.msg_flags & (libc::MSG_CTRUNC | libc::MSG_TRUNC) != 0 {
            return Err(CameraProtocolError::InvalidDescriptor);
        }

        length += count;
        if let Some(newline) = frame[..length].iter().position(|byte| *byte == b'\n') {
            if newline + 1 != length {
                return Err(CameraProtocolError::UnexpectedTrailingData);
            }
            return Ok((frame[..newline].to_vec(), received_fd));
        }
    }
}

unsafe fn close_control_descriptors(header: *mut libc::cmsghdr) {
    let header_bytes = unsafe { libc::CMSG_LEN(0) } as usize;
    let data_bytes = unsafe { (*header).cmsg_len as usize }.saturating_sub(header_bytes);
    let descriptor_count = data_bytes / mem::size_of::<RawFd>();
    for index in 0..descriptor_count {
        let descriptor = unsafe { libc::CMSG_DATA(header).cast::<RawFd>().add(index).read() };
        if descriptor >= 0 {
            unsafe { libc::close(descriptor) };
        }
    }
}

fn validate_version(version: u32) -> Result<(), CameraProtocolError> {
    if version == CAMERA_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(CameraProtocolError::UnsupportedVersion(version))
    }
}

fn write_value(writer: &mut impl Write, value: &impl Serialize) -> Result<(), CameraProtocolError> {
    let encoded = encode_frame(value)?;
    writer.write_all(&encoded)?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, CameraProtocolError> {
    let mut frame = Vec::with_capacity(128);
    let mut terminated = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let consumed = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |position| position + 1);
        if frame.len() + consumed > MAX_CAMERA_PROTOCOL_FRAME_BYTES {
            return Err(CameraProtocolError::FrameTooLarge);
        }
        terminated = available[consumed - 1] == b'\n';
        frame.extend_from_slice(&available[..consumed]);
        reader.consume(consumed);
        if terminated {
            break;
        }
    }
    if frame.is_empty() {
        return Ok(None);
    }
    if !terminated {
        return Err(CameraProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::os::fd::AsFd;

    use super::*;

    #[test]
    fn round_trips_fixed_capture_contract() {
        let request = CameraRequest::capture(7);
        let mut request_frame = Vec::new();
        write_request(&mut request_frame, &request).unwrap();
        assert_eq!(
            read_request(&mut Cursor::new(request_frame)).unwrap(),
            Some(request)
        );

        let response = CameraResponse::captured(7);
        let encoded = encode_frame(&response).unwrap();
        assert_eq!(
            decode_response(&encoded[..encoded.len() - 1]).unwrap(),
            response
        );

        let photo_request = CameraRequest::capture_photo(8);
        let mut photo_request_frame = Vec::new();
        write_request(&mut photo_request_frame, &photo_request).unwrap();
        assert_eq!(
            read_request(&mut Cursor::new(photo_request_frame)).unwrap(),
            Some(photo_request)
        );
        assert!(CameraResponse::photo_captured(8, 1024).validate().is_ok());
    }

    #[test]
    fn round_trips_bounded_photo_payload() {
        let thumbnail = vec![0x5a; CAMERA_FRAME_BYTES];
        let jpeg = vec![0xff, 0xd8, 1, 2, 3, 0xff, 0xd9];
        let payload = encode_photo_payload(&thumbnail, &jpeg).unwrap();
        let (decoded_thumbnail, decoded_jpeg) = decode_photo_payload(&payload).unwrap();
        assert_eq!(decoded_thumbnail, thumbnail);
        assert_eq!(decoded_jpeg, jpeg);

        let mut truncated = payload;
        truncated.pop();
        assert!(decode_photo_payload(&truncated).is_err());
    }

    #[test]
    fn rejects_non_fixed_metadata_and_oversized_frames() {
        let response = CameraResponse {
            protocol_version: CAMERA_PROTOCOL_VERSION,
            request_id: 1,
            outcome: CameraOutcome::Captured {
                width: 640,
                height: CAMERA_HEIGHT,
                pixel_format: CameraPixelFormat::Rgb565Le,
                size_bytes: CAMERA_FRAME_BYTES as u32,
            },
        };
        assert!(matches!(
            response.validate(),
            Err(CameraProtocolError::InvalidFrameMetadata)
        ));
        let mut oversized = vec![b'x'; MAX_CAMERA_PROTOCOL_FRAME_BYTES];
        oversized.push(b'\n');
        assert!(matches!(
            read_request(&mut Cursor::new(oversized)),
            Err(CameraProtocolError::FrameTooLarge)
        ));
    }

    #[test]
    fn transfers_exactly_one_cloexec_descriptor() {
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        let file = std::fs::File::open("Cargo.toml").unwrap();
        let frame = encode_frame(&CameraResponse::captured(9)).unwrap();
        send_frame_with_fd(&mut sender, &frame, file.as_fd()).unwrap();
        let (received_frame, descriptor) = recv_frame_with_fd(&receiver).unwrap();
        assert_eq!(decode_response(&received_frame).unwrap().request_id, 9);
        let descriptor = descriptor.unwrap();
        let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
    }
}
