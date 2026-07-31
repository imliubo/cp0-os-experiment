use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use cp0_manifest::{AppManifest, DisplayMode, ResourceLimits, Runtime, SCHEMA_VERSION, validate};

const DEVKIT_ROOT_ENV: &str = "CP0_DEVKIT_ROOT";

fn validate_devkit_root(path: PathBuf) -> Result<PathBuf, String> {
    let root = fs::canonicalize(&path).map_err(|error| {
        format!(
            "cannot open CardputerZero DevKit {}: {error}",
            path.display()
        )
    })?;
    let sdk = root.join("sdk/rust/Cargo.toml");
    let simulator = root.join("simulator/cp0-simulator.mjs");
    if !sdk.is_file() || !simulator.is_file() {
        return Err(format!(
            "CardputerZero DevKit {} is incomplete; expected sdk/rust and simulator",
            root.display()
        ));
    }
    Ok(root)
}

fn devkit_root() -> Result<PathBuf, String> {
    if let Some(configured) = std::env::var_os(DEVKIT_ROOT_ENV) {
        if configured.is_empty() {
            return Err(format!("{DEVKIT_ROOT_ENV} must not be empty"));
        }
        return validate_devkit_root(PathBuf::from(configured));
    }

    if let Ok(executable) = std::env::current_exe() {
        if let Some(root) = executable.parent().and_then(Path::parent) {
            let installed = root.to_path_buf();
            if installed.join("sdk/rust/Cargo.toml").is_file()
                && installed.join("simulator/cp0-simulator.mjs").is_file()
            {
                return validate_devkit_root(installed);
            }
        }
    }

    validate_devkit_root(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

pub fn new_project(path: impl AsRef<Path>, app_id: &str, name: &str) -> Result<(), String> {
    let path = path.as_ref();
    if path.exists() {
        return Err(format!("project path {} already exists", path.display()));
    }
    let crate_name = app_id
        .rsplit('.')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or("application ID has no crate name")?;
    let artifact_name = crate_name.replace('-', "_");
    let manifest = AppManifest {
        schema_version: SCHEMA_VERSION,
        id: app_id.into(),
        name: name.into(),
        version: "0.1.0".into(),
        sdk_version: "1.0".into(),
        runtime: Runtime::Wamr,
        entrypoint: format!("bin/{artifact_name}.wasm"),
        display: DisplayMode::Standard,
        resources: ResourceLimits {
            memory_mb: 24,
            storage_mb: 8,
        },
        permissions: Vec::new(),
        intents: Vec::new(),
    };
    validate(&manifest).map_err(|errors| errors.join("\n"))?;

    fs::create_dir_all(path.join("src"))
        .map_err(|error| format!("cannot create project directory: {error}"))?;
    let sdk_path = devkit_root()?.join("sdk/rust");
    let sdk_path = toml_string(&sdk_path.to_string_lossy());
    let cargo = format!(
        "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\ncp0-sdk = {{ path = \"{sdk_path}\" }}\n\n[profile.release]\nopt-level = \"s\"\nlto = true\ncodegen-units = 1\npanic = \"abort\"\nstrip = true\n\n[workspace]\n"
    );
    fs::write(path.join("Cargo.toml"), cargo)
        .map_err(|error| format!("cannot write Cargo.toml: {error}"))?;
    fs::write(path.join("src/lib.rs"), APP_TEMPLATE)
        .map_err(|error| format!("cannot write src/lib.rs: {error}"))?;
    let app_json = File::create(path.join("app.json"))
        .map_err(|error| format!("cannot create app.json: {error}"))?;
    let mut writer = BufWriter::new(app_json);
    serde_json::to_writer_pretty(&mut writer, &manifest)
        .map_err(|error| format!("cannot encode app.json: {error}"))?;
    writer
        .write_all(b"\n")
        .and_then(|()| writer.flush())
        .map_err(|error| format!("cannot finish app.json: {error}"))?;
    println!("created CardputerZero application at {}", path.display());
    Ok(())
}

pub fn build_project(path: impl AsRef<Path>) -> Result<PathBuf, String> {
    let project = fs::canonicalize(path.as_ref())
        .map_err(|error| format!("cannot open project directory: {error}"))?;
    let cargo_manifest = project.join("Cargo.toml");
    let app_path = project.join("app.json");
    let app = cp0_manifest::load_and_validate(&app_path)
        .map_err(|error| format!("invalid project manifest: {error}"))?;
    let metadata_output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version=1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(&cargo_manifest)
        .output()
        .map_err(|error| format!("cannot execute cargo metadata: {error}"))?;
    if !metadata_output.status.success() {
        return Err(format!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&metadata_output.stderr)
        ));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&metadata_output.stdout)
        .map_err(|error| format!("cargo metadata returned invalid JSON: {error}"))?;
    let target_directory = metadata
        .get("target_directory")
        .and_then(serde_json::Value::as_str)
        .ok_or("cargo metadata omitted target_directory")?;
    let canonical_manifest = fs::canonicalize(&cargo_manifest)
        .map_err(|error| format!("cannot resolve Cargo.toml: {error}"))?;
    let package = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .and_then(|packages| {
            packages.iter().find(|package| {
                package
                    .get("manifest_path")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|path| Path::new(path) == canonical_manifest)
            })
        })
        .ok_or("cargo metadata omitted the project package")?;
    let target_name = package
        .get("targets")
        .and_then(serde_json::Value::as_array)
        .and_then(|targets| {
            targets.iter().find(|target| {
                target
                    .get("kind")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "cdylib"))
            })
        })
        .and_then(|target| target.get("name"))
        .and_then(serde_json::Value::as_str)
        .ok_or("project must contain one cdylib target")?;

    let status = Command::new("cargo")
        .args(["build", "--target", "wasm32-unknown-unknown", "--release"])
        .arg("--manifest-path")
        .arg(&cargo_manifest)
        .status()
        .map_err(|error| format!("cannot execute cargo build: {error}"))?;
    if !status.success() {
        return Err("cargo build failed".into());
    }

    let artifact = Path::new(target_directory)
        .join("wasm32-unknown-unknown/release")
        .join(format!("{}.wasm", target_name.replace('-', "_")));
    if !artifact.is_file() {
        return Err(format!("cargo did not produce {}", artifact.display()));
    }
    let output = Path::new(target_directory)
        .join("cardputerzero")
        .join(&app.id)
        .join(&app.version);
    let entrypoint = output.join(&app.entrypoint);
    let entrypoint_parent = entrypoint
        .parent()
        .ok_or("application entrypoint has no parent")?;
    fs::create_dir_all(entrypoint_parent)
        .map_err(|error| format!("cannot create build output: {error}"))?;
    fs::copy(&artifact, &entrypoint)
        .map_err(|error| format!("cannot copy WASM artifact: {error}"))?;
    fs::copy(&app_path, output.join("app.json"))
        .map_err(|error| format!("cannot copy app.json: {error}"))?;
    println!("built CardputerZero application at {}", output.display());
    Ok(output)
}

