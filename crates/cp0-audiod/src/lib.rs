use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ffi::{CStr, CString, c_void};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Write};
use std::mem;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::ptr;
use std::time::Duration;

use cp0_audio_protocol::{
    AUDIO_SAMPLE_BYTES, AUDIO_SAMPLE_RATE_HZ, AudioCommand, AudioDirection, AudioErrorCode,
    AudioOutputState, AudioProtocolError, AudioRequest, AudioResponse, MAX_AUDIO_FRAMES,
    MUSIC_FRAME_BYTES, MUSIC_SAMPLE_RATE_HZ, decode_music_samples, decode_samples, read_request,
    write_response,
};

pub const DEFAULT_AUDIO_DEVICE: &str = "hw:ES8389Audio,0";
pub const DEFAULT_MIXER_CARD: &str = "hw:ES8389Audio";
pub const DEFAULT_KEY_SOUNDS_STATE: &str = "/var/lib/cardputerzero/audio/key-sounds.conf";
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);
const SND_PCM_STREAM_PLAYBACK: libc::c_int = 0;
const SND_PCM_STREAM_CAPTURE: libc::c_int = 1;
const SND_PCM_FORMAT_S16_LE: libc::c_int = 2;
const SND_PCM_ACCESS_RW_INTERLEAVED: libc::c_int = 3;
const PCM_LATENCY_MICROSECONDS: libc::c_uint = 20_000;
const HARDWARE_CHANNELS: libc::c_uint = 2;
const OUTPUT_VOLUME_STEP_PERCENT: u8 = 10;
const SND_MIXER_SCHN_MONO: libc::c_int = 0;
const KEY_CLICK_FRAMES: usize = 240;

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
    fn play_pcm_s16le_stereo_48k(&self, samples: &[u8]) -> Result<(), AudioDeviceError>;
    fn capture_pcm_s16le(&self, frames: u16) -> Result<Vec<u8>, AudioDeviceError>;
}

pub trait AudioOutputDevice {
    fn output_state(&self) -> Result<AudioOutputState, AudioDeviceError>;
    fn set_output_volume(&self, percent: u8) -> Result<AudioOutputState, AudioDeviceError>;
    fn set_output_muted(&self, muted: bool) -> Result<AudioOutputState, AudioDeviceError>;
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
type MixerOpen = unsafe extern "C" fn(*mut *mut c_void, libc::c_int) -> libc::c_int;
type MixerAttach = unsafe extern "C" fn(*mut c_void, *const libc::c_char) -> libc::c_int;
type MixerSelemRegister =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *mut c_void) -> libc::c_int;
type MixerFindSelem = unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void;
type SelemIdMalloc = unsafe extern "C" fn(*mut *mut c_void) -> libc::c_int;
type SelemIdFree = unsafe extern "C" fn(*mut c_void);
type SelemIdSetIndex = unsafe extern "C" fn(*mut c_void, libc::c_uint);
type SelemIdSetName = unsafe extern "C" fn(*mut c_void, *const libc::c_char);
type SelemGetVolumeRange =
    unsafe extern "C" fn(*mut c_void, *mut libc::c_long, *mut libc::c_long) -> libc::c_int;
type SelemGetVolume =
    unsafe extern "C" fn(*mut c_void, libc::c_int, *mut libc::c_long) -> libc::c_int;
type SelemSetVolumeAll = unsafe extern "C" fn(*mut c_void, libc::c_long) -> libc::c_int;
type SelemGetSwitch =
    unsafe extern "C" fn(*mut c_void, libc::c_int, *mut libc::c_int) -> libc::c_int;
type SelemSetSwitchAll = unsafe extern "C" fn(*mut c_void, libc::c_int) -> libc::c_int;

