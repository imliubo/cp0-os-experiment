#![no_std]

use core::panic::PanicInfo;
use cp0_sdk::{
    Error, camera,
    display::{self, Rect},
    input,
    ui::{Canvas, color},
};

static mut CAMERA_FRAME: [u16; camera::PIXEL_COUNT] = [0; camera::PIXEL_COUNT];

fn pixels() -> &'static mut [u16] {
    unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(CAMERA_FRAME).cast(),
            camera::PIXEL_COUNT,
        )
    }
}

fn frame() -> &'static mut [u8] {
    unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(CAMERA_FRAME).cast(),
            display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2,
        )
    }
}

fn placeholder(frame: &mut [u8], denied: bool) {
    let mut canvas = Canvas::new(frame, display::WIDTH, display::STANDARD_HEIGHT).unwrap();
    canvas.clear(color::BACKGROUND);
    canvas.fill_rect(
        Rect {
            x: 20,
            y: 18,
            width: 280,
            height: 84,
        },
        color::SURFACE,
    );
    canvas.stroke_rect(
        Rect {
            x: 20,
            y: 18,
            width: 280,
            height: 84,
        },
        if denied { color::DANGER } else { color::ACCENT },
    );
    canvas.draw_text(61, 48, "CAMERA", color::TEXT, 3);
    canvas.draw_text(
        if denied { 88 } else { 46 },
        118,
        if denied {
            "PERMISSION DENIED"
        } else {
            "ENTER TO CAPTURE"
        },
        if denied { color::DANGER } else { color::MUTED },
        1,
    );
}

fn live_badge(frame: &mut [u8]) {
    let mut canvas = Canvas::new(frame, display::WIDTH, display::STANDARD_HEIGHT).unwrap();
    canvas.fill_rect(
        Rect {
            x: 8,
            y: 8,
            width: 42,
            height: 15,
        },
        color::DANGER,
    );
    canvas.draw_text(15, 12, "LIVE", color::TEXT, 1);
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    placeholder(frame(), false);
    let mut dirty = true;
    loop {
        if dirty && display::present_rgb565(frame(), &[]).is_ok() {
            dirty = false;
        }
        match input::poll_key_event(250) {
            Ok(Some(event)) if event.pressed && matches!(event.code, 28 | 46) => {
                match camera::capture_rgb565(pixels()) {
                    Ok(()) => live_badge(frame()),
                    Err(Error::Denied) => placeholder(frame(), true),
                    Err(_) => return 1,
                }
                dirty = true;
            }
            Ok(_) => {}
            Err(_) => return 1,
        }
    }
}

#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
