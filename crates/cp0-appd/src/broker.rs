use std::collections::VecDeque;
use std::fmt;
use std::io::{self, BufRead, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

use serde::{Deserialize, Serialize};

pub const BROKER_PROTOCOL_VERSION: u32 = 1;
pub const MAX_BROKER_FRAME_BYTES: usize = 16 * 1024;
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
    PostNotification {
        title: String,
        body: String,
    },
    HttpGet {
        url: String,
    },
    OpenDocument,
    PlayAudio {
        samples_base64: String,
    },
    CaptureAudio {
        frames: u16,
    },
    CaptureCamera,
    ReadGpio {
        line: cp0_gpio_protocol::GpioLine,
    },
    WriteGpio {
        line: cp0_gpio_protocol::GpioLine,
        value: bool,
    },
    SendLora {
        payload_base64: String,
    },
    ReceiveLora {
        timeout_ms: u16,
    },
    StoragePut {
        key: String,
        value_base64: String,
    },
    StorageGet {
        key: String,
    },
    StorageDelete {
        key: String,
    },
    PhotoPut {
        key: String,
        value_base64: String,
    },
    PhotoGet {
        key: String,
    },
    PhotoIndexGet,
    PhotoDelete {
        key: String,
    },
    PhotoImportRgb565 {
        suggested_id: u64,
    },
    PhotoRemove {
        photo_id: u64,
    },
    SendIntent {
        action: String,
        payload_base64: String,
    },
    TakeIntent,
    UpdateMediaSession {
        state: crate::MediaPlaybackState,
        supported_actions: u8,
    },
    TakeMediaAction,
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
    HttpResponse {
        status_code: u16,
        body_base64: String,
    },
    PermissionPending {
        prompt_id: u64,
    },
    DocumentSelectionPending {
        prompt_id: u64,
    },
    DocumentOpened {
        document_id: String,
        size_bytes: u64,
    },
    AudioPlayed {
        frames: u16,
    },
    AudioCaptured {
        samples_base64: String,
    },
    CameraCaptured {
        width: u16,
        height: u16,
        pixel_format: cp0_camera_protocol::CameraPixelFormat,
        size_bytes: u32,
    },
    GpioValue {
        line: cp0_gpio_protocol::GpioLine,
        value: bool,
    },
    GpioWritten {
        line: cp0_gpio_protocol::GpioLine,
        value: bool,
    },
    LoraSent {
        bytes: u8,
    },
    LoraPacket {
        payload_base64: String,
        rssi_dbm: i16,
        snr_quarter_db: i8,
    },
    LoraNoPacket,
    StorageStored {
        used_bytes: u64,
    },
    StorageValue {
        value_base64: String,
    },
    StorageNotFound,
    StorageDeleted {
        existed: bool,
    },
    PhotoImported {
        photo_id: u64,
    },
    IntentAccepted {
        intent_id: u64,
    },
    IntentMessage {
        action: String,
        payload_base64: String,
    },
    IntentEmpty,
    MediaSessionUpdated {
        state: crate::MediaPlaybackState,
        supported_actions: u8,
    },
    MediaAction {
        action: crate::MediaAction,
    },
    MediaActionEmpty,
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
    Unavailable,
    BlockedAddress,
    UpstreamUnavailable,
    Timeout,
    Tls,
    TooManyRedirects,
    ResponseTooLarge,
    NotFound,
    Ambiguous,
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
    InvalidUrl,
    InvalidNetworkResponse,
    InvalidDocumentResponse,
    InvalidAudio,
    InvalidCameraResponse,
    InvalidRadio,
    InvalidStorage,
    InvalidIntent,
    InvalidMediaSession,
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
            Self::InvalidUrl => formatter.write_str("invalid or oversized HTTPS URL"),
            Self::InvalidNetworkResponse => formatter.write_str("invalid bounded HTTPS response"),
            Self::InvalidDocumentResponse => {
                formatter.write_str("invalid bounded document response")
            }
            Self::InvalidAudio => formatter.write_str("invalid bounded audio samples"),
            Self::InvalidCameraResponse => {
                formatter.write_str("invalid fixed camera frame metadata")
            }
            Self::InvalidRadio => formatter.write_str("invalid bounded LoRa operation"),
            Self::InvalidStorage => formatter.write_str("invalid private storage operation"),
            Self::InvalidIntent => formatter.write_str("invalid bounded intent operation"),
            Self::InvalidMediaSession => {
                formatter.write_str("invalid bounded media session operation")
            }
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
            BrokerCommand::HttpGet { url } if !cp0_network_protocol::is_valid_https_url(url) => {
                Err(BrokerProtocolError::InvalidUrl)
            }
            BrokerCommand::PlayAudio { samples_base64 }
                if cp0_audio_protocol::decode_samples(samples_base64).is_err() =>
            {
                Err(BrokerProtocolError::InvalidAudio)
            }
            BrokerCommand::CaptureAudio { frames }
                if *frames == 0 || usize::from(*frames) > cp0_audio_protocol::MAX_AUDIO_FRAMES =>
            {
                Err(BrokerProtocolError::InvalidAudio)
            }
            BrokerCommand::SendLora { payload_base64 }
                if cp0_radio_protocol::decode_payload(payload_base64).is_err() =>
            {
                Err(BrokerProtocolError::InvalidRadio)
            }
            BrokerCommand::ReceiveLora { timeout_ms }
                if *timeout_ms == 0
                    || *timeout_ms > cp0_radio_protocol::MAX_LORA_RECEIVE_TIMEOUT_MS =>
            {
                Err(BrokerProtocolError::InvalidRadio)
            }
            BrokerCommand::StoragePut { key, value_base64 }
            | BrokerCommand::PhotoPut { key, value_base64 } => {
                cp0_storage_protocol::validate_key(key)
                    .and_then(|()| cp0_storage_protocol::decode_value(value_base64).map(|_| ()))
                    .map_err(|_| BrokerProtocolError::InvalidStorage)
            }
            BrokerCommand::StorageGet { key }
            | BrokerCommand::StorageDelete { key }
            | BrokerCommand::PhotoGet { key }
            | BrokerCommand::PhotoDelete { key } => cp0_storage_protocol::validate_key(key)
                .map_err(|_| BrokerProtocolError::InvalidStorage),
            BrokerCommand::PhotoRemove { photo_id } if *photo_id == 0 => {
                Err(BrokerProtocolError::InvalidStorage)
            }
            BrokerCommand::SendIntent {
                action,
                payload_base64,
            } => {
                if !cp0_manifest::is_valid_intent_action(action) {
                    return Err(BrokerProtocolError::InvalidIntent);
                }
                let payload = cp0_network_protocol::decode_base64(payload_base64)
                    .map_err(|_| BrokerProtocolError::InvalidIntent)?;
                if payload.len() > crate::MAX_INTENT_PAYLOAD_BYTES {
                    return Err(BrokerProtocolError::InvalidIntent);
                }
                Ok(())
            }
            BrokerCommand::UpdateMediaSession {
                state,
                supported_actions,
            } if !crate::valid_media_session_update(*state, *supported_actions) => {
                Err(BrokerProtocolError::InvalidMediaSession)
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

    pub fn http_response(request_id: u64, status_code: u16, body_base64: String) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::HttpResponse {
                status_code,
                body_base64,
            },
        }
    }

    pub fn document_selection_pending(request_id: u64, prompt_id: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::DocumentSelectionPending { prompt_id },
        }
    }

    pub fn document_opened(request_id: u64, document_id: String, size_bytes: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::DocumentOpened {
                document_id,
                size_bytes,
            },
        }
    }

    pub fn audio_played(request_id: u64, frames: u16) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::AudioPlayed { frames },
        }
    }

    pub fn audio_captured(request_id: u64, samples_base64: String) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::AudioCaptured { samples_base64 },
        }
    }

    pub fn camera_captured(request_id: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::CameraCaptured {
                width: cp0_camera_protocol::CAMERA_WIDTH,
                height: cp0_camera_protocol::CAMERA_HEIGHT,
                pixel_format: cp0_camera_protocol::CameraPixelFormat::Rgb565Le,
                size_bytes: cp0_camera_protocol::CAMERA_FRAME_BYTES as u32,
            },
        }
    }

    pub fn gpio_value(request_id: u64, line: cp0_gpio_protocol::GpioLine, value: bool) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::GpioValue { line, value },
        }
    }

    pub fn gpio_written(request_id: u64, line: cp0_gpio_protocol::GpioLine, value: bool) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::GpioWritten { line, value },
        }
    }

    pub fn lora_sent(request_id: u64, bytes: u8) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::LoraSent { bytes },
        }
    }

    pub fn lora_packet(request_id: u64, payload: &[u8], rssi_dbm: i16, snr_quarter_db: i8) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::LoraPacket {
                payload_base64: cp0_radio_protocol::encode_base64(payload),
                rssi_dbm,
                snr_quarter_db,
            },
        }
    }

    pub fn lora_no_packet(request_id: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::LoraNoPacket,
        }
    }

    pub fn storage_stored(request_id: u64, used_bytes: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::StorageStored { used_bytes },
        }
    }

    pub fn storage_value(request_id: u64, value: &[u8]) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::StorageValue {
                value_base64: cp0_storage_protocol::encode_base64(value),
            },
        }
    }

    pub fn storage_not_found(request_id: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::StorageNotFound,
        }
    }

    pub fn storage_deleted(request_id: u64, existed: bool) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::StorageDeleted { existed },
        }
    }

    pub fn photo_imported(request_id: u64, photo_id: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::PhotoImported { photo_id },
        }
    }

    pub fn intent_accepted(request_id: u64, intent_id: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::IntentAccepted { intent_id },
        }
    }

    pub fn intent_message(request_id: u64, action: String, payload: &[u8]) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::IntentMessage {
                action,
                payload_base64: cp0_network_protocol::encode_base64(payload),
            },
        }
    }

    pub fn intent_empty(request_id: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::IntentEmpty,
        }
    }

    pub fn media_session_updated(
        request_id: u64,
        state: crate::MediaPlaybackState,
        supported_actions: u8,
    ) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::MediaSessionUpdated {
                state,
                supported_actions,
            },
        }
    }

    pub fn media_action(request_id: u64, action: crate::MediaAction) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::MediaAction { action },
        }
    }

    pub fn media_action_empty(request_id: u64) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id,
            outcome: BrokerOutcome::MediaActionEmpty,
        }
    }

    pub fn validate(&self) -> Result<(), BrokerProtocolError> {
        if self.protocol_version != BROKER_PROTOCOL_VERSION {
            return Err(BrokerProtocolError::UnsupportedVersion(
                self.protocol_version,
            ));
        }
        if let BrokerOutcome::HttpResponse {
            status_code,
            body_base64,
        } = &self.outcome
        {
            if !(100..=599).contains(status_code)
                || cp0_network_protocol::decode_base64(body_base64).is_err()
            {
                return Err(BrokerProtocolError::InvalidNetworkResponse);
            }
        }
        if let BrokerOutcome::DocumentOpened {
            document_id,
            size_bytes,
        } = &self.outcome
        {
            if !cp0_document_protocol::is_valid_document_id(document_id)
                || *size_bytes > cp0_document_protocol::MAX_DOCUMENT_BYTES
            {
                return Err(BrokerProtocolError::InvalidDocumentResponse);
            }
        }
        match &self.outcome {
            BrokerOutcome::AudioPlayed { frames }
                if *frames == 0 || usize::from(*frames) > cp0_audio_protocol::MAX_AUDIO_FRAMES =>
            {
                return Err(BrokerProtocolError::InvalidAudio);
            }
            BrokerOutcome::AudioCaptured { samples_base64 }
                if cp0_audio_protocol::decode_samples(samples_base64).is_err() =>
            {
                return Err(BrokerProtocolError::InvalidAudio);
            }
            BrokerOutcome::CameraCaptured {
                width,
                height,
                pixel_format,
                size_bytes,
            } if *width != cp0_camera_protocol::CAMERA_WIDTH
                || *height != cp0_camera_protocol::CAMERA_HEIGHT
                || *pixel_format != cp0_camera_protocol::CameraPixelFormat::Rgb565Le
                || *size_bytes != cp0_camera_protocol::CAMERA_FRAME_BYTES as u32 =>
            {
                return Err(BrokerProtocolError::InvalidCameraResponse);
            }
            BrokerOutcome::LoraSent { bytes }
                if *bytes == 0
                    || usize::from(*bytes) > cp0_radio_protocol::MAX_LORA_PAYLOAD_BYTES =>
            {
                return Err(BrokerProtocolError::InvalidRadio);
            }
            BrokerOutcome::LoraPacket {
                payload_base64,
                rssi_dbm,
                ..
            } if cp0_radio_protocol::decode_payload(payload_base64).is_err()
                || !(-200..=50).contains(rssi_dbm) =>
            {
                return Err(BrokerProtocolError::InvalidRadio);
            }
            BrokerOutcome::StorageStored { used_bytes }
                if *used_bytes > cp0_storage_protocol::SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES =>
            {
                return Err(BrokerProtocolError::InvalidStorage);
            }
            BrokerOutcome::StorageValue { value_base64 }
                if cp0_storage_protocol::decode_value(value_base64).is_err() =>
            {
                return Err(BrokerProtocolError::InvalidStorage);
            }
            BrokerOutcome::PhotoImported { photo_id } if *photo_id == 0 => {
                return Err(BrokerProtocolError::InvalidStorage);
            }
            BrokerOutcome::IntentAccepted { intent_id } if *intent_id == 0 => {
                return Err(BrokerProtocolError::InvalidIntent);
            }
            BrokerOutcome::IntentMessage {
                action,
                payload_base64,
            } => {
                if !cp0_manifest::is_valid_intent_action(action) {
                    return Err(BrokerProtocolError::InvalidIntent);
                }
                let payload = cp0_network_protocol::decode_base64(payload_base64)
                    .map_err(|_| BrokerProtocolError::InvalidIntent)?;
                if payload.len() > crate::MAX_INTENT_PAYLOAD_BYTES {
                    return Err(BrokerProtocolError::InvalidIntent);
                }
            }
            BrokerOutcome::MediaSessionUpdated {
                state,
                supported_actions,
            } if !crate::valid_media_session_update(*state, *supported_actions) => {
                return Err(BrokerProtocolError::InvalidMediaSession);
            }
            _ => {}
        }
        Ok(())
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
    decode_broker_request(&frame).map(Some)
}