#[derive(Debug)]
struct AlsaLibrary {
    handle: *mut c_void,
    pcm_open: PcmOpen,
    pcm_set_params: PcmSetParams,
    pcm_writei: PcmWrite,
    pcm_readi: PcmRead,
    pcm_recover: PcmRecover,
    pcm_start: PcmSimple,
    pcm_close: PcmSimple,
    mixer_open: MixerOpen,
    mixer_attach: MixerAttach,
    mixer_selem_register: MixerSelemRegister,
    mixer_load: PcmSimple,
    mixer_find_selem: MixerFindSelem,
    mixer_close: PcmSimple,
    selem_id_malloc: SelemIdMalloc,
    selem_id_free: SelemIdFree,
    selem_id_set_index: SelemIdSetIndex,
    selem_id_set_name: SelemIdSetName,
    selem_get_playback_volume_range: SelemGetVolumeRange,
    selem_get_playback_volume: SelemGetVolume,
    selem_set_playback_volume_all: SelemSetVolumeAll,
    selem_get_playback_switch: SelemGetSwitch,
    selem_set_playback_switch_all: SelemSetSwitchAll,
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
                pcm_start: load_symbol(handle, c"snd_pcm_start")?,
                pcm_close: load_symbol(handle, c"snd_pcm_close")?,
                mixer_open: load_symbol(handle, c"snd_mixer_open")?,
                mixer_attach: load_symbol(handle, c"snd_mixer_attach")?,
                mixer_selem_register: load_symbol(handle, c"snd_mixer_selem_register")?,
                mixer_load: load_symbol(handle, c"snd_mixer_load")?,
                mixer_find_selem: load_symbol(handle, c"snd_mixer_find_selem")?,
                mixer_close: load_symbol(handle, c"snd_mixer_close")?,
                selem_id_malloc: load_symbol(handle, c"snd_mixer_selem_id_malloc")?,
                selem_id_free: load_symbol(handle, c"snd_mixer_selem_id_free")?,
                selem_id_set_index: load_symbol(handle, c"snd_mixer_selem_id_set_index")?,
                selem_id_set_name: load_symbol(handle, c"snd_mixer_selem_id_set_name")?,
                selem_get_playback_volume_range: load_symbol(
                    handle,
                    c"snd_mixer_selem_get_playback_volume_range",
                )?,
                selem_get_playback_volume: load_symbol(
                    handle,
                    c"snd_mixer_selem_get_playback_volume",
                )?,
                selem_set_playback_volume_all: load_symbol(
                    handle,
                    c"snd_mixer_selem_set_playback_volume_all",
                )?,
                selem_get_playback_switch: load_symbol(
                    handle,
                    c"snd_mixer_selem_get_playback_switch",
                )?,
                selem_set_playback_switch_all: load_symbol(
                    handle,
                    c"snd_mixer_selem_set_playback_switch_all",
                )?,
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
    mixer_card: CString,
    playback: RefCell<Option<*mut c_void>>,
}

impl AlsaDevice {
    pub fn open_default() -> Result<Self, AudioDeviceError> {
        Ok(Self {
            library: AlsaLibrary::load()?,
            device: CString::new(DEFAULT_AUDIO_DEVICE).expect("static ALSA device has no NUL"),
            mixer_card: CString::new(DEFAULT_MIXER_CARD).expect("static ALSA card has no NUL"),
            playback: RefCell::new(None),
        })
    }

    fn playback_pcm(&self) -> Result<*mut c_void, AudioDeviceError> {
        if let Some(raw) = *self.playback.borrow() {
            return Ok(raw);
        }
        let pcm = self.open_pcm(SND_PCM_STREAM_PLAYBACK, MUSIC_SAMPLE_RATE_HZ)?;
        let raw = pcm.raw;
        mem::forget(pcm);
        self.playback.replace(Some(raw));
        Ok(raw)
    }

    fn close_playback_pcm(&self) {
        if let Some(raw) = self.playback.replace(None) {
            unsafe { (self.library.pcm_close)(raw) };
        }
    }

