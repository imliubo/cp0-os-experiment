#![no_std]

use core::panic::PanicInfo;
use cp0_sdk::{Error, audio, display, gpio, storage, system};

const FRAME_BYTES: usize = display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;
const MARKER_KEY: &str = "acceptance.marker";
const MARKER_VALUE: &[u8] = b"cp0-storage-v1";
const QUOTA_KEYS: u16 = 128;

static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];
static mut AUDIO_SAMPLES: [i16; audio::MAX_FRAMES] = [0; audio::MAX_FRAMES];
static mut QUOTA_VALUE: [u8; storage::MAX_VALUE_BYTES] = [0xa5; storage::MAX_VALUE_BYTES];

fn error_status(error: Error) -> &'static str {
    match error {
        Error::Denied => "denied",
        Error::Unavailable => "unavailable",
        Error::ResourceLimit => "resource-limit",
        Error::InvalidArgument | Error::Internal => "fail",
    }
}

fn audio_samples() -> &'static mut [i16] {
    unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(AUDIO_SAMPLES).cast::<i16>(),
            audio::MAX_FRAMES,
        )
    }
}

fn quota_value() -> &'static [u8] {
    unsafe {
        core::slice::from_raw_parts(
            core::ptr::addr_of!(QUOTA_VALUE).cast::<u8>(),
            storage::MAX_VALUE_BYTES,
        )
    }
}

fn probe_audio_playback() -> &'static str {
    let samples = audio_samples();
    for (index, sample) in samples.iter_mut().enumerate() {
        *sample = if index % 32 < 16 { 10_000 } else { -10_000 };
    }
    match audio::play_pcm_s16le(samples) {
        Ok(()) => "ok",
        Err(error) => error_status(error),
    }
}

fn probe_audio_capture() -> &'static str {
    let samples = audio_samples();
    samples.fill(0);
    match audio::capture_pcm_s16le(samples) {
        Ok(count) if count == samples.len() && samples.iter().any(|sample| *sample != 0) => {
            "ok-signal"
        }
        Ok(count) if count == samples.len() => "ok-silent",
        Ok(_) => "fail",
        Err(error) => error_status(error),
    }
}

fn probe_gpio() -> &'static str {
    let line = gpio::Line::GroveFunction;
    let original = match gpio::read(line) {
        Ok(value) => value,
        Err(error) => return error_status(error),
    };
    if let Err(error) = gpio::write(line, !original) {
        return error_status(error);
    }
    let toggled = gpio::read(line) == Ok(!original);
    let restored_write = gpio::write(line, original).is_ok();
    let restored_read = gpio::read(line) == Ok(original);
    if toggled && restored_write && restored_read {
        "ok"
    } else {
        "fail"
    }
}

fn quota_key(index: u16) -> [u8; 4] {
    [
        b'q',
        b'0' + ((index / 100) % 10) as u8,
        b'0' + ((index / 10) % 10) as u8,
        b'0' + (index % 10) as u8,
    ]
}

fn key_text(key: &[u8; 4]) -> &str {
    unsafe { core::str::from_utf8_unchecked(key) }
}

fn cleanup_quota_keys() -> bool {
    let mut ok = true;
    for index in 0..QUOTA_KEYS {
        let key = quota_key(index);
        if storage::delete(key_text(&key)).is_err() {
            ok = false;
        }
    }
    if storage::delete("quota.overflow").is_err() {
        ok = false;
    }
    ok
}

fn marker_present() -> Result<bool, Error> {
    let mut value = [0_u8; 32];
    storage::get(MARKER_KEY, &mut value).map(|result| {
        result
            .is_some_and(|length| length == MARKER_VALUE.len() && &value[..length] == MARKER_VALUE)
    })
}

fn probe_storage() -> &'static str {
    match marker_present() {
        Ok(true) => return "persist-ok",
        Err(_) => return "fail",
        Ok(false) => {}
    }
    let _ = storage::delete(MARKER_KEY);
    let _ = storage::delete("acceptance.result");
    if !cleanup_quota_keys() {
        return "fail";
    }

    let mut filled = true;
    for index in 0..QUOTA_KEYS {
        let key = quota_key(index);
        if storage::put(key_text(&key), quota_value()).is_err() {
            filled = false;
            break;
        }
    }
    let quota_rejected = filled
        && matches!(
            storage::put("quota.overflow", b"overflow"),
            Err(Error::ResourceLimit)
        );
    let cleaned = cleanup_quota_keys();
    if !filled || !quota_rejected || !cleaned {
        return "fail";
    }
    if storage::put(MARKER_KEY, MARKER_VALUE).is_err() {
        return "fail";
    }
    match marker_present() {
        Ok(true) => "quota-ok-new",
        Ok(false) | Err(_) => "fail",
    }
}

struct Summary {
    bytes: [u8; system::MAX_NOTIFICATION_BODY_CHARS],
    length: usize,
}

impl Summary {
    fn new() -> Self {
        Self {
            bytes: [0; system::MAX_NOTIFICATION_BODY_CHARS],
            length: 0,
        }
    }

    fn push(&mut self, value: &str) {
        let end = self.length + value.len();
        if end <= self.bytes.len() {
            self.bytes[self.length..end].copy_from_slice(value.as_bytes());
            self.length = end;
        }
    }

    fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.length]) }
    }
}

fn status_color(status: &str) -> u16 {
    if status == "ok" || status.starts_with("ok-") || status.ends_with("-ok") {
        0x07e0
    } else if status == "denied" {
        0xf800
    } else if status == "unavailable" || status == "resource-limit" {
        0xffe0
    } else if status == "quota-ok-new" {
        0x001f
    } else {
        0xf81f
    }
}

fn present_results(statuses: [&str; 4]) {
    let frame = unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(FRAME).cast::<u8>(), FRAME_BYTES)
    };
    for y in 0..usize::from(display::STANDARD_HEIGHT) {
        let segment =
            (y * statuses.len() / usize::from(display::STANDARD_HEIGHT)).min(statuses.len() - 1);
        let color = status_color(statuses[segment]).to_le_bytes();
        for x in 0..usize::from(display::WIDTH) {
            let offset = (y * usize::from(display::WIDTH) + x) * 2;
            frame[offset] = color[0];
            frame[offset + 1] = color[1];
        }
    }
    let _ = display::present_rgb565(frame, &[]);
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let storage_status = probe_storage();
    let playback_status = probe_audio_playback();
    let capture_status = probe_audio_capture();
    let gpio_status = probe_gpio();

    let mut summary = Summary::new();
    summary.push("audio-play=");
    summary.push(playback_status);
    summary.push(";audio-capture=");
    summary.push(capture_status);
    summary.push(";gpio=");
    summary.push(gpio_status);
    summary.push(";storage=");
    summary.push(storage_status);
    let _ = system::post_notification("CP0 Capability Probe", summary.as_str());
    let _ = storage::put("acceptance.result", summary.as_str().as_bytes());
    present_results([playback_status, capture_status, gpio_status, storage_status]);

    loop {
        let _ = system::wait_event(1000);
    }
}

#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
