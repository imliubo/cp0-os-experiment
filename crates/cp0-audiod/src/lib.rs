use std::collections::BTreeSet;
use std::ffi::{CStr, CString, c_void};
use std::fmt;
use std::io::{self, BufReader};
use std::mem;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::ptr;
use std::time::Duration;

use cp0_audio_protocol::{
    AUDIO_SAMPLE_BYTES, AUDIO_SAMPLE_RATE_HZ, AudioCommand, AudioErrorCode, AudioProtocolError,
    AudioRequest, AudioResponse, MAX_AUDIO_FRAMES, decode_samples, read_request, write_response,
};

pub const DEFAULT_AUDIO_DEVICE: &str = "hw:ES8389Audio,0";
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);
const SND_PCM_STREAM_PLAYBACK: libc::c_int = 0;
const SND_PCM_STREAM_CAPTURE: libc::c_int = 1;
const SND_PCM_FORMAT_S16_LE: libc::c_int = 2;
const SND_PCM_ACCESS_RW_INTERLEAVED: libc::c_int = 3;
const PCM_LATENCY_MICROSECONDS: libc::c_uint = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDeviceError {
    Busy,
    Unavailable,
    Device,
}

impl fmt::Display for AudioDeviceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => formatter.write_str("audio device is busy"),
            Self::Unavailable => formatter.write_str("audio device is unavailable"),
            Self::Device => formatter.write_str("audio device operation failed"),
        }
    }
}

impl std::error::Error for AudioDeviceError {}

pub trait AudioDevice {
    fn play_pcm_s16le(&self, samples: &[u8]) -> Result<(), AudioDeviceError>;
    fn capture_pcm_s16le(&self, frames: u16) -> Result<Vec<u8>, AudioDeviceError>;
}

type PcmOpen = unsafe extern "C" fn(
    *mut *mut c_void,
    *const libc::c_char,
    libc::c_int,
    libc::c_int,
) -> libc::c_int;
type PcmSetParams = unsafe extern "C" fn(
    *mut c_void,
    libc::c_int,
    libc::c_int,
    libc::c_uint,
    libc::c_uint,
    libc::c_int,
    libc::c_uint,
) -> libc::c_int;
type PcmWrite = unsafe extern "C" fn(*mut c_void, *const c_void, libc::c_ulong) -> libc::c_long;
type PcmRead = unsafe extern "C" fn(*mut c_void, *mut c_void, libc::c_ulong) -> libc::c_long;
type PcmRecover = unsafe extern "C" fn(*mut c_void, libc::c_int, libc::c_int) -> libc::c_int;
type PcmSimple = unsafe extern "C" fn(*mut c_void) -> libc::c_int;

#[derive(Debug)]
struct AlsaLibrary {
    handle: *mut c_void,
    pcm_open: PcmOpen,
    pcm_set_params: PcmSetParams,
    pcm_writei: PcmWrite,
    pcm_readi: PcmRead,
    pcm_recover: PcmRecover,
    pcm_drain: PcmSimple,
    pcm_close: PcmSimple,
}

impl AlsaLibrary {
    fn load() -> Result<Self, AudioDeviceError> {
        let library = c"libasound.so.2";
        let handle = unsafe { libc::dlopen(library.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return Err(AudioDeviceError::Unavailable);
        }
        let result = unsafe {
            Ok(Self {
                handle,
                pcm_open: load_symbol(handle, c"snd_pcm_open")?,
                pcm_set_params: load_symbol(handle, c"snd_pcm_set_params")?,
                pcm_writei: load_symbol(handle, c"snd_pcm_writei")?,
                pcm_readi: load_symbol(handle, c"snd_pcm_readi")?,
                pcm_recover: load_symbol(handle, c"snd_pcm_recover")?,
                pcm_drain: load_symbol(handle, c"snd_pcm_drain")?,
                pcm_close: load_symbol(handle, c"snd_pcm_close")?,
            })
        };
        if result.is_err() {
            unsafe { libc::dlclose(handle) };
        }
        result
    }
}

impl Drop for AlsaLibrary {
    fn drop(&mut self) {
        unsafe { libc::dlclose(self.handle) };
    }
}

unsafe fn load_symbol<T: Copy>(handle: *mut c_void, name: &CStr) -> Result<T, AudioDeviceError> {
    let symbol = unsafe { libc::dlsym(handle, name.as_ptr()) };
    if symbol.is_null() || mem::size_of::<T>() != mem::size_of::<*mut c_void>() {
        return Err(AudioDeviceError::Unavailable);
    }
    Ok(unsafe { mem::transmute_copy(&symbol) })
}

#[derive(Debug)]
pub struct AlsaDevice {
    library: AlsaLibrary,
    device: CString,
}

impl AlsaDevice {
    pub fn open_default() -> Result<Self, AudioDeviceError> {
        Ok(Self {
            library: AlsaLibrary::load()?,
            device: CString::new(DEFAULT_AUDIO_DEVICE).expect("static ALSA device has no NUL"),
        })
    }

