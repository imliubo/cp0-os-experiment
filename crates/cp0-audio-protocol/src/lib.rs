use std::fmt;
use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

pub const AUDIO_PROTOCOL_VERSION: u32 = 3;
pub const AUDIO_SAMPLE_RATE_HZ: u32 = 16_000;
pub const AUDIO_CHANNELS: u8 = 1;
pub const AUDIO_SAMPLE_BYTES: usize = 2;
pub const KEY_CLICK_FRAMES: u16 = 512;
pub const MAX_AUDIO_FRAMES: usize = 1024;
pub const MAX_AUDIO_BYTES: usize = MAX_AUDIO_FRAMES * AUDIO_SAMPLE_BYTES;
pub const MUSIC_SAMPLE_RATE_HZ: u32 = 48_000;
pub const MUSIC_CHANNELS: u8 = 2;
pub const MUSIC_FRAME_BYTES: usize = AUDIO_SAMPLE_BYTES * MUSIC_CHANNELS as usize;
pub const MAX_MUSIC_FRAMES: usize = 720;
pub const MAX_MUSIC_BYTES: usize = MAX_MUSIC_FRAMES * MUSIC_FRAME_BYTES;
pub const MAX_AUDIO_FRAME_BYTES: usize = 4 * 1024;
pub const MAX_AUDIO_ERROR_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: AudioCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AudioCommand {
    PlayPcmS16le { samples_base64: String },
    PlayPcmS16leStereo48k { samples_base64: String },
    CapturePcmS16le { frames: u16 },
    GetOutputState {},
    SetOutputVolume { percent: u8 },
    AdjustOutputVolume { direction: AudioDirection },
    SetOutputMuted { muted: bool },
    SetKeySoundsEnabled { enabled: bool },
    PlayKeyClick {},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioDirection {
    Decrease,
    Increase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioOutputState {
    pub available: bool,
    pub volume_percent: Option<u8>,
    pub muted: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    pub outcome: AudioOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AudioOutcome {
    Played {
        frames: u16,
    },
    Captured {
        samples_base64: String,
    },
    OutputState {
        state: AudioOutputState,
    },
    KeySoundsState {
        enabled: bool,
    },
    Error {
        code: AudioErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioErrorCode {
    InvalidRequest,
    Unauthorized,
    Busy,
    Unavailable,
    Device,
    Internal,
}

#[derive(Debug)]
pub enum AudioProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    FrameTooLarge,
    UnterminatedFrame,
    UnsupportedVersion(u32),
    InvalidSamples,
    InvalidFrameCount,
    InvalidOutputState,
    InvalidVolume,
    InvalidErrorMessage,
}

impl fmt::Display for AudioProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "audio protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid audio protocol JSON: {error}"),
            Self::FrameTooLarge => write!(
                formatter,
                "audio protocol frame exceeds {MAX_AUDIO_FRAME_BYTES} bytes"
            ),
            Self::UnterminatedFrame => {
                formatter.write_str("audio protocol frame is not newline terminated")
            }
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported audio protocol version {version}")
            }
            Self::InvalidSamples => formatter.write_str(
                "audio samples must be canonical base64 containing bounded S16_LE mono PCM",
            ),
            Self::InvalidFrameCount => {
                formatter.write_str("audio frame count is outside the supported range")
            }
            Self::InvalidOutputState => {
                formatter.write_str("audio output availability and values are inconsistent")
            }
            Self::InvalidVolume => {
                formatter.write_str("audio output volume is outside the supported range")
            }
            Self::InvalidErrorMessage => {
                formatter.write_str("audio error message is empty, too long or contains controls")
            }
        }
    }
}

impl std::error::Error for AudioProtocolError {}