    fn open_pcm(
        &self,
        stream: libc::c_int,
        sample_rate_hz: u32,
    ) -> Result<PcmHandle<'_>, AudioDeviceError> {
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
                HARDWARE_CHANNELS,
                sample_rate_hz,
                1,
                PCM_LATENCY_MICROSECONDS,
            )
        };
        if result < 0 {
            return Err(map_alsa_error(result));
        }
        Ok(handle)
    }

    fn open_mixer(&self) -> Result<MixerHandle<'_>, AudioDeviceError> {
        let mut raw = ptr::null_mut();
        let result = unsafe { (self.library.mixer_open)(&raw mut raw, 0) };
        if result < 0 || raw.is_null() {
            return Err(map_alsa_error(result));
        }
        let handle = MixerHandle {
            raw,
            library: &self.library,
        };
        for result in [
            unsafe { (self.library.mixer_attach)(raw, self.mixer_card.as_ptr()) },
            unsafe { (self.library.mixer_selem_register)(raw, ptr::null_mut(), ptr::null_mut()) },
            unsafe { (self.library.mixer_load)(raw) },
        ] {
            if result < 0 {
                return Err(map_alsa_error(result));
            }
        }
        Ok(handle)
    }

    fn mixer_element(
        &self,
        mixer: &MixerHandle<'_>,
        name: &CStr,
    ) -> Result<*mut c_void, AudioDeviceError> {
        let mut id = ptr::null_mut();
        let result = unsafe { (self.library.selem_id_malloc)(&raw mut id) };
        if result < 0 || id.is_null() {
            return Err(map_alsa_error(result));
        }
        let id = SelemIdHandle {
            raw: id,
            library: &self.library,
        };
        unsafe {
            (self.library.selem_id_set_index)(id.raw, 0);
            (self.library.selem_id_set_name)(id.raw, name.as_ptr());
        }
        let element = unsafe { (self.library.mixer_find_selem)(mixer.raw, id.raw) };
        if element.is_null() {
            Err(AudioDeviceError::Unavailable)
        } else {
            Ok(element)
        }
    }

    fn read_volume_percent(
        &self,
        mixer: &MixerHandle<'_>,
        name: &CStr,
    ) -> Result<u8, AudioDeviceError> {
        let element = self.mixer_element(mixer, name)?;
        let mut minimum = 0;
        let mut maximum = 0;
        let mut value = 0;
        for result in [
            unsafe {
                (self.library.selem_get_playback_volume_range)(
                    element,
                    &raw mut minimum,
                    &raw mut maximum,
                )
            },
            unsafe {
                (self.library.selem_get_playback_volume)(
                    element,
                    SND_MIXER_SCHN_MONO,
                    &raw mut value,
                )
            },
        ] {
            if result < 0 {
                return Err(map_alsa_error(result));
            }
        }
        if maximum <= minimum || value < minimum || value > maximum {
            return Err(AudioDeviceError::Device);
        }
        let percent = ((value - minimum) * 100 + (maximum - minimum) / 2) / (maximum - minimum);
        u8::try_from(percent).map_err(|_| AudioDeviceError::Device)
    }

    fn write_volume_percent(
        &self,
        mixer: &MixerHandle<'_>,
        name: &CStr,
        percent: u8,
    ) -> Result<(), AudioDeviceError> {
        let element = self.mixer_element(mixer, name)?;
        let mut minimum = 0;
        let mut maximum = 0;
        let result = unsafe {
            (self.library.selem_get_playback_volume_range)(
                element,
                &raw mut minimum,
                &raw mut maximum,
            )
        };
        if result < 0 {
            return Err(map_alsa_error(result));
        }
        if maximum <= minimum {
            return Err(AudioDeviceError::Device);
        }
        let value = minimum + ((maximum - minimum) * libc::c_long::from(percent) + 50) / 100;
        let result = unsafe { (self.library.selem_set_playback_volume_all)(element, value) };
        if result < 0 {
            Err(map_alsa_error(result))
        } else {
            Ok(())
        }
    }
}

impl AudioDevice for AlsaDevice {
    fn play_pcm_s16le(&self, samples: &[u8]) -> Result<(), AudioDeviceError> {
        let pcm = self.playback_pcm()?;
        let mono: Vec<i16> = samples
            .chunks_exact(AUDIO_SAMPLE_BYTES)
            .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
            .collect();
        let interleaved = mono_16k_to_stereo_48k(&mono);
        self.write_stereo_frames(pcm, &interleaved)
    }

    fn play_pcm_s16le_stereo_48k(&self, samples: &[u8]) -> Result<(), AudioDeviceError> {
        let pcm = self.playback_pcm()?;
        let interleaved: Vec<i16> = samples
            .chunks_exact(AUDIO_SAMPLE_BYTES)
            .map(|sample| i16::from_le_bytes([sample[0], sample[1]]))
            .collect();
        self.write_stereo_frames(pcm, &interleaved)
    }