    fn open_pcm(&self, stream: libc::c_int) -> Result<PcmHandle<'_>, AudioDeviceError> {
        let mut raw: *mut c_void = ptr::null_mut();
        let result =
            unsafe { (self.library.pcm_open)(&raw mut raw, self.device.as_ptr(), stream, 0) };
        if result < 0 || raw.is_null() {
            return Err(map_alsa_error(result));
        }
        let handle = PcmHandle {
            raw,
            library: &self.library,
        };
        let result = unsafe {
            (self.library.pcm_set_params)(
                raw,
                SND_PCM_FORMAT_S16_LE,
                SND_PCM_ACCESS_RW_INTERLEAVED,
                1,
                AUDIO_SAMPLE_RATE_HZ,
                1,
                PCM_LATENCY_MICROSECONDS,
            )
        };
        if result < 0 {
            return Err(map_alsa_error(result));
        }
        Ok(handle)
    }
}

impl AudioDevice for AlsaDevice {
    fn play_pcm_s16le(&self, samples: &[u8]) -> Result<(), AudioDeviceError> {
        let pcm = self.open_pcm(SND_PCM_STREAM_PLAYBACK)?;
        let words: Vec<i16> = samples
            .chunks_exact(AUDIO_SAMPLE_BYTES)
            .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
            .collect();
        let mut offset = 0;
        while offset < words.len() {
            let result = unsafe {
                (self.library.pcm_writei)(
                    pcm.raw,
                    words[offset..].as_ptr().cast(),
                    (words.len() - offset) as libc::c_ulong,
                )
            };
            if result < 0 {
                recover(&self.library, pcm.raw, result)?;
                continue;
            }
            if result == 0 {
                return Err(AudioDeviceError::Device);
            }
            offset += usize::try_from(result).map_err(|_| AudioDeviceError::Device)?;
        }
        let result = unsafe { (self.library.pcm_drain)(pcm.raw) };
        if result < 0 {
            return Err(map_alsa_error(result));
        }
        Ok(())
    }

    fn capture_pcm_s16le(&self, frames: u16) -> Result<Vec<u8>, AudioDeviceError> {
        let pcm = self.open_pcm(SND_PCM_STREAM_CAPTURE)?;
        let frame_count = usize::from(frames);
        let mut words = vec![0_i16; frame_count];
        let mut offset = 0;
        while offset < words.len() {
            let result = unsafe {
                (self.library.pcm_readi)(
                    pcm.raw,
                    words[offset..].as_mut_ptr().cast(),
                    (words.len() - offset) as libc::c_ulong,
                )
            };
            if result < 0 {
                recover(&self.library, pcm.raw, result)?;
                continue;
            }
            if result == 0 {
                return Err(AudioDeviceError::Device);
            }
            offset += usize::try_from(result).map_err(|_| AudioDeviceError::Device)?;
        }
        let mut samples = Vec::with_capacity(frame_count * AUDIO_SAMPLE_BYTES);
        for word in words {
            samples.extend_from_slice(&word.to_le_bytes());
        }
        Ok(samples)
    }
}

struct PcmHandle<'a> {
    raw: *mut c_void,
    library: &'a AlsaLibrary,
}

impl Drop for PcmHandle<'_> {
    fn drop(&mut self) {
        unsafe { (self.library.pcm_close)(self.raw) };
    }
}

fn recover(
    library: &AlsaLibrary,
    pcm: *mut c_void,
    error: libc::c_long,
) -> Result<(), AudioDeviceError> {
    let error = libc::c_int::try_from(error).map_err(|_| AudioDeviceError::Device)?;
    let result = unsafe { (library.pcm_recover)(pcm, error, 1) };
    if result < 0 {
        Err(map_alsa_error(result))
    } else {
        Ok(())
    }
}

fn map_alsa_error(error: libc::c_int) -> AudioDeviceError {
    match error.checked_neg() {
        Some(libc::EBUSY) | Some(libc::EAGAIN) => AudioDeviceError::Busy,
        Some(libc::ENOENT) | Some(libc::ENODEV) | Some(libc::ENXIO) => {
            AudioDeviceError::Unavailable
        }
        _ => AudioDeviceError::Device,
    }
}

#[derive(Debug)]
pub struct AudioServer<D> {
    device: D,
    trusted_uids: BTreeSet<u32>,
}

impl<D: AudioDevice> AudioServer<D> {
    pub fn new(device: D, trusted_uids: impl IntoIterator<Item = u32>) -> Self {
        Self {
            device,
            trusted_uids: trusted_uids.into_iter().collect(),
        }
    }