pub fn decode_broker_request(frame: &[u8]) -> Result<BrokerRequest, BrokerProtocolError> {
    let request: BrokerRequest = serde_json::from_slice(frame)?;
    request.validate()?;
    Ok(request)
}

pub fn recv_broker_request_with_fd(
    stream: &UnixStream,
) -> Result<(BrokerRequest, Option<OwnedFd>), BrokerProtocolError> {
    let (frame, descriptor) =
        cp0_document_protocol::recv_frame_with_fd(stream).map_err(|error| {
            BrokerProtocolError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                error.to_string(),
            ))
        })?;
    decode_broker_request(&frame).map(|request| (request, descriptor))
}

pub fn read_broker_response(
    reader: &mut impl BufRead,
) -> Result<Option<BrokerResponse>, BrokerProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: BrokerResponse = serde_json::from_slice(&frame)?;
    response.validate()?;
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
    let encoded = encode_broker_response(response)?;
    writer.write_all(&encoded)?;
    writer.flush()?;
    Ok(())
}

pub fn encode_broker_response(response: &BrokerResponse) -> Result<Vec<u8>, BrokerProtocolError> {
    response.validate()?;
    encode_value(response)
}

fn write_value(writer: &mut impl Write, value: &impl Serialize) -> Result<(), BrokerProtocolError> {
    let encoded = encode_value(value)?;
    writer.write_all(&encoded)?;
    writer.flush()?;
    Ok(())
}

