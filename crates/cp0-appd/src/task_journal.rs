use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::{
    CheckpointStatus, EvictedTask, TaskError, TaskId, TaskRegistry, TaskRegistrySnapshot, TaskState,
};

pub const TASK_JOURNAL_SCHEMA_VERSION: u32 = 1;
pub const MAX_EVICTION_HISTORY: usize = 64;
pub const DEFAULT_TASK_JOURNAL_PATH: &str = "/var/lib/cardputerzero/tasks/state.json";

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvictionReason {
    Capacity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvictionRecord {
    pub event_sequence: u64,
    pub reason: EvictionReason,
    pub task_id: TaskId,
    pub app_id: String,
    pub version: String,
    pub final_state: TaskState,
    pub created_sequence: u64,
    pub last_activated_sequence: u64,
    pub checkpoint: CheckpointStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskJournal {
    pub schema_version: u32,
    pub next_event_sequence: u64,
    pub registry: TaskRegistrySnapshot,
    pub evictions: Vec<EvictionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredRuntime {
    pub task_id: TaskId,
    pub runtime_generation: u64,
    pub app_id: String,
    pub version: String,
    pub unit: String,
    pub foreground: bool,
}

#[derive(Debug)]
pub enum TaskJournalError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Task(TaskError),
    Invalid(String),
}

impl fmt::Display for TaskJournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "task journal I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid task journal JSON: {error}"),
            Self::Task(error) => write!(formatter, "invalid task journal registry: {error}"),
            Self::Invalid(error) => write!(formatter, "invalid task journal: {error}"),
        }
    }
}

impl std::error::Error for TaskJournalError {}

impl From<std::io::Error> for TaskJournalError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for TaskJournalError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<TaskError> for TaskJournalError {
    fn from(error: TaskError) -> Self {
        Self::Task(error)
    }
}

impl TaskJournal {
    pub fn new(registry: &TaskRegistry) -> Self {
        Self {
            schema_version: TASK_JOURNAL_SCHEMA_VERSION,
            next_event_sequence: 1,
            registry: registry.snapshot(),
            evictions: Vec::new(),
        }
    }

    pub fn load(
        path: impl AsRef<Path>,
        enforce_root_owner: bool,
    ) -> Result<Self, TaskJournalError> {
        let path = path.as_ref();
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.mode() & 0o022 != 0
            || (enforce_root_owner && metadata.uid() != 0)
        {
            return Err(TaskJournalError::Invalid(
                "state file must be a root-owned regular file without group/world write access"
                    .into(),
            ));
        }
        let journal: Self = serde_json::from_reader(BufReader::new(File::open(path)?))?;
        journal.validate()?;
        Ok(journal)
    }

    pub fn restored_registry(&self) -> Result<TaskRegistry, TaskJournalError> {
        Ok(TaskRegistry::restore(self.registry.clone())?)
    }

    pub fn record_registry(&mut self, registry: &TaskRegistry) -> Result<(), TaskJournalError> {
        let snapshot = registry.snapshot();
        TaskRegistry::restore(snapshot.clone())?;
        self.registry = snapshot;
        Ok(())
    }

    pub fn reconcile_resident_units(
        &mut self,
        mut unit_is_active: impl FnMut(&str) -> bool,
    ) -> Result<Vec<RecoveredRuntime>, TaskJournalError> {
        let mut registry = self.restored_registry()?;
        let resident: Vec<_> = registry
            .creation_order()
            .filter(|task| task.state.is_resident())
            .cloned()
            .collect();
        let mut recovered = Vec::with_capacity(resident.len());
        for task in resident {
            let runtime = task
                .runtime()
                .expect("validated resident task has a runtime binding")
                .clone();
            if unit_is_active(&runtime.unit) {
                recovered.push(RecoveredRuntime {
                    task_id: task.task_id,
                    runtime_generation: runtime.token,
                    app_id: task.app_id,
                    version: task.version,
                    unit: runtime.unit,
                    foreground: task.state == TaskState::Foreground,
                });
            } else {
                registry.runtime_exited(runtime.token);
            }
        }
        self.record_registry(&registry)?;
        Ok(recovered)
    }

    pub fn record_capacity_eviction(
        &mut self,
        registry: &TaskRegistry,
        evicted: &EvictedTask,
    ) -> Result<(), TaskJournalError> {
        let event_sequence = self.next_event_sequence;
        self.next_event_sequence = self
            .next_event_sequence
            .checked_add(1)
            .ok_or_else(|| TaskJournalError::Invalid("event sequence exhausted".into()))?;
        self.evictions.push(EvictionRecord {
            event_sequence,
            reason: EvictionReason::Capacity,
            task_id: evicted.task.task_id,
            app_id: evicted.task.app_id.clone(),
            version: evicted.task.version.clone(),
            final_state: evicted.task.state,
            created_sequence: evicted.task.created_sequence,
            last_activated_sequence: evicted.task.last_activated_sequence,
            checkpoint: evicted.checkpoint.clone(),
        });
        if self.evictions.len() > MAX_EVICTION_HISTORY {
            self.evictions.remove(0);
        }
        self.record_registry(registry)?;
        self.validate()
    }

    pub fn save_atomic(&self, path: impl AsRef<Path>) -> Result<(), TaskJournalError> {
        self.validate()?;
        let path = path.as_ref();
        let parent = path.parent().ok_or_else(|| {
            TaskJournalError::Invalid("journal path must have a parent directory".into())
        })?;
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| TaskJournalError::Invalid("journal file name must be UTF-8".into()))?;
        let temporary_path = temporary_path(parent, file_name);

