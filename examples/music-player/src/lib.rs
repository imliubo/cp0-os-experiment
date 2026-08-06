#![no_std]

#[cfg(not(test))]
use core::panic::PanicInfo;
use cp0_sdk::{
    Error, audio,
    display::{self, Rect},
    documents::{self, Document},
    input,
    media::{self, Action, ActionCapabilities, PlaybackState},
    network, system,
    ui::{Canvas, color},
};

const KEY_BACKSPACE: u16 = 14;
const KEY_ENTER: u16 = 28;
const KEY_R: u16 = 19;
const KEY_F: u16 = 33;
const KEY_SPACE: u16 = 57;
const KEY_UP: u16 = 103;
const KEY_DOWN: u16 = 108;
const URL_BYTES: usize = 192;
const HEADER_BYTES: usize = 2048;
const STREAM_BYTES: usize = network::MAX_RANGE_BODY_BYTES;
const STREAM_SAMPLES: usize = STREAM_BYTES / 2;
const FRAME_BYTES: usize = display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;
const PLAYER_REDRAW_INTERVAL_MS: u64 = 250;

#[cfg(not(test))]
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];
#[cfg(not(test))]
static mut STREAM: [i16; STREAM_SAMPLES] = [0; STREAM_SAMPLES];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceKind {
    Local,
    Network,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlayerResult {
    Complete,
    Stopped,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WavInfo {
    data_offset: u64,
    data_bytes: u64,
}

struct Url {
    bytes: [u8; URL_BYTES],
    length: usize,
}

impl Url {
    fn new() -> Self {
        let mut value = Self {
            bytes: [0; URL_BYTES],
            length: 8,
        };
        value.bytes[..8].copy_from_slice(b"https://");
        value
    }

    fn push(&mut self, byte: u8) -> bool {
        if self.length == self.bytes.len() || !(b'!'..=b'~').contains(&byte) {
            return false;
        }
        self.bytes[self.length] = byte;
        self.length += 1;
        true
    }

    fn backspace(&mut self) -> bool {
        if self.length <= 8 {
            return false;
        }
        self.length -= 1;
        self.bytes[self.length] = 0;
        true
    }

    fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.length]) }
    }
}

trait Reader {
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, Error>;
    fn length(&self) -> Option<u64>;
}

struct LocalReader<'a> {
    document: &'a Document,
}

impl Reader for LocalReader<'_> {
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, Error> {
        let maximum = buffer.len().min(documents::MAX_READ_BYTES);
        self.document.read(offset, &mut buffer[..maximum])
    }

    fn length(&self) -> Option<u64> {
        Some(u64::from(self.document.len()))
    }
}

struct NetworkReader<'a> {
    url: &'a str,
}

impl Reader for NetworkReader<'_> {
    fn read_at(&self, offset: u64, buffer: &mut [u8]) -> Result<usize, Error> {
        let maximum = buffer.len().min(network::MAX_RANGE_BODY_BYTES);
        for _ in 0..600 {
            match network::http_get_range(self.url, offset, &mut buffer[..maximum]) {
                Ok(response) if response.status_code == 206 => return Ok(response.body_length),
                Ok(response) if response.status_code == 416 => return Ok(0),
                Ok(_) => return Err(Error::InvalidArgument),
                Err(Error::ResourceLimit | Error::Unavailable) => {
                    let _ = input::poll_key_event(100);
                }
                Err(error) => return Err(error),
            }
        }
        Err(Error::Unavailable)
    }

    fn length(&self) -> Option<u64> {
        None
    }
}

