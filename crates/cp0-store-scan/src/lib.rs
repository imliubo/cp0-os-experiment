use std::collections::{BTreeSet, HashSet};

use cp0_manifest::{AppManifest, Permission};
use cp0_package::CApp;
use cp0_store_metadata::{ImageAsset, StoreListing};
use cp0_store_risk::{RiskAssessment, classify_permissions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use wasmparser::{Parser, Payload, TypeRef, Validator};

const ABI_CONTRACT: &str = include_str!("../../../sdk/abi/cardputerzero-hostcalls-v1.json");
pub const SCANNER_VERSION: &str = "cp0-store-scan/1";
pub const MAX_FINDINGS: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct ScanAsset<'a> {
    pub descriptor: &'a ImageAsset,
    pub encoded: &'a [u8],
}

#[derive(Debug)]
pub struct ScanInput<'a> {
    pub expected_app_id: &'a str,
    pub expected_version: &'a str,
    pub expected_default_locale: &'a str,
    pub package: &'a [u8],
    pub listing: &'a [u8],
    pub assets: &'a [ScanAsset<'a>],
    pub trusted_developer_keys: &'a [[u8; 32]],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScanDisposition {
    ReadyForReview,
    NeedsChanges,
    Rejected,
}

impl ScanDisposition {
    pub const fn as_submission_state(self) -> &'static str {
        match self {
            Self::ReadyForReview => "ready-for-review",
            Self::NeedsChanges => "needs-changes",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindingSeverity {
    Error,
    Security,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanFinding {
    pub code: String,
    pub severity: FindingSeverity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanReport {
    pub scanner_version: String,
    pub disposition: ScanDisposition,
    pub developer_key_sha256: Option<String>,
    pub imports: Vec<String>,
    pub permissions: Vec<String>,
    pub findings: Vec<ScanFinding>,
    pub risk: Option<RiskAssessment>,
}

impl ScanReport {
    fn failed(code: &str, disposition: ScanDisposition, severity: FindingSeverity) -> Self {
        Self {
            scanner_version: SCANNER_VERSION.to_owned(),
            disposition,
            developer_key_sha256: None,
            imports: Vec::new(),
            permissions: Vec::new(),
            findings: vec![ScanFinding {
                code: code.to_owned(),
                severity,
            }],
            risk: None,
        }
    }
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
}

pub fn scan(input: &ScanInput<'_>) -> ScanReport {
    let package = match CApp::decode(input.package) {
        Ok(package) => package,
        Err(_) => {
            return ScanReport::failed(
                "package.invalid",
                ScanDisposition::Rejected,
                FindingSeverity::Security,
            );
        }
    };
    if package.verify_developer_signature().is_err() {
        return ScanReport::failed(
            "package.developer-signature-invalid",
            ScanDisposition::Rejected,
            FindingSeverity::Security,
        );
    }
    if package.store_key_id().is_some() {
        return ScanReport::failed(
            "package.store-signature-present",
            ScanDisposition::Rejected,
            FindingSeverity::Security,
        );
    }
    let Some(developer_key) = package.developer_public_key() else {
        return ScanReport::failed(
            "package.developer-key-missing",
            ScanDisposition::Rejected,
            FindingSeverity::Security,
        );
    };
    let developer_key_sha256 = lower_hex(&Sha256::digest(developer_key));
    if !input
        .trusted_developer_keys
        .iter()
        .any(|trusted| trusted == &developer_key)
    {
        let mut report = ScanReport::failed(
            "package.developer-key-untrusted",
            ScanDisposition::Rejected,
            FindingSeverity::Security,
        );
        report.developer_key_sha256 = Some(developer_key_sha256);
        return report;
    }

    let manifest = match package
        .entry("app.json")
        .ok_or(())
        .and_then(|encoded| cp0_manifest::parse_and_validate(encoded).map_err(|_| ()))
    {
        Ok(manifest) => manifest,
        Err(()) => {
            return report_with_key(
                "manifest.invalid",
                ScanDisposition::Rejected,
                FindingSeverity::Security,
                developer_key_sha256,
            );
        }
    };
    if manifest.id != input.expected_app_id || manifest.version != input.expected_version {
        return report_with_key(
            "manifest.identity-mismatch",
            ScanDisposition::Rejected,
            FindingSeverity::Security,
            developer_key_sha256,
        );
    }
    if !matches!(manifest.sdk_version.as_str(), "1.1" | "1.0" | "0.1") {
        return report_with_key(
            "manifest.sdk-unsupported",
            ScanDisposition::NeedsChanges,
            FindingSeverity::Error,
            developer_key_sha256,
        );
    }
    if !manifest.entrypoint.ends_with(".wasm") {
        return report_with_key(
            "manifest.entrypoint-not-wasm",
            ScanDisposition::Rejected,
            FindingSeverity::Security,
            developer_key_sha256,
        );
    }
    let Some(wasm) = package.entry(&manifest.entrypoint) else {
        return report_with_key(
            "manifest.entrypoint-missing",
            ScanDisposition::NeedsChanges,
            FindingSeverity::Error,
            developer_key_sha256,
        );
    };
    let imports = match inspect_imports(wasm) {
        Ok(imports) => imports,
        Err(code) => {
            return report_with_key(
                code,
                ScanDisposition::Rejected,
                FindingSeverity::Security,
                developer_key_sha256,
            );
        }
    };
    if !imports_match_permissions(&manifest, &imports) {
        return report_with_key(
            "wasm.permission-not-declared",
            ScanDisposition::Rejected,
            FindingSeverity::Security,
            developer_key_sha256,
        );
    }

    let listing = match cp0_store_metadata::parse_and_validate(input.listing) {
        Ok(listing) => listing,
        Err(_) => {
            return report_with_key(
                "listing.invalid",
                ScanDisposition::NeedsChanges,
                FindingSeverity::Error,
                developer_key_sha256,
            );
        }
    };
    if !listing_matches_manifest(&listing, &manifest) {
        return report_with_key(
            "listing.identity-mismatch",
            ScanDisposition::NeedsChanges,
            FindingSeverity::Error,
            developer_key_sha256,
        );
    }
    if listing.default_locale != input.expected_default_locale {
        return report_with_key(
            "listing.default-locale-mismatch",
            ScanDisposition::NeedsChanges,
            FindingSeverity::Error,
            developer_key_sha256,
        );
    }

    let descriptors = std::iter::once(&listing.icon)
        .chain(listing.screenshots.iter())
        .collect::<Vec<_>>();
    if descriptors.len() != input.assets.len()
        || descriptors
            .iter()
            .zip(input.assets)
            .any(|(expected, actual)| *expected != actual.descriptor)
    {
        return report_with_key(
            "asset.descriptor-mismatch",
            ScanDisposition::Rejected,
            FindingSeverity::Security,
            developer_key_sha256,
        );
    }
    for (index, asset) in input.assets.iter().enumerate() {
        if asset.encoded.len() as u64 != asset.descriptor.bytes
            || lower_hex(&Sha256::digest(asset.encoded)) != asset.descriptor.sha256
        {
            return report_with_key(
                "asset.digest-mismatch",
                ScanDisposition::Rejected,
                FindingSeverity::Security,
                developer_key_sha256,
            );
        }
        let dimensions = if index == 0 { (48, 48) } else { (320, 170) };
        if cp0_store_metadata::validate_png_structure(asset.encoded, dimensions.0, dimensions.1)
            .is_err()
        {
            return report_with_key(
                "asset.png-invalid",
                ScanDisposition::NeedsChanges,
                FindingSeverity::Error,
                developer_key_sha256,
            );
        }
    }

    let mut permissions = manifest
        .permissions
        .iter()
        .map(|request| request.name.as_str().to_owned())
        .collect::<Vec<_>>();
    permissions.sort();
    let declared_permissions = manifest
        .permissions
        .iter()
        .map(|request| request.name)
        .collect::<Vec<_>>();
    ScanReport {
        scanner_version: SCANNER_VERSION.to_owned(),
        disposition: ScanDisposition::ReadyForReview,
        developer_key_sha256: Some(developer_key_sha256),
        imports,
        permissions,
        findings: Vec::new(),
        risk: Some(classify_permissions(&declared_permissions)),
    }
}

pub fn report_sha256(report: &ScanReport) -> Result<String, serde_json::Error> {
    serde_json::to_vec(report).map(|encoded| lower_hex(&Sha256::digest(encoded)))
}

pub fn submission_content_sha256(
    package_sha256: &str,
    listing_sha256: &str,
    assets: &[ImageAsset],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"CardputerZero Store submission content v1\0");
    hash_field(&mut hasher, package_sha256.as_bytes());
    hash_field(&mut hasher, listing_sha256.as_bytes());
    for asset in assets {
        hash_field(&mut hasher, asset.path.as_bytes());
        hash_field(&mut hasher, asset.sha256.as_bytes());
        hasher.update(asset.bytes.to_be_bytes());
        hasher.update(asset.width.to_be_bytes());
        hasher.update(asset.height.to_be_bytes());
    }
    lower_hex(&hasher.finalize())
}

fn report_with_key(
    code: &str,
    disposition: ScanDisposition,
    severity: FindingSeverity,
    developer_key_sha256: String,
) -> ScanReport {
    let mut report = ScanReport::failed(code, disposition, severity);
    report.developer_key_sha256 = Some(developer_key_sha256);
    report
}

fn listing_matches_manifest(listing: &StoreListing, manifest: &AppManifest) -> bool {
    listing.app_id == manifest.id
        && listing.version == manifest.version
        && listing
            .localizations
            .iter()
            .find(|localized| localized.locale == listing.default_locale)
            .is_some_and(|localized| localized.name == manifest.name)
}

fn inspect_imports(wasm: &[u8]) -> Result<Vec<String>, &'static str> {
    Validator::new()
        .validate_all(wasm)
        .map_err(|_| "wasm.invalid")?;
    let abi: AbiContract = serde_json::from_str(ABI_CONTRACT).map_err(|_| "scanner.abi-invalid")?;
    if abi.schema_version != 1 || abi.abi_version != "1.1" || abi.imports.is_empty() {
        return Err("scanner.abi-invalid");
    }
    let allowed = abi
        .imports
        .iter()
        .map(|import| import.name.as_str())
        .collect::<HashSet<_>>();
    if allowed.len() != abi.imports.len() {
        return Err("scanner.abi-invalid");
    }
    let mut imports = BTreeSet::new();
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::ImportSection(section) = payload.map_err(|_| "wasm.invalid")? {
            for import in section.into_imports() {
                let import = import.map_err(|_| "wasm.invalid")?;
                if import.module != abi.module
                    || !matches!(import.ty, TypeRef::Func(_))
                    || !allowed.contains(import.name)
                {
                    return Err("wasm.import-unsupported");
                }
                if !imports.insert(import.name.to_owned()) {
                    return Err("wasm.import-duplicate");
                }
            }
        }
    }
    Ok(imports.into_iter().collect())
}

fn imports_match_permissions(manifest: &AppManifest, imports: &[String]) -> bool {
    let declared = manifest
        .permissions
        .iter()
        .map(|request| request.name)
        .collect::<BTreeSet<_>>();
    imports.iter().all(|import| {
        if import == "cp0_camera_capture_photo" {
            declared.contains(&Permission::CameraCapture)
                && declared.contains(&Permission::PhotosWrite)
        } else {
            required_permission(import).is_none_or(|permission| declared.contains(&permission))
        }
    })
}

fn required_permission(import: &str) -> Option<Permission> {
    match import {
        "cp0_post_notification" => Some(Permission::NotificationsPost),
        "cp0_http_get" | "cp0_http_get_range" => Some(Permission::NetworkClient),
        "cp0_document_open" | "cp0_document_read" | "cp0_document_close" => {
            Some(Permission::DocumentsOpen)
        }
        "cp0_audio_play_pcm_s16le" | "cp0_audio_play_pcm_s16le_stereo_48khz" => {
            Some(Permission::AudioPlayback)
        }
        "cp0_audio_capture_pcm_s16le" => Some(Permission::AudioCapture),
        "cp0_camera_capture_rgb565" => Some(Permission::CameraCapture),
        "cp0_gpio_read" | "cp0_gpio_write" => Some(Permission::HardwareGpio),
        "cp0_lora_send" | "cp0_lora_receive" => Some(Permission::RadioLora),
        "cp0_photos_get" | "cp0_photos_load_rgb565" | "cp0_photos_load_view_rgb565" => {
            Some(Permission::PhotosRead)
        }
        "cp0_photos_put"
        | "cp0_photos_index_get"
        | "cp0_photos_delete"
        | "cp0_photos_import_rgb565"
        | "cp0_photos_remove" => Some(Permission::PhotosWrite),
        _ => None,
    }
}

fn hash_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing hexadecimal into String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp0_manifest::{DisplayMode, PermissionRequest, ResourceLimits, Runtime};
    use cp0_package::PackageEntry;
    use cp0_store_metadata::{AgeRating, LocalizedListing, StoreCategory};

