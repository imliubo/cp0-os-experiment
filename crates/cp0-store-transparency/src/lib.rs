use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TRANSPARENCY_SCHEMA_VERSION: u32 = 1;
pub const MAX_LEAF_BYTES: usize = 4 * 1024;
pub const MAX_CHECKPOINT_BYTES: usize = 2 * 1024;
pub const MAX_TREE_LEAVES: usize = 1_000_000;

const LEAF_DOMAIN: &[u8] = b"CardputerZero transparency leaf v1\0";
const NODE_DOMAIN: &[u8] = b"CardputerZero transparency node v1\0";
const EMPTY_DOMAIN: &[u8] = b"CardputerZero transparency empty tree v1\0";
const CHECKPOINT_DOMAIN: &[u8] = b"CardputerZero transparency checkpoint v1\0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransparencyLeaf {
    pub schema_version: u32,
    pub tree_index: u64,
    pub catalog_sequence: u64,
    pub catalog_sha256: String,
    pub catalog_bytes: u32,
    pub store_key_id: String,
    pub published_unix_seconds: u64,
    pub source_event_id: String,
    pub source_release_id: String,
    pub job_kind: String,
    pub release_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub schema_version: u32,
    pub tree_size: u64,
    pub root_sha256: String,
    pub latest_catalog_sequence: u64,
    pub issued_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCheckpoint {
    pub checkpoint: Checkpoint,
    pub key_id: String,
    pub signature: String,
}

#[derive(Debug)]
pub enum TransparencyError {
    Json(serde_json::Error),
    Invalid(String),
    Signature(String),
    TooLarge,
}

impl fmt::Display for TransparencyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(_) => formatter.write_str("invalid transparency JSON"),
            Self::Invalid(message) => write!(formatter, "invalid transparency data: {message}"),
            Self::Signature(message) => {
                write!(formatter, "transparency signature error: {message}")
            }
            Self::TooLarge => formatter.write_str("transparency object exceeds its size bound"),
        }
    }
}

impl std::error::Error for TransparencyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Invalid(_) | Self::Signature(_) | Self::TooLarge => None,
        }
    }
}

impl From<serde_json::Error> for TransparencyError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl TransparencyLeaf {
    pub fn validate(&self) -> Result<(), TransparencyError> {
        if self.schema_version != TRANSPARENCY_SCHEMA_VERSION {
            return Err(TransparencyError::Invalid(
                "leaf schema version is unsupported".into(),
            ));
        }
        if self.tree_index >= MAX_TREE_LEAVES as u64 {
            return Err(TransparencyError::Invalid(
                "leaf index exceeds the v1 tree bound".into(),
            ));
        }
        if self.catalog_sequence == 0
            || self.catalog_bytes == 0
            || self.catalog_bytes as usize > 48 * 1024
            || self.published_unix_seconds == 0
        {
            return Err(TransparencyError::Invalid(
                "leaf Catalog metadata is invalid".into(),
            ));
        }
        if !is_lower_hex(&self.catalog_sha256, 32) || !is_lower_hex(&self.store_key_id, 32) {
            return Err(TransparencyError::Invalid(
                "leaf digest or Store key ID is invalid".into(),
            ));
        }
        if !valid_prefixed_id(&self.source_event_id, "evt_")
            || !valid_prefixed_id(&self.source_release_id, "rel_")
        {
            return Err(TransparencyError::Invalid(
                "leaf source identity is invalid".into(),
            ));
        }
        let valid_transition = matches!(
            (self.job_kind.as_str(), self.release_state.as_str()),
            ("publish-release", "publishing")
                | ("rebuild-catalog", "published" | "paused" | "removed")
        );
        if !valid_transition {
            return Err(TransparencyError::Invalid(
                "leaf publication kind and Release state do not match".into(),
            ));
        }
        Ok(())
    }
}

