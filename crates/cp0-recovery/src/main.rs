use std::env;
use std::process::ExitCode;

use cp0_recovery::{create_backup, restore_backup, verify_backup};

fn usage() -> ! {
    eprintln!("usage: cp0-recovery backup SOURCE_ROOT OUTPUT.cp0backup");
    eprintln!("       cp0-recovery verify INPUT.cp0backup");
    eprintln!("       cp0-recovery restore INPUT.cp0backup EMPTY_TARGET_ROOT");
    std::process::exit(2);
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let summary = match arguments.as_slice() {
        [command, source, output] if command == "backup" => create_backup(source, output)?,
        [command, input] if command == "verify" => verify_backup(input)?,
        [command, input, target] if command == "restore" => restore_backup(input, target)?,
        _ => usage(),
    };
    println!(
        "PASS cp0 backup v1 entries={} files={} bytes={} profile={}",
        summary.entry_count, summary.file_count, summary.file_bytes, summary.image_profile
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