    const EMPTY_WASM: &[u8] = b"\0asm\x01\0\0\0";

    #[test]
    fn accepts_exact_signed_bundle_from_a_trusted_key() {
        let fixture = Fixture::valid();
        let report = fixture.scan();
        assert_eq!(report.disposition, ScanDisposition::ReadyForReview);
        assert!(report.findings.is_empty());
        assert_eq!(
            report.risk.as_ref().map(|risk| risk.tier),
            Some(cp0_store_risk::RiskTier::Standard)
        );
        assert_eq!(
            report.developer_key_sha256.as_deref(),
            Some(fixture.key_sha256.as_str())
        );
        assert_eq!(report_sha256(&report).unwrap().len(), 64);
    }

    #[test]
    fn rejects_untrusted_keys_and_ambient_authority() {
        let mut fixture = Fixture::valid();
        fixture.trusted_keys.clear();
        let report = fixture.scan();
        assert_eq!(report.disposition, ScanDisposition::Rejected);
        assert!(report.risk.is_none());
        assert_eq!(report.findings[0].code, "package.developer-key-untrusted");

        let fixture = Fixture::with_wasm(&wasi_import_module());
        let report = fixture.scan();
        assert_eq!(report.disposition, ScanDisposition::Rejected);
        assert_eq!(report.findings[0].code, "wasm.import-unsupported");
    }