fn encode_value(value: &impl Serialize) -> Result<Vec<u8>, BrokerProtocolError> {
    let mut encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_BROKER_FRAME_BYTES {
        return Err(BrokerProtocolError::FrameTooLarge);
    }
    encoded.push(b'\n');
    Ok(encoded)
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

    fn http_request(url: &str) -> BrokerRequest {
        BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 2,
            command: BrokerCommand::HttpGet { url: url.into() },
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

    #[test]
    fn validates_https_requests_and_bounded_http_responses() {
        assert!(http_request("https://example.com/data").validate().is_ok());
        assert!(matches!(
            http_request("http://example.com").validate(),
            Err(BrokerProtocolError::InvalidUrl)
        ));
        let response =
            BrokerResponse::http_response(2, 200, cp0_network_protocol::encode_base64(b"body"));
        let mut frame = Vec::new();
        write_broker_response(&mut frame, &response).unwrap();
        assert_eq!(
            read_broker_response(&mut Cursor::new(frame)).unwrap(),
            Some(response)
        );
        let maximum = BrokerResponse::http_response(
            3,
            200,
            cp0_network_protocol::encode_base64(&vec![
                0;
                cp0_network_protocol::MAX_NETWORK_BODY_BYTES
            ]),
        );
        let mut maximum_frame = Vec::new();
        write_broker_response(&mut maximum_frame, &maximum).unwrap();
        assert!(maximum_frame.len() <= MAX_BROKER_FRAME_BYTES);
    }

    #[test]
    fn validates_descriptor_backed_document_response() {
        let request = BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 3,
            command: BrokerCommand::OpenDocument,
        };
        let mut encoded = Vec::new();
        write_broker_request(&mut encoded, &request).unwrap();
        assert_eq!(
            read_broker_request(&mut Cursor::new(encoded)).unwrap(),
            Some(request)
        );
        let response =
            BrokerResponse::document_opened(3, "00000000000000010000000000000002".into(), 1024);
        assert!(response.validate().is_ok());
        assert!(
            BrokerResponse::document_opened(3, "../../etc/passwd".into(), 1)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn validates_bounded_audio_requests_and_responses() {
        let samples = [0_u8, 0x80, 0xff, 0x7f];
        let request = BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 4,
            command: BrokerCommand::PlayAudio {
                samples_base64: cp0_audio_protocol::encode_base64(&samples),
            },
        };
        let mut frame = Vec::new();
        write_broker_request(&mut frame, &request).unwrap();
        assert_eq!(
            read_broker_request(&mut Cursor::new(frame)).unwrap(),
            Some(request)
        );
        assert!(
            BrokerRequest {
                protocol_version: BROKER_PROTOCOL_VERSION,
                request_id: 5,
                command: BrokerCommand::CaptureAudio { frames: 0 },
            }
            .validate()
            .is_err()
        );
        let response =
            BrokerResponse::audio_captured(5, cp0_audio_protocol::encode_base64(&samples));
        assert!(response.validate().is_ok());
        assert!(BrokerResponse::audio_played(5, 0).validate().is_err());
    }

    #[test]
    fn validates_fixed_camera_request_and_response() {
        let request = BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 6,
            command: BrokerCommand::CaptureCamera,
        };
        let mut frame = Vec::new();
        write_broker_request(&mut frame, &request).unwrap();
        assert_eq!(
            read_broker_request(&mut Cursor::new(frame)).unwrap(),
            Some(request)
        );
        assert!(BrokerResponse::camera_captured(6).validate().is_ok());
        let invalid = BrokerResponse {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 6,
            outcome: BrokerOutcome::CameraCaptured {
                width: 640,
                height: cp0_camera_protocol::CAMERA_HEIGHT,
                pixel_format: cp0_camera_protocol::CameraPixelFormat::Rgb565Le,
                size_bytes: cp0_camera_protocol::CAMERA_FRAME_BYTES as u32,
            },
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn round_trips_fixed_gpio_lines_without_paths_or_numbers() {
        let request = BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 7,
            command: BrokerCommand::WriteGpio {
                line: cp0_gpio_protocol::GpioLine::GroveFunction,
                value: true,
            },
        };
        let mut frame = Vec::new();
        write_broker_request(&mut frame, &request).unwrap();
        assert_eq!(
            read_broker_request(&mut Cursor::new(frame)).unwrap(),
            Some(request)
        );
        assert!(
            BrokerResponse::gpio_value(7, cp0_gpio_protocol::GpioLine::External5vPower, false)
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn round_trips_bounded_lora_packets_and_metadata() {
        let request = BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 10,
            command: BrokerCommand::ReceiveLora { timeout_ms: 250 },
        };
        let mut frame = Vec::new();
        write_broker_request(&mut frame, &request).unwrap();
        assert_eq!(
            read_broker_request(&mut Cursor::new(frame)).unwrap(),
            Some(request)
        );
        assert!(
            BrokerResponse::lora_packet(10, b"hello", -92, -5)
                .validate()
                .is_ok()
        );
        assert!(BrokerResponse::lora_no_packet(10).validate().is_ok());
        assert!(
            BrokerRequest {
                protocol_version: BROKER_PROTOCOL_VERSION,
                request_id: 10,
                command: BrokerCommand::ReceiveLora { timeout_ms: 0 },
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn round_trips_bounded_private_storage_operations() {
        let request = BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 12,
            command: BrokerCommand::StoragePut {
                key: "state.v1".into(),
                value_base64: cp0_storage_protocol::encode_base64(b"value"),
            },
        };
        let mut frame = Vec::new();
        write_broker_request(&mut frame, &request).unwrap();
        assert_eq!(
            read_broker_request(&mut Cursor::new(frame)).unwrap(),
            Some(request)
        );
        assert!(
            BrokerResponse::storage_value(12, b"value")
                .validate()
                .is_ok()
        );
        assert!(BrokerResponse::storage_not_found(12).validate().is_ok());
        assert!(BrokerResponse::storage_deleted(12, true).validate().is_ok());
        assert!(
            BrokerResponse::storage_stored(12, cp0_storage_protocol::MAX_STORAGE_QUOTA_BYTES + 1,)
                .validate()
                .is_ok()
        );
        assert!(
            BrokerResponse::storage_stored(
                12,
                cp0_storage_protocol::SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES + 1,
            )
            .validate()
            .is_err()
        );

        let photo_request = BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 13,
            command: BrokerCommand::PhotoPut {
                key: "p0000000000000001.c00".into(),
                value_base64: cp0_storage_protocol::encode_base64(b"pixels"),
            },
        };
        assert!(photo_request.validate().is_ok());
        let import = BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 14,
            command: BrokerCommand::PhotoImportRgb565 {
                suggested_id: 1_722_470_400_123,
            },
        };
        let mut frame = Vec::new();
        write_broker_request(&mut frame, &import).unwrap();
        assert_eq!(
            read_broker_request(&mut Cursor::new(frame)).unwrap(),
            Some(import)
        );
        assert!(
            BrokerResponse::photo_imported(14, 1_722_470_400_123)
                .validate()
                .is_ok()
        );
        assert!(
            BrokerRequest {
                protocol_version: BROKER_PROTOCOL_VERSION,
                request_id: 15,
                command: BrokerCommand::PhotoRemove { photo_id: 0 },
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn round_trips_bounded_intents_without_exposing_a_target() {
        let request = BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 14,
            command: BrokerCommand::SendIntent {
                action: "dev.cardputerzero.documents.open".into(),
                payload_base64: cp0_network_protocol::encode_base64(b"document-7"),
            },
        };
        let mut frame = Vec::new();
        write_broker_request(&mut frame, &request).unwrap();
        assert_eq!(
            read_broker_request(&mut Cursor::new(frame)).unwrap(),
            Some(request)
        );
        let accepted = BrokerResponse::intent_accepted(14, 1);
        let encoded = serde_json::to_string(&accepted).unwrap();
        assert!(!encoded.contains("target"));
        assert!(accepted.validate().is_ok());
        assert!(
            BrokerResponse::intent_message(
                15,
                "dev.cardputerzero.documents.open".into(),
                b"document-7"
            )
            .validate()
            .is_ok()
        );
        assert!(
            BrokerRequest {
                protocol_version: BROKER_PROTOCOL_VERSION,
                request_id: 14,
                command: BrokerCommand::SendIntent {
                    action: "../../escape".into(),
                    payload_base64: String::new(),
                },
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn round_trips_targetless_media_session_coordination() {
        let update = BrokerRequest {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: 16,
            command: BrokerCommand::UpdateMediaSession {
                state: crate::MediaPlaybackState::Playing,
                supported_actions: crate::MEDIA_ACTION_PLAY_PAUSE | crate::MEDIA_ACTION_NEXT,
            },
        };
        let mut frame = Vec::new();
        write_broker_request(&mut frame, &update).unwrap();
        let encoded = String::from_utf8(frame.clone()).unwrap();
        assert!(!encoded.contains("app_id"));
        assert_eq!(
            read_broker_request(&mut Cursor::new(frame)).unwrap(),
            Some(update)
        );
        assert!(
            BrokerResponse::media_session_updated(
                16,
                crate::MediaPlaybackState::Playing,
                crate::MEDIA_ACTION_PLAY_PAUSE | crate::MEDIA_ACTION_NEXT,
            )
            .validate()
            .is_ok()
        );
        assert!(
            BrokerResponse::media_action(17, crate::MediaAction::Next)
                .validate()
                .is_ok()
        );
        assert!(BrokerResponse::media_action_empty(17).validate().is_ok());

        for (state, supported_actions) in [
            (
                crate::MediaPlaybackState::Inactive,
                crate::MEDIA_ACTION_NEXT,
            ),
            (crate::MediaPlaybackState::Paused, 0),
            (crate::MediaPlaybackState::Playing, 1 << 7),
        ] {
            assert!(
                BrokerRequest {
                    protocol_version: BROKER_PROTOCOL_VERSION,
                    request_id: 18,
                    command: BrokerCommand::UpdateMediaSession {
                        state,
                        supported_actions,
                    },
                }
                .validate()
                .is_err()
            );
        }
    }
}