impl Checkpoint {
    pub fn validate(&self) -> Result<(), TransparencyError> {
        if self.schema_version != TRANSPARENCY_SCHEMA_VERSION {
            return Err(TransparencyError::Invalid(
                "checkpoint schema version is unsupported".into(),
            ));
        }
        if self.tree_size == 0
            || self.tree_size > MAX_TREE_LEAVES as u64
            || self.latest_catalog_sequence == 0
            || self.issued_unix_seconds == 0
            || !is_lower_hex(&self.root_sha256, 32)
        {
            return Err(TransparencyError::Invalid(
                "checkpoint metadata is invalid".into(),
            ));
        }
        Ok(())
    }
}

pub fn encode_leaf(leaf: &TransparencyLeaf) -> Result<Vec<u8>, TransparencyError> {
    leaf.validate()?;
    let encoded = serde_json::to_vec(leaf)?;
    if encoded.len() > MAX_LEAF_BYTES {
        return Err(TransparencyError::TooLarge);
    }
    Ok(encoded)
}

pub fn decode_leaf(encoded: &[u8]) -> Result<TransparencyLeaf, TransparencyError> {
    if encoded.is_empty() || encoded.len() > MAX_LEAF_BYTES {
        return Err(TransparencyError::TooLarge);
    }
    let leaf: TransparencyLeaf = serde_json::from_slice(encoded)?;
    leaf.validate()?;
    if serde_json::to_vec(&leaf)? != encoded {
        return Err(TransparencyError::Invalid(
            "leaf encoding is not canonical".into(),
        ));
    }
    Ok(leaf)
}

pub fn leaf_hash(leaf: &TransparencyLeaf) -> Result<[u8; 32], TransparencyError> {
    let encoded = encode_leaf(leaf)?;
    let mut hasher = Sha256::new();
    hasher.update(LEAF_DOMAIN);
    hasher.update((encoded.len() as u64).to_be_bytes());
    hasher.update(encoded);
    Ok(hasher.finalize().into())
}

pub fn merkle_root(leaves: &[TransparencyLeaf]) -> Result<[u8; 32], TransparencyError> {
    if leaves.len() > MAX_TREE_LEAVES {
        return Err(TransparencyError::Invalid(
            "transparency tree exceeds its v1 bound".into(),
        ));
    }
    let hashes = leaves
        .iter()
        .map(leaf_hash)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(merkle_root_from_hashes(&hashes))
}

pub fn merkle_root_from_hashes(hashes: &[[u8; 32]]) -> [u8; 32] {
    if hashes.is_empty() {
        return Sha256::digest(EMPTY_DOMAIN).into();
    }
    tree_hash(hashes)
}

pub fn sign_checkpoint(
    checkpoint: Checkpoint,
    signing_key: &[u8; 32],
) -> Result<SignedCheckpoint, TransparencyError> {
    checkpoint.validate()?;
    let canonical = serde_json::to_vec(&checkpoint)?;
    let key = SigningKey::from_bytes(signing_key);
    let public_key = key.verifying_key().to_bytes();
    let signature = key.sign(&checkpoint_message(&canonical));
    Ok(SignedCheckpoint {
        checkpoint,
        key_id: lower_hex(&cp0_package::key_id(&public_key)),
        signature: lower_hex(&signature.to_bytes()),
    })
}

pub fn verify_checkpoint(
    signed: &SignedCheckpoint,
    public_key: &[u8; 32],
) -> Result<(), TransparencyError> {
    signed.checkpoint.validate()?;
    if signed.key_id != lower_hex(&cp0_package::key_id(public_key)) {
        return Err(TransparencyError::Signature(
            "checkpoint key ID does not match the trusted key".into(),
        ));
    }
    let signature = decode_hex::<64>(&signed.signature).ok_or_else(|| {
        TransparencyError::Signature("checkpoint signature encoding is invalid".into())
    })?;
    let key = VerifyingKey::from_bytes(public_key)
        .map_err(|_| TransparencyError::Signature("trusted key is invalid".into()))?;
    let canonical = serde_json::to_vec(&signed.checkpoint)?;
    key.verify(
        &checkpoint_message(&canonical),
        &Signature::from_bytes(&signature),
    )
    .map_err(|_| TransparencyError::Signature("checkpoint signature does not match".into()))
}

