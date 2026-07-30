#![no_std]

use core::panic::PanicInfo;
use cp0_sdk::{Error, display, documents, input, network, system};

const FRAME_BYTES: usize =
    display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];
static mut NETWORK_BODY: [u8; network::MAX_RESPONSE_BODY_BYTES] =
    [0; network::MAX_RESPONSE_BODY_BYTES];
const KEY_N: u16 = 49;
const KEY_D: u16 = 32;

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

fn show_key(frame: &mut [u8], code: u16) {
    let color = 0x001fu16 | ((code & 0x1f) << 11) | ((code & 0x3f) << 5);
    for y in 126..142 {
        for x in 280..312 {
            let offset = (y * usize::from(display::WIDTH) + x) * 2;
            frame[offset] = color as u8;
            frame[offset + 1] = (color >> 8) as u8;
        }
    }
}

fn show_network_status(frame: &mut [u8], color: u16) {
    for y in 126..142 {
        for x in 8..48 {
            let offset = (y * usize::from(display::WIDTH) + x) * 2;
            frame[offset] = color as u8;
            frame[offset + 1] = (color >> 8) as u8;
        }
    }
}

fn request_network(frame: &mut [u8]) {
    let body = unsafe {
        core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(NETWORK_BODY).cast::<u8>(),
            network::MAX_RESPONSE_BODY_BYTES,
        )
    };
    match network::http_get("https://example.com/", body) {
        Ok(response) if (200..=299).contains(&response.status_code) => {
            show_network_status(frame, 0x07e0);
            let _ = system::post_notification("Network ready", "HTTPS request completed");
        }
        Ok(_) | Err(Error::Denied) => show_network_status(frame, 0xf800),
        Err(Error::Unavailable) => show_network_status(frame, 0xffe0),
        Err(_) => show_network_status(frame, 0xf81f),
    }
}

fn request_document(frame: &mut [u8]) {
    let mut buffer = [0_u8; 32];
    match documents::open() {
        Ok(document) => {
            let result = document.read(0, &mut buffer);
            let _ = document.close();
            match result {
                Ok(count) if count > 0 => show_network_status(frame, 0x07e0),
                Ok(_) => show_network_status(frame, 0xffe0),
                Err(_) => show_network_status(frame, 0xf81f),
            }
        }
        Err(Error::Denied) => show_network_status(frame, 0xf800),
        Err(Error::Unavailable) => show_network_status(frame, 0xffe0),
        Err(_) => show_network_status(frame, 0xf81f),
    }
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
        match input::poll_key_event(250) {
            Ok(Some(event)) if event.pressed => {
                if event.code == KEY_N {
                    request_network(frame);
                }
                if event.code == KEY_D {
                    request_document(frame);
                }
                show_key(frame, event.code);
                rendered = false;
            }
            Ok(_) => {}
            Err(Error::ResourceLimit) => {}
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
