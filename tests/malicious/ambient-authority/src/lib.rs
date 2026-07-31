#![no_std]

use core::panic::PanicInfo;

#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    fn path_open() -> i32;
    fn sock_open() -> i32;
}

#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    unsafe { path_open() | sock_open() }
}

#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
