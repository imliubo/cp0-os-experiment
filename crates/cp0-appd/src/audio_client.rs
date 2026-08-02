use std::fmt;
use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cp0_audio_protocol::{
    AudioErrorCode, AudioOutcome, AudioProtocolError, AudioRequest, decode_samples, read_response,
    write_request,
};

pub const DEFAULT_AUDIO_SOCKET: &str = "/run/cardputerzero-audiod/audio.sock";
const AUDIO_SERVICE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub enum AudioClientError {
    Io(std::io::Error),
    Protocol(AudioProtocolError),
    EmptyResponse,
    MismatchedRequestId,
    MismatchedFrameCount,
    Service(AudioErrorCode),
}

impl fmt::Display for AudioClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "audio service I/O error: {error}"),
            Self::Protocol(error) => write!(formatter, "audio service protocol error: {error}"),
            Self::EmptyResponse => formatter.write_str("audio service returned no response"),
            Self::MismatchedRequestId => {
                formatter.write_str("audio service returned a mismatched request ID")
            }
            Self::MismatchedFrameCount => {
                formatter.write_str("audio service returned a mismatched frame count")
            }
            Self::Service(code) => write!(formatter, "audio service rejected request: {code:?}"),
        }
    }
}

impl std::error::Error for AudioClientError {}

impl From<std::io::Error> for AudioClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<AudioProtocolError> for AudioClientError {
    fn from(error: AudioProtocolError) -> Self {
        Self::Protocol(error)
    }
}

#[derive(Debug, Clone)]
pub struct AudioClient {
    socket_path: PathBuf,
}

impl Default for AudioClient {
    fn default() -> Self {
        Self::new(DEFAULT_AUDIO_SOCKET)
    }
}

impl AudioClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub fn play(&self, request_id: u64, samples: &[u8]) -> Result<u16, AudioClientError> {
        let request = AudioRequest::playback(request_id, samples);
        match self.exchange(&request)? {
            AudioOutcome::Played { frames }
                if usize::from(frames) * cp0_audio_protocol::AUDIO_SAMPLE_BYTES
                    == samples.len() =>
            {
                Ok(frames)
            }
            AudioOutcome::Played { .. } => Err(AudioClientError::MismatchedFrameCount),
            AudioOutcome::Error { code, .. } => Err(AudioClientError::Service(code)),
            AudioOutcome::Captured { .. } | AudioOutcome::OutputState { .. } => {
                Err(AudioClientError::MismatchedFrameCount)
            }
        }
    }

    pub fn capture(&self, request_id: u64, frames: u16) -> Result<Vec<u8>, AudioClientError> {
        let request = AudioRequest::capture(request_id, frames);
        match self.exchange(&request)? {
            AudioOutcome::Captured { samples_base64 } => {
                let samples = decode_samples(&samples_base64)?;
                if samples.len() != usize::from(frames) * cp0_audio_protocol::AUDIO_SAMPLE_BYTES {
                    return Err(AudioClientError::MismatchedFrameCount);
                }
                Ok(samples)
            }
            AudioOutcome::Error { code, .. } => Err(AudioClientError::Service(code)),
            AudioOutcome::Played { .. } | AudioOutcome::OutputState { .. } => {
                Err(AudioClientError::MismatchedFrameCount)
            }
        }
    }

    fn exchange(&self, request: &AudioRequest) -> Result<AudioOutcome, AudioClientError> {
        let mut stream = UnixStream::connect(&self.socket_path)?;
        stream.set_read_timeout(Some(AUDIO_SERVICE_TIMEOUT))?;
        stream.set_write_timeout(Some(AUDIO_SERVICE_TIMEOUT))?;
        write_request(&mut stream, request)?;
        let response = read_response(&mut BufReader::new(stream.try_clone()?))?
            .ok_or(AudioClientError::EmptyResponse)?;
        if response.request_id != request.request_id {
            return Err(AudioClientError::MismatchedRequestId);
        }
        Ok(response.outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use cp0_audio_protocol::{AudioResponse, read_request, write_response};

    use super::*;

    #[test]
    fn exchanges_strict_playback_and_capture_requests() {
        let (mut client, mut service) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let mut reader = BufReader::new(service.try_clone().unwrap());
            let playback = read_request(&mut reader).unwrap().unwrap();
            write_response(&mut service, &AudioResponse::played(playback.request_id, 2)).unwrap();
        });
        let request = AudioRequest::playback(3, &[1, 0, 2, 0]);
        let outcome = exchange_stream(&mut client, &request).unwrap();
        worker.join().unwrap();
        assert_eq!(outcome, AudioOutcome::Played { frames: 2 });

        let (mut client, mut service) = UnixStream::pair().unwrap();
        let worker = thread::spawn(move || {
            let mut reader = BufReader::new(service.try_clone().unwrap());
            let capture = read_request(&mut reader).unwrap().unwrap();
            write_response(
                &mut service,
                &AudioResponse::captured(capture.request_id, &[0, 128, 255, 127]),
            )
            .unwrap();
        });
        let request = AudioRequest::capture(4, 2);
        let outcome = exchange_stream(&mut client, &request).unwrap();
        worker.join().unwrap();
        let AudioOutcome::Captured { samples_base64 } = outcome else {
            panic!("expected capture response");
        };
        assert_eq!(decode_samples(&samples_base64).unwrap(), [0, 128, 255, 127]);
    }

    fn exchange_stream(
        stream: &mut UnixStream,
        request: &AudioRequest,
    ) -> Result<AudioOutcome, AudioClientError> {
        write_request(&mut *stream, request)?;
        let response = read_response(&mut BufReader::new(stream.try_clone()?))?
            .ok_or(AudioClientError::EmptyResponse)?;
        if response.request_id != request.request_id {
            return Err(AudioClientError::MismatchedRequestId);
        }
        Ok(response.outcome)
    }
}