fn little_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn little_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn parse_wav_header(header: &[u8], source_length: Option<u64>) -> Result<WavInfo, ()> {
    if header.len() < 44 || &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(());
    }
    let mut cursor = 12_usize;
    let mut format_valid = false;
    while cursor.checked_add(8).is_some_and(|end| end <= header.len()) {
        let chunk = &header[cursor..cursor + 4];
        let size = little_u32(&header[cursor + 4..cursor + 8]) as usize;
        let data = cursor + 8;
        if chunk == b"fmt " {
            if size < 16 || data.checked_add(16).is_none_or(|end| end > header.len()) {
                return Err(());
            }
            format_valid = little_u16(&header[data..data + 2]) == 1
                && little_u16(&header[data + 2..data + 4]) == 2
                && little_u32(&header[data + 4..data + 8]) == audio::MUSIC_SAMPLE_RATE_HZ
                && little_u32(&header[data + 8..data + 12]) == 192_000
                && little_u16(&header[data + 12..data + 14]) == 4
                && little_u16(&header[data + 14..data + 16]) == 16;
        } else if chunk == b"data" {
            let data_offset = data as u64;
            let data_bytes = size as u64;
            let data_end = data_offset.checked_add(data_bytes).ok_or(())?;
            if !format_valid
                || data_bytes == 0
                || data_bytes % 4 != 0
                || data_end > network::MAX_RESOURCE_BYTES
                || source_length.is_some_and(|length| data_end > length)
            {
                return Err(());
            }
            return Ok(WavInfo {
                data_offset,
                data_bytes,
            });
        }
        cursor = data
            .checked_add(size)
            .and_then(|next| next.checked_add(size & 1))
            .ok_or(())?;
    }
    Err(())
}

#[cfg(not(test))]
fn frame() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(FRAME).cast(), FRAME_BYTES) }
}

#[cfg(not(test))]
fn stream() -> &'static mut [i16] {
    unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(STREAM).cast(), STREAM_SAMPLES)
    }
}

#[cfg(not(test))]
fn stream_bytes(samples: &mut [i16]) -> &mut [u8] {
    unsafe {
        core::slice::from_raw_parts_mut(
            samples.as_mut_ptr().cast(),
            core::mem::size_of_val(samples),
        )
    }
}

#[cfg(not(test))]
fn draw_menu(pixels: &mut [u8], selected: usize, status: &str) {
    let mut canvas = Canvas::new(pixels, display::WIDTH, display::STANDARD_HEIGHT).unwrap();
    canvas.clear(color::BACKGROUND);
    canvas.draw_text(12, 8, "MUSIC", color::TEXT, 2);
    canvas.draw_text(12, 29, "48 KHZ PCM WAV", color::MUTED, 1);
    for (index, label) in ["LOCAL LIBRARY", "NETWORK URL"].iter().enumerate() {
        let y = 52 + index as u16 * 38;
        canvas.fill_rect(
            Rect {
                x: 12,
                y,
                width: 296,
                height: 30,
            },
            if selected == index {
                color::SURFACE_RAISED
            } else {
                color::SURFACE
            },
        );
        canvas.stroke_rect(
            Rect {
                x: 12,
                y,
                width: 296,
                height: 30,
            },
            if selected == index {
                color::ACCENT
            } else {
                color::MUTED
            },
        );
        canvas.draw_text(26, y + 11, label, color::TEXT, 1);
    }
    canvas.draw_text(12, 132, status, color::MUTED, 1);
}

#[cfg(not(test))]
fn draw_url(pixels: &mut [u8], url: &Url) {
    let mut canvas = Canvas::new(pixels, display::WIDTH, display::STANDARD_HEIGHT).unwrap();
    canvas.clear(color::BACKGROUND);
    canvas.draw_text(12, 8, "NETWORK MUSIC", color::TEXT, 2);
    canvas.draw_text(12, 31, "HTTPS WAV URL", color::MUTED, 1);
    canvas.fill_rect(
        Rect {
            x: 10,
            y: 48,
            width: 300,
            height: 52,
        },
        color::SURFACE,
    );
    canvas.stroke_rect(
        Rect {
            x: 10,
            y: 48,
            width: 300,
            height: 52,
        },
        color::ACCENT,
    );
    let value = url.as_str();
    let visible = if value.len() > 47 {
        &value[value.len() - 47..]
    } else {
        value
    };
    canvas.draw_text(17, 69, visible, color::TEXT, 1);
    canvas.draw_text(12, 118, "ENTER PLAY   BACKSPACE EDIT", color::MUTED, 1);
}