pub fn encode_checkpoint(signed: &SignedCheckpoint) -> Result<Vec<u8>, TransparencyError> {
    signed.checkpoint.validate()?;
    if !is_lower_hex(&signed.key_id, 32) || !is_lower_hex(&signed.signature, 64) {
        return Err(TransparencyError::Invalid(
            "checkpoint signature fields are invalid".into(),
        ));
    }
    let encoded = serde_json::to_vec(signed)?;
    if encoded.len() > MAX_CHECKPOINT_BYTES {
        return Err(TransparencyError::TooLarge);
    }
    Ok(encoded)
}

pub fn decode_checkpoint(encoded: &[u8]) -> Result<SignedCheckpoint, TransparencyError> {
    if encoded.is_empty() || encoded.len() > MAX_CHECKPOINT_BYTES {
        return Err(TransparencyError::TooLarge);
    }
    let signed: SignedCheckpoint = serde_json::from_slice(encoded)?;
    signed.checkpoint.validate()?;
    if !is_lower_hex(&signed.key_id, 32) || !is_lower_hex(&signed.signature, 64) {
        return Err(TransparencyError::Invalid(
            "checkpoint signature fields are invalid".into(),
        ));
    }
    if serde_json::to_vec(&signed)? != encoded {
        return Err(TransparencyError::Invalid(
            "checkpoint encoding is not canonical".into(),
        ));
    }
    Ok(signed)
}

pub fn verify_log(
    signed: &SignedCheckpoint,
    public_key: &[u8; 32],
    leaves: &[TransparencyLeaf],
) -> Result<(), TransparencyError> {
    verify_checkpoint(signed, public_key)?;
    if leaves.len() != signed.checkpoint.tree_size as usize || leaves.is_empty() {
        return Err(TransparencyError::Invalid(
            "checkpoint tree size does not match the supplied log".into(),
        ));
    }
    for (index, leaf) in leaves.iter().enumerate() {
        leaf.validate()?;
        if leaf.tree_index != index as u64 {
            return Err(TransparencyError::Invalid(
                "transparency leaves are not contiguous".into(),
            ));
        }
    }
    let latest = leaves
        .last()
        .ok_or_else(|| TransparencyError::Invalid("transparency log is empty".into()))?;
    if latest.catalog_sequence != signed.checkpoint.latest_catalog_sequence
        || lower_hex(&merkle_root(leaves)?) != signed.checkpoint.root_sha256
    {
        return Err(TransparencyError::Invalid(
            "checkpoint does not commit to the supplied log".into(),
        ));
    }
    Ok(())
}

pub fn verify_append_only_prefix(
    older: &SignedCheckpoint,
    newer: &SignedCheckpoint,
    public_key: &[u8; 32],
    leaves: &[TransparencyLeaf],
) -> Result<(), TransparencyError> {
    if older.checkpoint.tree_size > newer.checkpoint.tree_size
        || newer.checkpoint.tree_size as usize != leaves.len()
    {
        return Err(TransparencyError::Invalid(
            "checkpoint sizes do not form an append-only prefix".into(),
        ));
    }
    let old_size = older.checkpoint.tree_size as usize;
    verify_log(older, public_key, &leaves[..old_size])?;
    verify_log(newer, public_key, leaves)
}

pub fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing hexadecimal into String cannot fail");
    }
    output
}