impl From<io::Error> for AudioProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for AudioProtocolError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl AudioRequest {
    pub fn playback(request_id: u64, samples: &[u8]) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            command: AudioCommand::PlayPcmS16le {
                samples_base64: encode_base64(samples),
            },
        }
    }

    pub fn playback_stereo_48k(request_id: u64, samples: &[u8]) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            command: AudioCommand::PlayPcmS16leStereo48k {
                samples_base64: encode_base64(samples),
            },
        }
    }

    pub fn capture(request_id: u64, frames: u16) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            command: AudioCommand::CapturePcmS16le { frames },
        }
    }

    pub const fn get_output_state(request_id: u64) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            command: AudioCommand::GetOutputState {},
        }
    }

    pub const fn set_output_volume(request_id: u64, percent: u8) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            command: AudioCommand::SetOutputVolume { percent },
        }
    }

    pub const fn adjust_output_volume(request_id: u64, direction: AudioDirection) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            command: AudioCommand::AdjustOutputVolume { direction },
        }
    }

    pub const fn set_output_muted(request_id: u64, muted: bool) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            command: AudioCommand::SetOutputMuted { muted },
        }
    }

    pub const fn play_key_click(request_id: u64) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            command: AudioCommand::PlayKeyClick {},
        }
    }

    pub const fn set_key_sounds_enabled(request_id: u64, enabled: bool) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            command: AudioCommand::SetKeySoundsEnabled { enabled },
        }
    }

    pub fn validate(&self) -> Result<(), AudioProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.command {
            AudioCommand::PlayPcmS16le { samples_base64 } => {
                decode_samples(samples_base64).map(|_| ())
            }
            AudioCommand::PlayPcmS16leStereo48k { samples_base64 } => {
                decode_music_samples(samples_base64).map(|_| ())
            }
            AudioCommand::CapturePcmS16le { frames }
                if *frames == 0 || usize::from(*frames) > MAX_AUDIO_FRAMES =>
            {
                Err(AudioProtocolError::InvalidFrameCount)
            }
            AudioCommand::CapturePcmS16le { .. } => Ok(()),
            AudioCommand::SetOutputVolume { percent } if *percent > 100 => {
                Err(AudioProtocolError::InvalidVolume)
            }
            AudioCommand::GetOutputState {}
            | AudioCommand::SetOutputVolume { .. }
            | AudioCommand::AdjustOutputVolume { .. }
            | AudioCommand::SetOutputMuted { .. }
            | AudioCommand::SetKeySoundsEnabled { .. }
            | AudioCommand::PlayKeyClick {} => Ok(()),
        }
    }
}

impl AudioResponse {
    pub fn played(request_id: u64, frames: u16) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            outcome: AudioOutcome::Played { frames },
        }
    }

    pub fn captured(request_id: u64, samples: &[u8]) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            outcome: AudioOutcome::Captured {
                samples_base64: encode_base64(samples),
            },
        }
    }

    pub const fn output_state(request_id: u64, state: AudioOutputState) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            outcome: AudioOutcome::OutputState { state },
        }
    }

    pub const fn key_sounds_state(request_id: u64, enabled: bool) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            outcome: AudioOutcome::KeySoundsState { enabled },
        }
    }

    pub fn error(request_id: u64, code: AudioErrorCode, message: impl Into<String>) -> Self {
        Self {
            protocol_version: AUDIO_PROTOCOL_VERSION,
            request_id,
            outcome: AudioOutcome::Error {
                code,
                message: message.into(),
            },
        }
    }

    pub fn validate(&self) -> Result<(), AudioProtocolError> {
        validate_version(self.protocol_version)?;
        match &self.outcome {
            AudioOutcome::Played { frames }
                if *frames == 0 || usize::from(*frames) > MAX_AUDIO_FRAMES =>
            {
                Err(AudioProtocolError::InvalidFrameCount)
            }
            AudioOutcome::Captured { samples_base64 } => decode_samples(samples_base64).map(|_| ()),
            AudioOutcome::OutputState { state } => state.validate(),
            AudioOutcome::KeySoundsState { .. } => Ok(()),
            AudioOutcome::Error { message, .. }
                if message.is_empty()
                    || message.chars().count() > MAX_AUDIO_ERROR_CHARS
                    || message.chars().any(char::is_control) =>
            {
                Err(AudioProtocolError::InvalidErrorMessage)
            }
            _ => Ok(()),
        }
    }
}

impl AudioOutputState {
    pub const fn available(volume_percent: u8, muted: bool) -> Self {
        Self {
            available: true,
            volume_percent: Some(volume_percent),
            muted: Some(muted),
        }
    }

    pub const fn unavailable() -> Self {
        Self {
            available: false,
            volume_percent: None,
            muted: None,
        }
    }

