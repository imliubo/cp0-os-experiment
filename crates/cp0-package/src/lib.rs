use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

pub const FORMAT_VERSION: u16 = 1;
pub const MAX_ENTRIES: usize = 256;
pub const MAX_PATH_BYTES: usize = 240;
pub const MAX_ENTRY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;

const MAGIC: &[u8; 8] = b"CP0CAPP\0";
const FLAG_DEVELOPER_SIGNATURE: u16 = 1 << 0;
const FLAG_STORE_SIGNATURE: u16 = 1 << 1;
const KNOWN_FLAGS: u16 = FLAG_DEVELOPER_SIGNATURE | FLAG_STORE_SIGNATURE;
const DEVELOPER_DOMAIN: &[u8] = b"CardputerZero developer package signature v1\0";
const STORE_DOMAIN: &[u8] = b"CardputerZero store package signature v1\0";
const FIXED_HEADER_BYTES: usize = 8 + 2 + 2 + 4 + 8 + 32 + 64 + 32 + 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEntry {
    pub path: String,
    pub contents: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CApp {
    entries: Vec<PackageEntry>,
    developer_public_key: Option<[u8; 32]>,
    developer_signature: Option<[u8; 64]>,
    store_key_id: Option<[u8; 32]>,
    store_signature: Option<[u8; 64]>,
}

#[derive(Debug)]
pub enum PackageError {
    Io(std::io::Error),
    Invalid(String),
    Signature(String),
}

impl fmt::Display for PackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "package I/O error: {error}"),
            Self::Invalid(error) => write!(formatter, "invalid .capp package: {error}"),
            Self::Signature(error) => write!(formatter, "package signature error: {error}"),
        }
    }
}

impl std::error::Error for PackageError {}

