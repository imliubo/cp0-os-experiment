#![no_std]

use core::panic::PanicInfo;
use cp0_sdk::{Error, display, system};

const FRAME_BYTES: usize =
    display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];

fn prepare_frame() -> &'static mut [u8] {
    // The frame lives in the WASM data section rather than the 64 KiB call
    // stack. The Runtime validates its complete linear-memory range.
    let frame = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(FRAME).cast::<u8>(),
            FRAME_BYTES,
        )
    };
    for y in 0..usize::from(display::STANDARD_HEIGHT) {
        for x in 0..usize::from(display::WIDTH) {
            let border = x < 4
                || x >= usize::from(display::WIDTH) - 4
                || y < 4
                || y >= usize::from(display::STANDARD_HEIGHT) - 4;
            let pixel: u16 = if border {
                0xffff
            } else if y < 50 {
                0xf800
            } else if y < 100 {
                0x07e0
            } else {
                0x001f
            };
            let offset = (y * usize::from(display::WIDTH) + x) * 2;
            frame[offset] = pixel as u8;
            frame[offset + 1] = (pixel >> 8) as u8;
        }
    }
    frame
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    const TITLE: &str = "Hello Card";
    const BODY: &str = "Runtime host call is active";
    let mut posted = false;
    let mut rendered = false;
    let frame = prepare_frame();

    loop {
        if !rendered {
            match display::present_rgb565(frame, &[]) {
                Ok(()) => rendered = true,
                Err(Error::Unavailable | Error::ResourceLimit) => {}
                Err(_) => return 1,
            }
        }
        if !posted {
            match system::post_notification(TITLE, BODY) {
                Ok(()) => posted = true,
                Err(Error::Unavailable) => {}
                Err(_) => return 1,
            }
        }
        if system::wait_event(250).is_err() {
            return 1;
        }
    }
}

#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
