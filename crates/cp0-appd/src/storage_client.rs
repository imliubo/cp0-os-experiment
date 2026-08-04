use std::fmt;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cp0_storage_protocol::{
    StorageErrorCode, StorageOutcome, StorageProtocolError, StorageRequest, decode_value,
    read_response, write_request,
};

pub const DEFAULT_STORAGE_SOCKET: &str = "/run/cardputerzero-storaged/storage.sock";
const STORAGE_SERVICE_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug)]
pub enum StorageClientError {
    Io(std::io::Error),
    Protocol(StorageProtocolError),
    EmptyResponse,
    MismatchedRequestId,
    MismatchedOutcome,
    Service(StorageErrorCode),
}

impl fmt::Display for StorageClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "storage service I/O error: {error}"),
            Self::Protocol(error) => write!(formatter, "storage service protocol error: {error}"),
            Self::EmptyResponse => formatter.write_str("storage service returned no response"),
            Self::MismatchedRequestId => {
                formatter.write_str("storage service returned a mismatched request ID")
            }
            Self::MismatchedOutcome => {
                formatter.write_str("storage service returned a mismatched operation")
            }
            Self::Service(code) => write!(formatter, "storage service rejected request: {code:?}"),
        }
    }
}

impl std::error::Error for StorageClientError {}

impl From<std::io::Error> for StorageClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<StorageProtocolError> for StorageClientError {
    fn from(error: StorageProtocolError) -> Self {
        Self::Protocol(error)
    }
}

#[derive(Debug, Clone)]
pub struct StorageClient {
    socket_path: PathBuf,
}

impl Default for StorageClient {
    fn default() -> Self {
        Self::new(DEFAULT_STORAGE_SOCKET)
    }
}