#[cfg(not(test))]
fn draw_player(pixels: &mut [u8], kind: SourceKind, paused: bool, position: u64, total: u64) {
    let mut canvas = Canvas::new(pixels, display::WIDTH, display::STANDARD_HEIGHT).unwrap();
    canvas.clear(color::BACKGROUND);
    canvas.draw_text(12, 8, "NOW PLAYING", color::TEXT, 2);
    canvas.draw_text(
        12,
        31,
        if kind == SourceKind::Local {
            "LOCAL LIBRARY"
        } else {
            "HTTPS STREAM"
        },
        color::MUTED,
        1,
    );
    canvas.fill_rect(
        Rect {
            x: 12,
            y: 52,
            width: 296,
            height: 42,
        },
        color::SURFACE,
    );
    canvas.draw_text(
        if paused { 116 } else { 110 },
        66,
        if paused { "PAUSED" } else { "PLAYING" },
        if paused {
            color::WARNING
        } else {
            color::SUCCESS
        },
        2,
    );
    let progress = if total == 0 {
        0
    } else {
        ((position.saturating_mul(288) / total).min(288)) as u16
    };
    canvas.fill_rect(
        Rect {
            x: 16,
            y: 106,
            width: 288,
            height: 6,
        },
        color::MUTED,
    );
    if progress > 0 {
        canvas.fill_rect(
            Rect {
                x: 16,
                y: 106,
                width: progress,
                height: 6,
            },
            color::ACCENT,
        );
    }
    canvas.draw_text(34, 128, "SPACE PAUSE   R RESTART   F STOP", color::MUTED, 1);
}

#[cfg(not(test))]
fn open_document() -> Result<Document, Error> {
    for _ in 0..600 {
        match documents::open() {
            Err(Error::ResourceLimit | Error::Unavailable) => {
                let _ = input::poll_key_event(100);
            }
            result => return result,
        }
    }
    Err(Error::Unavailable)
}

#[cfg(not(test))]
fn player_action(paused: &mut bool, restart: &mut bool) -> bool {
    if let Ok(Some(action)) = media::take_action() {
        match action {
            Action::PlayPause => *paused = !*paused,
            Action::Previous => *restart = true,
            Action::Next => return true,
        }
    }
    match input::poll_key_event(if *paused { 100 } else { 0 }) {
        Ok(Some(event)) if event.pressed && event.code == KEY_SPACE => *paused = !*paused,
        Ok(Some(event)) if event.pressed && event.code == KEY_R => *restart = true,
        Ok(Some(event)) if event.pressed && event.code == KEY_F => return true,
        _ => {}
    }
    false
}

