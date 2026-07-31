#![no_main]

use std::io::Cursor;

use cp0_appd::{read_request, read_response};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = read_request(&mut Cursor::new(data));
    let _ = read_response(&mut Cursor::new(data));

    let mut terminated = Vec::with_capacity(data.len().saturating_add(1));
    terminated.extend_from_slice(data);
    terminated.push(b'\n');
    let _ = read_request(&mut Cursor::new(&terminated));
    let _ = read_response(&mut Cursor::new(&terminated));
});