impl From<std::io::Error> for PackageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl CApp {
    pub fn from_directory(root: impl AsRef<Path>) -> Result<Self, PackageError> {
        let root = fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(PackageError::Invalid(
                "package source is not a directory".into(),
            ));
        }
        let mut entries = Vec::new();
        collect_directory(&root, &root, &mut entries)?;
        Self::new(entries)
    }

    pub fn new(mut entries: Vec<PackageEntry>) -> Result<Self, PackageError> {
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        validate_entries(&entries)?;
        Ok(Self {
            entries,
            developer_public_key: None,
            developer_signature: None,
            store_key_id: None,
            store_signature: None,
        })
    }

    pub fn entries(&self) -> &[PackageEntry] {
        &self.entries
    }

    pub fn entry(&self, path: &str) -> Option<&[u8]> {
        self.entries
            .binary_search_by(|entry| entry.path.as_str().cmp(path))
            .ok()
            .map(|index| self.entries[index].contents.as_slice())
    }

    pub fn developer_public_key(&self) -> Option<[u8; 32]> {
        self.developer_public_key
    }

    pub fn store_key_id(&self) -> Option<[u8; 32]> {
        self.store_key_id
    }

    pub fn sign_developer(&mut self, signing_key: &[u8; 32]) -> Result<(), PackageError> {
        let key = SigningKey::from_bytes(signing_key);
        let payload = self.encode_payload()?;
        let signature = key.sign(&signature_message(DEVELOPER_DOMAIN, &payload));
        self.developer_public_key = Some(key.verifying_key().to_bytes());
        self.developer_signature = Some(signature.to_bytes());
        self.store_key_id = None;
        self.store_signature = None;
        Ok(())
    }

    pub fn sign_store(&mut self, signing_key: &[u8; 32]) -> Result<(), PackageError> {
        self.verify_developer_signature()?;
        let key = SigningKey::from_bytes(signing_key);
        let payload = self.encode_payload()?;
        let message = self.store_signature_message(&payload)?;
        self.store_key_id = Some(key_id(&key.verifying_key().to_bytes()));
        self.store_signature = Some(key.sign(&message).to_bytes());
        Ok(())
    }

    pub fn verify_developer_signature(&self) -> Result<(), PackageError> {
        let public_key = self
            .developer_public_key
            .ok_or_else(|| PackageError::Signature("developer signature is missing".into()))?;
        let signature = self
            .developer_signature
            .ok_or_else(|| PackageError::Signature("developer signature is incomplete".into()))?;
        let key = VerifyingKey::from_bytes(&public_key)
            .map_err(|_| PackageError::Signature("developer public key is invalid".into()))?;
        let payload = self.encode_payload()?;
        key.verify(
            &signature_message(DEVELOPER_DOMAIN, &payload),
            &Signature::from_bytes(&signature),
        )
        .map_err(|_| PackageError::Signature("developer signature does not match".into()))
    }

    pub fn verify_store_signature(&self, public_key: &[u8; 32]) -> Result<(), PackageError> {
        self.verify_developer_signature()?;
        let expected_key_id = self
            .store_key_id
            .ok_or_else(|| PackageError::Signature("store signature is missing".into()))?;
        if key_id(public_key) != expected_key_id {
            return Err(PackageError::Signature(
                "store public key does not match package key ID".into(),
            ));
        }
        let signature = self
            .store_signature
            .ok_or_else(|| PackageError::Signature("store signature is incomplete".into()))?;
        let key = VerifyingKey::from_bytes(public_key)
            .map_err(|_| PackageError::Signature("store public key is invalid".into()))?;
        let payload = self.encode_payload()?;
        key.verify(
            &self.store_signature_message(&payload)?,
            &Signature::from_bytes(&signature),
        )
        .map_err(|_| PackageError::Signature("store signature does not match".into()))
    }

    pub fn encode(&self) -> Result<Vec<u8>, PackageError> {
        validate_entries(&self.entries)?;
        validate_signature_state(self)?;
        let payload = self.encode_payload()?;
        let flags = u16::from(self.developer_signature.is_some()) * FLAG_DEVELOPER_SIGNATURE
            | u16::from(self.store_signature.is_some()) * FLAG_STORE_SIGNATURE;
        let mut output = Vec::with_capacity(FIXED_HEADER_BYTES + payload.len());
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        output.extend_from_slice(&flags.to_le_bytes());
        output.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        output.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        output.extend_from_slice(&self.developer_public_key.unwrap_or([0; 32]));
        output.extend_from_slice(&self.developer_signature.unwrap_or([0; 64]));
        output.extend_from_slice(&self.store_key_id.unwrap_or([0; 32]));
        output.extend_from_slice(&self.store_signature.unwrap_or([0; 64]));
        output.extend_from_slice(&payload);
        Ok(output)
    }

    pub fn decode(encoded: &[u8]) -> Result<Self, PackageError> {
        if encoded.len() < FIXED_HEADER_BYTES
            || encoded.len() > FIXED_HEADER_BYTES + MAX_PAYLOAD_BYTES
        {
            return Err(PackageError::Invalid(
                "encoded size is outside limits".into(),
            ));
        }
        let mut reader = Cursor::new(encoded);
        if read_array::<8>(&mut reader)? != *MAGIC {
            return Err(PackageError::Invalid("magic does not match".into()));
        }
        if read_u16(&mut reader)? != FORMAT_VERSION {
            return Err(PackageError::Invalid(
                "format version is unsupported".into(),
            ));
        }
        let flags = read_u16(&mut reader)?;
        if flags & !KNOWN_FLAGS != 0 {
            return Err(PackageError::Invalid("unknown header flags are set".into()));
        }
        if flags & FLAG_STORE_SIGNATURE != 0 && flags & FLAG_DEVELOPER_SIGNATURE == 0 {
            return Err(PackageError::Invalid(
                "store signature requires a developer signature".into(),
            ));
        }
        let entry_count = read_u32(&mut reader)? as usize;
        if entry_count == 0 || entry_count > MAX_ENTRIES {
            return Err(PackageError::Invalid(
                "entry count is outside limits".into(),
            ));
        }
        let payload_length = usize::try_from(read_u64(&mut reader)?)
            .map_err(|_| PackageError::Invalid("payload length overflows this host".into()))?;
        if payload_length > MAX_PAYLOAD_BYTES
            || FIXED_HEADER_BYTES + payload_length != encoded.len()
        {
            return Err(PackageError::Invalid(
                "payload length does not match file size".into(),
            ));
        }
        let developer_public = read_array::<32>(&mut reader)?;
        let developer_signature = read_array::<64>(&mut reader)?;
        let store_key_id = read_array::<32>(&mut reader)?;
        let store_signature = read_array::<64>(&mut reader)?;
        let payload_start = reader.position() as usize;
        let entries = decode_payload(&encoded[payload_start..], entry_count)?;
        let package = Self {
            entries,
            developer_public_key: optional_field(
                flags & FLAG_DEVELOPER_SIGNATURE != 0,
                developer_public,
                "developer public key",
            )?,
            developer_signature: optional_field(
                flags & FLAG_DEVELOPER_SIGNATURE != 0,
                developer_signature,
                "developer signature",
            )?,
            store_key_id: optional_field(
                flags & FLAG_STORE_SIGNATURE != 0,
                store_key_id,
                "store key ID",
            )?,
            store_signature: optional_field(
                flags & FLAG_STORE_SIGNATURE != 0,
                store_signature,
                "store signature",
            )?,
        };
        validate_signature_state(&package)?;
        Ok(package)
    }

    fn encode_payload(&self) -> Result<Vec<u8>, PackageError> {
        validate_entries(&self.entries)?;
        let mut payload = Vec::new();
        for entry in &self.entries {
            payload.extend_from_slice(&(entry.path.len() as u16).to_le_bytes());
            payload.extend_from_slice(&(entry.contents.len() as u32).to_le_bytes());
            payload.extend_from_slice(entry.path.as_bytes());
            payload.extend_from_slice(&entry.contents);
        }
        if payload.len() > MAX_PAYLOAD_BYTES {
            return Err(PackageError::Invalid("package payload is too large".into()));
        }
        Ok(payload)
    }

    fn store_signature_message(&self, payload: &[u8]) -> Result<Vec<u8>, PackageError> {
        let developer_public = self
            .developer_public_key
            .ok_or_else(|| PackageError::Signature("developer signature is missing".into()))?;
        let developer_signature = self
            .developer_signature
            .ok_or_else(|| PackageError::Signature("developer signature is incomplete".into()))?;
        let mut message = Vec::with_capacity(
            STORE_DOMAIN.len() + developer_public.len() + developer_signature.len() + payload.len(),
        );
        message.extend_from_slice(STORE_DOMAIN);
        message.extend_from_slice(&developer_public);
        message.extend_from_slice(&developer_signature);
        message.extend_from_slice(payload);
        Ok(message)
    }
}

