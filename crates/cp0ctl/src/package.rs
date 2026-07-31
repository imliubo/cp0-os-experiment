use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use cp0_package::{CApp, public_key};

pub fn generate_key(secret_path: &str, public_path: &str) -> Result<(), String> {
    let mut secret = [0_u8; 32];
    File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut secret))
        .map_err(|error| format!("cannot read operating-system randomness: {error}"))?;
    write_new(secret_path, &secret, 0o600)?;
    if let Err(error) = write_new(public_path, &public_key(&secret), 0o644) {
        return Err(format!(
            "secret key was created at {secret_path}, but public key creation failed: {error}"
        ));
    }
    println!("created developer key pair: {secret_path} {public_path}");
    Ok(())
}

pub fn package_project(project_path: &str, output_path: &str) -> Result<(), String> {
    let build = crate::project::build_project(project_path)?;
    let package = CApp::from_directory(&build)
        .map_err(|error| format!("cannot package build output: {error}"))?;
    let manifest = manifest_from_package(&package)?;
    if package.entry(&manifest.entrypoint).is_none() {
        return Err(format!(
            "package is missing manifest entrypoint {}",
            manifest.entrypoint
        ));
    }
    write_new(
        output_path,
        &package.encode().map_err(|error| error.to_string())?,
        0o644,
    )?;
    println!(
        "packaged {} {} as {}",
        manifest.id, manifest.version, output_path
    );
    Ok(())
}

pub fn sign_package(
    role: &str,
    input_path: &str,
    output_path: &str,
    secret_path: &str,
) -> Result<(), String> {
    let encoded = fs::read(input_path)
        .map_err(|error| format!("cannot read input package {input_path}: {error}"))?;
    let mut package = CApp::decode(&encoded).map_err(|error| error.to_string())?;
    manifest_from_package(&package)?;
    let secret = read_key(secret_path, "secret")?;
    match role {
        "developer" => package
            .sign_developer(&secret)
            .map_err(|error| error.to_string())?,
        "store" => package
            .sign_store(&secret)
            .map_err(|error| error.to_string())?,
        _ => return Err("signature role must be developer or store".into()),
    }
    write_new(
        output_path,
        &package.encode().map_err(|error| error.to_string())?,
        0o644,
    )?;
    println!("added {role} signature: {output_path}");
    Ok(())
}

pub fn verify_package(path: &str, store_public_path: Option<&str>) -> Result<(), String> {
    let package = read_package(path)?;
    let manifest = manifest_from_package(&package)?;
    if package.entry(&manifest.entrypoint).is_none() {
        return Err(format!(
            "package is missing manifest entrypoint {}",
            manifest.entrypoint
        ));
    }
    package
        .verify_developer_signature()
        .map_err(|error| error.to_string())?;
    let signature = if let Some(public_path) = store_public_path {
        let public = read_key(public_path, "public")?;
        package
            .verify_store_signature(&public)
            .map_err(|error| error.to_string())?;
        "developer and store"
    } else {
        "developer"
    };
    println!(
        "verified {signature} signature for {} {} ({} entries)",
        manifest.id,
        manifest.version,
        package.entries().len()
    );
    Ok(())
}

pub fn read_package(path: impl AsRef<Path>) -> Result<CApp, String> {
    let path = path.as_ref();
    let encoded = fs::read(path)
        .map_err(|error| format!("cannot read package {}: {error}", path.display()))?;
    CApp::decode(&encoded).map_err(|error| error.to_string())
}

pub fn manifest_from_package(package: &CApp) -> Result<cp0_manifest::AppManifest, String> {
    let encoded = package
        .entry("app.json")
        .ok_or("package does not contain app.json")?;
    cp0_manifest::parse_and_validate(encoded)
        .map_err(|error| format!("package manifest is invalid: {error}"))
}

fn read_key(path: &str, kind: &str) -> Result<[u8; 32], String> {
    let encoded =
        fs::read(path).map_err(|error| format!("cannot read {kind} key {path}: {error}"))?;
    encoded.try_into().map_err(|value: Vec<u8>| {
        format!(
            "{kind} key {path} must contain exactly 32 raw bytes, got {}",
            value.len()
        )
    })
}

fn write_new(path: impl AsRef<Path>, contents: &[u8], mode: u32) -> Result<(), String> {
    let path = path.as_ref();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot finish {}: {error}", path.display()))?;
    Ok(())
}

pub fn default_package_path(project_path: &str) -> Result<PathBuf, String> {
    let project = fs::canonicalize(project_path)
        .map_err(|error| format!("cannot open project directory: {error}"))?;
    let manifest = cp0_manifest::load_and_validate(project.join("app.json"))
        .map_err(|error| format!("invalid project manifest: {error}"))?;
    Ok(project.join(format!("{}-{}.capp", manifest.id, manifest.version)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_key_length() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("cp0ctl-key-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let key = root.join("short.key");
        fs::write(&key, b"short").unwrap();
        assert!(read_key(key.to_str().unwrap(), "secret").is_err());
    }
}
