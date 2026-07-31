#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|encoded: &[u8]| {
    let _ = cp0_manifest::parse_and_validate(encoded);
});