    #[test]
    fn rejects_undeclared_capability_and_malformed_assets() {
        let mut fixture = Fixture::with_wasm(&host_import_module("cp0_http_get"));
        let report = fixture.scan();
        assert_eq!(report.findings[0].code, "wasm.permission-not-declared");

        fixture = Fixture::with_wasm(&host_import_module("cp0_http_get_range"));
        let report = fixture.scan();
        assert_eq!(report.findings[0].code, "wasm.permission-not-declared");

        fixture = Fixture::with_wasm(&host_import_module("cp0_audio_play_pcm_s16le_stereo_48khz"));
        let report = fixture.scan();
        assert_eq!(report.findings[0].code, "wasm.permission-not-declared");

        fixture = Fixture::with_wasm(&host_import_module("cp0_camera_capture_photo"));
        let report = fixture.scan();
        assert_eq!(report.findings[0].code, "wasm.permission-not-declared");

        fixture = Fixture::valid();
        fixture.assets[0].encoded[0] ^= 0xff;
        fixture.asset_descriptors[0].sha256 =
            lower_hex(&Sha256::digest(&fixture.assets[0].encoded));
        fixture.asset_descriptors[0].bytes = fixture.assets[0].encoded.len() as u64;
        fixture.listing.icon = fixture.asset_descriptors[0].clone();
        fixture.listing_encoded = serde_json::to_vec(&fixture.listing).unwrap();
        let report = fixture.scan();
        assert_eq!(report.findings[0].code, "asset.png-invalid");

        fixture = Fixture::valid();
        let report = fixture.scan_for_locale("zh-CN");
        assert_eq!(report.findings[0].code, "listing.default-locale-mismatch");
    }

