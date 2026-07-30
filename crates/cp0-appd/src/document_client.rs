use std::fmt;
use std::net::Shutdown;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use cp0_document_protocol::{
    DOCUMENT_PROTOCOL_VERSION, DocumentCommand, DocumentErrorCode, DocumentOutcome,
    DocumentProtocolError, DocumentRequest, DocumentSummary, recv_frame_with_fd, write_request,
};

pub const DEFAULT_DOCUMENT_SOCKET: &str = "/run/cardputerzero-documentd/documents.sock";
const DOCUMENT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub struct OpenedDocument {
    pub summary: DocumentSummary,
    pub descriptor: OwnedFd,
}

#[derive(Debug)]
pub enum DocumentClientError {
    Io(std::io::Error),
    Protocol(DocumentProtocolError),
    EmptyResponse,
    MismatchedRequestId,
    MissingDescriptor,
    UnexpectedDescriptor,
    MismatchedDocument,
    Service(DocumentErrorCode),
}

impl fmt::Display for DocumentClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "document service I/O error: {error}"),
            Self::Protocol(error) => write!(formatter, "document service protocol error: {error}"),
            Self::EmptyResponse => formatter.write_str("document service returned no response"),
            Self::MismatchedRequestId => {
                formatter.write_str("document service returned a mismatched request ID")
            }
            Self::MissingDescriptor => {
                formatter.write_str("document service did not return a file descriptor")
            }
            Self::UnexpectedDescriptor => {
                formatter.write_str("document service returned an unexpected file descriptor")
            }
            Self::MismatchedDocument => {
                formatter.write_str("document service opened a different document")
            }
            Self::Service(code) => write!(formatter, "document service rejected request: {code:?}"),
        }
    }
}

impl std::error::Error for DocumentClientError {}

impl From<std::io::Error> for DocumentClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<DocumentProtocolError> for DocumentClientError {
    fn from(error: DocumentProtocolError) -> Self {
        Self::Protocol(error)
    }
}

#[derive(Debug, Clone)]
pub struct DocumentClient {
    socket_path: PathBuf,
}

impl Default for DocumentClient {
    fn default() -> Self {
        Self::new(DEFAULT_DOCUMENT_SOCKET)
    }
}

impl DocumentClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn list(&self, request_id: u64) -> Result<Vec<DocumentSummary>, DocumentClientError> {
        let (outcome, descriptor) = self.exchange(DocumentRequest {
            protocol_version: DOCUMENT_PROTOCOL_VERSION,
            request_id,
            command: DocumentCommand::List,
        })?;
        if descriptor.is_some() {
            return Err(DocumentClientError::UnexpectedDescriptor);
        }
        match outcome {
            DocumentOutcome::Documents { documents } => Ok(documents),
            DocumentOutcome::Error { code, .. } => Err(DocumentClientError::Service(code)),
            DocumentOutcome::Opened { .. } => Err(DocumentClientError::UnexpectedDescriptor),
        }
    }

    pub fn open(
        &self,
        request_id: u64,
        document_id: &str,
    ) -> Result<OpenedDocument, DocumentClientError> {
        let (outcome, descriptor) = self.exchange(DocumentRequest {
            protocol_version: DOCUMENT_PROTOCOL_VERSION,
            request_id,
            command: DocumentCommand::Open {
                document_id: document_id.into(),
            },
        })?;
        match outcome {
            DocumentOutcome::Opened { document } => {
                if document.document_id != document_id {
                    return Err(DocumentClientError::MismatchedDocument);
                }
                let descriptor = descriptor.ok_or(DocumentClientError::MissingDescriptor)?;
                Ok(OpenedDocument {
                    summary: document,
                    descriptor,
                })
            }
            DocumentOutcome::Error { code, .. } => {
                if descriptor.is_some() {
                    return Err(DocumentClientError::UnexpectedDescriptor);
                }
                Err(DocumentClientError::Service(code))
            }
            DocumentOutcome::Documents { .. } => Err(DocumentClientError::MissingDescriptor),
        }
    }

    fn exchange(
        &self,
        request: DocumentRequest,
    ) -> Result<(DocumentOutcome, Option<OwnedFd>), DocumentClientError> {
        let stream = UnixStream::connect(&self.socket_path)?;
        Self::exchange_stream(stream, request)
    }

    fn exchange_stream(
        mut stream: UnixStream,
        request: DocumentRequest,
    ) -> Result<(DocumentOutcome, Option<OwnedFd>), DocumentClientError> {
        stream.set_read_timeout(Some(DOCUMENT_TIMEOUT))?;
        stream.set_write_timeout(Some(DOCUMENT_TIMEOUT))?;
        write_request(&mut stream, &request)?;
        stream.shutdown(Shutdown::Write)?;
        let (frame, descriptor) = recv_frame_with_fd(&stream)?;
        if frame.is_empty() {
            return Err(DocumentClientError::EmptyResponse);
        }
        let response = cp0_document_protocol::decode_response(&frame)?;
        if response.request_id != request.request_id {
            return Err(DocumentClientError::MismatchedRequestId);
        }
        Ok((response.outcome, descriptor))
    }
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;
    use std::os::fd::AsFd;
    use std::os::unix::net::UnixStream;
    use std::thread;

    use cp0_document_protocol::{
        DocumentResponse, DocumentSummary, encode_frame, read_request, send_frame_with_fd,
    };

    use super::*;

    #[test]
    fn receives_only_the_descriptor_bound_to_the_open_response() {
        let summary = DocumentSummary {
            document_id: "00000000000000010000000000000002".into(),
            name: "hello.txt".into(),
            size_bytes: 5,
        };
        let service_summary = summary.clone();
        let (client_stream, mut service_stream) = UnixStream::pair().unwrap();
        let service = thread::spawn(move || {
            let mut reader = BufReader::new(service_stream.try_clone().unwrap());
            let open_request = read_request(&mut reader).unwrap().unwrap();
            let file = std::fs::File::open("Cargo.toml").unwrap();
            let response = DocumentResponse::opened(open_request.request_id, service_summary);
            send_frame_with_fd(
                &mut service_stream,
                &encode_frame(&response).unwrap(),
                file.as_fd(),
            )
            .unwrap();
        });

        let request = DocumentRequest {
            protocol_version: DOCUMENT_PROTOCOL_VERSION,
            request_id: 11,
            command: DocumentCommand::Open {
                document_id: summary.document_id.clone(),
            },
        };
        let (outcome, descriptor) =
            DocumentClient::exchange_stream(client_stream, request).unwrap();
        assert_eq!(outcome, DocumentOutcome::Opened { document: summary });
        assert!(descriptor.is_some());
        service.join().unwrap();
    }
}
