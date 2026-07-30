#![no_std]

use core::panic::PanicInfo;

#[link(wasm_import_module = "cardputerzero")]
unsafe extern "C" {
    fn cp0_wait_event(timeout_ms: i32) -> i32;
    fn cp0_post_notification(
        title: *const u8,
        title_length: u32,
        body: *const u8,
        body_length: u32,
    ) -> i32;
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    const TITLE: &[u8] = b"Hello Card";
    const BODY: &[u8] = b"Runtime host call is active";
    let mut posted = false;

    loop {
        if !posted {
            let result = unsafe {
                cp0_post_notification(
                    TITLE.as_ptr(),
                    TITLE.len() as u32,
                    BODY.as_ptr(),
                    BODY.len() as u32,
                )
            };
            if result == 0 {
                posted = true;
            } else if result != -2 {
                return result;
            }
        }
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
