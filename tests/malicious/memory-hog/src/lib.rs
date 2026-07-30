#![no_std]

use core::panic::PanicInfo;

const MEMORY_BYTES: usize = 40 * 1024 * 1024;

#[unsafe(no_mangle)]
static mut MEMORY: [u8; MEMORY_BYTES] = [0; MEMORY_BYTES];

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let mut offset = 0;
    unsafe {
        while offset < MEMORY_BYTES {
            core::ptr::addr_of_mut!(MEMORY[offset]).write_volatile(1);
            offset += 4096;
        }
    }
    0
}

#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
