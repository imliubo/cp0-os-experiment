#![no_main]

use cp0_recovery::verify_backup_bytes;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|encoded: &[u8]| {
    let _ = verify_backup_bytes(encoded);
});
