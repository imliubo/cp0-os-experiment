use std::collections::BTreeSet;
use std::ffi::CString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use cp0_document_protocol::{
    DocumentCommand, DocumentErrorCode, DocumentProtocolError, DocumentRequest, DocumentResponse,
    DocumentSummary, MAX_DOCUMENT_BYTES, MAX_DOCUMENTS, encode_frame, is_valid_document_id,
    is_valid_document_name, read_request, send_frame_with_fd, write_response,
};

pub const DEFAULT_DOCUMENT_ROOT: &str = "/var/lib/cardputerzero/documents";
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub enum DocumentStoreError {
    Io(io::Error),
    InvalidRoot,
    NotFound,
    InvalidDocument,
}

impl fmt::Display for DocumentStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "document store I/O error: {error}"),
            Self::InvalidRoot => formatter
                .write_str("document root must be an absolute, non-symbolic-link directory"),
            Self::NotFound => formatter.write_str("document does not exist"),
            Self::InvalidDocument => formatter.write_str(
                "document must be a bounded regular file directly inside the document root",
            ),
        }
    }
}

impl std::error::Error for DocumentStoreError {}

impl From<io::Error> for DocumentStoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug)]
pub struct OpenedDocument {
    pub summary: DocumentSummary,
    pub descriptor: OwnedFd,
}

#[derive(Debug, Clone)]
pub struct DocumentStore {
    root: PathBuf,
}

impl Default for DocumentStore {
    fn default() -> Self {
        Self::new(DEFAULT_DOCUMENT_ROOT)
    }
}

impl DocumentStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn list(&self) -> Result<Vec<DocumentSummary>, DocumentStoreError> {
        self.validate_root()?;
        let mut documents = Vec::new();
        let mut seen = BTreeSet::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let Some(name) = entry.file_name().into_string().ok() else {
                continue;
            };
            if !is_valid_document_name(&name) {
                continue;
            }
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if !metadata.file_type().is_file() || metadata.len() > MAX_DOCUMENT_BYTES {
                continue;
            }
            let document_id = metadata_id(&metadata);
            if seen.insert(document_id.clone()) {
                documents.push(DocumentSummary {
                    document_id,
                    name,
                    size_bytes: metadata.len(),
                });
            }
        }
        documents.sort_by(|left, right| {
            left.name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.document_id.cmp(&right.document_id))
        });
        documents.truncate(MAX_DOCUMENTS);
        Ok(documents)
    }

    pub fn open(&self, document_id: &str) -> Result<OpenedDocument, DocumentStoreError> {
        if !is_valid_document_id(document_id) {
            return Err(DocumentStoreError::NotFound);
        }
        self.validate_root()?;
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&self.root)?;

        for entry in fs::read_dir(&self.root)? {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let Some(name) = entry.file_name().into_string().ok() else {
                continue;
            };
            if !is_valid_document_name(&name) {
                continue;
            }
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if !metadata.file_type().is_file()
                || metadata.len() > MAX_DOCUMENT_BYTES
                || metadata_id(&metadata) != document_id
            {
                continue;
            }
            let c_name = CString::new(entry.file_name().as_bytes())
                .map_err(|_| DocumentStoreError::InvalidDocument)?;
            let raw_descriptor = unsafe {
                libc::openat(
                    root.as_raw_fd(),
                    c_name.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if raw_descriptor < 0 {
                let error = io::Error::last_os_error();
                if matches!(error.raw_os_error(), Some(libc::ENOENT) | Some(libc::ELOOP)) {
                    continue;
                }
                return Err(DocumentStoreError::Io(error));
            }
            let descriptor = unsafe { OwnedFd::from_raw_fd(raw_descriptor) };
            let opened_metadata = File::from(descriptor.try_clone()?).metadata()?;
            if !opened_metadata.file_type().is_file()
                || opened_metadata.len() > MAX_DOCUMENT_BYTES
                || metadata_id(&opened_metadata) != document_id
            {
                return Err(DocumentStoreError::InvalidDocument);
            }
            return Ok(OpenedDocument {
                summary: DocumentSummary {
                    document_id: document_id.into(),
                    name,
                    size_bytes: opened_metadata.len(),
                },
                descriptor,
            });
        }
        Err(DocumentStoreError::NotFound)
    }

    fn validate_root(&self) -> Result<(), DocumentStoreError> {
        if !self.root.is_absolute() {
            return Err(DocumentStoreError::InvalidRoot);
        }
        let metadata = fs::symlink_metadata(&self.root)?;
        if !metadata.file_type().is_dir() {
            return Err(DocumentStoreError::InvalidRoot);
        }
        Ok(())
    }
}

fn metadata_id(metadata: &fs::Metadata) -> String {
    format!("{:016x}{:016x}", metadata.dev(), metadata.ino())
}

#[derive(Debug)]
pub struct DocumentServer {
    store: DocumentStore,
    trusted_uids: BTreeSet<u32>,
}

impl DocumentServer {
    pub fn new(store: DocumentStore, trusted_uids: impl IntoIterator<Item = u32>) -> Self {
        Self {
            store,
            trusted_uids: trusted_uids.into_iter().collect(),
        }
    }

