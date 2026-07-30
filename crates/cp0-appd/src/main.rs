use std::env;
use std::process::ExitCode;

use cp0_appd::{AppLayout, build_sandbox_plan};

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();

    match arguments.as_slice() {
        [command, manifest_path, app_user] if command == "plan" => {
            let manifest = match cp0_manifest::load_and_validate(manifest_path) {
                Ok(manifest) => manifest,
                Err(error) => {
                    eprintln!("cannot plan application sandbox: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let plan = match build_sandbox_plan(&manifest, app_user, &AppLayout::default()) {
                Ok(plan) => plan,
                Err(error) => {
                    eprintln!("cannot plan application sandbox: {error}");
                    return ExitCode::FAILURE;
                }
            };
            match serde_json::to_writer_pretty(std::io::stdout().lock(), &plan) {
                Ok(()) => {
                    println!();
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("cannot write application sandbox plan: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("usage: cp0-appd plan <app.json> <cp0-app-N>");
            ExitCode::from(2)
        }
    }
}
