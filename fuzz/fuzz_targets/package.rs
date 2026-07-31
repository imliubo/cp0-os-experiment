#![no_main]

use std::sync::OnceLock;

use cp0_package::{CApp, PackageEntry};
use libfuzzer_sys::fuzz_target;

fn seed() -> &'static [u8] {
    static SEED: OnceLock<Vec<u8>> = OnceLock::new();
    SEED.get_or_init(|| {
        CApp::new(vec![
            PackageEntry {
                path: "app.json".into(),
                contents: br#"{"schema_version":1}"#.to_vec(),
            },
            PackageEntry {
                path: "bin/app.wasm".into(),
                contents: b"\0asm\x01\0\0\0".to_vec(),
            },
        ])
        .expect("static fuzz seed")
        .encode()
        .expect("encode static fuzz seed")
    })
}

fn exercise(encoded: &[u8]) {
    if let Ok(package) = CApp::decode(encoded) {
        let _ = package.encode();
        let _ = package.verify_developer_signature();
        let _ = package.verify_store_signature(&[0; 32]);
    }
}

fuzz_target!(|data: &[u8]| {
    exercise(data);

    let mut structured = seed().to_vec();
    let length = structured.len();
    for (index, byte) in data.iter().take(length).enumerate() {
        let position = (index.wrapping_mul(131) + usize::from(*byte)) % length;
        structured[position] ^= byte.wrapping_add(1);
    }
    exercise(&structured);
});