    fn capture_pcm_s16le(&self, frames: u16) -> Result<Vec<u8>, AudioDeviceError> {
        let pcm = self.open_pcm(SND_PCM_STREAM_CAPTURE, AUDIO_SAMPLE_RATE_HZ)?;
        let result = unsafe { (self.library.pcm_start)(pcm.raw) };
        if result < 0 {
            return Err(map_alsa_error(result));
        }
        let frame_count = usize::from(frames);
        let mut interleaved = vec![0_i16; frame_count * HARDWARE_CHANNELS as usize];
        let mut offset = 0;
        while offset < frame_count {
            let result = unsafe {
                (self.library.pcm_readi)(
                    pcm.raw,
                    interleaved[offset * HARDWARE_CHANNELS as usize..]
                        .as_mut_ptr()
                        .cast(),
                    (frame_count - offset) as libc::c_ulong,
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
        let mono = stereo_to_mono(&interleaved);
        let mut samples = Vec::with_capacity(frame_count * AUDIO_SAMPLE_BYTES);
        for mixed in mono {
            samples.extend_from_slice(&(mixed as i16).to_le_bytes());
        }
        Ok(samples)
    }
}

impl AlsaDevice {
    fn write_stereo_frames(
        &self,
        pcm: *mut c_void,
        interleaved: &[i16],
    ) -> Result<(), AudioDeviceError> {
        let frame_count = interleaved.len() / HARDWARE_CHANNELS as usize;
        let mut offset = 0;
        while offset < frame_count {
            let result = unsafe {
                (self.library.pcm_writei)(
                    pcm,
                    interleaved[offset * HARDWARE_CHANNELS as usize..]
                        .as_ptr()
                        .cast(),
                    (frame_count - offset) as libc::c_ulong,
                )
            };
            if result < 0 {
                if let Err(error) = recover(&self.library, pcm, result) {
                    self.close_playback_pcm();
                    return Err(error);
                }
                continue;
            }
            if result == 0 {
                return Err(AudioDeviceError::Device);
            }
            offset += usize::try_from(result).map_err(|_| AudioDeviceError::Device)?;
        }
        Ok(())
    }
}

impl Drop for AlsaDevice {
    fn drop(&mut self) {
        self.close_playback_pcm();
    }
}

fn mono_16k_to_stereo_48k(mono: &[i16]) -> Vec<i16> {
    let mut interleaved = Vec::with_capacity(mono.len() * HARDWARE_CHANNELS as usize * 3);
    for sample in mono {
        for _ in 0..3 {
            interleaved.extend_from_slice(&[*sample, *sample]);
        }
    }
    interleaved
}

fn stereo_to_mono(interleaved: &[i16]) -> Vec<i16> {
    interleaved
        .chunks_exact(HARDWARE_CHANNELS as usize)
        .map(|channels| ((i32::from(channels[0]) + i32::from(channels[1])) / 2) as i16)
        .collect()
}

impl AudioOutputDevice for AlsaDevice {
    fn output_state(&self) -> Result<AudioOutputState, AudioDeviceError> {
        let mixer = self.open_mixer()?;
        let left = self.read_volume_percent(&mixer, c"DACL")?;
        let right = self.read_volume_percent(&mixer, c"DACR")?;
        let speaker = self.mixer_element(&mixer, c"Speaker")?;
        let mut enabled = 0;
        let result = unsafe {
            (self.library.selem_get_playback_switch)(speaker, SND_MIXER_SCHN_MONO, &raw mut enabled)
        };
        if result < 0 {
            return Err(map_alsa_error(result));
        }
        let volume = (u16::from(left) + u16::from(right) + 1) / 2;
        Ok(AudioOutputState::available(
            u8::try_from(volume).map_err(|_| AudioDeviceError::Device)?,
            enabled == 0,
        ))
    }

    fn set_output_volume(&self, percent: u8) -> Result<AudioOutputState, AudioDeviceError> {
        if percent > 100 {
            return Err(AudioDeviceError::Device);
        }
        let mixer = self.open_mixer()?;
        self.write_volume_percent(&mixer, c"DACL", percent)?;
        self.write_volume_percent(&mixer, c"DACR", percent)?;
        drop(mixer);
        self.output_state()
    }

    fn set_output_muted(&self, muted: bool) -> Result<AudioOutputState, AudioDeviceError> {
        let mixer = self.open_mixer()?;
        let speaker = self.mixer_element(&mixer, c"Speaker")?;
        let result = unsafe {
            (self.library.selem_set_playback_switch_all)(speaker, libc::c_int::from(!muted))
        };
        if result < 0 {
            return Err(map_alsa_error(result));
        }
        drop(mixer);
        self.output_state()
    }
}

struct PcmHandle<'a> {
    raw: *mut c_void,
    library: &'a AlsaLibrary,
}

struct MixerHandle<'a> {
    raw: *mut c_void,
    library: &'a AlsaLibrary,
}

impl Drop for MixerHandle<'_> {
    fn drop(&mut self) {
        unsafe { (self.library.mixer_close)(self.raw) };
    }
}

