#![no_std]

use core::panic::PanicInfo;
use cp0_sdk::{Error, system};

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    const TITLE: &str = "Hello Card";
    const BODY: &str = "Runtime host call is active";
    let mut posted = false;

    loop {
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