pub fn run_project(arguments: &[String]) -> Result<(), String> {
    let project_path = arguments
        .first()
        .ok_or("run requires a project directory")?;
    let build = build_project(project_path)?;
    let manifest_path = build.join("app.json");
    let manifest = cp0_manifest::load_and_validate(&manifest_path)
        .map_err(|error| format!("invalid built manifest: {error}"))?;
    let wasm = build.join(&manifest.entrypoint);
    let simulator_output = build.join("simulator");
    let mut duration = "1000".to_owned();
    let mut permissions = "deny".to_owned();
    let mut keys = String::new();
    let mut output = simulator_output.join("frame.ppm");
    let mut profile = simulator_output.join("profile.json");

    let mut index = 1;
    while index < arguments.len() {
        let option = &arguments[index];
        let value = arguments
            .get(index + 1)
            .ok_or_else(|| format!("run option {option} requires a value"))?;
        match option.as_str() {
            "--duration" => {
                let milliseconds = value
                    .parse::<u32>()
                    .ok()
                    .filter(|value| (100..=30_000).contains(value))
                    .ok_or("run duration must be between 100 and 30000 milliseconds")?;
                duration = milliseconds.to_string();
            }
            "--permissions" if matches!(value.as_str(), "allow" | "deny") => {
                permissions.clone_from(value);
            }
            "--permissions" => return Err("run permissions must be allow or deny".into()),
            "--keys" => keys.clone_from(value),
            "--output" => output = PathBuf::from(value),
            "--profile" => profile = PathBuf::from(value),
            _ => return Err(format!("unknown run option {option}")),
        }
        index += 2;
    }

    let bundled_script = devkit_root()?.join("simulator/cp0-simulator.mjs");
    let script = std::env::var_os("CP0_SIMULATOR_SCRIPT")
        .map(PathBuf::from)
        .unwrap_or(bundled_script);
    if !script.is_file() {
        return Err(format!(
            "CardputerZero simulator is missing: {}",
            script.display()
        ));
    }
    let node = std::env::var_os("CP0_NODE").unwrap_or_else(|| "node".into());
    let status = Command::new(node)
        .arg(script)
        .args(["--wasm", &wasm.to_string_lossy()])
        .args(["--manifest", &manifest_path.to_string_lossy()])
        .args(["--duration", &duration])
        .args(["--permissions", &permissions])
        .args(["--keys", &keys])
        .args(["--output", &output.to_string_lossy()])
        .args(["--profile", &profile.to_string_lossy()])
        .status()
        .map_err(|error| format!("cannot execute CardputerZero simulator: {error}"))?;
    if !status.success() {
        return Err("CardputerZero simulator failed".into());
    }
    Ok(())
}

fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

const APP_TEMPLATE: &str = r#"#![no_std]

#[cfg(not(test))]
use core::panic::PanicInfo;
#[cfg(not(test))]
use cp0_sdk::system;

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    loop {
        if system::wait_event(250).is_err() {
            return 1;
        }
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffolds_and_builds_sdk_only_project() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("cp0ctl-new-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        new_project(&path, "dev.cardputerzero.generated", "Generated").unwrap();
        let manifest = cp0_manifest::load_and_validate(path.join("app.json")).unwrap();
        assert_eq!(manifest.sdk_version, "1.0");
        let source = fs::read_to_string(path.join("src/lib.rs")).unwrap();
        assert!(source.contains("cp0_sdk::system"));
        assert!(source.contains("#[cfg(not(test))]"));
        assert!(!source.contains("extern \"C\" {"));
        let test_status = Command::new("cargo")
            .args(["test", "--manifest-path"])
            .arg(path.join("Cargo.toml"))
            .status()
            .unwrap();
        assert!(test_status.success());
        let output = build_project(&path).unwrap();
        assert!(output.join("app.json").is_file());
        assert!(output.join("bin/generated.wasm").is_file());
    }
}
