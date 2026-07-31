#![no_main]

use std::io::Cursor;

use cp0_store_protocol::{
    decode_signed_catalog, encode_signed_catalog, read_request, read_response, verify_catalog,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(catalog) = decode_signed_catalog(data) {
        let _ = encode_signed_catalog(&catalog);
        let _ = verify_catalog(&catalog, &[0; 32]);
    }

    let _ = read_request(&mut Cursor::new(data));
    let _ = read_response(&mut Cursor::new(data));

    let mut terminated = Vec::with_capacity(data.len().saturating_add(1));
    terminated.extend_from_slice(data);
    terminated.push(b'\n');
    let _ = read_request(&mut Cursor::new(&terminated));
    let _ = read_response(&mut Cursor::new(&terminated));
});
