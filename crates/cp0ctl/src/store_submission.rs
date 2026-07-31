use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use cp0_package::CApp;
use cp0_store_metadata::{ImageAsset, StoreListing};
use sha2::{Digest, Sha256};

pub fn validate_submission(package_path: &str, listing_path: &str) -> Result<(), String> {
    let package_encoded = read_bounded(
        Path::new(package_path),
        cp0_store_protocol::MAX_PACKAGE_BYTES as usize,
        "developer package",
    )?;
    let package = CApp::decode(&package_encoded)
        .map_err(|error| format!("invalid developer package: {error}"))?;
    package
        .verify_developer_signature()
        .map_err(|error| format!("developer package signature is invalid: {error}"))?;
    if package.store_key_id().is_some() {
        return Err("Store submission must not already contain a Store signature".into());
    }
    let manifest = crate::package::manifest_from_package(&package)?;
    if package.entry(&manifest.entrypoint).is_none() {
        return Err(format!(
            "developer package is missing manifest entrypoint {}",
            manifest.entrypoint
        ));
    }

    let listing_path = Path::new(listing_path);
    let listing_encoded = read_bounded(
        listing_path,
        cp0_store_metadata::MAX_LISTING_BYTES,
        "Store listing",
    )?;
    let listing = cp0_store_metadata::parse_and_validate(&listing_encoded)
        .map_err(|error| error.to_string())?;
    validate_identity(&listing, &manifest)?;

    let asset_root = listing_path.parent().unwrap_or_else(|| Path::new("."));
    validate_asset(asset_root, &listing.icon, (48, 48))?;
    for screenshot in &listing.screenshots {
        validate_asset(asset_root, screenshot, (320, 170))?;
    }

    println!(
        "validated Store submission {} {}: package_sha256={} listing_sha256={} assets={}",
        manifest.id,
        manifest.version,
        lower_hex(&Sha256::digest(&package_encoded)),
        lower_hex(&Sha256::digest(&listing_encoded)),
        listing.screenshots.len() + 1
    );
    Ok(())
}

fn validate_identity(
    listing: &StoreListing,
    manifest: &cp0_manifest::AppManifest,
) -> Result<(), String> {
    if listing.app_id != manifest.id || listing.version != manifest.version {
        return Err("Store listing does not identify the exact package manifest".into());
    }
    let default = listing
        .localizations
        .iter()
        .find(|localized| localized.locale == listing.default_locale)
        .ok_or("Store listing has no default localization")?;
    if default.name != manifest.name {
        return Err("default Store name does not match the package manifest name".into());
    }
    Ok(())
}

fn validate_asset(
    root: &Path,
    asset: &ImageAsset,
    expected_dimensions: (u16, u16),
) -> Result<(), String> {
    let path = checked_asset_path(root, &asset.path)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
        .map_err(|error| format!("cannot open Store asset {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect Store asset {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() != asset.bytes {
        return Err(format!(
            "Store asset {} is not a regular file of the declared size",
            path.display()
        ));
    }
    let mut encoded = Vec::with_capacity(asset.bytes as usize);
    file.by_ref()
        .take(asset.bytes.saturating_add(1))
        .read_to_end(&mut encoded)
        .map_err(|error| format!("cannot read Store asset {}: {error}", path.display()))?;
    if encoded.len() as u64 != asset.bytes {
        return Err(format!(
            "Store asset {} changed while it was being validated",
            path.display()
        ));
    }
    let digest = lower_hex(&Sha256::digest(&encoded));
    if digest != asset.sha256 {
        return Err(format!(
            "Store asset {} SHA-256 does not match the listing",
            path.display()
        ));
    }
    cp0_store_metadata::validate_png_structure(
        &encoded,
        expected_dimensions.0,
        expected_dimensions.1,
    )
    .map_err(|error| format!("invalid Store asset {}: {error}", path.display()))
}

fn checked_asset_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    let components = relative.split('/').collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        path.push(component);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect Store asset {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Store asset path contains a symbolic link: {}",
                path.display()
            ));
        }
        let final_component = index + 1 == components.len();
        if (!final_component && !metadata.is_dir()) || (final_component && !metadata.is_file()) {
            return Err(format!(
                "Store asset path has an invalid component: {}",
                path.display()
            ));
        }
    }
    Ok(path)
}

fn read_bounded(path: &Path, limit: usize, kind: &str) -> Result<Vec<u8>, String> {
    let path_metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {kind} {}: {error}", path.display()))?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        return Err(format!("{kind} {} must be a regular file", path.display()));
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| format!("cannot open {kind} {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect {kind} {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(format!(
            "{kind} {} is not a regular file within the {limit}-byte limit",
            path.display()
        ));
    }
    let mut encoded = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut encoded)
        .map_err(|error| format!("cannot read {kind} {}: {error}", path.display()))?;
    if encoded.len() > limit || encoded.len() as u64 != metadata.len() {
        return Err(format!("{kind} changed while it was being read"));
    }
    Ok(encoded)
}