pub fn key_id(public_key: &[u8; 32]) -> [u8; 32] {
    Sha256::digest(public_key).into()
}

pub fn public_key(signing_key: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(signing_key)
        .verifying_key()
        .to_bytes()
}

fn collect_directory(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<PackageEntry>,
) -> Result<(), PackageError> {
    let mut children = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let file_type = child.file_type()?;
        if file_type.is_symlink() {
            return Err(PackageError::Invalid(format!(
                "symbolic links are not supported: {}",
                child.path().display()
            )));
        }
        if file_type.is_dir() {
            collect_directory(root, &child.path(), entries)?;
        } else if file_type.is_file() {
            let relative = child
                .path()
                .strip_prefix(root)
                .map_err(|_| PackageError::Invalid("entry escaped package root".into()))?
                .to_path_buf();
            let path = portable_path(&relative)?;
            let metadata = child.metadata()?;
            if metadata.len() > MAX_ENTRY_BYTES as u64 {
                return Err(PackageError::Invalid(format!("entry is too large: {path}")));
            }
            entries.push(PackageEntry {
                path,
                contents: fs::read(child.path())?,
            });
        } else {
            return Err(PackageError::Invalid(format!(
                "unsupported filesystem object: {}",
                child.path().display()
            )));
        }
    }
    Ok(())
}

