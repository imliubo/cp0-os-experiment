#![no_std]

use core::panic::PanicInfo;

#[link(wasm_import_module = "cardputerzero")]
unsafe extern "C" {
    fn cp0_wait_event(timeout_ms: i32) -> i32;
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    loop {
        let result = unsafe { cp0_wait_event(250) };
        if result != 0 {
            return result;
        }
    }
}

#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