    pub fn serve(&self, listener: UnixListener) -> io::Result<()> {
        loop {
            let (stream, _) = listener.accept()?;
            if let Err(error) = self.handle_connection(stream) {
                eprintln!("cp0-audiod: rejected connection: {error}");
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
                    &AudioResponse::error(
                        0,
                        AudioErrorCode::InvalidRequest,
                        "invalid audio service request",
                    ),
                )
                .map_err(protocol_io)?;
                eprintln!("cp0-audiod: invalid request: {error}");
                return Ok(());
            }
        };
        if !self.trusted_uids.contains(&uid) {
            write_response(
                &mut stream,
                &AudioResponse::error(
                    request.request_id,
                    AudioErrorCode::Unauthorized,
                    "peer UID is not authorized for audio access",
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }
        write_response(&mut stream, &self.dispatch(request)).map_err(protocol_io)
    }

    pub fn dispatch(&self, request: AudioRequest) -> AudioResponse {
        let request_id = request.request_id;
        match request.command {
            AudioCommand::PlayPcmS16le { samples_base64 } => {
                let samples = match decode_samples(&samples_base64) {
                    Ok(samples) => samples,
                    Err(_) => {
                        return AudioResponse::error(
                            request_id,
                            AudioErrorCode::InvalidRequest,
                            "invalid bounded playback samples",
                        );
                    }
                };
                let frames = u16::try_from(samples.len() / AUDIO_SAMPLE_BYTES)
                    .expect("audio protocol frame count fits u16");
                match self.device.play_pcm_s16le(&samples) {
                    Ok(()) => AudioResponse::played(request_id, frames),
                    Err(error) => device_error_response(request_id, error),
                }
            }
            AudioCommand::CapturePcmS16le { frames } => {
                match self.device.capture_pcm_s16le(frames) {
                    Ok(samples)
                        if samples.len() == usize::from(frames) * AUDIO_SAMPLE_BYTES
                            && samples.len() <= MAX_AUDIO_FRAMES * AUDIO_SAMPLE_BYTES =>
                    {
                        AudioResponse::captured(request_id, &samples)
                    }
                    Ok(_) => AudioResponse::error(
                        request_id,
                        AudioErrorCode::Internal,
                        "audio device returned an invalid capture length",
                    ),
                    Err(error) => device_error_response(request_id, error),
                }
            }
        }
    }
}

fn device_error_response(request_id: u64, error: AudioDeviceError) -> AudioResponse {
    let (code, message) = match error {
        AudioDeviceError::Busy => (AudioErrorCode::Busy, "audio device is busy"),
        AudioDeviceError::Unavailable => {
            (AudioErrorCode::Unavailable, "audio device is unavailable")
        }
        AudioDeviceError::Device => (AudioErrorCode::Device, "audio device operation failed"),
    };
    AudioResponse::error(request_id, code, message)
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = mem::size_of::<libc::ucred>() as libc::socklen_t;
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
    if length as usize != mem::size_of::<libc::ucred>() {
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

fn protocol_io(error: AudioProtocolError) -> io::Error {
    match error {
        AudioProtocolError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use cp0_audio_protocol::{AudioOutcome, AudioRequest};

    use super::*;

    #[derive(Debug, Default)]
    struct MockDevice {
        played: RefCell<Vec<u8>>,
        captured: Vec<u8>,
    }

    impl AudioDevice for MockDevice {
        fn play_pcm_s16le(&self, samples: &[u8]) -> Result<(), AudioDeviceError> {
            self.played.borrow_mut().extend_from_slice(samples);
            Ok(())
        }

        fn capture_pcm_s16le(&self, frames: u16) -> Result<Vec<u8>, AudioDeviceError> {
            Ok(self
                .captured
                .iter()
                .copied()
                .take(usize::from(frames) * AUDIO_SAMPLE_BYTES)
                .collect())
        }
    }

    #[test]
    fn dispatches_bounded_playback() {
        let server = AudioServer::new(MockDevice::default(), [0]);
        let response = server.dispatch(AudioRequest::playback(4, &[1, 0, 2, 0]));
        assert_eq!(response.outcome, AudioOutcome::Played { frames: 2 });
        assert_eq!(&*server.device.played.borrow(), &[1, 0, 2, 0]);
    }

    #[test]
    fn dispatches_exact_capture_length() {
        let server = AudioServer::new(
            MockDevice {
                played: RefCell::new(Vec::new()),
                captured: vec![0, 128, 255, 127],
            },
            [0],
        );
        let response = server.dispatch(AudioRequest::capture(8, 2));
        let AudioOutcome::Captured { samples_base64 } = response.outcome else {
            panic!("expected captured audio");
        };
        assert_eq!(decode_samples(&samples_base64).unwrap(), [0, 128, 255, 127]);
    }

    #[test]
    fn rejects_invalid_device_capture_length() {
        let server = AudioServer::new(MockDevice::default(), [0]);
        let response = server.dispatch(AudioRequest::capture(9, 1));
        assert!(matches!(
            response.outcome,
            AudioOutcome::Error {
                code: AudioErrorCode::Internal,
                ..
            }
        ));
    }
}
