use std::fmt;
use std::io::{self, BufRead, Write};
use std::mem;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

pub const DOCUMENT_PROTOCOL_VERSION: u32 = 1;
pub const MAX_DOCUMENT_FRAME_BYTES: usize = 4 * 1024;
pub const MAX_DOCUMENTS: usize = 16;
pub const MAX_DOCUMENT_NAME_CHARS: usize = 48;
pub const MAX_DOCUMENT_NAME_BYTES: usize = 128;
pub const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;
pub const DOCUMENT_ID_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: DocumentCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DocumentCommand {
    List,
    Open { document_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: DocumentOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DocumentOutcome {
    Documents {
        documents: Vec<DocumentSummary>,
    },
    Opened {
        document: DocumentSummary,
    },
    Error {
        code: DocumentErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentSummary {
    pub document_id: String,
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DocumentErrorCode {
    InvalidRequest,
    Unauthorized,
    NotFound,
    ResourceExhausted,
    Internal,
}

#[derive(Debug)]
pub enum DocumentProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnexpectedTrailingData,
    UnsupportedVersion(u32),
    InvalidDocumentId,
    InvalidDocumentList,
    InvalidDescriptor,
}

impl fmt::Display for DocumentProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "document protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid document protocol JSON: {error}"),
            Self::FrameTooLarge => write!(
                formatter,
                "document protocol frame exceeds {MAX_DOCUMENT_FRAME_BYTES} bytes"
            ),
            Self::UnterminatedFrame => {
                formatter.write_str("document protocol frame is not newline terminated")
            }
            Self::UnexpectedTrailingData => {
                formatter.write_str("document protocol frame has trailing data")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported document protocol version {version}")
            }
            Self::InvalidDocumentId => formatter.write_str("invalid document ID"),
            Self::InvalidDocumentList => formatter.write_str("invalid bounded document list"),
            Self::InvalidDescriptor => {
                formatter.write_str("invalid document file descriptor transfer")
            }
        }
    }
}

impl std::error::Error for DocumentProtocolError {}

impl From<io::Error> for DocumentProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DocumentProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl DocumentRequest {
    pub fn validate(&self) -> Result<(), DocumentProtocolError> {
        validate_version(self.protocol_version)?;
        if let DocumentCommand::Open { document_id } = &self.command {
            if !is_valid_document_id(document_id) {
                return Err(DocumentProtocolError::InvalidDocumentId);
            }
        }
        Ok(())
    }
}

impl DocumentResponse {
    pub fn documents(request_id: u64, documents: Vec<DocumentSummary>) -> Self {
        Self {
            protocol_version: DOCUMENT_PROTOCOL_VERSION,
            request_id,
            outcome: DocumentOutcome::Documents { documents },
        }
    }

    pub fn opened(request_id: u64, document: DocumentSummary) -> Self {
        Self {
            protocol_version: DOCUMENT_PROTOCOL_VERSION,
            request_id,
            outcome: DocumentOutcome::Opened { document },
        }
    }

    pub fn error(request_id: u64, code: DocumentErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: DOCUMENT_PROTOCOL_VERSION,
            request_id,
            outcome: DocumentOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), DocumentProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.outcome {
            DocumentOutcome::Documents { documents } => {
                if documents.len() > MAX_DOCUMENTS
                    || documents.iter().any(|document| !document.validate())
                {
                    return Err(DocumentProtocolError::InvalidDocumentList);
                }
            }
            DocumentOutcome::Opened { document } if !document.validate() => {
                return Err(DocumentProtocolError::InvalidDocumentList);
            }
            _ => {}
        }
        Ok(())
    }
}

impl DocumentSummary {
    pub fn validate(&self) -> bool {
        is_valid_document_id(&self.document_id)
            && is_valid_document_name(&self.name)
            && self.size_bytes <= MAX_DOCUMENT_BYTES
    }
}

pub fn is_valid_document_id(value: &str) -> bool {
    value.len() == DOCUMENT_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn is_valid_document_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DOCUMENT_NAME_BYTES
        && value.chars().count() <= MAX_DOCUMENT_NAME_CHARS
        && !value.chars().any(char::is_control)
        && !value.contains('/')
}

pub fn read_request(
    reader: &mut impl BufRead,
) -> Result<Option<DocumentRequest>, DocumentProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: DocumentRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<DocumentResponse>, DocumentProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    decode_response(&frame).map(Some)
}

pub fn decode_response(frame: &[u8]) -> Result<DocumentResponse, DocumentProtocolError> {
    let response: DocumentResponse = serde_json::from_slice(frame)?;
    response.validate()?;
    Ok(response)
}

pub fn write_request(
    writer: &mut impl Write,
    request: &DocumentRequest,
) -> Result<(), DocumentProtocolError> {
    request.validate()?;
    write_value(writer, request)
}

pub fn write_response(
    writer: &mut impl Write,
    response: &DocumentResponse,
) -> Result<(), DocumentProtocolError> {
    response.validate()?;
    write_value(writer, response)
}

pub fn encode_frame(value: &impl Serialize) -> Result<Vec<u8>, DocumentProtocolError> {
    let mut encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_DOCUMENT_FRAME_BYTES {
        return Err(DocumentProtocolError::FrameTooLarge);
    }
    encoded.push(b'\n');
    Ok(encoded)
}