fn tree_hash(hashes: &[[u8; 32]]) -> [u8; 32] {
    if hashes.len() == 1 {
        return hashes[0];
    }
    let split = largest_power_of_two_less_than(hashes.len());
    let left = tree_hash(&hashes[..split]);
    let right = tree_hash(&hashes[split..]);
    let mut hasher = Sha256::new();
    hasher.update(NODE_DOMAIN);
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

fn largest_power_of_two_less_than(value: usize) -> usize {
    debug_assert!(value >= 2);
    let highest = 1_usize << (usize::BITS - 1 - value.leading_zeros());
    if highest == value {
        highest / 2
    } else {
        highest
    }
}

fn checkpoint_message(canonical: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(CHECKPOINT_DOMAIN.len() + 8 + canonical.len());
    message.extend_from_slice(CHECKPOINT_DOMAIN);
    message.extend_from_slice(&(canonical.len() as u64).to_be_bytes());
    message.extend_from_slice(canonical);
    message
}

fn valid_prefixed_id(value: &str, prefix: &str) -> bool {
    value.len() == prefix.len() + 32
        && value.starts_with(prefix)
        && value[prefix.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_lower_hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes * 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn decode_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    if !is_lower_hex(value, N) {
        return None;
    }
    let mut decoded = [0_u8; N];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        decoded[index] = (hex_digit(chunk[0])? << 4) | hex_digit(chunk[1])?;
    }
    Some(decoded)
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(index: u64, sequence: u64) -> TransparencyLeaf {
        TransparencyLeaf {
            schema_version: 1,
            tree_index: index,
            catalog_sequence: sequence,
            catalog_sha256: format!("{sequence:064x}"),
            catalog_bytes: 700,
            store_key_id: "a".repeat(64),
            published_unix_seconds: 1_800_000_000 + sequence,
            source_event_id: format!("evt_{:032x}", sequence),
            source_release_id: format!("rel_{:032x}", sequence),
            job_kind: "publish-release".into(),
            release_state: "publishing".into(),
        }
    }

    fn checkpoint(leaves: &[TransparencyLeaf], secret: &[u8; 32]) -> SignedCheckpoint {
        sign_checkpoint(
            Checkpoint {
                schema_version: 1,
                tree_size: leaves.len() as u64,
                root_sha256: lower_hex(&merkle_root(leaves).unwrap()),
                latest_catalog_sequence: leaves.last().unwrap().catalog_sequence,
                issued_unix_seconds: leaves.last().unwrap().published_unix_seconds,
            },
            secret,
        )
        .unwrap()
    }

    #[test]
    fn leaf_and_checkpoint_encodings_are_deterministic() {
        let leaves = vec![leaf(0, 4), leaf(1, 8), leaf(2, 9)];
        let secret = [7_u8; 32];
        let signed = checkpoint(&leaves, &secret);
        let encoded = encode_checkpoint(&signed).unwrap();
        assert_eq!(decode_checkpoint(&encoded).unwrap(), signed);
        assert_eq!(
            decode_leaf(&encode_leaf(&leaves[0]).unwrap()).unwrap(),
            leaves[0]
        );
        verify_log(&signed, &cp0_package::public_key(&secret), &leaves).unwrap();
    }

    #[test]
    fn root_commits_to_order_and_every_leaf_field() {
        let leaves = vec![leaf(0, 1), leaf(1, 2), leaf(2, 3)];
        let root = merkle_root(&leaves).unwrap();
        let mut changed = leaves.clone();
        changed[1].catalog_bytes += 1;
        assert_ne!(root, merkle_root(&changed).unwrap());
        changed = leaves.clone();
        changed.swap(0, 1);
        assert_ne!(root, merkle_root(&changed).unwrap());
    }

    #[test]
    fn signature_and_log_tampering_are_rejected() {
        let leaves = vec![leaf(0, 1), leaf(1, 2)];
        let secret = [9_u8; 32];
        let signed = checkpoint(&leaves, &secret);
        assert!(verify_log(&signed, &cp0_package::public_key(&[8_u8; 32]), &leaves).is_err());
        let mut changed = leaves.clone();
        changed[1].catalog_sha256 = "f".repeat(64);
        assert!(verify_log(&signed, &cp0_package::public_key(&secret), &changed).is_err());
    }

    #[test]
    fn complete_prefix_verification_detects_forks() {
        let secret = [3_u8; 32];
        let old = vec![leaf(0, 10), leaf(1, 11)];
        let mut current = old.clone();
        current.push(leaf(2, 20));
        let old_checkpoint = checkpoint(&old, &secret);
        let current_checkpoint = checkpoint(&current, &secret);
        verify_append_only_prefix(
            &old_checkpoint,
            &current_checkpoint,
            &cp0_package::public_key(&secret),
            &current,
        )
        .unwrap();
        let mut fork = current;
        fork[0].catalog_bytes += 1;
        assert!(
            verify_append_only_prefix(
                &old_checkpoint,
                &current_checkpoint,
                &cp0_package::public_key(&secret),
                &fork,
            )
            .is_err()
        );
    }
}
