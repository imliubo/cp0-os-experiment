#![no_std]

use core::panic::PanicInfo;
use cp0_sdk::{display, system};

const FRAME_BYTES: usize = display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let frame = unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(FRAME).cast::<u8>(), FRAME_BYTES)
    };
    for y in 0..usize::from(display::STANDARD_HEIGHT) {
        let color = if y < 12 { 0xffff_u16 } else { 0x07e0_u16 }.to_le_bytes();
        for x in 0..usize::from(display::WIDTH) {
            let offset = (y * usize::from(display::WIDTH) + x) * 2;
            frame[offset] = color[0];
            frame[offset + 1] = color[1];
        }
    }
    let _ = display::present_rgb565(frame, &[]);
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
