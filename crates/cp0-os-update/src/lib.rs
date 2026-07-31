use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

pub const BOARD_ID: &str = "cardputerzero-cm0-v0.6";
pub const RELEASE_FORMAT: &str = "cp0-os-release-v1";
pub const BOOT_STATE_FORMAT: &str = "cp0-ab-state-v1";
pub const BOOT_STATE_RECORD_FORMAT: &str = "cp0-ab-state-record-v1";
pub const MAX_BOOT_ATTEMPTS: u8 = 3;

const STATE_CHECKSUM_DOMAIN: &[u8] = b"CardputerZero A/B boot state checksum v1\0";
const BLOCK_SIZE: u64 = 4096;
const MAX_JSON_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_VERITY_DATA_BLOCKS: u64 = MAX_JSON_INTEGER / BLOCK_SIZE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateError(String);

impl UpdateError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for UpdateError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BootSlot {
    A,
    B,
}

impl BootSlot {
    pub fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub sha256: String,
    pub size: u64,
}

impl Artifact {
    fn validate(&self, name: &str) -> Result<(), UpdateError> {
        if !is_lower_hex(&self.sha256, 32) {
            return Err(UpdateError::new(format!(
                "{name} SHA-256 must be 64 lowercase hexadecimal characters"
            )));
        }
        if self.size == 0 || self.size > MAX_JSON_INTEGER {
            return Err(UpdateError::new(format!(
                "{name} size is outside the exact JSON integer range"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerityDescriptor {
    pub root_hash: String,
    pub salt: String,
    pub data_blocks: u64,
    pub hash_tree: Artifact,
}

impl VerityDescriptor {
    fn validate(&self, rootfs_size: u64) -> Result<(), UpdateError> {
        if !is_lower_hex(&self.root_hash, 32) {
            return Err(UpdateError::new(
                "verity root hash must be 64 lowercase hexadecimal characters",
            ));
        }
        if self.salt.len() < 32 || self.salt.len() > 128 || !is_lower_hex_any(&self.salt) {
            return Err(UpdateError::new(
                "verity salt must contain 16 to 64 bytes of lowercase hexadecimal data",
            ));
        }
        if self.data_blocks == 0 || self.data_blocks > MAX_VERITY_DATA_BLOCKS {
            return Err(UpdateError::new(
                "verity data block count is outside the supported range",
            ));
        }
        let data_bytes = self
            .data_blocks
            .checked_mul(BLOCK_SIZE)
            .ok_or_else(|| UpdateError::new("verity data size overflows u64"))?;
        if data_bytes != rootfs_size {
            return Err(UpdateError::new(
                "rootfs size must contain exactly the declared 4096-byte verity data blocks",
            ));
        }
        self.hash_tree.validate("verity hash tree")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FitDescriptor {
    pub artifact: Artifact,
    pub configuration: String,
}

impl FitDescriptor {
    fn validate(&self) -> Result<(), UpdateError> {
        self.artifact.validate("FIT artifact")?;
        if self.configuration != "conf-a" && self.configuration != "conf-b" {
            return Err(UpdateError::new(
                "FIT configuration must be conf-a or conf-b",
            ));
        }
        Ok(())
    }
}

/// Policy metadata carried inside an already authenticated OS update envelope.
///
/// RAUC CMS and signed FIT verification are deliberately outside this type.
/// Callers must not treat successful JSON validation as signature verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseMetadata {
    pub format: String,
    pub board_id: String,
    pub version: String,
    pub sequence: u64,
    pub data_layout_min: u32,
    pub data_layout_max: u32,
    pub rootfs: Artifact,
    pub verity: VerityDescriptor,
    pub fit: FitDescriptor,
}

impl ReleaseMetadata {
    pub fn decode(encoded: &[u8]) -> Result<Self, UpdateError> {
        if encoded.len() > 16 * 1024 {
            return Err(UpdateError::new("OS release metadata exceeds 16384 bytes"));
        }
        let metadata: Self = serde_json::from_slice(encoded)
            .map_err(|error| UpdateError::new(format!("invalid OS release metadata: {error}")))?;
        metadata.validate()?;
        Ok(metadata)
    }

    pub fn validate(&self) -> Result<(), UpdateError> {
        if self.format != RELEASE_FORMAT {
            return Err(UpdateError::new("unsupported OS release metadata format"));
        }
        if self.board_id != BOARD_ID {
            return Err(UpdateError::new("OS release targets a different board"));
        }
        if self.version.len() < 5 || self.version.len() > 64 {
            return Err(UpdateError::new(
                "OS release version must contain 5 to 64 bytes",
            ));
        }
        let parsed_version = Version::parse(&self.version)
            .map_err(|error| UpdateError::new(format!("invalid OS version: {error}")))?;
        if !parsed_version.build.is_empty() {
            return Err(UpdateError::new(
                "OS release version must not contain SemVer build metadata",
            ));
        }
        if self.sequence == 0 || self.sequence > MAX_JSON_INTEGER {
            return Err(UpdateError::new(
                "OS release sequence is outside the exact JSON integer range",
            ));
        }
        if self.data_layout_min == 0 || self.data_layout_min > self.data_layout_max {
            return Err(UpdateError::new("invalid persistent data layout range"));
        }
        self.rootfs.validate("rootfs artifact")?;
        self.verity.validate(self.rootfs.size)?;
        self.fit.validate()
    }

    /// Applies board, data-layout and monotonic-sequence policy after the
    /// surrounding RAUC bundle and FIT signatures have been authenticated.
    pub fn authorize_install(
        &self,
        installed_sequence: u64,
        data_layout: u32,
        target_slot: BootSlot,
    ) -> Result<(), UpdateError> {
        self.validate()?;
        if self.sequence <= installed_sequence {
            return Err(UpdateError::new("OS release sequence is not newer"));
        }
        if data_layout < self.data_layout_min || data_layout > self.data_layout_max {
            return Err(UpdateError::new(
                "OS release is incompatible with the persistent data layout",
            ));
        }
        let expected_configuration = match target_slot {
            BootSlot::A => "conf-a",
            BootSlot::B => "conf-b",
        };
        if self.fit.configuration != expected_configuration {
            return Err(UpdateError::new(
                "FIT configuration does not match the target slot",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingBoot {
    pub slot: BootSlot,
    pub sequence: u64,
    pub attempts_remaining: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootState {
    pub format: String,
    pub generation: u64,
    pub confirmed_slot: BootSlot,
    pub confirmed_sequence: u64,
    pub pending: Option<PendingBoot>,
    pub last_attempted: Option<BootSlot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootDecision {
    pub slot: BootSlot,
    pub sequence: u64,
    pub pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthReport {
    pub compositor: bool,
    pub appd: bool,
    pub data_mount: bool,
}

impl HealthReport {
    pub fn is_healthy(self) -> bool {
        self.compositor && self.appd && self.data_mount
    }
}

impl BootState {
    pub fn factory(slot: BootSlot, sequence: u64) -> Self {
        Self {
            format: BOOT_STATE_FORMAT.into(),
            generation: 0,
            confirmed_slot: slot,
            confirmed_sequence: sequence,
            pending: None,
            last_attempted: None,
        }
    }

    pub fn validate(&self) -> Result<(), UpdateError> {
        if self.format != BOOT_STATE_FORMAT {
            return Err(UpdateError::new("unsupported A/B boot state format"));
        }
        if self.confirmed_sequence > MAX_JSON_INTEGER {
            return Err(UpdateError::new(
                "confirmed sequence exceeds the exact JSON integer range",
            ));
        }
        if let Some(pending) = &self.pending {
            if pending.slot == self.confirmed_slot {
                return Err(UpdateError::new("pending slot is already confirmed"));
            }
            if pending.sequence <= self.confirmed_sequence {
                return Err(UpdateError::new(
                    "pending sequence is not newer than the confirmed sequence",
                ));
            }
            if pending.sequence > MAX_JSON_INTEGER {
                return Err(UpdateError::new(
                    "pending sequence exceeds the exact JSON integer range",
                ));
            }
            if pending.attempts_remaining > MAX_BOOT_ATTEMPTS {
                return Err(UpdateError::new(
                    "pending boot attempt count exceeds policy",
                ));
            }
        }
        Ok(())
    }

    pub fn stage(&self, slot: BootSlot, sequence: u64) -> Result<Self, UpdateError> {
        self.validate()?;
        if slot == self.confirmed_slot {
            return Err(UpdateError::new("an update must target the inactive slot"));
        }
        if sequence <= self.confirmed_sequence
            || self
                .pending
                .as_ref()
                .is_some_and(|pending| sequence <= pending.sequence)
        {
            return Err(UpdateError::new("update sequence is not monotonic"));
        }
        let mut next = self.next_generation()?;
        next.pending = Some(PendingBoot {
            slot,
            sequence,
            attempts_remaining: MAX_BOOT_ATTEMPTS,
        });
        next.last_attempted = None;
        Ok(next)
    }

    /// Returns the state that must be durably persisted before transferring
    /// control to the selected slot, together with the boot decision.
    pub fn prepare_boot(&self) -> Result<(Self, BootDecision), UpdateError> {
        self.validate()?;
        let mut next = self.next_generation()?;
        let decision = match &mut next.pending {
            Some(pending) if pending.attempts_remaining > 0 => {
                pending.attempts_remaining -= 1;
                next.last_attempted = Some(pending.slot);
                BootDecision {
                    slot: pending.slot,
                    sequence: pending.sequence,
                    pending: true,
                }
            }
            Some(_) => {
                next.pending = None;
                next.last_attempted = Some(next.confirmed_slot);
                BootDecision {
                    slot: next.confirmed_slot,
                    sequence: next.confirmed_sequence,
                    pending: false,
                }
            }
            None => {
                next.last_attempted = Some(next.confirmed_slot);
                BootDecision {
                    slot: next.confirmed_slot,
                    sequence: next.confirmed_sequence,
                    pending: false,
                }
            }
        };
        Ok((next, decision))
    }

    pub fn confirm(&self, health: HealthReport) -> Result<Self, UpdateError> {
        self.validate()?;
        if !health.is_healthy() {
            return Err(UpdateError::new("slot health ceremony is incomplete"));
        }
        let pending = self
            .pending
            .as_ref()
            .ok_or_else(|| UpdateError::new("there is no pending slot to confirm"))?;
        if self.last_attempted != Some(pending.slot) {
            return Err(UpdateError::new(
                "the pending slot has not completed a recorded boot attempt",
            ));
        }
        let mut next = self.next_generation()?;
        next.confirmed_slot = pending.slot;
        next.confirmed_sequence = pending.sequence;
        next.pending = None;
        next.last_attempted = Some(pending.slot);
        Ok(next)
    }

    pub fn encode_record(&self) -> Result<Vec<u8>, UpdateError> {
        self.validate()?;
        let state = serde_json::to_vec(self)
            .map_err(|error| UpdateError::new(format!("cannot encode boot state: {error}")))?;
        let record = BootStateRecord {
            format: BOOT_STATE_RECORD_FORMAT.into(),
            checksum: state_checksum(&state),
            state: self.clone(),
        };
        let mut encoded = serde_json::to_vec(&record)
            .map_err(|error| UpdateError::new(format!("cannot encode boot record: {error}")))?;
        encoded.push(b'\n');
        Ok(encoded)
    }

    pub fn decode_record(encoded: &[u8]) -> Result<Self, UpdateError> {
        if encoded.len() > 4096 {
            return Err(UpdateError::new("boot state record exceeds 4096 bytes"));
        }
        let record: BootStateRecord = serde_json::from_slice(encoded)
            .map_err(|error| UpdateError::new(format!("invalid boot state record: {error}")))?;
        if record.format != BOOT_STATE_RECORD_FORMAT {
            return Err(UpdateError::new("unsupported boot state record format"));
        }
        record.state.validate()?;
        let state = serde_json::to_vec(&record.state)
            .map_err(|error| UpdateError::new(format!("cannot encode boot state: {error}")))?;
        if record.checksum != state_checksum(&state) {
            return Err(UpdateError::new("boot state checksum does not match"));
        }
        Ok(record.state)
    }

    pub fn newest_valid_record<'a, I>(records: I) -> Result<Self, UpdateError>
    where
        I: IntoIterator<Item = &'a [u8]>,
    {
        let mut selected: Option<Self> = None;
        for encoded in records {
            let Ok(candidate) = Self::decode_record(encoded) else {
                continue;
            };
            match &selected {
                Some(current) if current.generation > candidate.generation => {}
                Some(current) if current.generation == candidate.generation => {
                    if current != &candidate {
                        return Err(UpdateError::new(
                            "boot state records have a split generation",
                        ));
                    }
                }
                _ => selected = Some(candidate),
            }
        }
        selected.ok_or_else(|| UpdateError::new("no valid boot state record remains"))
    }

    fn next_generation(&self) -> Result<Self, UpdateError> {
        let mut next = self.clone();
        next.generation = next
            .generation
            .checked_add(1)
            .ok_or_else(|| UpdateError::new("boot state generation is exhausted"))?;
        Ok(next)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootStateRecord {
    format: String,
    checksum: String,
    state: BootState,
}

fn state_checksum(state: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(STATE_CHECKSUM_DOMAIN);
    hasher.update(state);
    lower_hex(&hasher.finalize())
}

fn is_lower_hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes * 2 && is_lower_hex_any(value)
}

fn is_lower_hex_any(value: &str) -> bool {
    value.len() % 2 == 0
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(byte: char, size: u64) -> Artifact {
        Artifact {
            sha256: byte.to_string().repeat(64),
            size,
        }
    }

    fn release() -> ReleaseMetadata {
        ReleaseMetadata {
            format: RELEASE_FORMAT.into(),
            board_id: BOARD_ID.into(),
            version: "1.2.3".into(),
            sequence: 7,
            data_layout_min: 1,
            data_layout_max: 1,
            rootfs: artifact('a', 4096 * 100),
            verity: VerityDescriptor {
                root_hash: "b".repeat(64),
                salt: "c".repeat(64),
                data_blocks: 100,
                hash_tree: artifact('d', 4096 * 4),
            },
            fit: FitDescriptor {
                artifact: artifact('e', 1024 * 1024),
                configuration: "conf-b".into(),
            },
        }
    }

    fn healthy() -> HealthReport {
        HealthReport {
            compositor: true,
            appd: true,
            data_mount: true,
        }
    }

    #[test]
    fn release_policy_rejects_stale_wrong_board_and_wrong_layout() {
        let candidate = release();
        candidate.authorize_install(6, 1, BootSlot::B).unwrap();

        assert!(candidate.authorize_install(7, 1, BootSlot::B).is_err());
        assert!(candidate.authorize_install(6, 2, BootSlot::B).is_err());
        assert!(candidate.authorize_install(6, 1, BootSlot::A).is_err());

        let mut wrong_board = candidate.clone();
        wrong_board.board_id = "another-board".into();
        assert!(wrong_board.authorize_install(6, 1, BootSlot::B).is_err());

        let mut partial_rootfs_block = candidate;
        partial_rootfs_block.rootfs.size += 1;
        assert!(partial_rootfs_block.validate().is_err());
    }

    #[test]
    fn release_decoder_is_bounded_and_rejects_unknown_fields() {
        let encoded = serde_json::to_vec(&release()).unwrap();
        assert_eq!(ReleaseMetadata::decode(&encoded).unwrap(), release());

        let mut value = serde_json::to_value(release()).unwrap();
        value["unexpected"] = serde_json::Value::Bool(true);
        assert!(ReleaseMetadata::decode(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(ReleaseMetadata::decode(&vec![b' '; 16 * 1024 + 1]).is_err());
    }

    #[test]
    fn only_a_booted_healthy_pending_slot_can_be_confirmed() {
        let factory = BootState::factory(BootSlot::A, 4);
        let staged = factory.stage(BootSlot::B, 5).unwrap();
        assert!(staged.confirm(healthy()).is_err());

        let (attempted, decision) = staged.prepare_boot().unwrap();
        assert_eq!(decision.slot, BootSlot::B);
        assert!(decision.pending);
        assert_eq!(attempted.pending.as_ref().unwrap().attempts_remaining, 2);

        let mut incomplete = healthy();
        incomplete.appd = false;
        assert!(attempted.confirm(incomplete).is_err());

        let confirmed = attempted.confirm(healthy()).unwrap();
        assert_eq!(confirmed.confirmed_slot, BootSlot::B);
        assert_eq!(confirmed.confirmed_sequence, 5);
        assert!(confirmed.pending.is_none());
    }

    #[test]
    fn three_failed_attempts_fall_back_to_the_confirmed_slot() {
        let mut state = BootState::factory(BootSlot::A, 11)
            .stage(BootSlot::B, 12)
            .unwrap();
        for remaining in (0..MAX_BOOT_ATTEMPTS).rev() {
            let (next, decision) = state.prepare_boot().unwrap();
            assert_eq!(decision.slot, BootSlot::B);
            assert_eq!(next.pending.as_ref().unwrap().attempts_remaining, remaining);
            state = next;
        }

        let (fallback, decision) = state.prepare_boot().unwrap();
        assert_eq!(decision.slot, BootSlot::A);
        assert!(!decision.pending);
        assert!(fallback.pending.is_none());
        assert_eq!(fallback.confirmed_sequence, 11);
    }

    #[test]
    fn checksummed_redundant_records_tolerate_one_torn_copy() {
        let old = BootState::factory(BootSlot::A, 1);
        let current = old.stage(BootSlot::B, 2).unwrap();
        let old_record = old.encode_record().unwrap();
        let current_record = current.encode_record().unwrap();
        let mut torn = current_record.clone();
        torn.truncate(torn.len() / 2);

        let selected =
            BootState::newest_valid_record([old_record.as_slice(), current_record.as_slice()])
                .unwrap();
        assert_eq!(selected, current);

        let selected =
            BootState::newest_valid_record([torn.as_slice(), old_record.as_slice()]).unwrap();
        assert_eq!(selected, old);
    }

    #[test]
    fn corruption_and_unknown_fields_fail_closed() {
        let state = BootState::factory(BootSlot::A, 1);
        let encoded = state.encode_record().unwrap();
        let mut changed_state: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        changed_state["state"]["last_attempted"] = serde_json::Value::String("a".into());
        assert!(BootState::decode_record(&serde_json::to_vec(&changed_state).unwrap()).is_err());

        let with_unknown = serde_json::to_vec(&serde_json::json!({
            "format": BOOT_STATE_RECORD_FORMAT,
            "checksum": "0".repeat(64),
            "state": state,
            "unexpected": true
        }))
        .unwrap();
        assert!(BootState::decode_record(&with_unknown).is_err());
    }

    #[test]
    fn conflicting_records_at_the_same_generation_fail_closed() {
        let a = BootState::factory(BootSlot::A, 1).encode_record().unwrap();
        let b = BootState::factory(BootSlot::B, 1).encode_record().unwrap();
        assert!(BootState::newest_valid_record([a.as_slice(), b.as_slice()]).is_err());
    }

    #[test]
    fn one_hundred_interrupted_updates_always_retain_a_bootable_slot() {
        let mut state = BootState::factory(BootSlot::A, 1);
        for sequence in 2..=101 {
            let target = state.confirmed_slot.other();
            state = state.stage(target, sequence).unwrap();

            // Persist-before-boot is modeled by a checksummed round trip. Each
            // cycle then loses power before the health confirmation ceremony.
            for _ in 0..MAX_BOOT_ATTEMPTS {
                let (prepared, decision) = state.prepare_boot().unwrap();
                assert!(decision.pending);
                state = BootState::decode_record(&prepared.encode_record().unwrap()).unwrap();
            }

            let (fallback, decision) = state.prepare_boot().unwrap();
            assert!(!decision.pending);
            assert_eq!(decision.slot, state.confirmed_slot);
            state = BootState::decode_record(&fallback.encode_record().unwrap()).unwrap();
        }
        assert_eq!(state.confirmed_slot, BootSlot::A);
        assert_eq!(state.confirmed_sequence, 1);
    }
}
