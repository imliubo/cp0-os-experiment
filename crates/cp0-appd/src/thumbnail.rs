use std::collections::BTreeMap;
use std::fmt;

use crate::{MAX_TASKS, TaskId, TaskRecord};

pub const THUMBNAIL_WIDTH: usize = 160;
pub const THUMBNAIL_HEIGHT: usize = 85;
pub const THUMBNAIL_PIXELS: usize = THUMBNAIL_WIDTH * THUMBNAIL_HEIGHT;
pub const THUMBNAIL_BYTES: usize = THUMBNAIL_PIXELS * 2;
pub const THUMBNAIL_REFRESH_MILLISECONDS: u64 = 500;
pub const MAX_THUMBNAIL_CACHE_BYTES: usize = MAX_TASKS * THUMBNAIL_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbnailIdentity {
    pub task_id: TaskId,
    pub account_uid: u32,
    pub runtime_generation: u64,
    pub thumbnail_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailFrame {
    pub identity: ThumbnailIdentity,
    pub captured_monotonic_milliseconds: u64,
    pub pixels: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThumbnailError {
    InvalidPixels,
    InvalidIdentity,
    IdentityMismatch,
    StaleGeneration,
    RefreshLimited,
    CacheFull,
}

impl fmt::Display for ThumbnailError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidPixels => "thumbnail must be exactly 160x85 RGB565",
            Self::InvalidIdentity => "thumbnail identity fields must be non-zero",
            Self::IdentityMismatch => "thumbnail identity does not match the trusted task",
            Self::StaleGeneration => "thumbnail generation is stale",
            Self::RefreshLimited => "thumbnail refresh exceeds the 2 Hz limit",
            Self::CacheFull => "thumbnail cache is full",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ThumbnailError {}

#[derive(Debug, Default)]
pub struct ThumbnailCache {
    frames: BTreeMap<TaskId, ThumbnailFrame>,
}

impl ThumbnailCache {
    pub fn insert_trusted(
        &mut self,
        task: &TaskRecord,
        account_uid: u32,
        identity: ThumbnailIdentity,
        captured_monotonic_milliseconds: u64,
        pixels: Vec<u16>,
    ) -> Result<(), ThumbnailError> {
        if pixels.len() != THUMBNAIL_PIXELS {
            return Err(ThumbnailError::InvalidPixels);
        }
        if account_uid == 0
            || identity.account_uid == 0
            || identity.runtime_generation == 0
            || identity.thumbnail_generation == 0
        {
            return Err(ThumbnailError::InvalidIdentity);
        }
        let expected_runtime = task.runtime().map(|runtime| runtime.token);
        if identity.task_id != task.task_id
            || identity.account_uid != account_uid
            || expected_runtime != Some(identity.runtime_generation)
        {
            return Err(ThumbnailError::IdentityMismatch);
        }
        if let Some(previous) = self.frames.get(&task.task_id) {
            if identity.thumbnail_generation <= previous.identity.thumbnail_generation {
                return Err(ThumbnailError::StaleGeneration);
            }
            if captured_monotonic_milliseconds
                < previous
                    .captured_monotonic_milliseconds
                    .saturating_add(THUMBNAIL_REFRESH_MILLISECONDS)
            {
                return Err(ThumbnailError::RefreshLimited);
            }
        } else if self.frames.len() == MAX_TASKS {
            return Err(ThumbnailError::CacheFull);
        }
        self.frames.insert(
            task.task_id,
            ThumbnailFrame {
                identity,
                captured_monotonic_milliseconds,
                pixels,
            },
        );
        Ok(())
    }

    pub fn frame(
        &self,
        task_id: TaskId,
        runtime_generation: Option<u64>,
    ) -> Option<&ThumbnailFrame> {
        let frame = self.frames.get(&task_id)?;
        if runtime_generation.is_some()
            && runtime_generation != Some(frame.identity.runtime_generation)
        {
            return None;
        }
        Some(frame)
    }

    pub fn retain_last_frame(&self, task_id: TaskId) -> Option<&ThumbnailFrame> {
        self.frames.get(&task_id)
    }

    pub fn remove(&mut self, task_id: TaskId) -> Option<ThumbnailFrame> {
        self.frames.remove(&task_id)
    }

    pub fn memory_bytes(&self) -> usize {
        self.frames.len() * THUMBNAIL_BYTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RuntimeBinding, TaskRegistry};

    fn registry_with_task(token: u64) -> (TaskRegistry, TaskId) {
        let mut registry = TaskRegistry::new(MAX_TASKS).unwrap();
        let outcome = registry
            .launch(
                format!("dev.cardputerzero.app{token}"),
                "1.0.0",
                RuntimeBinding::new(token, format!("cardputerzero-app-{token}.service")).unwrap(),
                None,
            )
            .unwrap();
        (registry, outcome.task_id)
    }

    #[test]
    fn accepts_only_trusted_current_runtime_identity() {
        let (registry, task_id) = registry_with_task(7);
        let task = registry.task(task_id).unwrap();
        let mut cache = ThumbnailCache::default();
        let identity = ThumbnailIdentity {
            task_id,
            account_uid: 20_007,
            runtime_generation: 7,
            thumbnail_generation: 1,
        };
        cache
            .insert_trusted(task, 20_007, identity, 500, vec![0x1234; THUMBNAIL_PIXELS])
            .unwrap();
        assert!(cache.frame(task_id, Some(7)).is_some());
        assert!(cache.frame(task_id, Some(8)).is_none());

        let mut spoofed = identity;
        spoofed.account_uid = 20_008;
        spoofed.thumbnail_generation = 2;
        assert_eq!(
            cache.insert_trusted(task, 20_007, spoofed, 1_000, vec![0; THUMBNAIL_PIXELS]),
            Err(ThumbnailError::IdentityMismatch)
        );
    }

    #[test]
    fn rejects_stale_and_over_rate_frames() {
        let (registry, task_id) = registry_with_task(3);
        let task = registry.task(task_id).unwrap();
        let mut cache = ThumbnailCache::default();
        let mut identity = ThumbnailIdentity {
            task_id,
            account_uid: 20_003,
            runtime_generation: 3,
            thumbnail_generation: 1,
        };
        cache
            .insert_trusted(task, 20_003, identity, 100, vec![0; THUMBNAIL_PIXELS])
            .unwrap();
        assert_eq!(
            cache.insert_trusted(task, 20_003, identity, 600, vec![0; THUMBNAIL_PIXELS]),
            Err(ThumbnailError::StaleGeneration)
        );
        identity.thumbnail_generation = 2;
        assert_eq!(
            cache.insert_trusted(task, 20_003, identity, 599, vec![0; THUMBNAIL_PIXELS]),
            Err(ThumbnailError::RefreshLimited)
        );
        cache
            .insert_trusted(task, 20_003, identity, 600, vec![0; THUMBNAIL_PIXELS])
            .unwrap();
    }

    #[test]
    fn ten_frames_stay_inside_fixed_memory_budget() {
        let mut cache = ThumbnailCache::default();
        let mut registry = TaskRegistry::new(MAX_TASKS).unwrap();
        for token in 1..=MAX_TASKS as u64 {
            let task_id = registry
                .launch(
                    format!("dev.cardputerzero.app{token}"),
                    "1.0.0",
                    RuntimeBinding::new(token, format!("cardputerzero-app-{token}.service"))
                        .unwrap(),
                    None,
                )
                .unwrap()
                .task_id;
            cache
                .insert_trusted(
                    registry.task(task_id).unwrap(),
                    20_000 + token as u32,
                    ThumbnailIdentity {
                        task_id,
                        account_uid: 20_000 + token as u32,
                        runtime_generation: token,
                        thumbnail_generation: 1,
                    },
                    0,
                    vec![0; THUMBNAIL_PIXELS],
                )
                .unwrap();
        }
        assert_eq!(cache.memory_bytes(), MAX_THUMBNAIL_CACHE_BYTES);
    }
}