#[cfg(not(test))]
fn play(reader: &impl Reader, kind: SourceKind, pixels: &mut [u8]) -> PlayerResult {
    let mut header = [0_u8; HEADER_BYTES];
    let header_count = match reader.read_at(0, &mut header) {
        Ok(count) => count,
        Err(_) => return PlayerResult::Failed,
    };
    let info = match parse_wav_header(&header[..header_count], reader.length()) {
        Ok(info) => info,
        Err(()) => return PlayerResult::Failed,
    };
    let mut position = 0_u64;
    let mut paused = false;
    let mut restart = false;
    let mut last_redraw = system::monotonic_milliseconds();
    let _ = media::update_session(PlaybackState::Playing, ActionCapabilities::ALL);
    draw_player(pixels, kind, paused, position, info.data_bytes);
    let _ = display::present_rgb565(pixels, &[]);

    while position < info.data_bytes {
        let was_paused = paused;
        if player_action(&mut paused, &mut restart) {
            let _ = media::update_session(PlaybackState::Inactive, ActionCapabilities::NONE);
            return PlayerResult::Stopped;
        }
        if restart {
            position = 0;
            restart = false;
        }
        if paused != was_paused {
            let state = if paused {
                PlaybackState::Paused
            } else {
                PlaybackState::Playing
            };
            let _ = media::update_session(state, ActionCapabilities::ALL);
            draw_player(pixels, kind, paused, position, info.data_bytes);
            let _ = display::present_rgb565(pixels, &[]);
            last_redraw = system::monotonic_milliseconds();
        }
        if paused {
            continue;
        }
        let samples = stream();
        let bytes = stream_bytes(samples);
        let wanted = (info.data_bytes - position).min(bytes.len() as u64) as usize;
        let count = match reader.read_at(info.data_offset + position, &mut bytes[..wanted]) {
            Ok(count) => count.min(wanted),
            Err(_) => {
                let _ = media::update_session(PlaybackState::Inactive, ActionCapabilities::NONE);
                return PlayerResult::Failed;
            }
        };
        let usable = count - count % 4;
        if usable == 0 {
            let _ = media::update_session(PlaybackState::Inactive, ActionCapabilities::NONE);
            return PlayerResult::Failed;
        }
        for chunk in
            samples[..usable / 2].chunks(audio::MAX_MUSIC_FRAMES * audio::MUSIC_CHANNELS as usize)
        {
            if audio::play_pcm_s16le_stereo_48khz(chunk).is_err() {
                let _ = media::update_session(PlaybackState::Inactive, ActionCapabilities::NONE);
                return PlayerResult::Failed;
            }
        }
        position += usable as u64;
        let now = system::monotonic_milliseconds();
        if position == info.data_bytes
            || now.saturating_sub(last_redraw) >= PLAYER_REDRAW_INTERVAL_MS
        {
            draw_player(pixels, kind, false, position, info.data_bytes);
            let _ = display::present_rgb565(pixels, &[]);
            last_redraw = now;
        }
    }
    let _ = media::update_session(PlaybackState::Inactive, ActionCapabilities::NONE);
    PlayerResult::Complete
}