    pub fn serve(&self, listener: UnixListener) -> io::Result<()> {
        loop {
            let (stream, _) = listener.accept()?;
            if let Err(error) = self.handle_connection(stream) {
                eprintln!("cp0-documentd: rejected connection: {error}");
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
                    &DocumentResponse::error(
                        0,
                        DocumentErrorCode::InvalidRequest,
                        "invalid document service request",
                    ),
                )
                .map_err(protocol_io)?;
                eprintln!("cp0-documentd: invalid request: {error}");
                return Ok(());
            }
        };
        if !self.trusted_uids.contains(&uid) {
            return write_response(
                &mut stream,
                &DocumentResponse::error(
                    request.request_id,
                    DocumentErrorCode::Unauthorized,
                    "peer UID is not authorized to use the document service",
                ),
            )
            .map_err(protocol_io);
        }
        self.dispatch_to_stream(request, &mut stream)
    }

    fn dispatch_to_stream(
        &self,
        request: DocumentRequest,
        stream: &mut UnixStream,
    ) -> io::Result<()> {
        let request_id = request.request_id;
        match request.command {
            DocumentCommand::List => {
                let response = match self.store.list() {
                    Ok(documents) => DocumentResponse::documents(request_id, documents),
                    Err(error) => store_error_response(request_id, &error),
                };
                write_response(stream, &response).map_err(protocol_io)
            }
            DocumentCommand::Open { document_id } => match self.store.open(&document_id) {
                Ok(opened) => {
                    let response = DocumentResponse::opened(request_id, opened.summary);
                    let frame = encode_frame(&response).map_err(protocol_io)?;
                    send_frame_with_fd(stream, &frame, opened.descriptor.as_fd())
                        .map_err(protocol_io)
                }
                Err(error) => write_response(stream, &store_error_response(request_id, &error))
                    .map_err(protocol_io),
            },
        }
    }

    pub fn dispatch(&self, request: DocumentRequest) -> DocumentResponse {
        let request_id = request.request_id;
        match request.command {
            DocumentCommand::List => self.store.list().map_or_else(
                |error| store_error_response(request_id, &error),
                |documents| DocumentResponse::documents(request_id, documents),
            ),
            DocumentCommand::Open { document_id } => self.store.open(&document_id).map_or_else(
                |error| store_error_response(request_id, &error),
                |opened| DocumentResponse::opened(request_id, opened.summary),
            ),
        }
    }
}

fn store_error_response(request_id: u64, error: &DocumentStoreError) -> DocumentResponse {
    let (code, message) = match error {
        DocumentStoreError::NotFound => (DocumentErrorCode::NotFound, "document does not exist"),
        DocumentStoreError::InvalidDocument => (
            DocumentErrorCode::InvalidRequest,
            "document is not a bounded regular file",
        ),
        DocumentStoreError::InvalidRoot | DocumentStoreError::Io(_) => (
            DocumentErrorCode::Internal,
            "document service storage is unavailable",
        ),
    };
    DocumentResponse::error(request_id, code, message)
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

fn protocol_io(error: DocumentProtocolError) -> io::Error {
    match error {
        DocumentProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::unix::fs::symlink;

    use cp0_document_protocol::{DOCUMENT_PROTOCOL_VERSION, DocumentOutcome};

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("documentd-{name}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir_all(&path).unwrap();
        fs::canonicalize(path).unwrap()
    }

    #[test]
    fn lists_only_bounded_direct_regular_files() {
        let root = fixture("list");
        fs::write(root.join("notes.txt"), b"hello").unwrap();
        fs::create_dir(root.join("folder")).unwrap();
        symlink("notes.txt", root.join("alias.txt")).unwrap();
        fs::write(root.join("bad\nname"), b"bad").unwrap();

        let documents = DocumentStore::new(&root).list().unwrap();
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].name, "notes.txt");
        assert_eq!(documents[0].size_bytes, 5);
    }

    #[test]
    fn opens_selected_inode_read_only_and_rejects_forged_ids() {
        let root = fixture("open");
        fs::write(root.join("notes.txt"), b"trusted content").unwrap();
        let store = DocumentStore::new(&root);
        let summary = store.list().unwrap().remove(0);
        let opened = store.open(&summary.document_id).unwrap();
        let mut file = File::from(opened.descriptor);
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        assert_eq!(content, "trusted content");
        assert!(file.write_all(b"no").is_err());
        assert!(matches!(
            store.open("00000000000000000000000000000000"),
            Err(DocumentStoreError::NotFound)
        ));
        file.seek(SeekFrom::Start(0)).unwrap();
    }

    #[test]
    fn detects_symlink_swap_before_open() {
        let root = fixture("swap");
        let outside = root
            .parent()
            .unwrap()
            .join(format!("documentd-outside-{}", std::process::id()));
        fs::write(&outside, b"outside").unwrap();
        fs::write(root.join("selected.txt"), b"inside").unwrap();
        let store = DocumentStore::new(&root);
        let id = store.list().unwrap().remove(0).document_id;
        fs::remove_file(root.join("selected.txt")).unwrap();
        symlink(&outside, root.join("selected.txt")).unwrap();
        assert!(matches!(store.open(&id), Err(DocumentStoreError::NotFound)));
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn dispatch_never_returns_a_path() {
        let root = fixture("dispatch");
        fs::write(root.join("report.txt"), b"report").unwrap();
        let server = DocumentServer::new(DocumentStore::new(&root), [0]);
        let response = server.dispatch(DocumentRequest {
            protocol_version: DOCUMENT_PROTOCOL_VERSION,
            request_id: 4,
            command: DocumentCommand::List,
        });
        match response.outcome {
            DocumentOutcome::Documents { documents } => {
                assert_eq!(documents.len(), 1);
                assert!(!documents[0].document_id.contains('/'));
                assert_eq!(documents[0].name, "report.txt");
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }
}
