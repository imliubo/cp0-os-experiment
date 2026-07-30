use std::collections::VecDeque;
use std::fmt;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

pub const BROKER_PROTOCOL_VERSION: u32 = 1;
pub const MAX_BROKER_FRAME_BYTES: usize = 4 * 1024;
pub const MAX_NOTIFICATION_TITLE_CHARS: usize = 32;
pub const MAX_NOTIFICATION_BODY_CHARS: usize = 160;
pub const MAX_PENDING_NOTIFICATIONS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrokerRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: BrokerCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum BrokerCommand {
    PostNotification { title: String, body: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrokerResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: BrokerOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum BrokerOutcome {
    Ok {
        notification_id: u64,
    },
    PermissionPending {
        prompt_id: u64,
    },
    Error {
        code: BrokerErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrokerErrorCode {
    InvalidRequest,
    Unauthorized,
    Undeclared,
    Denied,
    ResourceExhausted,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notification {
    pub notification_id: u64,
    pub app_id: String,
    pub app_name: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug)]
pub enum BrokerProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidNotification,
}

impl fmt::Display for BrokerProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "broker protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid broker protocol JSON: {error}"),
            Self::FrameTooLarge => write!(
                formatter,
                "broker protocol frame exceeds {MAX_BROKER_FRAME_BYTES} bytes"
            ),
            Self::UnterminatedFrame => {
                formatter.write_str("broker protocol frame is not newline terminated")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported broker protocol version {version}")
            }
            Self::InvalidNotification => formatter.write_str(
                "notification title/body is empty, too long or contains control characters",
            ),
        }
    }
}

impl std::error::Error for BrokerProtocolError {}

impl From<io::Error> for BrokerProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for BrokerProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl BrokerRequest {
    pub fn validate(&self) -> Result<(), BrokerProtocolError> {
        if self.protocol_version != BROKER_PROTOCOL_VERSION {
            return Err(BrokerProtocolError::UnsupportedVersion(
                self.protocol_version,
            ));
        }
        match &self.command {
            BrokerCommand::PostNotification { title, body }
                if !(1..=MAX_NOTIFICATION_TITLE_CHARS).contains(&title.chars().count())
                    || body.chars().count() > MAX_NOTIFICATION_BODY_CHARS
                    || title.chars().chain(body.chars()).any(char::is_control) =>
            {
                Err(BrokerProtocolError::InvalidNotification)
            }
            _ => Ok(()),
        }
    }
}

impl BrokerResponse {
    pub fn success(request_id: u64, notification_id: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::Ok { notification_id },
        }
    }

    pub fn permission_pending(request_id: u64, prompt_id: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::PermissionPending { prompt_id },
        }
    }

    pub fn error(request_id: u64, code: BrokerErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }
}

pub fn read_broker_request(
    reader: &mut impl BufRead,
) -> Result<Option<BrokerRequest>, BrokerProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: BrokerRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn read_broker_response(
    reader: &mut impl BufRead,
) -> Result<Option<BrokerResponse>, BrokerProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: BrokerResponse = serde_json::from_slice(&frame)?;
    if response.protocol_version != BROKER_PROTOCOL_VERSION {
        return Err(BrokerProtocolError::UnsupportedVersion(
            response.protocol_version,
        ));
    }
    Ok(Some(response))
}

pub fn write_broker_request(
    writer: &mut impl Write,
    request: &BrokerRequest,
) -> Result<(), BrokerProtocolError> {
    request.validate()?;
    write_value(writer, request)
}

pub fn write_broker_response(
    writer: &mut impl Write,
    response: &BrokerResponse,
) -> Result<(), BrokerProtocolError> {
    write_value(writer, response)
}

fn write_value(writer: &mut impl Write, value: &impl Serialize) -> Result<(), BrokerProtocolError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_BROKER_FRAME_BYTES {
        return Err(BrokerProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, BrokerProtocolError> {
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
        if frame.len() + consumed > MAX_BROKER_FRAME_BYTES {
            return Err(BrokerProtocolError::FrameTooLarge);
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
        return Err(BrokerProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[derive(Debug, Default)]
pub struct NotificationQueue {
    notifications: VecDeque<Notification>,
    next_notification_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationQueueError {
    Full,
}

impl NotificationQueue {
    pub fn enqueue(
        &mut self,
        app_id: &str,
        app_name: &str,
        title: String,
        body: String,
    ) -> Result<u64, NotificationQueueError> {
        if self.notifications.len() >= MAX_PENDING_NOTIFICATIONS {
            return Err(NotificationQueueError::Full);
        }
        self.next_notification_id = self.next_notification_id.wrapping_add(1).max(1);
        let notification_id = self.next_notification_id;
        self.notifications.push_back(Notification {
            notification_id,
            app_id: app_id.into(),
            app_name: app_name.into(),
            title,
            body,
        });
        Ok(notification_id)
    }

    pub fn take(&mut self) -> Option<Notification> {
        self.notifications.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn request(title: &str, body: &str) -> BrokerRequest {
        BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 1,
            command: BrokerCommand::PostNotification {
                title: title.into(),
                body: body.into(),
            },
        }
    }

    #[test]
    fn parses_strict_bounded_notification_request() {
        let encoded = serde_json::to_vec(&request("Ready", "Build complete")).unwrap();
        let mut framed = encoded;
        framed.push(b'\n');
        assert_eq!(
            read_broker_request(&mut Cursor::new(framed)).unwrap(),
            Some(request("Ready", "Build complete"))
        );
        let mut round_trip = Vec::new();
        write_broker_request(&mut round_trip, &request("Ready", "Build complete")).unwrap();
        assert_eq!(
            read_broker_request(&mut Cursor::new(round_trip)).unwrap(),
            Some(request("Ready", "Build complete"))
        );
        let response = BrokerResponse::permission_pending(1, 7);
        let mut encoded_response = Vec::new();
        write_broker_response(&mut encoded_response, &response).unwrap();
        assert_eq!(
            read_broker_response(&mut Cursor::new(encoded_response)).unwrap(),
            Some(response)
        );
        assert!(matches!(
            request("", "body").validate(),
            Err(BrokerProtocolError::InvalidNotification)
        ));
        assert!(matches!(
            request("title", "bad\nbody").validate(),
            Err(BrokerProtocolError::InvalidNotification)
        ));
    }

    #[test]
    fn bounds_notification_queue_and_preserves_identity() {
        let mut queue = NotificationQueue::default();
        for index in 0..MAX_PENDING_NOTIFICATIONS {
            queue
                .enqueue(
                    "dev.cardputerzero.hello",
                    "Hello",
                    format!("Title {index}"),
                    "Body".into(),
                )
                .unwrap();
        }
        assert!(
            queue
                .enqueue(
                    "dev.cardputerzero.hello",
                    "Hello",
                    "Overflow".into(),
                    "Body".into()
                )
                .is_err()
        );
        let notification = queue.take().unwrap();
        assert_eq!(notification.notification_id, 1);
        assert_eq!(notification.app_id, "dev.cardputerzero.hello");
        assert_eq!(notification.app_name, "Hello");
    }
}