fn portable_path(path: &Path) -> Result<String, PackageError> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            return Err(PackageError::Invalid("entry path is not normalized".into()));
        };
        let part = part
            .to_str()
            .ok_or_else(|| PackageError::Invalid("entry path is not UTF-8".into()))?;
        if part.is_empty() || part == "." || part == ".." || part.contains('\\') {
            return Err(PackageError::Invalid("entry path is not portable".into()));
        }
        parts.push(part);
    }
    let path = parts.join("/");
    if path.is_empty() || path.len() > MAX_PATH_BYTES {
        return Err(PackageError::Invalid(
            "entry path length is outside limits".into(),
        ));
    }
    Ok(path)
}

fn validate_entries(entries: &[PackageEntry]) -> Result<(), PackageError> {
    if entries.is_empty() || entries.len() > MAX_ENTRIES {
        return Err(PackageError::Invalid(
            "entry count is outside limits".into(),
        ));
    }
    let mut paths = BTreeSet::<&str>::new();
    let mut total = 0_usize;
    for entry in entries {
        if portable_path(Path::new(&entry.path))? != entry.path {
            return Err(PackageError::Invalid(format!(
                "entry path is not canonical: {}",
                entry.path
            )));
        }
        if !paths.insert(entry.path.as_str()) {
            return Err(PackageError::Invalid(format!(
                "entry path is duplicated: {}",
                entry.path
            )));
        }
        if entry.contents.len() > MAX_ENTRY_BYTES {
            return Err(PackageError::Invalid(format!(
                "entry is too large: {}",
                entry.path
            )));
        }
        total = total
            .checked_add(2 + 4 + entry.path.len() + entry.contents.len())
            .ok_or_else(|| PackageError::Invalid("package payload size overflow".into()))?;
    }
    if total > MAX_PAYLOAD_BYTES {
        return Err(PackageError::Invalid("package payload is too large".into()));
    }
    if !paths.contains("app.json") {
        return Err(PackageError::Invalid("app.json is missing".into()));
    }
    Ok(())
}

fn validate_signature_state(package: &CApp) -> Result<(), PackageError> {
    if package.developer_public_key.is_some() != package.developer_signature.is_some() {
        return Err(PackageError::Invalid(
            "developer signature fields are inconsistent".into(),
        ));
    }
    if package.store_key_id.is_some() != package.store_signature.is_some() {
        return Err(PackageError::Invalid(
            "store signature fields are inconsistent".into(),
        ));
    }
    if package.store_signature.is_some() && package.developer_signature.is_none() {
        return Err(PackageError::Invalid(
            "store signature requires a developer signature".into(),
        ));
    }
    Ok(())
}

fn signature_message(domain: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(domain.len() + payload.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(payload);
    message
}

fn decode_payload(payload: &[u8], entry_count: usize) -> Result<Vec<PackageEntry>, PackageError> {
    let mut reader = Cursor::new(payload);
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let path_length = read_u16(&mut reader)? as usize;
        let contents_length = read_u32(&mut reader)? as usize;
        if path_length == 0 || path_length > MAX_PATH_BYTES || contents_length > MAX_ENTRY_BYTES {
            return Err(PackageError::Invalid(
                "entry length is outside limits".into(),
            ));
        }
        let mut path = vec![0; path_length];
        reader.read_exact(&mut path)?;
        let path = String::from_utf8(path)
            .map_err(|_| PackageError::Invalid("entry path is not UTF-8".into()))?;
        let mut contents = vec![0; contents_length];
        reader.read_exact(&mut contents)?;
        entries.push(PackageEntry { path, contents });
    }
    if reader.position() as usize != payload.len() {
        return Err(PackageError::Invalid("payload has trailing bytes".into()));
    }
    validate_entries(&entries)?;
    if entries.windows(2).any(|pair| pair[0].path >= pair[1].path) {
        return Err(PackageError::Invalid(
            "entries are not in canonical order".into(),
        ));
    }
    Ok(entries)
}