struct SelemIdHandle<'a> {
    raw: *mut c_void,
    library: &'a AlsaLibrary,
}

impl Drop for SelemIdHandle<'_> {
    fn drop(&mut self) {
        unsafe { (self.library.selem_id_free)(self.raw) };
    }
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
    trusted_pcm_uids: BTreeSet<u32>,
    trusted_output_uids: BTreeSet<u32>,
    key_sounds_enabled: std::sync::atomic::AtomicBool,
    key_sounds_state: Option<PathBuf>,
}

impl<D: AudioDevice + AudioOutputDevice> AudioServer<D> {
    pub fn new(
        device: D,
        trusted_pcm_uids: impl IntoIterator<Item = u32>,
        trusted_output_uids: impl IntoIterator<Item = u32>,
    ) -> Self {
        Self {
            device,
            trusted_pcm_uids: trusted_pcm_uids.into_iter().collect(),
            trusted_output_uids: trusted_output_uids.into_iter().collect(),
            key_sounds_enabled: std::sync::atomic::AtomicBool::new(true),
            key_sounds_state: None,
        }
    }

    pub fn new_with_key_sounds_state(
        device: D,
        trusted_pcm_uids: impl IntoIterator<Item = u32>,
        trusted_output_uids: impl IntoIterator<Item = u32>,
        state_path: impl Into<PathBuf>,
    ) -> io::Result<Self> {
        let state_path = state_path.into();
        let enabled = load_key_sounds_state(&state_path)?.unwrap_or(true);
        Ok(Self {
            device,
            trusted_pcm_uids: trusted_pcm_uids.into_iter().collect(),
            trusted_output_uids: trusted_output_uids.into_iter().collect(),
            key_sounds_enabled: std::sync::atomic::AtomicBool::new(enabled),
            key_sounds_state: Some(state_path),
        })
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
        let authorized = self.command_authorized(uid, &request.command);
        if !authorized {
            write_response(
                &mut stream,
                &AudioResponse::error(
                    request.request_id,
                    AudioErrorCode::Unauthorized,
                    "peer UID is not authorized for this audio operation",
                ),
            )
            .map_err(protocol_io)?;
            return Ok(());
        }
        let mutating = matches!(
            request.command,
            AudioCommand::SetOutputVolume { .. }
                | AudioCommand::AdjustOutputVolume { .. }
                | AudioCommand::SetOutputMuted { .. }
        );
        let response = self.dispatch(request);
        if mutating {
            if let cp0_audio_protocol::AudioOutcome::OutputState { state } = &response.outcome {
                eprintln!(
                    "cp0-audiod: audit uid={uid} volume_percent={} muted={}",
                    state.volume_percent.unwrap_or(0),
                    state.muted.unwrap_or(false)
                );
            }
        }
        write_response(&mut stream, &response).map_err(protocol_io)
    }