        let result = (|| -> Result<(), TaskJournalError> {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&temporary_path)?;
            let mut writer = BufWriter::new(file);
            serde_json::to_writer_pretty(&mut writer, self)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            writer.get_ref().sync_all()?;
            fs::rename(&temporary_path, path)?;
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result
    }

    fn validate(&self) -> Result<(), TaskJournalError> {
        if self.schema_version != TASK_JOURNAL_SCHEMA_VERSION {
            return Err(TaskJournalError::Invalid(format!(
                "schema_version must be {TASK_JOURNAL_SCHEMA_VERSION}"
            )));
        }
        TaskRegistry::restore(self.registry.clone())?;
        if self.next_event_sequence == 0 || self.evictions.len() > MAX_EVICTION_HISTORY {
            return Err(TaskJournalError::Invalid(
                "eviction history bounds are invalid".into(),
            ));
        }
        let mut previous = 0;
        for eviction in &self.evictions {
            if eviction.event_sequence <= previous
                || eviction.event_sequence >= self.next_event_sequence
                || eviction.task_id.0 == 0
                || eviction.created_sequence == 0
                || eviction.last_activated_sequence == 0
                || !cp0_manifest::is_valid_app_id(&eviction.app_id)
                || !cp0_manifest::is_valid_app_version(&eviction.version)
            {
                return Err(TaskJournalError::Invalid(
                    "eviction record identity or ordering is invalid".into(),
                ));
            }
            previous = eviction.event_sequence;
        }
        Ok(())
    }
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
    use crate::{CheckpointFailure, EvictionCheckpoint, RuntimeBinding};

    fn runtime(token: u64) -> RuntimeBinding {
        RuntimeBinding::new(token, format!("cardputerzero-app-{token}.service")).unwrap()
    }

    #[test]
    fn atomic_journal_round_trip_keeps_evicted_final_state() {
        let root = PathBuf::from("target/task-journal-tests/round-trip");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("state.json");
        let _ = fs::remove_file(&path);

        let mut registry = TaskRegistry::new(1).unwrap();
        let first = registry
            .launch("dev.cardputerzero.first", "1.0.0", runtime(1), None)
            .unwrap();
        let outcome = registry
            .launch(
                "dev.cardputerzero.second",
                "1.0.0",
                runtime(2),
                Some(EvictionCheckpoint {
                    task_id: first.task_id,
                    status: CheckpointStatus::Unavailable {
                        reason: CheckpointFailure::Timeout,
                    },
                }),
            )
            .unwrap();
        let mut journal = TaskJournal::new(&TaskRegistry::new(1).unwrap());
        journal
            .record_capacity_eviction(&registry, &outcome.evicted.unwrap())
            .unwrap();
        journal.save_atomic(&path).unwrap();

        let loaded = TaskJournal::load(&path, false).unwrap();
        assert_eq!(loaded, journal);
        assert_eq!(loaded.evictions[0].task_id, first.task_id);
        assert_eq!(
            loaded.evictions[0].checkpoint,
            CheckpointStatus::Unavailable {
                reason: CheckpointFailure::Timeout
            }
        );
        assert_eq!(
            loaded
                .restored_registry()
                .unwrap()
                .foreground()
                .unwrap()
                .app_id,
            "dev.cardputerzero.second"
        );
    }

    #[test]
    fn journal_rejects_tampered_sequence_and_registry() {
        let mut registry = TaskRegistry::new(1).unwrap();
        registry
            .launch("dev.cardputerzero.first", "1.0.0", runtime(1), None)
            .unwrap();
        let mut journal = TaskJournal::new(&registry);
        journal.registry.next_task_id = 1;
        assert!(matches!(journal.validate(), Err(TaskJournalError::Task(_))));
    }

    #[test]
    fn eviction_history_is_bounded_without_reordering_events() {
        let mut registry = TaskRegistry::new(1).unwrap();
        registry
            .launch("dev.cardputerzero.seed", "1.0.0", runtime(1), None)
            .unwrap();
        let mut journal = TaskJournal::new(&registry);
        for index in 2..=(MAX_EVICTION_HISTORY as u64 + 3) {
            let victim = registry.oldest_task().unwrap().task_id;
            let outcome = registry
                .launch(
                    format!("dev.cardputerzero.app{index}"),
                    "1.0.0",
                    runtime(index),
                    Some(EvictionCheckpoint {
                        task_id: victim,
                        status: CheckpointStatus::Unavailable {
                            reason: CheckpointFailure::Unsupported,
                        },
                    }),
                )
                .unwrap();
            journal
                .record_capacity_eviction(&registry, &outcome.evicted.unwrap())
                .unwrap();
        }
        assert_eq!(journal.evictions.len(), MAX_EVICTION_HISTORY);
        assert!(journal.evictions[0].event_sequence > 1);
        assert!(
            journal
                .evictions
                .windows(2)
                .all(|events| events[0].event_sequence < events[1].event_sequence)
        );
    }

    #[test]
    fn daemon_restart_recovers_active_units_and_crashes_missing_generation() {
        let mut registry = TaskRegistry::new(3).unwrap();
        let missing = registry
            .launch("dev.cardputerzero.first", "1.0.0", runtime(41), None)
            .unwrap()
            .task_id;
        let active = registry
            .launch("dev.cardputerzero.second", "1.0.0", runtime(42), None)
            .unwrap()
            .task_id;
        let mut journal = TaskJournal::new(&registry);

        let recovered = journal
            .reconcile_resident_units(|unit| unit == "cardputerzero-app-42.service")
            .unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].task_id, active);
        assert_eq!(recovered[0].runtime_generation, 42);
        assert!(recovered[0].foreground);
        let restored = journal.restored_registry().unwrap();
        assert_eq!(restored.task(missing).unwrap().state, TaskState::Crashed);
        assert_eq!(restored.foreground().unwrap().task_id, active);
    }
}
