use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use cp0_manifest::{AppManifest, Permission};
use cp0_package::CApp;
use cp0_store_protocol::{
    CATALOG_SCHEMA_VERSION, Catalog, CatalogApp, MAX_CATALOG_APPS, MAX_CATALOG_LIFETIME_SECONDS,
    MAX_SUMMARY_CHARS, encode_signed_catalog, is_valid_https_url, lower_hex, sign_catalog,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use wasmparser::{Parser, Payload, TypeRef, Validator};

const REVIEW_SCHEMA_VERSION: u32 = 1;
const ABI_CONTRACT: &str = include_str!("../../../sdk/abi/cardputerzero-hostcalls-v1.json");

pub struct PublishOptions<'a> {
    pub submissions: &'a str,
    pub reviews: &'a str,
    pub output: &'a str,
    pub base_url: &'a str,
    pub sequence: &'a str,
    pub published: &'a str,
    pub expires: &'a str,
    pub secret: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AbiContract {
    schema_version: u32,
    abi_version: String,
    module: String,
    imports: Vec<AbiImport>,
}

#[derive(Debug, Deserialize)]
struct AbiImport {
    name: String,
    #[serde(flatten)]
    _metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    schema_version: u32,
    decision: ReviewDecision,
    app_id: String,
    version: String,
    submission_sha256: String,
    summary: String,
    reviewer: String,
    reviewed_unix_seconds: u64,
    approved_permissions: Vec<Permission>,
    approved_imports: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ReviewDecision {
    Approved,
    Rejected,
}

struct ReviewedPackage {
    manifest: AppManifest,
    summary: String,
    encoded: Vec<u8>,
}

pub fn publish(options: PublishOptions<'_>) -> Result<(), String> {
    let sequence = parse_nonzero(options.sequence, "catalog sequence")?;
    let published = parse_nonzero(options.published, "published timestamp")?;
    let expires = parse_nonzero(options.expires, "expiry timestamp")?;
    if expires
        .checked_sub(published)
        .is_none_or(|lifetime| lifetime == 0 || lifetime > MAX_CATALOG_LIFETIME_SECONDS)
    {
        return Err("catalog validity interval is outside limits".into());
    }
    let base_url = options.base_url.trim_end_matches('/');
    if !is_valid_https_url(&format!("{base_url}/catalog.json")) {
        return Err("store base URL must be bounded HTTPS without credentials or fragments".into());
    }
    let secret = read_key(options.secret)?;
    let abi = load_abi()?;
    let submissions = scan_submissions(Path::new(options.submissions))?;
    if submissions.is_empty() {
        return Err("submission directory contains no .capp packages".into());
    }
    if submissions.len() > MAX_CATALOG_APPS {
        return Err(format!(
            "submission directory contains more than {MAX_CATALOG_APPS} packages"
        ));
    }

    let mut reviewed = BTreeMap::new();
    for path in submissions {
        let package = review_package(&path, Path::new(options.reviews), &abi, published, &secret)?;
        if reviewed
            .insert(package.manifest.id.clone(), package)
            .is_some()
        {
            return Err("catalog contains more than one version of an application".into());
        }
    }

    let mut catalog_apps = Vec::with_capacity(reviewed.len());
    let mut artifacts = Vec::with_capacity(reviewed.len());
    for (app_id, package) in reviewed {
        let file_name = format!("{}.capp", package.manifest.version);
        let package_sha256 = lower_hex(&Sha256::digest(&package.encoded));
        let package_url = format!("{base_url}/apps/{app_id}/{file_name}");
        if !is_valid_https_url(&package_url) {
            return Err(format!("generated package URL is invalid for {app_id}"));
        }
        let mut permissions = package
            .manifest
            .permissions
            .iter()
            .map(|request| request.name)
            .collect::<Vec<_>>();
        permissions.sort_by_key(|permission| permission.as_str());
        catalog_apps.push(CatalogApp {
            app_id: app_id.clone(),
            name: package.manifest.name,
            version: package.manifest.version,
            sdk_version: package.manifest.sdk_version,
            summary: package.summary,
            package_url,
            package_sha256,
            package_bytes: package.encoded.len() as u64,
            permissions,
        });
        artifacts.push((app_id, file_name, package.encoded));
    }
    let catalog = Catalog {
        schema_version: CATALOG_SCHEMA_VERSION,
        sequence,
        published_unix_seconds: published,
        expires_unix_seconds: expires,
        apps: catalog_apps,
    };
    let signed = sign_catalog(catalog, &secret).map_err(|error| error.to_string())?;
    let encoded = encode_signed_catalog(&signed).map_err(|error| error.to_string())?;

    let output = Path::new(options.output);
    if output.exists() {
        return Err(format!("store output already exists: {}", output.display()));
    }
    fs::create_dir(output)
        .map_err(|error| format!("cannot create store output {}: {error}", output.display()))?;
    fs::create_dir(output.join("apps"))
        .map_err(|error| format!("cannot create store application directory: {error}"))?;
    for (app_id, file_name, package) in artifacts {
        let app_directory = output.join("apps").join(&app_id);
        fs::create_dir(&app_directory)
            .map_err(|error| format!("cannot create output for {app_id}: {error}"))?;
        write_new(app_directory.join(file_name), &package, 0o644)?;
    }
    write_new(output.join("catalog.json"), &encoded, 0o644)?;
    write_new(
        output.join("store.pub"),
        &cp0_package::public_key(&secret),
        0o644,
    )?;
    println!(
        "published {} reviewed applications in catalog sequence {} at {}",
        signed.catalog.apps.len(),
        sequence,
        output.display()
    );
    Ok(())
}

fn scan_submissions(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let metadata = fs::metadata(directory).map_err(|error| {
        format!(
            "cannot read submission directory {}: {error}",
            directory.display()
        )
    })?;
    if !metadata.is_dir() {
        return Err("submission path is not a directory".into());
    }
    let mut packages = Vec::new();
    for entry in
        fs::read_dir(directory).map_err(|error| format!("cannot scan submissions: {error}"))?
    {
        let entry = entry.map_err(|error| format!("cannot scan submissions: {error}"))?;
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "capp") {
            continue;
        }
        if !entry
            .file_type()
            .map_err(|error| format!("cannot inspect submission {}: {error}", path.display()))?
            .is_file()
        {
            return Err(format!(
                "submission is not a regular file: {}",
                path.display()
            ));
        }
        packages.push(path);
    }
    packages.sort();
    Ok(packages)
}

fn review_package(
    path: &Path,
    review_root: &Path,
    abi: &AbiContract,
    published: u64,
    store_secret: &[u8; 32],
) -> Result<ReviewedPackage, String> {
    let submission = fs::read(path)
        .map_err(|error| format!("cannot read submission {}: {error}", path.display()))?;
    let submission_sha256 = lower_hex(&Sha256::digest(&submission));
    let mut package = CApp::decode(&submission)
        .map_err(|error| format!("invalid submission {}: {error}", path.display()))?;
    package
        .verify_developer_signature()
        .map_err(|error| format!("untrusted submission {}: {error}", path.display()))?;
    if package.store_key_id().is_some() {
        return Err(format!(
            "submission {} already contains a store signature",
            path.display()
        ));
    }
    let manifest = crate::package::manifest_from_package(&package)?;
    if !matches!(manifest.sdk_version.as_str(), "1.0" | "0.1") {
        return Err(format!(
            "{} uses unsupported SDK {}",
            manifest.id, manifest.sdk_version
        ));
    }
    if !manifest.entrypoint.ends_with(".wasm") {
        return Err(format!(
            "{} store review requires a WebAssembly entrypoint",
            manifest.id
        ));
    }
    let wasm = package.entry(&manifest.entrypoint).ok_or_else(|| {
        format!(
            "{} is missing entrypoint {}",
            manifest.id, manifest.entrypoint
        )
    })?;
    let imports = inspect_imports(wasm, abi)?;
    validate_import_permissions(&manifest, &imports)?;

    let review_path = review_root.join(format!("{}-{}.review.json", manifest.id, manifest.version));
    let review_encoded = fs::read(&review_path)
        .map_err(|error| format!("cannot read review {}: {error}", review_path.display()))?;
    let review: Review = serde_json::from_slice(&review_encoded)
        .map_err(|error| format!("invalid review {}: {error}", review_path.display()))?;
    validate_review(&review, &manifest, &submission_sha256, &imports, published)?;

    package
        .sign_store(store_secret)
        .map_err(|error| format!("cannot sign {}: {error}", manifest.id))?;
    let encoded = package
        .encode()
        .map_err(|error| format!("cannot encode {}: {error}", manifest.id))?;
    Ok(ReviewedPackage {
        manifest,
        summary: review.summary,
        encoded,
    })
}

fn load_abi() -> Result<AbiContract, String> {
    let abi: AbiContract = serde_json::from_str(ABI_CONTRACT)
        .map_err(|error| format!("invalid built-in ABI: {error}"))?;
    if abi.schema_version != 1
        || abi.abi_version != "1.0"
        || abi.module != "cardputerzero"
        || abi.imports.is_empty()
    {
        return Err("built-in ABI identity is invalid".into());
    }
    let mut names = BTreeSet::new();
    if abi.imports.iter().any(|import| !names.insert(&import.name)) {
        return Err("built-in ABI contains duplicate imports".into());
    }
    Ok(abi)
}

fn inspect_imports(wasm: &[u8], abi: &AbiContract) -> Result<Vec<String>, String> {
    Validator::new()
        .validate_all(wasm)
        .map_err(|error| format!("entrypoint is invalid WebAssembly: {error}"))?;
    let allowed = abi
        .imports
        .iter()
        .map(|import| import.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut imports = BTreeSet::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::ImportSection(section) =
            payload.map_err(|error| format!("entrypoint is invalid WebAssembly: {error}"))?
        {
            for import in section.into_imports() {
                let import = import
                    .map_err(|error| format!("entrypoint import section is invalid: {error}"))?;
                if import.module != abi.module
                    || !matches!(import.ty, TypeRef::Func(_))
                    || !allowed.contains(import.name)
                {
                    return Err(format!(
                        "entrypoint imports unsupported symbol {}::{}",
                        import.module, import.name
                    ));
                }
                if !imports.insert(import.name.to_owned()) {
                    return Err(format!("entrypoint imports {} more than once", import.name));
                }
            }
        }
    }
    Ok(imports.into_iter().collect())
}

fn validate_import_permissions(manifest: &AppManifest, imports: &[String]) -> Result<(), String> {
    let declared = manifest
        .permissions
        .iter()
        .map(|request| request.name)
        .collect::<BTreeSet<_>>();
    for import in imports {
        let required = match import.as_str() {
            "cp0_post_notification" => Some(Permission::NotificationsPost),
            "cp0_http_get" => Some(Permission::NetworkClient),
            "cp0_document_open" | "cp0_document_read" | "cp0_document_close" => {
                Some(Permission::DocumentsOpen)
            }
            "cp0_audio_play_pcm_s16le" => Some(Permission::AudioPlayback),
            "cp0_audio_capture_pcm_s16le" => Some(Permission::AudioCapture),
            "cp0_camera_capture_rgb565" => Some(Permission::CameraCapture),
            "cp0_gpio_read" | "cp0_gpio_write" => Some(Permission::HardwareGpio),
            "cp0_lora_send" | "cp0_lora_receive" => Some(Permission::RadioLora),
            _ => None,
        };
        if required.is_some_and(|permission| !declared.contains(&permission)) {
            return Err(format!(
                "{} imports {import} without declaring its permission",
                manifest.id
            ));
        }
    }
    Ok(())
}

fn validate_review(
    review: &Review,
    manifest: &AppManifest,
    submission_sha256: &str,
    imports: &[String],
    published: u64,
) -> Result<(), String> {
    if review.schema_version != REVIEW_SCHEMA_VERSION
        || review.decision != ReviewDecision::Approved
        || review.app_id != manifest.id
        || review.version != manifest.version
        || review.submission_sha256 != submission_sha256
    {
        return Err(format!(
            "review does not approve the exact submission for {} {}",
            manifest.id, manifest.version
        ));
    }
    let summary_chars = review.summary.chars().count();
    if !(1..=MAX_SUMMARY_CHARS).contains(&summary_chars)
        || has_unsafe_text(&review.summary)
        || !(3..=64).contains(&review.reviewer.chars().count())
        || has_unsafe_text(&review.reviewer)
        || review.reviewed_unix_seconds == 0
        || review.reviewed_unix_seconds > published
    {
        return Err(format!("review metadata is invalid for {}", manifest.id));
    }
    let mut permissions = manifest
        .permissions
        .iter()
        .map(|request| request.name)
        .collect::<Vec<_>>();
    permissions.sort_by_key(|permission| permission.as_str());
    if review.approved_permissions != permissions || review.approved_imports != imports {
        return Err(format!(
            "review does not approve the exact permissions and imports for {}",
            manifest.id
        ));
    }
    Ok(())
}

fn parse_nonzero(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| format!("{name} must be a non-zero unsigned integer"))
}