    pub fn validate(&self) -> Result<(), AudioProtocolError> {
        match (self.available, self.volume_percent, self.muted) {
            (true, Some(percent), Some(_)) if percent <= 100 => Ok(()),
            (false, None, None) => Ok(()),
            _ => Err(AudioProtocolError::InvalidOutputState),
        }
    }
}

pub fn write_request(
    writer: &mut impl Write,
    request: &AudioRequest,
) -> Result<(), AudioProtocolError> {
    request.validate()?;
    write_frame(writer, request)
}

pub fn read_request(reader: &mut impl BufRead) -> Result<Option<AudioRequest>, AudioProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let request: AudioRequest = serde_json::from_slice(&frame)?;
    request.validate()?;
    Ok(Some(request))
}

pub fn write_response(
    writer: &mut impl Write,
    response: &AudioResponse,
) -> Result<(), AudioProtocolError> {
    response.validate()?;
    write_frame(writer, response)
}

pub fn read_response(
    reader: &mut impl BufRead,
) -> Result<Option<AudioResponse>, AudioProtocolError> {
    let Some(frame) = read_frame(reader)? else {
        return Ok(None);
    };
    let response: AudioResponse = serde_json::from_slice(&frame)?;
    response.validate()?;
    Ok(Some(response))
}

pub fn encode_base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[usize::from(first >> 2)] as char);
        output.push(ALPHABET[usize::from((first & 0x03) << 4 | second >> 4)] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[usize::from((second & 0x0f) << 2 | third >> 6)] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[usize::from(third & 0x3f)] as char
        } else {
            '='
        });
    }
    output
}

pub fn decode_samples(input: &str) -> Result<Vec<u8>, AudioProtocolError> {
    decode_bounded_samples(input, MAX_AUDIO_BYTES, AUDIO_SAMPLE_BYTES)
}

pub fn decode_music_samples(input: &str) -> Result<Vec<u8>, AudioProtocolError> {
    decode_bounded_samples(input, MAX_MUSIC_BYTES, MUSIC_FRAME_BYTES)
}

fn decode_bounded_samples(
    input: &str,
    maximum_bytes: usize,
    frame_bytes: usize,
) -> Result<Vec<u8>, AudioProtocolError> {
    let encoded = input.as_bytes();
    if encoded.is_empty() || encoded.len() % 4 != 0 || encoded.len() > maximum_bytes.div_ceil(3) * 4
    {
        return Err(AudioProtocolError::InvalidSamples);
    }
    let mut output = Vec::with_capacity(encoded.len() / 4 * 3);
    for (index, chunk) in encoded.chunks_exact(4).enumerate() {
        let last = index + 1 == encoded.len() / 4;
        let a = decode_digit(chunk[0])?;
        let b = decode_digit(chunk[1])?;
        let c = if chunk[2] == b'=' {
            if !last || chunk[3] != b'=' || b & 0x0f != 0 {
                return Err(AudioProtocolError::InvalidSamples);
            }
            None
        } else {
            Some(decode_digit(chunk[2])?)
        };
        let d = if chunk[3] == b'=' {
            if !last || c.is_some_and(|value| value & 0x03 != 0) {
                return Err(AudioProtocolError::InvalidSamples);
            }
            None
        } else {
            if c.is_none() {
                return Err(AudioProtocolError::InvalidSamples);
            }
            Some(decode_digit(chunk[3])?)
        };
        output.push(a << 2 | b >> 4);
        if let Some(c) = c {
            output.push(b << 4 | c >> 2);
            if let Some(d) = d {
                output.push(c << 6 | d);
            }
        }
    }
    if output.is_empty() || output.len() > maximum_bytes || output.len() % frame_bytes != 0 {
        return Err(AudioProtocolError::InvalidSamples);
    }
    Ok(output)
}

fn decode_digit(byte: u8) -> Result<u8, AudioProtocolError> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(AudioProtocolError::InvalidSamples),
    }
}

fn validate_version(version: u32) -> Result<(), AudioProtocolError> {
    if version == AUDIO_PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(AudioProtocolError::UnsupportedVersion(version))
    }
}

fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<(), AudioProtocolError> {
    let encoded = serde_json::to_vec(value)?;
    if encoded.len() + 1 > MAX_AUDIO_FRAME_BYTES {
        return Err(AudioProtocolError::FrameTooLarge);
    }
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, AudioProtocolError> {
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
        if frame.len() + consumed > MAX_AUDIO_FRAME_BYTES {
            return Err(AudioProtocolError::FrameTooLarge);
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
        return Err(AudioProtocolError::UnterminatedFrame);
    }
    frame.pop();
    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn round_trips_bounded_playback_and_capture() {
        let samples = [0x00, 0x80, 0xff, 0x7f];
        let playback = AudioRequest::playback(7, &samples);
        let mut frame = Vec::new();
        write_request(&mut frame, &playback).unwrap();
        assert_eq!(
            read_request(&mut Cursor::new(frame)).unwrap(),
            Some(playback)
        );

        let response = AudioResponse::captured(7, &samples);
        let mut frame = Vec::new();
        write_response(&mut frame, &response).unwrap();
        let decoded = read_response(&mut Cursor::new(frame)).unwrap().unwrap();
        let AudioOutcome::Captured { samples_base64 } = decoded.outcome else {
            panic!("expected captured samples");
        };
        assert_eq!(decode_samples(&samples_base64).unwrap(), samples);

        let stereo = [0_u8, 0x80, 0xff, 0x7f];
        let playback = AudioRequest::playback_stereo_48k(8, &stereo);
        let mut frame = Vec::new();
        write_request(&mut frame, &playback).unwrap();
        assert_eq!(
            read_request(&mut Cursor::new(frame)).unwrap(),
            Some(playback)
        );
    }

    #[test]
    fn round_trips_bounded_output_settings() {
        let requests = [
            AudioRequest::get_output_state(1),
            AudioRequest::set_output_volume(2, 75),
            AudioRequest::adjust_output_volume(3, AudioDirection::Decrease),
            AudioRequest::set_output_muted(4, true),
            AudioRequest::play_key_click(5),
            AudioRequest::set_key_sounds_enabled(6, false),
        ];
        for request in requests {
            let mut frame = Vec::new();
            write_request(&mut frame, &request).unwrap();
            assert_eq!(
                read_request(&mut Cursor::new(frame)).unwrap(),
                Some(request)
            );
        }

        for state in [
            AudioOutputState::available(75, false),
            AudioOutputState::unavailable(),
        ] {
            let response = AudioResponse::output_state(5, state);
            let mut frame = Vec::new();
            write_response(&mut frame, &response).unwrap();
            assert_eq!(
                read_response(&mut Cursor::new(frame)).unwrap(),
                Some(response)
            );
        }
        let response = AudioResponse::key_sounds_state(6, false);
        let mut frame = Vec::new();
        write_response(&mut frame, &response).unwrap();
        assert_eq!(
            read_response(&mut Cursor::new(frame)).unwrap(),
            Some(response)
        );
    }

    #[test]
    fn rejects_invalid_output_settings() {
        assert!(AudioRequest::set_output_volume(1, 101).validate().is_err());
        assert!(
            AudioOutputState {
                available: false,
                volume_percent: Some(0),
                muted: None,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rejects_odd_noncanonical_and_oversized_samples() {
        for encoded in ["", "A", "====", "AB==", "AAB=", "AA=="] {
            assert!(decode_samples(encoded).is_err(), "accepted {encoded}");
        }
        let oversized = encode_base64(&vec![0; MAX_AUDIO_BYTES + 2]);
        assert!(decode_samples(&oversized).is_err());
        for invalid in [vec![0; 2], vec![0; 6], vec![0; MAX_MUSIC_BYTES + 4]] {
            assert!(decode_music_samples(&encode_base64(&invalid)).is_err());
        }
        assert!(decode_music_samples(&encode_base64(&vec![0; MAX_MUSIC_BYTES])).is_ok());
    }

    #[test]
    fn validates_capture_count_and_frame_bound() {
        for frames in [0, MAX_AUDIO_FRAMES as u16 + 1] {
            assert!(AudioRequest::capture(1, frames).validate().is_err());
        }
        let mut oversized = vec![b'x'; MAX_AUDIO_FRAME_BYTES];
        oversized.push(b'\n');
        assert!(matches!(
            read_request(&mut Cursor::new(oversized)),
            Err(AudioProtocolError::FrameTooLarge)
        ));
    }
}