fn key_character(code: u16, shifted: bool) -> Option<u8> {
    let byte = match code {
        2..=10 => {
            let normal = b"123456789";
            let shifted_row = b"!@#$%^&*(";
            if shifted {
                shifted_row[(code - 2) as usize]
            } else {
                normal[(code - 2) as usize]
            }
        }
        11 => {
            if shifted {
                b')'
            } else {
                b'0'
            }
        }
        12 => {
            if shifted {
                b'_'
            } else {
                b'-'
            }
        }
        13 => {
            if shifted {
                b'+'
            } else {
                b'='
            }
        }
        16..=25 => b"qwertyuiop"[(code - 16) as usize],
        30..=38 => b"asdfghjkl"[(code - 30) as usize],
        44..=50 => b"zxcvbnm"[(code - 44) as usize],
        51 => {
            if shifted {
                b'<'
            } else {
                b','
            }
        }
        52 => {
            if shifted {
                b'>'
            } else {
                b'.'
            }
        }
        53 => {
            if shifted {
                b'?'
            } else {
                b'/'
            }
        }
        _ => return None,
    };
    Some(if shifted && byte.is_ascii_lowercase() {
        byte.to_ascii_uppercase()
    } else {
        byte
    })
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let pixels = frame();
    let mut selected = 0_usize;
    let mut url = Url::new();
    let mut editing_url = false;
    let mut status = "SELECT A SOURCE";
    loop {
        if editing_url {
            draw_url(pixels, &url);
        } else {
            draw_menu(pixels, selected, status);
        }
        if display::present_rgb565(pixels, &[]).is_err() {
            return 1;
        }
        let event = match input::poll_key_event(250) {
            Ok(Some(event)) if event.pressed => event,
            Ok(_) => continue,
            Err(_) => return 1,
        };
        if editing_url {
            if event.code == KEY_BACKSPACE {
                url.backspace();
            } else if event.code == KEY_ENTER && url.length > 8 {
                let reader = NetworkReader { url: url.as_str() };
                status = match play(&reader, SourceKind::Network, pixels) {
                    PlayerResult::Complete => "NETWORK TRACK COMPLETE",
                    PlayerResult::Stopped => "NETWORK TRACK STOPPED",
                    PlayerResult::Failed => "CHECK URL, FORMAT, PERMISSIONS",
                };
                editing_url = false;
            } else if let Some(byte) =
                key_character(event.code, event.modifiers & input::MODIFIER_SHIFT != 0)
            {
                url.push(byte);
            }
            continue;
        }
        match event.code {
            KEY_UP | KEY_DOWN => selected ^= 1,
            KEY_ENTER if selected == 0 => match open_document() {
                Ok(document) => {
                    let result = play(
                        &LocalReader {
                            document: &document,
                        },
                        SourceKind::Local,
                        pixels,
                    );
                    let _ = document.close();
                    status = match result {
                        PlayerResult::Complete => "LOCAL TRACK COMPLETE",
                        PlayerResult::Stopped => "LOCAL TRACK STOPPED",
                        PlayerResult::Failed => "USE 48 KHZ STEREO PCM WAV",
                    };
                }
                Err(Error::Denied) => status = "DOCUMENT PERMISSION DENIED",
                Err(_) => status = "LOCAL LIBRARY UNAVAILABLE",
            },
            KEY_ENTER => editing_url = true,
            _ => {}
        }
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav() -> [u8; 48] {
        let mut bytes = [0_u8; 48];
        bytes[..4].copy_from_slice(b"RIFF");
        bytes[4..8].copy_from_slice(&40_u32.to_le_bytes());
        bytes[8..12].copy_from_slice(b"WAVE");
        bytes[12..16].copy_from_slice(b"fmt ");
        bytes[16..20].copy_from_slice(&16_u32.to_le_bytes());
        bytes[20..22].copy_from_slice(&1_u16.to_le_bytes());
        bytes[22..24].copy_from_slice(&2_u16.to_le_bytes());
        bytes[24..28].copy_from_slice(&48_000_u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&192_000_u32.to_le_bytes());
        bytes[32..34].copy_from_slice(&4_u16.to_le_bytes());
        bytes[34..36].copy_from_slice(&16_u16.to_le_bytes());
        bytes[36..40].copy_from_slice(b"data");
        bytes[40..44].copy_from_slice(&4_u32.to_le_bytes());
        bytes
    }

    #[test]
    fn accepts_only_the_music_pcm_contract() {
        assert_eq!(
            parse_wav_header(&wav(), Some(48)),
            Ok(WavInfo {
                data_offset: 44,
                data_bytes: 4,
            })
        );
        let mut invalid = wav();
        invalid[24..28].copy_from_slice(&44_100_u32.to_le_bytes());
        assert_eq!(parse_wav_header(&invalid, Some(48)), Err(()));

        let mut oversized = wav();
        oversized[40..44].copy_from_slice(&(network::MAX_RESOURCE_BYTES as u32).to_le_bytes());
        assert_eq!(parse_wav_header(&oversized, None), Err(()));
    }

    #[test]
    fn url_editor_preserves_https_prefix_and_symbols() {
        let mut url = Url::new();
        assert!(!url.backspace());
        assert!(url.push(b'a'));
        assert!(url.push(b'.'));
        assert!(url.push(b'/'));
        assert_eq!(url.as_str(), "https://a./");
        assert_eq!(key_character(53, false), Some(b'/'));
        assert_eq!(key_character(52, false), Some(b'.'));
    }
}