    pub fn command_authorized(&self, uid: u32, command: &AudioCommand) -> bool {
        if matches!(command, AudioCommand::PlayKeyClick {}) {
            self.trusted_output_uids.contains(&uid) || self.trusted_pcm_uids.contains(&uid)
        } else if is_output_command(command) {
            self.trusted_output_uids.contains(&uid)
        } else {
            self.trusted_pcm_uids.contains(&uid)
        }
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
            AudioCommand::PlayPcmS16leStereo48k { samples_base64 } => {
                let samples = match decode_music_samples(&samples_base64) {
                    Ok(samples) => samples,
                    Err(_) => {
                        return AudioResponse::error(
                            request_id,
                            AudioErrorCode::InvalidRequest,
                            "invalid bounded stereo playback samples",
                        );
                    }
                };
                let frames = u16::try_from(samples.len() / MUSIC_FRAME_BYTES)
                    .expect("music protocol frame count fits u16");
                match self.device.play_pcm_s16le_stereo_48k(&samples) {
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
            AudioCommand::GetOutputState {} => match self.device.output_state() {
                Ok(state) => AudioResponse::output_state(request_id, state),
                Err(AudioDeviceError::Unavailable) => {
                    AudioResponse::output_state(request_id, AudioOutputState::unavailable())
                }
                Err(error) => device_error_response(request_id, error),
            },
            AudioCommand::SetOutputVolume { percent } => {
                self.output_response(request_id, self.device.set_output_volume(percent))
            }
            AudioCommand::AdjustOutputVolume { direction } => {
                let state = match self.device.output_state() {
                    Ok(state) => state,
                    Err(error) => return device_error_response(request_id, error),
                };
                let Some(current) = state.volume_percent else {
                    return AudioResponse::output_state(
                        request_id,
                        AudioOutputState::unavailable(),
                    );
                };
                let percent = match direction {
                    AudioDirection::Decrease => current.saturating_sub(OUTPUT_VOLUME_STEP_PERCENT),
                    AudioDirection::Increase => {
                        current.saturating_add(OUTPUT_VOLUME_STEP_PERCENT).min(100)
                    }
                };
                let result = self
                    .device
                    .set_output_volume(percent)
                    .and_then(|_| self.device.set_output_muted(false));
                self.output_response(request_id, result)
            }
            AudioCommand::SetOutputMuted { muted } => {
                self.output_response(request_id, self.device.set_output_muted(muted))
            }
            AudioCommand::SetKeySoundsEnabled { enabled } => {
                if let Some(path) = &self.key_sounds_state
                    && let Err(error) = save_key_sounds_state(path, enabled)
                {
                    eprintln!("cp0-audiod: cannot persist key sounds setting: {error}");
                    return AudioResponse::error(
                        request_id,
                        AudioErrorCode::Internal,
                        "key sounds setting could not be persisted",
                    );
                }
                self.key_sounds_enabled
                    .store(enabled, std::sync::atomic::Ordering::Relaxed);
                AudioResponse::key_sounds_state(request_id, enabled)
            }
            AudioCommand::PlayKeyClick {} => {
                if !self
                    .key_sounds_enabled
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    return AudioResponse::played(
                        request_id,
                        u16::try_from(KEY_CLICK_FRAMES).expect("key click fits u16"),
                    );
                }
                let samples = key_click_samples();
                match self.device.play_pcm_s16le(&samples) {
                    Ok(()) => AudioResponse::played(
                        request_id,
                        u16::try_from(KEY_CLICK_FRAMES).expect("key click fits u16"),
                    ),
                    Err(error) => device_error_response(request_id, error),
                }
            }
        }
    }

    fn output_response(
        &self,
        request_id: u64,
        result: Result<AudioOutputState, AudioDeviceError>,
    ) -> AudioResponse {
        match result {
            Ok(state) => AudioResponse::output_state(request_id, state),
            Err(error) => device_error_response(request_id, error),
        }
    }
}

fn load_key_sounds_state(path: &Path) -> io::Result<Option<bool>> {
    let encoded = match fs::read(path) {
        Ok(encoded) => encoded,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    match encoded.as_slice() {
        b"version=1\nenabled=0\n" => Ok(Some(false)),
        b"version=1\nenabled=1\n" => Ok(Some(true)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid key sounds state",
        )),
    }
}

fn save_key_sounds_state(path: &Path, enabled: bool) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "state path has no parent"))?;
    let temporary = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&temporary)?;
    file.write_all(if enabled {
        b"version=1\nenabled=1\n"
    } else {
        b"version=1\nenabled=0\n"
    })?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    File::open(parent)?.sync_all()
}