pub fn send_frame_with_fd(
    stream: &mut UnixStream,
    frame: &[u8],
    descriptor: BorrowedFd<'_>,
) -> Result<(), DocumentProtocolError> {
    if frame.is_empty() || frame.len() > MAX_DOCUMENT_FRAME_BYTES || !frame.ends_with(b"\n") {
        return Err(DocumentProtocolError::UnterminatedFrame);
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
        .map_err(|_| DocumentProtocolError::InvalidDescriptor)?;

    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        if header.is_null() {
            return Err(DocumentProtocolError::InvalidDescriptor);
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
            return Err(DocumentProtocolError::Io(io::Error::last_os_error()));
        }
        break result as usize;
    };
    if count == 0 || count > frame.len() {
        return Err(DocumentProtocolError::InvalidDescriptor);
    }
    stream.write_all(&frame[count..])?;
    stream.flush()?;
    Ok(())
}

pub fn recv_frame_with_fd(
    stream: &UnixStream,
) -> Result<(Vec<u8>, Option<OwnedFd>), DocumentProtocolError> {
    let mut frame = [0_u8; MAX_DOCUMENT_FRAME_BYTES];
    let mut length = 0_usize;
    let mut received_fd = None;

    loop {
        if length == frame.len() {
            return Err(DocumentProtocolError::FrameTooLarge);
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
            .map_err(|_| DocumentProtocolError::InvalidDescriptor)?;

        let count = loop {
            let result = unsafe { libc::recvmsg(stream.as_raw_fd(), &raw mut message, 0) };
            if result < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if result < 0 {
                return Err(DocumentProtocolError::Io(io::Error::last_os_error()));
            }
            break result as usize;
        };
        if count == 0 {
            return Err(DocumentProtocolError::UnterminatedFrame);
        }
        unsafe {
            let mut header = libc::CMSG_FIRSTHDR(&message);
            while !header.is_null() {
                if (*header).cmsg_level == libc::SOL_SOCKET
                    && (*header).cmsg_type == libc::SCM_RIGHTS
                {
                    let header_bytes = libc::CMSG_LEN(0) as usize;
                    if ((*header).cmsg_len as usize) < header_bytes {
                        return Err(DocumentProtocolError::InvalidDescriptor);
                    }
                    let data_bytes = (*header).cmsg_len as usize - header_bytes;
                    if data_bytes != mem::size_of::<RawFd>() || received_fd.is_some() {
                        return Err(DocumentProtocolError::InvalidDescriptor);
                    }
                    let raw_fd = libc::CMSG_DATA(header).cast::<RawFd>().read();
                    if libc::fcntl(raw_fd, libc::F_SETFD, libc::FD_CLOEXEC) != 0 {
                        libc::close(raw_fd);
                        return Err(DocumentProtocolError::Io(io::Error::last_os_error()));
                    }
                    received_fd = Some(OwnedFd::from_raw_fd(raw_fd));
                }
                header = libc::CMSG_NXTHDR(&message, header);
            }
        }
        if message.msg_flags & (libc::MSG_CTRUNC | libc::MSG_TRUNC) != 0 {
            return Err(DocumentProtocolError::InvalidDescriptor);
        }

        length += count;
        if let Some(newline) = frame[..length].iter().position(|byte| *byte == b'\n') {
            if newline + 1 != length {
                return Err(DocumentProtocolError::UnexpectedTrailingData);
            }
            return Ok((frame[..newline].to_vec(), received_fd));
        }
    }
}

fn validate_version(version: u32) -> Result<(), DocumentProtocolError> {
    if version == DOCUMENT_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(DocumentProtocolError::UnsupportedVersion(version))
    }
}

fn write_value(
    writer: &mut impl Write,
    value: &impl Serialize,
) -> Result<(), DocumentProtocolError> {
    let encoded = encode_frame(value)?;
    writer.write_all(&encoded)?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, DocumentProtocolError> {
    let mut frame = Vec::with_capacity(256);
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
        if frame.len() + consumed > MAX_DOCUMENT_FRAME_BYTES {
            return Err(DocumentProtocolError::FrameTooLarge);
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
        return Err(DocumentProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::os::fd::{AsFd, AsRawFd};
    use std::os::unix::net::UnixStream;

    use super::*;

    fn summary() -> DocumentSummary {
        DocumentSummary {
            document_id: "00000000000000010000000000000002".into(),
            name: "notes.txt".into(),
            size_bytes: 17,
        }
    }

    #[test]
    fn validates_ids_names_and_bounds() {
        assert!(summary().validate());
        assert!(!is_valid_document_id("../etc/passwd"));
        assert!(!is_valid_document_id("0000000000000001000000000000000G"));
        assert!(!is_valid_document_name("sub/path.txt"));
        assert!(!is_valid_document_name("bad\nname"));
    }

    #[test]
    fn round_trips_strict_protocol() {
        let request = DocumentRequest {
            protocol_version: 1,
            request_id: 7,
            command: DocumentCommand::Open {
                document_id: summary().document_id,
            },
        };
        let mut encoded = Vec::new();
        write_request(&mut encoded, &request).unwrap();
        assert_eq!(
            read_request(&mut Cursor::new(encoded)).unwrap(),
            Some(request)
        );
    }

    #[test]
    fn transfers_exactly_one_cloexec_descriptor() {
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        let file = std::fs::File::open("Cargo.toml").unwrap();
        let response = DocumentResponse::opened(9, summary());
        let frame = encode_frame(&response).unwrap();
        send_frame_with_fd(&mut sender, &frame, file.as_fd()).unwrap();
        let (received, descriptor) = recv_frame_with_fd(&receiver).unwrap();
        assert_eq!(decode_response(&received).unwrap(), response);
        let descriptor = descriptor.unwrap();
        assert!(descriptor.as_raw_fd() >= 0);
        let flags = unsafe { libc::fcntl(descriptor.as_raw_fd(), libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0);
    }
}