    struct Fixture {
        package: Vec<u8>,
        listing: StoreListing,
        listing_encoded: Vec<u8>,
        asset_descriptors: Vec<ImageAsset>,
        assets: Vec<OwnedAsset>,
        trusted_keys: Vec<[u8; 32]>,
        key_sha256: String,
    }

    struct OwnedAsset {
        encoded: Vec<u8>,
    }

    impl Fixture {
        fn valid() -> Self {
            Self::with_wasm(EMPTY_WASM)
        }

        fn with_wasm(wasm: &[u8]) -> Self {
            let manifest = manifest();
            let mut package = CApp::new(vec![
                PackageEntry {
                    path: "app.json".into(),
                    contents: serde_json::to_vec(&manifest).unwrap(),
                },
                PackageEntry {
                    path: "app.wasm".into(),
                    contents: wasm.to_vec(),
                },
            ])
            .unwrap();
            let signing_key = [7_u8; 32];
            package.sign_developer(&signing_key).unwrap();
            let developer_key = package.developer_public_key().unwrap();
            let package = package.encode().unwrap();
            let icon = png(48, 48);
            let screenshot = png(320, 170);
            let asset_descriptors = vec![
                descriptor("icon.png", &icon, 48, 48),
                descriptor("screen.png", &screenshot, 320, 170),
            ];
            let listing = listing(&asset_descriptors);
            let listing_encoded = serde_json::to_vec(&listing).unwrap();
            Self {
                package,
                listing,
                listing_encoded,
                asset_descriptors,
                assets: vec![
                    OwnedAsset { encoded: icon },
                    OwnedAsset {
                        encoded: screenshot,
                    },
                ],
                trusted_keys: vec![developer_key],
                key_sha256: lower_hex(&Sha256::digest(developer_key)),
            }
        }