fn lower_hex(bytes: &[u8]) -> String {
    cp0_store_protocol::lower_hex(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp0_package::PackageEntry;
    use cp0_store_metadata::{AgeRating, LocalizedListing, StoreCategory};

    const EMPTY_WASM: &[u8] = b"\0asm\x01\0\0\0";

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

    fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&(data.len() as u32).to_be_bytes());
        encoded.extend_from_slice(kind);
        encoded.extend_from_slice(data);
        encoded.extend_from_slice(&png_crc32(&encoded[4..]).to_be_bytes());
        encoded
    }

    fn structural_png(width: u16, height: u16) -> Vec<u8> {
        let mut encoded = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&u32::from(width).to_be_bytes());
        header.extend_from_slice(&u32::from(height).to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        encoded.extend_from_slice(&png_chunk(b"IHDR", &header));
        encoded.extend_from_slice(&png_chunk(b"IDAT", &[0x78, 0x01]));
        encoded.extend_from_slice(&png_chunk(b"IEND", &[]));
        encoded
    }

    fn fixture(name: &str) -> (PathBuf, PathBuf, StoreListing) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("store-validate-{name}-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        let images = root.join("store/images");
        fs::create_dir_all(&images).unwrap();

        let manifest = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "id": "dev.cardputerzero.notes",
            "name": "Notes",
            "version": "1.2.0",
            "sdk_version": "1.0",
            "runtime": "wamr",
            "entrypoint": "bin/app.wasm",
            "display": "standard",
            "resources": {"memory_mb": 16, "storage_mb": 4},
            "permissions": []
        }))
        .unwrap();
        let mut package = CApp::new(vec![
            PackageEntry {
                path: "app.json".into(),
                contents: manifest,
            },
            PackageEntry {
                path: "bin/app.wasm".into(),
                contents: EMPTY_WASM.to_vec(),
            },
        ])
        .unwrap();
        package.sign_developer(&[3; 32]).unwrap();
        let package_path = root.join("notes.capp");
        fs::write(&package_path, package.encode().unwrap()).unwrap();

        let icon = structural_png(48, 48);
        let screenshot = structural_png(320, 170);
        fs::write(images.join("icon.png"), &icon).unwrap();
        fs::write(images.join("screen-1.png"), &screenshot).unwrap();
        let listing = StoreListing {
            schema_version: 1,
            app_id: "dev.cardputerzero.notes".into(),
            version: "1.2.0".into(),
            default_locale: "en-US".into(),
            category: StoreCategory::Productivity,
            age_rating: AgeRating::FourPlus,
            privacy_url: "https://example.com/privacy".into(),
            support_url: "https://example.com/support".into(),
            icon: ImageAsset {
                path: "images/icon.png".into(),
                sha256: lower_hex(&Sha256::digest(&icon)),
                bytes: icon.len() as u64,
                width: 48,
                height: 48,
            },
            screenshots: vec![ImageAsset {
                path: "images/screen-1.png".into(),
                sha256: lower_hex(&Sha256::digest(&screenshot)),
                bytes: screenshot.len() as u64,
                width: 320,
                height: 170,
            }],
            localizations: vec![LocalizedListing {
                locale: "en-US".into(),
                name: "Notes".into(),
                subtitle: "Small-screen notes".into(),
                description: "Capture notes offline.".into(),
                keywords: vec!["notes".into()],
                release_notes: "Initial release.".into(),
            }],
        };
        (root, package_path, listing)
    }

    #[test]
    fn validates_exact_signed_package_listing_and_assets() {
        let (root, package, listing) = fixture("valid");
        let listing_path = root.join("store/listing.json");
        fs::write(&listing_path, serde_json::to_vec_pretty(&listing).unwrap()).unwrap();
        validate_submission(package.to_str().unwrap(), listing_path.to_str().unwrap()).unwrap();
    }

    #[test]
    fn rejects_asset_digest_mismatch_and_store_signed_submission() {
        let (root, package_path, mut listing) = fixture("mismatch");
        listing.icon.sha256 = "00".repeat(32);
        let listing_path = root.join("store/listing.json");
        fs::write(&listing_path, serde_json::to_vec(&listing).unwrap()).unwrap();
        assert!(
            validate_submission(
                package_path.to_str().unwrap(),
                listing_path.to_str().unwrap()
            )
            .unwrap_err()
            .contains("SHA-256")
        );

        let mut package = crate::package::read_package(&package_path).unwrap();
        package.sign_store(&[9; 32]).unwrap();
        fs::write(&package_path, package.encode().unwrap()).unwrap();
        assert!(
            validate_submission(
                package_path.to_str().unwrap(),
                listing_path.to_str().unwrap()
            )
            .unwrap_err()
            .contains("must not already contain a Store signature")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_link_asset_components() {
        use std::os::unix::fs::symlink;

        let (root, package, mut listing) = fixture("symlink");
        let real = root.join("store/real-images");
        fs::rename(root.join("store/images"), &real).unwrap();
        symlink(&real, root.join("store/images")).unwrap();
        listing.icon.path = "images/icon.png".into();
        let listing_path = root.join("store/listing.json");
        fs::write(&listing_path, serde_json::to_vec(&listing).unwrap()).unwrap();
        assert!(
            validate_submission(package.to_str().unwrap(), listing_path.to_str().unwrap())
                .unwrap_err()
                .contains("symbolic link")
        );
    }
}
