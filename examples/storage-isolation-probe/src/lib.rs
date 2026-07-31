#![no_std]

use core::panic::PanicInfo;
use cp0_sdk::{display, storage, system};

const FRAME_BYTES: usize = display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];

fn present(color: u16) {
    let frame = unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(FRAME).cast::<u8>(), FRAME_BYTES)
    };
    let color = color.to_le_bytes();
    for pixel in frame.chunks_exact_mut(2) {
        pixel.copy_from_slice(&color);
    }
    let _ = display::present_rgb565(frame, &[]);
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let mut value = [0_u8; 32];
    let (body, color) = match storage::get("acceptance.marker", &mut value) {
        Ok(None) => ("storage-isolation=ok", 0x07e0),
        Ok(Some(_)) => ("storage-isolation=leak", 0xf800),
        Err(_) => ("storage-isolation=error", 0xf81f),
    };
    let _ = system::post_notification("CP0 Isolation Probe", body);
    let _ = storage::put("isolation.result", body.as_bytes());
    present(color);
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
