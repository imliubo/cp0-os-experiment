use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

pub const MAX_TASKS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    Foreground,
    Background,
    Frozen,
    Checkpointed,
    Crashed,
}

impl TaskState {
    pub fn is_resident(self) -> bool {
        matches!(self, Self::Foreground | Self::Background | Self::Frozen)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckpointFailure {
    Unsupported,
    Timeout,
    Failed,
    TooLarge,
    VersionMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CheckpointStatus {
    NotRequested,
    Available { schema_version: u32, bytes: u32 },
    Unavailable { reason: CheckpointFailure },
}

impl CheckpointStatus {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeBinding {
    pub token: u64,
    pub unit: String,
}

impl RuntimeBinding {
    pub fn new(token: u64, unit: impl Into<String>) -> Result<Self, TaskError> {
        if token == 0 {
            return Err(TaskError::InvalidRuntimeToken);
        }
        let unit = unit.into();
        if unit.is_empty() {
            return Err(TaskError::InvalidRuntimeUnit);
        }
        Ok(Self { token, unit })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRecord {
    pub task_id: TaskId,
    pub app_id: String,
    pub version: String,
    pub state: TaskState,
    pub created_sequence: u64,
    pub last_activated_sequence: u64,
    pub checkpoint: CheckpointStatus,
    pub thumbnail_generation: Option<u64>,
    runtime: Option<RuntimeBinding>,
}

impl TaskRecord {
    pub fn runtime(&self) -> Option<&RuntimeBinding> {
        self.runtime.as_ref()
    }
}

pub const TASK_SNAPSHOT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRegistrySnapshot {
    pub schema_version: u32,
    pub capacity: usize,
    pub next_task_id: u64,
    pub next_sequence: u64,
    pub tasks: Vec<TaskRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictionCheckpoint {
    pub task_id: TaskId,
    pub status: CheckpointStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvictedTask {
    pub task: TaskRecord,
    pub checkpoint: CheckpointStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOutcome {
    pub task_id: TaskId,
    pub backgrounded: Option<TaskId>,
    pub evicted: Option<EvictedTask>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationOutcome {
    pub task_id: TaskId,
    pub backgrounded: Option<TaskId>,
    pub requires_thaw: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskError {
    InvalidCapacity,
    InvalidRuntimeToken,
    InvalidRuntimeUnit,
    AppAlreadyResident(String),
    TaskNotFound(TaskId),
    TaskNotResident(TaskId),
    ForegroundCannotFreeze(TaskId),
    ForegroundCannotCheckpoint(TaskId),
    MissingEvictionCheckpoint(TaskId),
    UnexpectedEvictionCheckpoint(TaskId),
    StaleEvictionCheckpoint { expected: TaskId, provided: TaskId },
    InvalidSnapshot(String),
}

impl fmt::Display for TaskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCapacity => formatter.write_str("task capacity must be non-zero"),
            Self::InvalidRuntimeToken => formatter.write_str("runtime token must be non-zero"),
            Self::InvalidRuntimeUnit => formatter.write_str("runtime unit must be non-empty"),
            Self::AppAlreadyResident(app_id) => {
                write!(
                    formatter,
                    "application {app_id} already has a resident task"
                )
            }
            Self::TaskNotFound(task_id) => write!(formatter, "task {} was not found", task_id.0),
            Self::TaskNotResident(task_id) => {
                write!(formatter, "task {} has no resident runtime", task_id.0)
            }
            Self::ForegroundCannotFreeze(task_id) => {
                write!(formatter, "foreground task {} cannot be frozen", task_id.0)
            }
            Self::ForegroundCannotCheckpoint(task_id) => {
                write!(
                    formatter,
                    "foreground task {} cannot be checkpointed",
                    task_id.0
                )
            }
            Self::MissingEvictionCheckpoint(task_id) => write!(
                formatter,
                "capacity eviction for task {} requires a checkpoint outcome",
                task_id.0
            ),
            Self::UnexpectedEvictionCheckpoint(task_id) => write!(
                formatter,
                "task {} was supplied for eviction while capacity is available",
                task_id.0
            ),
            Self::StaleEvictionCheckpoint { expected, provided } => write!(
                formatter,
                "capacity victim changed from task {} to task {}",
                provided.0, expected.0
            ),
            Self::InvalidSnapshot(reason) => write!(formatter, "invalid task snapshot: {reason}"),
        }
    }
}

impl std::error::Error for TaskError {}

#[derive(Debug, Clone)]
pub struct TaskRegistry {
    capacity: usize,
    next_task_id: u64,
    next_sequence: u64,
    tasks: Vec<TaskRecord>,
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new(MAX_TASKS).expect("the product task limit is valid")
    }
}

impl TaskRegistry {
    pub fn new(capacity: usize) -> Result<Self, TaskError> {
        if capacity == 0 {
            return Err(TaskError::InvalidCapacity);
        }
        Ok(Self {
            capacity,
            next_task_id: 1,
            next_sequence: 1,
            tasks: Vec::with_capacity(capacity),
        })
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn foreground(&self) -> Option<&TaskRecord> {
        self.tasks
            .iter()
            .find(|task| task.state == TaskState::Foreground)
    }

    pub fn task(&self, task_id: TaskId) -> Option<&TaskRecord> {
        self.tasks.iter().find(|task| task.task_id == task_id)
    }

    pub fn task_for_app(&self, app_id: &str) -> Option<&TaskRecord> {
        self.tasks.iter().find(|task| task.app_id == app_id)
    }

    pub fn oldest_task(&self) -> Option<&TaskRecord> {
        self.tasks.first()
    }

    pub fn creation_order(&self) -> impl Iterator<Item = &TaskRecord> {
        self.tasks.iter()
    }

    pub fn switcher_order(&self) -> Vec<&TaskRecord> {
        let mut tasks: Vec<_> = self.tasks.iter().collect();
        tasks.sort_by_key(|task| std::cmp::Reverse(task.last_activated_sequence));
        tasks
    }

    pub fn snapshot(&self) -> TaskRegistrySnapshot {
        TaskRegistrySnapshot {
            schema_version: TASK_SNAPSHOT_SCHEMA_VERSION,
            capacity: self.capacity,
            next_task_id: self.next_task_id,
            next_sequence: self.next_sequence,
            tasks: self.tasks.clone(),
        }
    }

    pub fn restore(snapshot: TaskRegistrySnapshot) -> Result<Self, TaskError> {
        if snapshot.schema_version != TASK_SNAPSHOT_SCHEMA_VERSION {
            return Err(TaskError::InvalidSnapshot(format!(
                "schema_version must be {TASK_SNAPSHOT_SCHEMA_VERSION}"
            )));
        }
        if snapshot.capacity == 0 || snapshot.capacity > MAX_TASKS {
            return Err(TaskError::InvalidSnapshot(format!(
                "capacity must be between 1 and {MAX_TASKS}"
            )));
        }
        if snapshot.tasks.len() > snapshot.capacity {
            return Err(TaskError::InvalidSnapshot(
                "task count exceeds capacity".into(),
            ));
        }

        let registry = Self {
            capacity: snapshot.capacity,
            next_task_id: snapshot.next_task_id,
            next_sequence: snapshot.next_sequence,
            tasks: snapshot.tasks,
        };
        registry.validate_snapshot()?;
        Ok(registry)
    }

    pub fn launch(
        &mut self,
        app_id: impl Into<String>,
        version: impl Into<String>,
        runtime: RuntimeBinding,
        eviction: Option<EvictionCheckpoint>,
    ) -> Result<LaunchOutcome, TaskError> {
        let app_id = app_id.into();
        let version = version.into();
        if let Some(existing) = self.task_for_app(&app_id) {
            if existing.state.is_resident() {
                return Err(TaskError::AppAlreadyResident(app_id));
            }
            if let Some(eviction) = eviction {
                return Err(TaskError::UnexpectedEvictionCheckpoint(eviction.task_id));
            }
            let task_id = existing.task_id;
            let backgrounded = self.background_foreground(Some(task_id));
            let activation_sequence = self.take_sequence();
            let task = self
                .task_mut(task_id)
                .expect("existing task remains present");
            task.version = version;
            task.state = TaskState::Foreground;
            task.last_activated_sequence = activation_sequence;
            task.runtime = Some(runtime);
            return Ok(LaunchOutcome {
                task_id,
                backgrounded,
                evicted: None,
            });
        }

        let evicted = if self.tasks.len() == self.capacity {
            let expected = self
                .tasks
                .first()
                .expect("a full non-zero registry has an oldest task")
                .task_id;
            let eviction = eviction.ok_or(TaskError::MissingEvictionCheckpoint(expected))?;
            Some(self.evict_capacity_victim(eviction)?)
        } else if let Some(eviction) = eviction {
            return Err(TaskError::UnexpectedEvictionCheckpoint(eviction.task_id));
        } else {
            None
        };

        let backgrounded = self.background_foreground(None);
        let task_id = TaskId(self.next_task_id);
        self.next_task_id = self.next_task_id.checked_add(1).unwrap_or(1);
        let created_sequence = self.take_sequence();
        self.tasks.push(TaskRecord {
            task_id,
            app_id,
            version,
            state: TaskState::Foreground,
            created_sequence,
            last_activated_sequence: created_sequence,
            checkpoint: CheckpointStatus::NotRequested,
            thumbnail_generation: None,
            runtime: Some(runtime),
        });
        debug_assert!(self.invariants_hold());
        Ok(LaunchOutcome {
            task_id,
            backgrounded,
            evicted,
        })
    }

    pub fn evict_capacity_victim(
        &mut self,
        eviction: EvictionCheckpoint,
    ) -> Result<EvictedTask, TaskError> {
        if self.tasks.len() != self.capacity {
            return Err(TaskError::UnexpectedEvictionCheckpoint(eviction.task_id));
        }
        let expected = self
            .tasks
            .first()
            .expect("a full non-zero registry has an oldest task")
            .task_id;
        if expected != eviction.task_id {
            return Err(TaskError::StaleEvictionCheckpoint {
                expected,
                provided: eviction.task_id,
            });
        }
        let task = self.tasks.remove(0);
        debug_assert!(self.invariants_hold());
        Ok(EvictedTask {
            task,
            checkpoint: eviction.status,
        })
    }

    pub fn activate(&mut self, task_id: TaskId) -> Result<ActivationOutcome, TaskError> {
        let state = self
            .task(task_id)
            .ok_or(TaskError::TaskNotFound(task_id))?
            .state;
        if !state.is_resident() {
            return Err(TaskError::TaskNotResident(task_id));
        }
        let requires_thaw = state == TaskState::Frozen;
        let backgrounded = self.background_foreground(Some(task_id));
        let sequence = self.take_sequence();
        let task = self
            .task_mut(task_id)
            .expect("validated task remains present");
        task.state = TaskState::Foreground;
        task.last_activated_sequence = sequence;
        debug_assert!(self.invariants_hold());
        Ok(ActivationOutcome {
            task_id,
            backgrounded,
            requires_thaw,
        })
    }

    pub fn freeze(&mut self, task_id: TaskId) -> Result<(), TaskError> {
        let task = self
            .task_mut(task_id)
            .ok_or(TaskError::TaskNotFound(task_id))?;
        match task.state {
            TaskState::Foreground => Err(TaskError::ForegroundCannotFreeze(task_id)),
            TaskState::Background | TaskState::Frozen => {
                task.state = TaskState::Frozen;
                Ok(())
            }
            TaskState::Checkpointed | TaskState::Crashed => {
                Err(TaskError::TaskNotResident(task_id))
            }
        }
    }

    pub fn checkpoint(
        &mut self,
        task_id: TaskId,
        status: CheckpointStatus,
    ) -> Result<(), TaskError> {
        let task = self
            .task_mut(task_id)
            .ok_or(TaskError::TaskNotFound(task_id))?;
        if task.state == TaskState::Foreground {
            return Err(TaskError::ForegroundCannotCheckpoint(task_id));
        }
        task.state = TaskState::Checkpointed;
        task.checkpoint = status;
        task.runtime = None;
        debug_assert!(self.invariants_hold());
        Ok(())
    }

    pub fn runtime_exited(&mut self, runtime_token: u64) -> Option<TaskId> {
        let task = self.tasks.iter_mut().find(|task| {
            task.runtime
                .as_ref()
                .is_some_and(|runtime| runtime.token == runtime_token)
        })?;
        task.runtime = None;
        task.state = TaskState::Crashed;
        let task_id = task.task_id;
        debug_assert!(self.invariants_hold());
        Some(task_id)
    }

    pub fn update_thumbnail(&mut self, task_id: TaskId, generation: u64) -> Result<(), TaskError> {
        let task = self
            .task_mut(task_id)
            .ok_or(TaskError::TaskNotFound(task_id))?;
        task.thumbnail_generation = Some(generation);
        Ok(())
    }

    pub fn close(&mut self, task_id: TaskId) -> Result<TaskRecord, TaskError> {
        let index = self
            .tasks
            .iter()
            .position(|task| task.task_id == task_id)
            .ok_or(TaskError::TaskNotFound(task_id))?;
        let task = self.tasks.remove(index);
        debug_assert!(self.invariants_hold());
        Ok(task)
    }

    fn task_mut(&mut self, task_id: TaskId) -> Option<&mut TaskRecord> {
        self.tasks.iter_mut().find(|task| task.task_id == task_id)
    }

    fn background_foreground(&mut self, except: Option<TaskId>) -> Option<TaskId> {
        let task = self
            .tasks
            .iter_mut()
            .find(|task| task.state == TaskState::Foreground && Some(task.task_id) != except)?;
        task.state = TaskState::Background;
        Some(task.task_id)
    }

    fn take_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.checked_add(1).unwrap_or(1);
        sequence
    }

    fn invariants_hold(&self) -> bool {
        self.tasks.len() <= self.capacity
            && self
                .tasks
                .windows(2)
                .all(|pair| pair[0].created_sequence < pair[1].created_sequence)
            && self
                .tasks
                .iter()
                .filter(|task| task.state == TaskState::Foreground)
                .count()
                <= 1
            && self.tasks.iter().all(|task| {
                task.state.is_resident() == task.runtime.is_some()
                    && task.task_id.0 != 0
                    && task.created_sequence != 0
                    && task.last_activated_sequence != 0
            })
    }

    fn validate_snapshot(&self) -> Result<(), TaskError> {
        if !self.invariants_hold() {
            return Err(TaskError::InvalidSnapshot(
                "task state invariants do not hold".into(),
            ));
        }
        if self.next_task_id == 0 || self.next_sequence == 0 {
            return Err(TaskError::InvalidSnapshot(
                "next identifiers must be non-zero".into(),
            ));
        }

        let mut task_ids = BTreeSet::new();
        let mut app_ids = BTreeSet::new();
        let mut runtime_tokens = BTreeSet::new();
        let mut max_task_id = 0;
        let mut max_sequence = 0;
        for task in &self.tasks {
            if !cp0_manifest::is_valid_app_id(&task.app_id)
                || !cp0_manifest::is_valid_app_version(&task.version)
            {
                return Err(TaskError::InvalidSnapshot(
                    "task has an invalid application identity".into(),
                ));
            }
            if !task_ids.insert(task.task_id) || !app_ids.insert(&task.app_id) {
                return Err(TaskError::InvalidSnapshot(
                    "task IDs and application IDs must be unique".into(),
                ));
            }
            if let Some(runtime) = task.runtime() {
                if runtime.unit.len() > 128
                    || !runtime.unit.starts_with("cardputerzero-app-")
                    || !runtime.unit.ends_with(".service")
                    || !runtime_tokens.insert(runtime.token)
                {
                    return Err(TaskError::InvalidSnapshot(
                        "runtime binding is invalid or duplicated".into(),
                    ));
                }
            }
            if task.thumbnail_generation == Some(0) {
                return Err(TaskError::InvalidSnapshot(
                    "thumbnail generation must be non-zero".into(),
                ));
            }
            if let CheckpointStatus::Available {
                schema_version,
                bytes,
            } = &task.checkpoint
            {
                if *schema_version == 0 || *bytes > 8 * 1024 {
                    return Err(TaskError::InvalidSnapshot(
                        "checkpoint metadata is outside ABI bounds".into(),
                    ));
                }
            }
            max_task_id = max_task_id.max(task.task_id.0);
            max_sequence = max_sequence
                .max(task.created_sequence)
                .max(task.last_activated_sequence);
        }
        if self.next_task_id <= max_task_id || self.next_sequence <= max_sequence {
            return Err(TaskError::InvalidSnapshot(
                "next identifiers would reuse durable task history".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(token: u64) -> RuntimeBinding {
        RuntimeBinding::new(token, format!("cardputerzero-app-{token}.service")).unwrap()
    }

    fn launch(registry: &mut TaskRegistry, index: u64) -> LaunchOutcome {
        registry
            .launch(
                format!("dev.cardputerzero.app{index}"),
                "1.0.0",
                runtime(index),
                None,
            )
            .unwrap()
    }

    #[test]
    fn product_registry_defaults_to_ten_tasks() {
        assert_eq!(TaskRegistry::default().capacity(), MAX_TASKS);
        assert!(TaskRegistry::new(0).is_err());
    }

    #[test]
    fn launching_backgrounds_the_previous_foreground() {
        let mut registry = TaskRegistry::new(3).unwrap();
        let first = launch(&mut registry, 1);
        let second = launch(&mut registry, 2);

        assert_eq!(second.backgrounded, Some(first.task_id));
        assert_eq!(
            registry.task(first.task_id).unwrap().state,
            TaskState::Background
        );
        assert_eq!(registry.foreground().unwrap().task_id, second.task_id);
    }

    #[test]
    fn activation_is_mru_ordered_and_keeps_creation_order_for_fifo() {
        let mut registry = TaskRegistry::new(3).unwrap();
        let first = launch(&mut registry, 1);
        let second = launch(&mut registry, 2);
        let third = launch(&mut registry, 3);

        let activated = registry.activate(first.task_id).unwrap();
        assert_eq!(activated.backgrounded, Some(third.task_id));
        assert!(!activated.requires_thaw);
        assert_eq!(registry.foreground().unwrap().task_id, first.task_id);
        assert_eq!(
            registry
                .switcher_order()
                .into_iter()
                .map(|task| task.task_id)
                .collect::<Vec<_>>(),
            vec![first.task_id, third.task_id, second.task_id]
        );
        assert_eq!(registry.oldest_task().unwrap().task_id, first.task_id);
    }

    #[test]
    fn eleventh_launch_requires_and_evicts_strict_fifo_victim() {
        let mut registry = TaskRegistry::default();
        let first = launch(&mut registry, 1);
        for index in 2..=10 {
            launch(&mut registry, index);
        }
        registry.activate(first.task_id).unwrap();

        let error = registry
            .launch("dev.cardputerzero.app11", "1.0.0", runtime(11), None)
            .unwrap_err();
        assert_eq!(error, TaskError::MissingEvictionCheckpoint(first.task_id));
        assert_eq!(registry.len(), 10);

        let outcome = registry
            .launch(
                "dev.cardputerzero.app11",
                "1.0.0",
                runtime(11),
                Some(EvictionCheckpoint {
                    task_id: first.task_id,
                    status: CheckpointStatus::Available {
                        schema_version: 3,
                        bytes: 512,
                    },
                }),
            )
            .unwrap();
        let evicted = outcome.evicted.unwrap();
        assert_eq!(evicted.task.task_id, first.task_id);
        assert!(evicted.checkpoint.is_available());
        assert_eq!(registry.len(), 10);
        assert!(registry.task(first.task_id).is_none());
        assert_eq!(
            registry.foreground().unwrap().app_id,
            "dev.cardputerzero.app11"
        );
    }

    #[test]
    fn failed_checkpoint_never_blocks_capacity_eviction() {
        let mut registry = TaskRegistry::new(1).unwrap();
        let first = launch(&mut registry, 1);
        let outcome = registry
            .launch(
                "dev.cardputerzero.app2",
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

        assert_eq!(registry.len(), 1);
        assert_eq!(
            outcome.evicted.unwrap().checkpoint,
            CheckpointStatus::Unavailable {
                reason: CheckpointFailure::Timeout
            }
        );
    }

    #[test]
    fn capacity_eviction_can_commit_before_replacement_start() {
        let mut registry = TaskRegistry::new(2).unwrap();
        let first = launch(&mut registry, 1);
        let second = launch(&mut registry, 2);

        let evicted = registry
            .evict_capacity_victim(EvictionCheckpoint {
                task_id: first.task_id,
                status: CheckpointStatus::Available {
                    schema_version: 4,
                    bytes: 128,
                },
            })
            .unwrap();
        assert_eq!(evicted.task.task_id, first.task_id);
        assert_eq!(registry.len(), 1);
        assert_eq!(registry.foreground().unwrap().task_id, second.task_id);

        let replacement = registry
            .launch("dev.cardputerzero.app3", "1.0.0", runtime(3), None)
            .unwrap();
        assert!(replacement.evicted.is_none());
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.foreground().unwrap().task_id, replacement.task_id);
    }

    #[test]
    fn foreground_fifo_victim_can_be_evicted_after_checkpoint_attempt() {
        let mut registry = TaskRegistry::new(2).unwrap();
        let first = launch(&mut registry, 1);
        launch(&mut registry, 2);
        registry.activate(first.task_id).unwrap();

        let outcome = registry
            .launch(
                "dev.cardputerzero.app3",
                "1.0.0",
                runtime(3),
                Some(EvictionCheckpoint {
                    task_id: first.task_id,
                    status: CheckpointStatus::Unavailable {
                        reason: CheckpointFailure::Unsupported,
                    },
                }),
            )
            .unwrap();

        assert_eq!(outcome.evicted.unwrap().task.task_id, first.task_id);
        assert_eq!(
            registry.foreground().unwrap().app_id,
            "dev.cardputerzero.app3"
        );
    }

    #[test]
    fn stale_eviction_result_cannot_remove_the_wrong_task() {
        let mut registry = TaskRegistry::new(2).unwrap();
        let first = launch(&mut registry, 1);
        launch(&mut registry, 2);

        let error = registry
            .launch(
                "dev.cardputerzero.app3",
                "1.0.0",
                runtime(3),
                Some(EvictionCheckpoint {
                    task_id: TaskId(first.task_id.0 + 1),
                    status: CheckpointStatus::Unavailable {
                        reason: CheckpointFailure::Unsupported,
                    },
                }),
            )
            .unwrap_err();
        assert!(matches!(error, TaskError::StaleEvictionCheckpoint { .. }));
        assert_eq!(registry.len(), 2);
        assert_eq!(registry.oldest_task().unwrap().task_id, first.task_id);
    }

    #[test]
    fn frozen_and_checkpointed_tasks_have_explicit_resume_requirements() {
        let mut registry = TaskRegistry::new(3).unwrap();
        let first = launch(&mut registry, 1);
        launch(&mut registry, 2);
        registry.freeze(first.task_id).unwrap();

        let activated = registry.activate(first.task_id).unwrap();
        assert!(activated.requires_thaw);
        launch(&mut registry, 3);
        registry
            .checkpoint(
                first.task_id,
                CheckpointStatus::Available {
                    schema_version: 1,
                    bytes: 64,
                },
            )
            .unwrap();
        assert_eq!(
            registry.activate(first.task_id),
            Err(TaskError::TaskNotResident(first.task_id))
        );

        let restored = registry
            .launch("dev.cardputerzero.app1", "1.0.1", runtime(4), None)
            .unwrap();
        assert_eq!(restored.task_id, first.task_id);
        assert_eq!(registry.foreground().unwrap().version, "1.0.1");
    }

    #[test]
    fn runtime_crash_only_changes_the_matching_task() {
        let mut registry = TaskRegistry::new(2).unwrap();
        let first = launch(&mut registry, 1);
        let second = launch(&mut registry, 2);

        assert_eq!(registry.runtime_exited(1), Some(first.task_id));
        assert_eq!(
            registry.task(first.task_id).unwrap().state,
            TaskState::Crashed
        );
        assert_eq!(registry.foreground().unwrap().task_id, second.task_id);
        assert_eq!(registry.runtime_exited(99), None);
    }

    #[test]
    fn snapshot_round_trip_preserves_fifo_and_mru_sequences() {
        let mut registry = TaskRegistry::new(3).unwrap();
        let first = launch(&mut registry, 1);
        launch(&mut registry, 2);
        registry.activate(first.task_id).unwrap();
        registry.update_thumbnail(first.task_id, 4).unwrap();

        let restored = TaskRegistry::restore(registry.snapshot()).unwrap();
        assert_eq!(restored.snapshot(), registry.snapshot());
        assert_eq!(restored.oldest_task().unwrap().task_id, first.task_id);
        assert_eq!(restored.switcher_order()[0].task_id, first.task_id);
    }

    #[test]
    fn snapshot_rejects_duplicate_identity_and_stale_next_id() {
        let mut registry = TaskRegistry::new(3).unwrap();
        launch(&mut registry, 1);
        launch(&mut registry, 2);
        let mut snapshot = registry.snapshot();
        snapshot.tasks[1].app_id = snapshot.tasks[0].app_id.clone();
        assert!(matches!(
            TaskRegistry::restore(snapshot),
            Err(TaskError::InvalidSnapshot(_))
        ));

        let mut snapshot = registry.snapshot();
        snapshot.next_task_id = snapshot.tasks[1].task_id.0;
        assert!(matches!(
            TaskRegistry::restore(snapshot),
            Err(TaskError::InvalidSnapshot(_))
        ));
    }

    #[test]
    fn randomized_lifecycle_sequences_keep_global_invariants() {
        let mut registry = TaskRegistry::default();
        let mut random = 0x4d59_5df4_d0f3_3173_u64;
        let mut next_app = 1_u64;
        let mut next_runtime = 1_u64;

        for _ in 0..5_000 {
            random = random
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let tasks: Vec<_> = registry.creation_order().cloned().collect();
            match random % 6 {
                0 | 1 => {
                    let app_id = format!("dev.cardputerzero.random{next_app}");
                    let eviction =
                        (registry.len() == registry.capacity()).then(|| EvictionCheckpoint {
                            task_id: registry.oldest_task().unwrap().task_id,
                            status: CheckpointStatus::Unavailable {
                                reason: CheckpointFailure::Unsupported,
                            },
                        });
                    registry
                        .launch(&app_id, "1.0.0", runtime(next_runtime), eviction)
                        .unwrap();
                    next_app += 1;
                    next_runtime += 1;
                }
                2 if !tasks.is_empty() => {
                    let task = &tasks[(random as usize) % tasks.len()];
                    if task.state.is_resident() {
                        let _ = registry.activate(task.task_id);
                    }
                }
                3 => {
                    if let Some(task) = tasks
                        .iter()
                        .find(|task| task.state == TaskState::Background)
                    {
                        registry.freeze(task.task_id).unwrap();
                    }
                }
                4 => {
                    if let Some(task) = tasks
                        .iter()
                        .find(|task| task.state != TaskState::Foreground)
                    {
                        if task.state.is_resident() {
                            registry
                                .checkpoint(
                                    task.task_id,
                                    CheckpointStatus::Unavailable {
                                        reason: CheckpointFailure::Timeout,
                                    },
                                )
                                .unwrap();
                        }
                    }
                }
                5 if !tasks.is_empty() => {
                    let task = &tasks[(random as usize) % tasks.len()];
                    registry.close(task.task_id).unwrap();
                }
                _ => {}
            }
            assert!(registry.invariants_hold());
            assert!(registry.len() <= MAX_TASKS);
            assert!(TaskRegistry::restore(registry.snapshot()).is_ok());
        }
    }
}
