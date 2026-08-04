#![no_std]

#[cfg(not(test))]
use core::panic::PanicInfo;
use cp0_sdk::{
    display::{self, Rect},
    input, system,
    ui::{Canvas, color},
};

const KEY_ENTER: u16 = 28;
const KEY_R: u16 = 19;
const KEY_SPACE: u16 = 57;
const FRAME_BYTES: usize = display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;

#[cfg(not(test))]
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];

#[derive(Clone, Copy)]
struct Stopwatch {
    running: bool,
    accumulated_ms: u64,
    started_at_ms: u64,
}

impl Stopwatch {
    const fn new() -> Self {
        Self {
            running: false,
            accumulated_ms: 0,
            started_at_ms: 0,
        }
    }

    fn toggle(&mut self, now: u64) {
        if self.running {
            self.accumulated_ms = self.elapsed(now);
            self.running = false;
        } else {
            self.started_at_ms = now;
            self.running = true;
        }
    }

    fn reset(&mut self, now: u64) {
        self.accumulated_ms = 0;
        self.started_at_ms = now;
    }

    fn elapsed(self, now: u64) -> u64 {
        if self.running {
            self.accumulated_ms
                .saturating_add(now.saturating_sub(self.started_at_ms))
        } else {
            self.accumulated_ms
        }
    }
}

#[cfg(not(test))]
fn frame() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(FRAME).cast(), FRAME_BYTES) }
}

fn render(stopwatch: Stopwatch, now: u64, pixels: &mut [u8]) {
    let mut canvas = Canvas::new(pixels, display::WIDTH, display::STANDARD_HEIGHT).unwrap();
    canvas.clear(color::BACKGROUND);
    canvas.draw_text(88, 9, "STOPWATCH", color::TEXT, 2);
    canvas.fill_rect(
        Rect {
            x: 18,
            y: 38,
            width: 284,
            height: 72,
        },
        color::SURFACE,
    );
    canvas.stroke_rect(
        Rect {
            x: 18,
            y: 38,
            width: 284,
            height: 72,
        },
        if stopwatch.running {
            color::SUCCESS
        } else {
            color::MUTED
        },
    );
    let mut time = [0_u8; 12];
    format_elapsed(stopwatch.elapsed(now), &mut time);
    let text = unsafe { core::str::from_utf8_unchecked(&time) };
    canvas.draw_text(46, 61, text, color::TEXT, 3);
    canvas.fill_rect(
        Rect {
            x: 118,
            y: 126,
            width: 84,
            height: 18,
        },
        if stopwatch.running {
            color::SUCCESS
        } else {
            color::SURFACE_RAISED
        },
    );
    canvas.draw_text(
        if stopwatch.running { 139 } else { 136 },
        132,
        if stopwatch.running {
            "RUNNING"
        } else {
            "PAUSED"
        },
        color::TEXT,
        1,
    );
}

fn format_elapsed(milliseconds: u64, output: &mut [u8; 12]) {
    let tenths = (milliseconds / 100) % 10;
    let total_seconds = milliseconds / 1000;
    let seconds = total_seconds % 60;
    let minutes = total_seconds / 60 % 60;
    let hours = (total_seconds / 3600).min(99);
    output[0] = b'0' + (hours / 10) as u8;
    output[1] = b'0' + (hours % 10) as u8;
    output[2] = b':';
    output[3] = b'0' + (minutes / 10) as u8;
    output[4] = b'0' + (minutes % 10) as u8;
    output[5] = b':';
    output[6] = b'0' + (seconds / 10) as u8;
    output[7] = b'0' + (seconds % 10) as u8;
    output[8] = b'.';
    output[9] = b'0' + tenths as u8;
    output[10] = b' ';
    output[11] = b' ';
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let pixels = frame();
    let mut stopwatch = Stopwatch::new();
    let mut dirty = true;
    let mut next_render = 0;
    loop {
        let now = system::monotonic_milliseconds();
        if stopwatch.running && now >= next_render {
            next_render = now.saturating_add(100);
            dirty = true;
        }
        if dirty {
            render(stopwatch, now, pixels);
            if display::present_rgb565(pixels, &[]).is_ok() {
                dirty = false;
            }
        }
        match input::poll_key_event(50) {
            Ok(Some(event)) if event.pressed => {
                if matches!(event.code, KEY_ENTER | KEY_SPACE) {
                    stopwatch.toggle(now);
                    dirty = true;
                } else if event.code == KEY_R {
                    stopwatch.reset(now);
                    dirty = true;
                }
            }
            Ok(_) => {}
            Err(_) => return 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_resume_and_reset_keep_elapsed_time_monotonic() {
        let mut stopwatch = Stopwatch::new();
        stopwatch.toggle(100);
        assert_eq!(stopwatch.elapsed(650), 550);
        stopwatch.toggle(650);
        assert_eq!(stopwatch.elapsed(900), 550);
        stopwatch.toggle(1000);
        assert_eq!(stopwatch.elapsed(1200), 750);
        stopwatch.reset(1300);
        assert_eq!(stopwatch.elapsed(1500), 200);
    }

    #[test]
    fn formats_bounded_stopwatch_display() {
        let mut output = [0_u8; 12];
        format_elapsed(3_723_400, &mut output);
        assert_eq!(&output[..10], b"01:02:03.4");
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