impl StorageClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub fn put(
        &self,
        request_id: u64,
        app_id: &str,
        quota_bytes: u64,
        key: &str,
        value: &[u8],
    ) -> Result<u64, StorageClientError> {
        match self.exchange(&StorageRequest::put(
            request_id,
            app_id,
            quota_bytes,
            key,
            value,
        ))? {
            StorageOutcome::Stored { used_bytes } => Ok(used_bytes),
            StorageOutcome::Error { code, .. } => Err(StorageClientError::Service(code)),
            _ => Err(StorageClientError::MismatchedOutcome),
        }
    }

    pub fn get(
        &self,
        request_id: u64,
        app_id: &str,
        quota_bytes: u64,
        key: &str,
    ) -> Result<Option<Vec<u8>>, StorageClientError> {
        match self.exchange(&StorageRequest::get(request_id, app_id, quota_bytes, key))? {
            StorageOutcome::Value { value_base64 } => Ok(Some(decode_value(&value_base64)?)),
            StorageOutcome::NotFound => Ok(None),
            StorageOutcome::Error { code, .. } => Err(StorageClientError::Service(code)),
            _ => Err(StorageClientError::MismatchedOutcome),
        }
    }

    pub fn delete(
        &self,
        request_id: u64,
        app_id: &str,
        quota_bytes: u64,
        key: &str,
    ) -> Result<bool, StorageClientError> {
        match self.exchange(&StorageRequest::delete(
            request_id,
            app_id,
            quota_bytes,
            key,
        ))? {
            StorageOutcome::Deleted { existed, .. } => Ok(existed),
            StorageOutcome::Error { code, .. } => Err(StorageClientError::Service(code)),
            _ => Err(StorageClientError::MismatchedOutcome),
        }
    }

    pub fn usage(
        &self,
        request_id: u64,
        app_id: &str,
        quota_bytes: u64,
    ) -> Result<u64, StorageClientError> {
        match self.exchange(&StorageRequest::usage(request_id, app_id, quota_bytes))? {
            StorageOutcome::Usage { used_bytes } => Ok(used_bytes),
            StorageOutcome::Error { code, .. } => Err(StorageClientError::Service(code)),
            _ => Err(StorageClientError::MismatchedOutcome),
        }
    }

    pub fn put_blob_chunk(
        &self,
        request_id: u64,
        app_id: &str,
        quota_bytes: u64,
        key: &str,
        offset: u32,
        total_bytes: u32,
        value: &[u8],
    ) -> Result<u64, StorageClientError> {
        match self.exchange(&StorageRequest::put_blob_chunk(
            request_id,
            app_id,
            quota_bytes,
            key,
            offset,
            total_bytes,
            value,
        ))? {
            StorageOutcome::Stored { used_bytes } => Ok(used_bytes),
            StorageOutcome::Error { code, .. } => Err(StorageClientError::Service(code)),
            _ => Err(StorageClientError::MismatchedOutcome),
        }
    }

    pub fn get_blob_chunk(
        &self,
        request_id: u64,
        app_id: &str,
        quota_bytes: u64,
        key: &str,
        offset: u32,
        length: u32,
    ) -> Result<Option<Vec<u8>>, StorageClientError> {
        match self.exchange(&StorageRequest::get_blob_chunk(
            request_id,
            app_id,
            quota_bytes,
            key,
            offset,
            length,
        ))? {
            StorageOutcome::Value { value_base64 } => Ok(Some(decode_value(&value_base64)?)),
            StorageOutcome::NotFound => Ok(None),
            StorageOutcome::Error { code, .. } => Err(StorageClientError::Service(code)),
            _ => Err(StorageClientError::MismatchedOutcome),
        }
    }

    pub fn delete_blob(
        &self,
        request_id: u64,
        app_id: &str,
        quota_bytes: u64,
        key: &str,
    ) -> Result<bool, StorageClientError> {
        match self.exchange(&StorageRequest::delete_blob(
            request_id,
            app_id,
            quota_bytes,
            key,
        ))? {
            StorageOutcome::Deleted { existed, .. } => Ok(existed),
            StorageOutcome::Error { code, .. } => Err(StorageClientError::Service(code)),
            _ => Err(StorageClientError::MismatchedOutcome),
        }
    }

    fn exchange(&self, request: &StorageRequest) -> Result<StorageOutcome, StorageClientError> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(STORAGE_SERVICE_TIMEOUT))?;
        stream.set_write_timeout(Some(STORAGE_SERVICE_TIMEOUT))?;
        write_request(&mut stream, request)?;
        let response = read_response(&mut BufReader::new(stream.try_clone()?))?
            .ok_or(StorageClientError::EmptyResponse)?;
        if response.request_id != request.request_id {
            return Err(StorageClientError::MismatchedRequestId);
        }
        Ok(response.outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use cp0_storage_protocol::{MIB, StorageResponse, read_request, write_response};

    use super::*;

    #[test]
    fn correlates_identity_key_and_request_id() {
        let (mut client, mut service) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let request = read_request(&mut BufReader::new(service.try_clone().unwrap()))
                .unwrap()
                .unwrap();
            assert_eq!(request.app_id, "dev.cardputerzero.test");
            write_response(
                &mut service,
                &StorageResponse::value(request.request_id, b"state"),
            )
            .unwrap();
        });
        let request = StorageRequest::get(11, "dev.cardputerzero.test", MIB, "key");
        write_request(&mut client, &request).unwrap();
        let response = read_response(&mut BufReader::new(client.try_clone().unwrap()))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
        assert_eq!(response.request_id, 11);
        assert!(matches!(response.outcome, StorageOutcome::Value { .. }));
    }

    #[test]
    fn returns_only_usage_bound_to_the_request() {
        let (mut client, mut service) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let request = read_request(&mut BufReader::new(service.try_clone().unwrap()))
                .unwrap()
                .unwrap();
            assert!(matches!(
                request.command,
                cp0_storage_protocol::StorageCommand::Usage
            ));
            write_response(
                &mut service,
                &StorageResponse::usage(request.request_id, 4096),
            )
            .unwrap();
        });
        let request = StorageRequest::usage(12, "dev.cardputerzero.test", MIB);
        write_request(&mut client, &request).unwrap();
        let response = read_response(&mut BufReader::new(client.try_clone().unwrap()))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
        assert_eq!(response.request_id, 12);
        assert_eq!(response.outcome, StorageOutcome::Usage { used_bytes: 4096 });
    }
}