fn optional_field<const N: usize>(
    present: bool,
    value: [u8; N],
    name: &str,
) -> Result<Option<[u8; N]>, PackageError> {
    if present {
        if value == [0; N] {
            return Err(PackageError::Invalid(format!("{name} is all zero")));
        }
        Ok(Some(value))
    } else if value == [0; N] {
        Ok(None)
    } else {
        Err(PackageError::Invalid(format!(
            "unused {name} field is not zero"
        )))
    }
}

fn read_u16(reader: &mut Cursor<&[u8]>) -> Result<u16, PackageError> {
    Ok(u16::from_le_bytes(read_array(reader)?))
}

fn read_u32(reader: &mut Cursor<&[u8]>) -> Result<u32, PackageError> {
    Ok(u32::from_le_bytes(read_array(reader)?))
}

fn read_u64(reader: &mut Cursor<&[u8]>) -> Result<u64, PackageError> {
    Ok(u64::from_le_bytes(read_array(reader)?))
}

fn read_array<const N: usize>(reader: &mut Cursor<&[u8]>) -> Result<[u8; N], PackageError> {
    let mut value = [0; N];
    reader
        .read_exact(&mut value)
        .map_err(|_| PackageError::Invalid("package is truncated".into()))?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> CApp {
        CApp::new(vec![
            PackageEntry {
                path: "bin/example.wasm".into(),
                contents: b"wasm fixture".to_vec(),
            },
            PackageEntry {
                path: "app.json".into(),
                contents: br#"{"schema_version":1}"#.to_vec(),
            },
        ])
        .unwrap()
    }

    #[test]
    fn encoding_is_canonical_and_round_trips() {
        let first = package().encode().unwrap();
        let second = package().encode().unwrap();
        assert_eq!(first, second);
        let decoded = CApp::decode(&first).unwrap();
        assert_eq!(decoded.entries()[0].path, "app.json");
        assert_eq!(decoded.encode().unwrap(), first);
    }

    #[test]
    fn developer_and_store_signatures_detect_tampering() {
        let developer_key = [7; 32];
        let store_key = [11; 32];
        let mut package = package();
        package.sign_developer(&developer_key).unwrap();
        package.verify_developer_signature().unwrap();
        package.sign_store(&store_key).unwrap();
        package
            .verify_store_signature(&public_key(&store_key))
            .unwrap();

        let mut encoded = package.encode().unwrap();
        *encoded.last_mut().unwrap() ^= 1;
        let tampered = CApp::decode(&encoded).unwrap();
        assert!(tampered.verify_developer_signature().is_err());
        assert!(
            tampered
                .verify_store_signature(&public_key(&store_key))
                .is_err()
        );
    }

    #[test]
    fn rejects_escape_duplicate_and_noncanonical_paths() {
        for path in ["../app.json", "/app.json", "assets\\icon", "./app.json"] {
            assert!(
                CApp::new(vec![PackageEntry {
                    path: path.into(),
                    contents: Vec::new(),
                }])
                .is_err(),
                "accepted {path}"
            );
        }
        assert!(
            CApp::new(vec![
                PackageEntry {
                    path: "app.json".into(),
                    contents: Vec::new(),
                },
                PackageEntry {
                    path: "app.json".into(),
                    contents: Vec::new(),
                },
            ])
            .is_err()
        );
    }
}
