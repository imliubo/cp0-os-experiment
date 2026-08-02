use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::CheckpointFailure;

pub const CHECKPOINT_SCHEMA_VERSION: u32 = 1;
pub const MAX_CHECKPOINT_BYTES: usize = 8 * 1024;
pub const CHECKPOINT_TIMEOUT: Duration = Duration::from_millis(250);

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointBlob {
    pub format_version: u32,
    pub app_id: String,
    pub package_version: String,
    pub schema_version: u32,
    pub payload_length: u32,
    pub payload_sha256: String,
    pub payload: Vec<u8>,
}

#[derive(Debug)]
pub enum CheckpointError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Invalid(String),
    Incompatible(CheckpointFailure),
}

impl fmt::Display for CheckpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "checkpoint I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid checkpoint JSON: {error}"),
            Self::Invalid(error) => write!(formatter, "invalid checkpoint: {error}"),
            Self::Incompatible(reason) => {
                write!(formatter, "checkpoint is incompatible: {reason:?}")
            }
        }
    }
}

impl std::error::Error for CheckpointError {}

impl From<std::io::Error> for CheckpointError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for CheckpointError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl CheckpointBlob {
    pub fn new(
        app_id: impl Into<String>,
        package_version: impl Into<String>,
        schema_version: u32,
        payload: Vec<u8>,
    ) -> Result<Self, CheckpointError> {
        let app_id = app_id.into();
        let package_version = package_version.into();
        if !cp0_manifest::is_valid_app_id(&app_id)
            || !cp0_manifest::is_valid_app_version(&package_version)
        {
            return Err(CheckpointError::Invalid(
                "application identity is invalid".into(),
            ));
        }
        if schema_version == 0 {
            return Err(CheckpointError::Invalid(
                "schema version must be non-zero".into(),
            ));
        }
        if payload.len() > MAX_CHECKPOINT_BYTES {
            return Err(CheckpointError::Incompatible(CheckpointFailure::TooLarge));
        }
        let payload_length = u32::try_from(payload.len()).expect("8 KiB fits u32");
        let payload_sha256 = lower_hex(&Sha256::digest(&payload));
        Ok(Self {
            format_version: CHECKPOINT_SCHEMA_VERSION,
            app_id,
            package_version,
            schema_version,
            payload_length,
            payload_sha256,
            payload,
        })
    }

    pub fn validate(&self) -> Result<(), CheckpointError> {
        if self.format_version != CHECKPOINT_SCHEMA_VERSION
            || !cp0_manifest::is_valid_app_id(&self.app_id)
            || !cp0_manifest::is_valid_app_version(&self.package_version)
            || self.schema_version == 0
            || self.payload.len() > MAX_CHECKPOINT_BYTES
            || self.payload_length as usize != self.payload.len()
            || self.payload_sha256.len() != 64
            || !self
                .payload_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || self.payload_sha256 != lower_hex(&Sha256::digest(&self.payload))
        {
            return Err(CheckpointError::Invalid(
                "metadata, bounds or payload digest does not match".into(),
            ));
        }
        Ok(())
    }

    pub fn validate_for_restore(
        &self,
        app_id: &str,
        package_version: &str,
        accepted_schema_versions: &[u32],
    ) -> Result<(), CheckpointError> {
        self.validate()?;
        if self.app_id != app_id
            || self.package_version != package_version
            || !accepted_schema_versions.contains(&self.schema_version)
        {
            return Err(CheckpointError::Incompatible(
                CheckpointFailure::VersionMismatch,
            ));
        }
        Ok(())
    }

    pub fn load(path: impl AsRef<Path>, enforce_root_owner: bool) -> Result<Self, CheckpointError> {
        let path = path.as_ref();
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.mode() & 0o077 != 0
            || (enforce_root_owner && metadata.uid() != 0)
        {
            return Err(CheckpointError::Invalid(
                "checkpoint must be a private root-owned regular file".into(),
            ));
        }
        let checkpoint: Self = serde_json::from_reader(BufReader::new(File::open(path)?))?;
        checkpoint.validate()?;
        Ok(checkpoint)
    }

    pub fn save_atomic(&self, path: impl AsRef<Path>) -> Result<(), CheckpointError> {
        self.validate()?;
        let path = path.as_ref();
        let parent = path.parent().ok_or_else(|| {
            CheckpointError::Invalid("checkpoint path must have a parent directory".into())
        })?;
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| CheckpointError::Invalid("checkpoint file name must be UTF-8".into()))?;
        let temporary = temporary_path(parent, file_name);
        let result = (|| -> Result<(), CheckpointError> {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&temporary)?;
            let mut writer = BufWriter::new(file);
            serde_json::to_writer(&mut writer, self)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            writer.get_ref().sync_all()?;
            fs::rename(&temporary, path)?;
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

pub fn capture_with_timeout<F>(
    app_id: &str,
    package_version: &str,
    timeout: Duration,
    capture: F,
) -> Result<CheckpointBlob, CheckpointFailure>
where
    F: FnOnce() -> Result<(u32, Vec<u8>), CheckpointFailure> + Send + 'static,
{
    let app_id = app_id.to_owned();
    let package_version = package_version.to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("cp0-checkpoint-sim".into())
        .spawn(move || {
            let result = capture().and_then(|(schema_version, payload)| {
                CheckpointBlob::new(app_id, package_version, schema_version, payload).map_err(
                    |error| match error {
                        CheckpointError::Incompatible(reason) => reason,
                        _ => CheckpointFailure::Failed,
                    },
                )
            });
            let _ = sender.send(result);
        })
        .map_err(|_| CheckpointFailure::Failed)?;
    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(CheckpointFailure::Timeout),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(CheckpointFailure::Failed),
    }
}

fn lower_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[usize::from(byte >> 4)] as char);
        encoded.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