fn is_output_command(command: &AudioCommand) -> bool {
    matches!(
        command,
        AudioCommand::GetOutputState {}
            | AudioCommand::SetOutputVolume { .. }
            | AudioCommand::AdjustOutputVolume { .. }
            | AudioCommand::SetOutputMuted { .. }
            | AudioCommand::SetKeySoundsEnabled { .. }
            | AudioCommand::PlayKeyClick {}
    )
}

fn key_click_samples() -> Vec<u8> {
    let mut samples = Vec::with_capacity(KEY_CLICK_FRAMES * AUDIO_SAMPLE_BYTES);
    for frame in 0..KEY_CLICK_FRAMES {
        let envelope = i32::try_from(KEY_CLICK_FRAMES - frame).expect("bounded frame");
        let polarity = if frame % 16 < 8 { 1 } else { -1 };
        let value = (polarity * 1800 * envelope
            / i32::try_from(KEY_CLICK_FRAMES).expect("bounded frames")) as i16;
        samples.extend_from_slice(&value.to_le_bytes());
    }
    samples
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use cp0_audio_protocol::{AudioOutcome, AudioRequest};

    use super::*;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct MockDevice {
        played: RefCell<Vec<u8>>,
        captured: Vec<u8>,
        output: RefCell<AudioOutputState>,
    }

    impl Default for MockDevice {
        fn default() -> Self {
            Self {
                played: RefCell::new(Vec::new()),
                captured: Vec::new(),
                output: RefCell::new(AudioOutputState::available(60, false)),
            }
        }
    }

    impl AudioDevice for MockDevice {
        fn play_pcm_s16le(&self, samples: &[u8]) -> Result<(), AudioDeviceError> {
            self.played.borrow_mut().extend_from_slice(samples);
            Ok(())
        }

        fn play_pcm_s16le_stereo_48k(&self, samples: &[u8]) -> Result<(), AudioDeviceError> {
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

    impl AudioOutputDevice for MockDevice {
        fn output_state(&self) -> Result<AudioOutputState, AudioDeviceError> {
            Ok(*self.output.borrow())
        }

        fn set_output_volume(&self, percent: u8) -> Result<AudioOutputState, AudioDeviceError> {
            self.output.borrow_mut().volume_percent = Some(percent);
            self.output_state()
        }

        fn set_output_muted(&self, muted: bool) -> Result<AudioOutputState, AudioDeviceError> {
            self.output.borrow_mut().muted = Some(muted);
            self.output_state()
        }
    }

    #[test]
    fn dispatches_bounded_playback() {
        let server = AudioServer::new(MockDevice::default(), [0], [1000]);
        let response = server.dispatch(AudioRequest::playback(4, &[1, 0, 2, 0]));
        assert_eq!(response.outcome, AudioOutcome::Played { frames: 2 });
        assert_eq!(&*server.device.played.borrow(), &[1, 0, 2, 0]);

        let response = server.dispatch(AudioRequest::playback_stereo_48k(5, &[1, 0, 2, 0]));
        assert_eq!(response.outcome, AudioOutcome::Played { frames: 1 });
        assert_eq!(&*server.device.played.borrow(), &[1, 0, 2, 0, 1, 0, 2, 0]);
    }

    #[test]
    fn dispatches_exact_capture_length() {
        let server = AudioServer::new(
            MockDevice {
                played: RefCell::new(Vec::new()),
                captured: vec![0, 128, 255, 127],
                output: RefCell::new(AudioOutputState::available(60, false)),
            },
            [0],
            [1000],
        );
        let response = server.dispatch(AudioRequest::capture(8, 2));
        let AudioOutcome::Captured { samples_base64 } = response.outcome else {
            panic!("expected captured audio");
        };
        assert_eq!(decode_samples(&samples_base64).unwrap(), [0, 128, 255, 127]);
    }

    #[test]
    fn rejects_invalid_device_capture_length() {
        let server = AudioServer::new(MockDevice::default(), [0], [1000]);
        let response = server.dispatch(AudioRequest::capture(9, 1));
        assert!(matches!(
            response.outcome,
            AudioOutcome::Error {
                code: AudioErrorCode::Internal,
                ..
            }
        ));
    }

    #[test]
    fn reports_and_adjusts_output_state() {
        let server = AudioServer::new(MockDevice::default(), [0], [1000]);
        assert_eq!(
            server.dispatch(AudioRequest::get_output_state(10)).outcome,
            AudioOutcome::OutputState {
                state: AudioOutputState::available(60, false)
            }
        );
        assert_eq!(
            server
                .dispatch(AudioRequest::adjust_output_volume(
                    11,
                    AudioDirection::Decrease,
                ))
                .outcome,
            AudioOutcome::OutputState {
                state: AudioOutputState::available(50, false)
            }
        );
        assert_eq!(
            server
                .dispatch(AudioRequest::set_output_muted(12, true))
                .outcome,
            AudioOutcome::OutputState {
                state: AudioOutputState::available(50, true)
            }
        );
    }

    #[test]
    fn separates_pcm_and_shell_output_authority() {
        let server = AudioServer::new(MockDevice::default(), [0], [1000]);
        assert!(server.command_authorized(0, &AudioRequest::playback(1, &[0, 0]).command));
        assert!(!server.command_authorized(1000, &AudioRequest::playback(1, &[0, 0]).command));
        assert!(server.command_authorized(1000, &AudioRequest::get_output_state(2).command));
        assert!(server.command_authorized(1000, &AudioRequest::play_key_click(4).command));
        assert!(!server.command_authorized(0, &AudioRequest::set_output_muted(3, true).command));
    }

    #[test]
    fn plays_a_bounded_system_key_click() {
        let server = AudioServer::new(MockDevice::default(), [0], [1000]);
        assert_eq!(
            server.dispatch(AudioRequest::play_key_click(20)).outcome,
            AudioOutcome::Played {
                frames: KEY_CLICK_FRAMES as u16
            }
        );
        let samples = server.device.played.borrow();
        assert_eq!(samples.len(), KEY_CLICK_FRAMES * AUDIO_SAMPLE_BYTES);
        assert!(samples.iter().any(|sample| *sample != 0));
    }

    #[test]
    fn disabled_key_sounds_do_not_touch_the_audio_device() {
        let server = AudioServer::new(MockDevice::default(), [0], [1000]);
        assert_eq!(
            server
                .dispatch(AudioRequest::set_key_sounds_enabled(21, false))
                .outcome,
            AudioOutcome::KeySoundsState { enabled: false }
        );
        assert_eq!(
            server.dispatch(AudioRequest::play_key_click(22)).outcome,
            AudioOutcome::Played {
                frames: KEY_CLICK_FRAMES as u16
            }
        );
        assert!(server.device.played.borrow().is_empty());
    }

    #[test]
    fn key_sounds_setting_survives_audio_server_restart() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!(
                "audiod-key-sounds-{}-{}",
                std::process::id(),
                TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&root).unwrap();
        let state = root.join("key-sounds.conf");
        let server =
            AudioServer::new_with_key_sounds_state(MockDevice::default(), [0], [1000], &state)
                .unwrap();
        assert!(matches!(
            server
                .dispatch(AudioRequest::set_key_sounds_enabled(30, false))
                .outcome,
            AudioOutcome::KeySoundsState { enabled: false }
        ));
        drop(server);

        let restarted =
            AudioServer::new_with_key_sounds_state(MockDevice::default(), [0], [1000], &state)
                .unwrap();
        restarted.dispatch(AudioRequest::play_key_click(31));
        assert!(restarted.device.played.borrow().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adapts_the_mono_sdk_contract_to_stereo_hardware() {
        assert_eq!(
            mono_16k_to_stereo_48k(&[i16::MIN, 0, i16::MAX]),
            [
                i16::MIN,
                i16::MIN,
                i16::MIN,
                i16::MIN,
                i16::MIN,
                i16::MIN,
                0,
                0,
                0,
                0,
                0,
                0,
                i16::MAX,
                i16::MAX,
                i16::MAX,
                i16::MAX,
                i16::MAX,
                i16::MAX,
            ]
        );
        assert_eq!(stereo_to_mono(&[i16::MIN, i16::MAX, 100, 300]), [0, 200]);
    }
}
