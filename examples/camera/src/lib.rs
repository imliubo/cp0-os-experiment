#![no_std]

#[cfg(not(test))]
use core::panic::PanicInfo;
use cp0_sdk::{
    Error, camera,
    display::{self, Rect},
    input, system,
    ui::{Canvas, color},
};

const KEY_ENTER: u16 = 28;
const KEY_SPACE: u16 = 57;
const FRAME_BYTES: usize = camera::FRAME_BYTES;
const PREVIEW_INTERVAL_MS: u64 = 1_000 / camera::PREVIEW_FPS as u64;
const RETRY_INTERVAL_MS: u64 = 2000;

static mut CAMERA_FRAME: [u16; camera::PIXEL_COUNT] = [0; camera::PIXEL_COUNT];
static mut DISPLAY_FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];

#[derive(Clone, Copy)]
enum Status {
    Starting,
    Live,
    Capturing,
    Saved,
    Authorize,
    Denied,
    Unavailable,
}

fn camera_pixels() -> &'static mut [u16] {
    unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(CAMERA_FRAME).cast(),
            camera::PIXEL_COUNT,
        )
    }
}

fn display_frame() -> &'static mut [u8] {
    unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(DISPLAY_FRAME).cast(), FRAME_BYTES)
    }
}

fn render_preview(source: &[u16], target: &mut [u8], status: Status) {
    let source =
        unsafe { core::slice::from_raw_parts(source.as_ptr().cast::<u8>(), camera::FRAME_BYTES) };
    target.copy_from_slice(source);
    let mut canvas = Canvas::new(target, display::WIDTH, display::IMMERSIVE_HEIGHT).unwrap();
    canvas.fill_rect(
        Rect {
            x: 8,
            y: 8,
            width: 62,
            height: 17,
        },
        color::DANGER,
    );
    canvas.draw_text(18, 13, "CAMERA", color::TEXT, 1);
    canvas.fill_rect(
        Rect {
            x: 88,
            y: 145,
            width: 144,
            height: 18,
        },
        color::SURFACE,
    );
    let (label, label_color, x) = match status {
        Status::Capturing => ("CAPTURING", color::WARNING, 133),
        Status::Saved => ("PHOTO SAVED", color::SUCCESS, 127),
        Status::Authorize => ("AUTHORIZE PHOTOS", color::WARNING, 112),
        Status::Denied => ("ACCESS DENIED", color::DANGER, 121),
        Status::Unavailable => ("CAMERA OFFLINE", color::DANGER, 118),
        _ => ("LIVE", color::TEXT, 148),
    };
    canvas.draw_text(x, 151, label, label_color, 1);
}

fn render_placeholder(target: &mut [u8], status: Status) {
    let mut canvas = Canvas::new(target, display::WIDTH, display::IMMERSIVE_HEIGHT).unwrap();
    canvas.clear(color::BACKGROUND);
    canvas.fill_rect(
        Rect {
            x: 32,
            y: 31,
            width: 256,
            height: 104,
        },
        color::SURFACE,
    );
    canvas.stroke_rect(
        Rect {
            x: 32,
            y: 31,
            width: 256,
            height: 104,
        },
        match status {
            Status::Denied | Status::Unavailable => color::DANGER,
            _ => color::ACCENT,
        },
    );
    canvas.draw_text(91, 55, "CAMERA", color::TEXT, 3);
    let (label, x) = match status {
        Status::Denied => ("ACCESS DENIED", 121),
        Status::Unavailable => ("CAMERA OFFLINE", 118),
        _ => ("STARTING", 136),
    };
    canvas.draw_text(x, 108, label, color::MUTED, 1);
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let pixels = camera_pixels();
    let frame = display_frame();
    let mut status = Status::Starting;
    let mut has_frame = false;
    let mut next_preview = 0;
    let mut status_until = 0;
    let mut dirty = true;

    loop {
        let now = system::monotonic_milliseconds();
        if now >= next_preview {
            match camera::capture_rgb565(pixels) {
                Ok(()) => {
                    has_frame = true;
                    if now >= status_until {
                        status = Status::Live;
                    }
                    dirty = true;
                }
                Err(Error::Denied) => {
                    status = Status::Denied;
                    dirty = true;
                }
                Err(Error::Unavailable | Error::ResourceLimit) => {
                    if !has_frame {
                        status = Status::Unavailable;
                        dirty = true;
                    }
                }
                Err(_) => return 1,
            }
            let interval = if matches!(status, Status::Unavailable | Status::Denied) {
                RETRY_INTERVAL_MS
            } else {
                PREVIEW_INTERVAL_MS
            };
            next_preview = next_preview.max(now).saturating_add(interval);
        }

        match input::poll_key_event(1) {
            Ok(Some(event))
                if event.pressed && has_frame && matches!(event.code, KEY_ENTER | KEY_SPACE) =>
            {
                status = Status::Capturing;
                render_preview(pixels, frame, status);
                let _ = display::present_rgb565(frame, &[]);
                match camera::capture_photo() {
                    Ok(_) => status = Status::Saved,
                    Err(Error::Unavailable) => status = Status::Authorize,
                    Err(Error::Denied) => status = Status::Denied,
                    Err(_) => status = Status::Unavailable,
                }
                status_until = system::monotonic_milliseconds().saturating_add(1200);
                next_preview = 0;
                dirty = true;
            }
            Ok(_) => {}
            Err(_) => return 1,
        }

        if dirty {
            if has_frame {
                render_preview(pixels, frame, status);
            } else {
                render_placeholder(frame, status);
            }
            if display::present_rgb565(frame, &[]).is_ok() {
                dirty = false;
            }
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