fn read_key(path: &str) -> Result<[u8; 32], String> {
    fs::read(path)
        .map_err(|error| format!("cannot read store secret key {path}: {error}"))?
        .try_into()
        .map_err(|value: Vec<u8>| {
            format!(
                "store secret key {path} must contain 32 raw bytes, got {}",
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
        .map_err(|error| format!("cannot finish {}: {error}", path.display()))
}

fn has_unsafe_text(value: &str) -> bool {
    value.chars().any(|character| character.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp0_package::PackageEntry;

    const EMPTY_WASM: &[u8] = b"\0asm\x01\0\0\0";

    fn fixture_root(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("store-publish-{name}-{}", std::process::id()))
    }

    fn manifest_json() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "id": "dev.cardputerzero.reviewed",
            "name": "Reviewed App",
            "version": "1.0.0",
            "sdk_version": "1.0",
            "runtime": "wamr",
            "entrypoint": "bin/app.wasm",
            "display": "standard",
            "resources": {"memory_mb": 16, "storage_mb": 4},
            "permissions": []
        }))
        .unwrap()
    }

    fn prepare_fixture(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = fixture_root(name);
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        let submissions = root.join("submissions");
        let reviews = root.join("reviews");
        fs::create_dir_all(&submissions).unwrap();
        fs::create_dir_all(&reviews).unwrap();
        let developer_secret = [3; 32];
        let mut package = CApp::new(vec![
            PackageEntry {
                path: "app.json".into(),
                contents: manifest_json(),
            },
            PackageEntry {
                path: "bin/app.wasm".into(),
                contents: EMPTY_WASM.to_vec(),
            },
        ])
        .unwrap();
        package.sign_developer(&developer_secret).unwrap();
        let encoded = package.encode().unwrap();
        fs::write(submissions.join("reviewed.capp"), &encoded).unwrap();
        let review = serde_json::json!({
            "schema_version": 1,
            "decision": "approved",
            "app_id": "dev.cardputerzero.reviewed",
            "version": "1.0.0",
            "submission_sha256": lower_hex(&Sha256::digest(&encoded)),
            "summary": "A fully reviewed application",
            "reviewer": "release-reviewer",
            "reviewed_unix_seconds": 1_800_000_000_u64,
            "approved_permissions": [],
            "approved_imports": []
        });
        fs::write(
            reviews.join("dev.cardputerzero.reviewed-1.0.0.review.json"),
            serde_json::to_vec_pretty(&review).unwrap(),
        )
        .unwrap();
        let secret = root.join("store.secret");
        fs::write(&secret, [7; 32]).unwrap();
        (root, submissions, secret)
    }

    #[test]
    fn publishes_deterministic_review_bound_catalog_and_packages() {
        let (root, submissions, secret) = prepare_fixture("deterministic");
        let reviews = root.join("reviews");
        let first = root.join("first");
        let second = root.join("second");
        for output in [&first, &second] {
            publish(PublishOptions {
                submissions: submissions.to_str().unwrap(),
                reviews: reviews.to_str().unwrap(),
                output: output.to_str().unwrap(),
                base_url: "https://store.example.com/v1",
                sequence: "8",
                published: "1800000010",
                expires: "1800086410",
                secret: secret.to_str().unwrap(),
            })
            .unwrap();
        }

        let relative_package = Path::new("apps/dev.cardputerzero.reviewed/1.0.0.capp");
        assert_eq!(
            fs::read(first.join("catalog.json")).unwrap(),
            fs::read(second.join("catalog.json")).unwrap()
        );
        assert_eq!(
            fs::read(first.join(relative_package)).unwrap(),
            fs::read(second.join(relative_package)).unwrap()
        );
        let public: [u8; 32] = fs::read(first.join("store.pub"))
            .unwrap()
            .try_into()
            .unwrap();
        let signed = cp0_store_protocol::decode_signed_catalog(
            &fs::read(first.join("catalog.json")).unwrap(),
        )
        .unwrap();
        cp0_store_protocol::verify_catalog(&signed, &public).unwrap();
        let package = CApp::decode(&fs::read(first.join(relative_package)).unwrap()).unwrap();
        package.verify_store_signature(&public).unwrap();
    }

    #[test]
    fn rejects_unknown_imports_and_imports_without_permissions() {
        let abi = load_abi().unwrap();
        let mut unknown = EMPTY_WASM.to_vec();
        unknown.extend_from_slice(&[1, 4, 1, 0x60, 0, 0]);
        unknown.extend_from_slice(&[
            2, 13, 1, 4, b'e', b'v', b'i', b'l', 4, b'o', b'p', b'e', b'n', 0, 0,
        ]);
        assert!(inspect_imports(&unknown, &abi).is_err());

        let manifest = cp0_manifest::parse_and_validate(&manifest_json()).unwrap();
        assert!(validate_import_permissions(&manifest, &["cp0_http_get".into()]).is_err());
    }
}