        fn scan(&self) -> ScanReport {
            self.scan_for_locale("en-US")
        }

        fn scan_for_locale(&self, expected_default_locale: &str) -> ScanReport {
            let assets = self
                .asset_descriptors
                .iter()
                .zip(&self.assets)
                .map(|(descriptor, asset)| ScanAsset {
                    descriptor,
                    encoded: &asset.encoded,
                })
                .collect::<Vec<_>>();
            super::scan(&ScanInput {
                expected_app_id: "dev.example.scan",
                expected_version: "1.0.0",
                expected_default_locale,
                package: &self.package,
                listing: &self.listing_encoded,
                assets: &assets,
                trusted_developer_keys: &self.trusted_keys,
            })
        }
    }

    fn manifest() -> AppManifest {
        AppManifest {
            schema_version: 1,
            id: "dev.example.scan".into(),
            name: "Scan Test".into(),
            version: "1.0.0".into(),
            sdk_version: "1.1".into(),
            runtime: Runtime::Wamr,
            entrypoint: "app.wasm".into(),
            display: DisplayMode::Standard,
            resources: ResourceLimits {
                memory_mb: 16,
                storage_mb: 16,
            },
            permissions: Vec::<PermissionRequest>::new(),
            intents: Vec::new(),
        }
    }

    fn listing(assets: &[ImageAsset]) -> StoreListing {
        StoreListing {
            schema_version: 1,
            app_id: "dev.example.scan".into(),
            version: "1.0.0".into(),
            default_locale: "en-US".into(),
            category: StoreCategory::Utilities,
            age_rating: AgeRating::FourPlus,
            privacy_url: "https://example.com/privacy".into(),
            support_url: "https://example.com/support".into(),
            icon: assets[0].clone(),
            screenshots: vec![assets[1].clone()],
            localizations: vec![LocalizedListing {
                locale: "en-US".into(),
                name: "Scan Test".into(),
                subtitle: "A scanner fixture".into(),
                description: "A deterministic scanner fixture used by tests.".into(),
                keywords: vec!["scanner".into()],
                release_notes: "Initial release.".into(),
            }],
        }
    }

    fn descriptor(path: &str, encoded: &[u8], width: u16, height: u16) -> ImageAsset {
        ImageAsset {
            path: path.into(),
            sha256: lower_hex(&Sha256::digest(encoded)),
            bytes: encoded.len() as u64,
            width,
            height,
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut encoded = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        encoded.extend_from_slice(&png_chunk(b"IHDR", &ihdr));
        encoded.extend_from_slice(&png_chunk(b"IDAT", &[]));
        encoded.extend_from_slice(&png_chunk(b"IEND", &[]));
        encoded
    }

    fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&(data.len() as u32).to_be_bytes());
        encoded.extend_from_slice(kind);
        encoded.extend_from_slice(data);
        encoded.extend_from_slice(&png_crc32(&encoded[4..]).to_be_bytes());
        encoded
    }

    fn png_crc32(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    fn host_import_module(name: &str) -> Vec<u8> {
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        wasm.extend_from_slice(&[1, 4, 1, 0x60, 0, 0]);
        let module = b"cardputerzero";
        let mut section = vec![1, module.len() as u8];
        section.extend_from_slice(module);
        section.push(name.len() as u8);
        section.extend_from_slice(name.as_bytes());
        section.extend_from_slice(&[0, 0]);
        wasm.push(2);
        wasm.push(section.len() as u8);
        wasm.extend_from_slice(&section);
        wasm
    }

    fn wasi_import_module() -> Vec<u8> {
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        wasm.extend_from_slice(&[1, 4, 1, 0x60, 0, 0]);
        let module = b"wasi_snapshot_preview1";
        let name = b"path_open";
        let mut section = vec![1, module.len() as u8];
        section.extend_from_slice(module);
        section.push(name.len() as u8);
        section.extend_from_slice(name);
        section.extend_from_slice(&[0, 0]);
        wasm.push(2);
        wasm.push(section.len() as u8);
        wasm.extend_from_slice(&section);
        wasm
    }
}
