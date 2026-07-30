use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    match args.as_slice() {
        [manifest, validate, path] if manifest == "manifest" && validate == "validate" => {
            match cp0_manifest::load_and_validate(path) {
                Ok(app) => {
                    println!(
                        "valid CardputerZero app manifest: {} {}",
                        app.id, app.version
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("manifest validation failed:\n{error}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: cp0ctl manifest validate <app.json>");
            ExitCode::from(2)
        }
    }
}