fn temporary_path(parent: &Path, file_name: &str) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".{file_name}.tmp-{}-{sequence}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CheckpointStatus, EvictionCheckpoint, RuntimeBinding, TaskJournal, TaskRegistry};

    #[test]
    fn checkpoint_round_trip_is_bounded_and_digest_bound() {
        let root = PathBuf::from("target/checkpoint-tests/round-trip");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("checkpoint.json");
        let _ = fs::remove_file(&path);
        let checkpoint = CheckpointBlob::new(
            "dev.cardputerzero.notes",
            "1.2.3",
            4,
            b"draft=hello".to_vec(),
        )
        .unwrap();
        checkpoint.save_atomic(&path).unwrap();
        let loaded = CheckpointBlob::load(&path, false).unwrap();
        assert_eq!(loaded, checkpoint);
        loaded
            .validate_for_restore("dev.cardputerzero.notes", "1.2.3", &[4])
            .unwrap();
    }

    #[test]
    fn tamper_and_version_mismatch_fail_closed() {
        let mut checkpoint = CheckpointBlob::new(
            "dev.cardputerzero.notes",
            "1.2.3",
            4,
            b"draft=hello".to_vec(),
        )
        .unwrap();
        checkpoint.payload[0] ^= 1;
        assert!(matches!(
            checkpoint.validate(),
            Err(CheckpointError::Invalid(_))
        ));

        let checkpoint =
            CheckpointBlob::new("dev.cardputerzero.notes", "1.2.3", 4, Vec::new()).unwrap();
        assert!(matches!(
            checkpoint.validate_for_restore("dev.cardputerzero.notes", "2.0.0", &[4]),
            Err(CheckpointError::Incompatible(
                CheckpointFailure::VersionMismatch
            ))
        ));
    }

    #[test]
    fn oversize_and_timeout_have_explicit_outcomes() {
        assert!(matches!(
            CheckpointBlob::new(
                "dev.cardputerzero.notes",
                "1.0.0",
                1,
                vec![0; MAX_CHECKPOINT_BYTES + 1]
            ),
            Err(CheckpointError::Incompatible(CheckpointFailure::TooLarge))
        ));
        let result = capture_with_timeout(
            "dev.cardputerzero.notes",
            "1.0.0",
            Duration::from_millis(5),
            || {
                std::thread::sleep(Duration::from_millis(30));
                Ok((1, vec![1, 2, 3]))
            },
        );
        assert!(matches!(result, Err(CheckpointFailure::Timeout)));
    }

    #[test]
    fn successful_capture_copies_state_before_returning() {
        let checkpoint = capture_with_timeout(
            "dev.cardputerzero.notes",
            "1.0.0",
            CHECKPOINT_TIMEOUT,
            || Ok((7, vec![1, 2, 3, 4])),
        )
        .unwrap();
        assert_eq!(checkpoint.schema_version, 7);
        assert_eq!(checkpoint.payload, vec![1, 2, 3, 4]);
    }

    #[test]
    fn eleventh_app_eviction_restores_saved_state_on_next_launch() {
        let mut tasks = TaskRegistry::default();
        for index in 1..=10_u64 {
            tasks
                .launch(
                    format!("dev.cardputerzero.app{index}"),
                    "1.0.0",
                    RuntimeBinding::new(index, format!("cardputerzero-app-{index}.service"))
                        .unwrap(),
                    None,
                )
                .unwrap();
        }
        let oldest = tasks.oldest_task().unwrap().clone();
        let saved =
            capture_with_timeout(&oldest.app_id, &oldest.version, CHECKPOINT_TIMEOUT, || {
                Ok((3, b"page=7;cursor=12".to_vec()))
            })
            .unwrap();
        let outcome = tasks
            .launch(
                "dev.cardputerzero.app11",
                "1.0.0",
                RuntimeBinding::new(11, "cardputerzero-app-11.service").unwrap(),
                Some(EvictionCheckpoint {
                    task_id: oldest.task_id,
                    status: CheckpointStatus::Available {
                        schema_version: saved.schema_version,
                        bytes: saved.payload_length,
                    },
                }),
            )
            .unwrap();
        let mut journal = TaskJournal::new(&TaskRegistry::default());
        journal
            .record_capacity_eviction(&tasks, outcome.evicted.as_ref().unwrap())
            .unwrap();
        assert!(tasks.task(oldest.task_id).is_none());
        assert_eq!(tasks.len(), 10);
        assert_eq!(journal.evictions[0].app_id, oldest.app_id);

        saved
            .validate_for_restore(&oldest.app_id, &oldest.version, &[3])
            .unwrap();
        let relaunched = tasks
            .launch(
                oldest.app_id,
                oldest.version,
                RuntimeBinding::new(12, "cardputerzero-app-1.service").unwrap(),
                Some(EvictionCheckpoint {
                    task_id: tasks.oldest_task().unwrap().task_id,
                    status: CheckpointStatus::Unavailable {
                        reason: CheckpointFailure::Unsupported,
                    },
                }),
            )
            .unwrap();
        assert_ne!(relaunched.task_id, oldest.task_id);
        assert_eq!(saved.payload, b"page=7;cursor=12");
    }
}
