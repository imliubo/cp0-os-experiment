use crate::{TaskId, TaskRegistry, TaskState};

pub const FOREGROUND_CPU_WEIGHT: u16 = 100;
pub const BACKGROUND_CPU_WEIGHT: u16 = 20;
pub const FROZEN_CPU_WEIGHT: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    Healthy,
    Constrained,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceAction {
    SetCpuWeight { task_id: TaskId, weight: u16 },
    Freeze { task_id: TaskId },
    CheckpointAndStop { task_id: TaskId },
    RevokeForegroundLeases { task_id: TaskId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundCapability {
    Network,
    AudioPlayback,
    Camera,
    AudioCapture,
    GpioOutput,
    RadioTransmit,
}

impl BackgroundCapability {
    pub const fn may_continue(self) -> bool {
        matches!(self, Self::Network | Self::AudioPlayback)
    }
}

pub fn plan_resources(tasks: &TaskRegistry, pressure: MemoryPressure) -> Vec<ResourceAction> {
    let mut actions = Vec::new();
    for task in tasks
        .creation_order()
        .filter(|task| task.state.is_resident())
    {
        let weight = match task.state {
            TaskState::Foreground => FOREGROUND_CPU_WEIGHT,
            TaskState::Background => BACKGROUND_CPU_WEIGHT,
            TaskState::Frozen => FROZEN_CPU_WEIGHT,
            TaskState::Checkpointed | TaskState::Crashed => continue,
        };
        actions.push(ResourceAction::SetCpuWeight {
            task_id: task.task_id,
            weight,
        });
    }

    match pressure {
        MemoryPressure::Healthy => {}
        MemoryPressure::Constrained => {
            if let Some(task_id) = least_recent(tasks, TaskState::Background) {
                actions.push(ResourceAction::Freeze { task_id });
            }
        }
        MemoryPressure::Critical => {
            if let Some(task_id) = least_recent(tasks, TaskState::Frozen) {
                actions.push(ResourceAction::CheckpointAndStop { task_id });
            } else if let Some(task_id) = least_recent(tasks, TaskState::Background) {
                actions.push(ResourceAction::Freeze { task_id });
            }
        }
    }
    actions
}

pub fn plan_foreground_change(previous: Option<TaskId>, next: TaskId) -> Vec<ResourceAction> {
    previous
        .filter(|previous| *previous != next)
        .map(|task_id| vec![ResourceAction::RevokeForegroundLeases { task_id }])
        .unwrap_or_default()
}

fn least_recent(tasks: &TaskRegistry, state: TaskState) -> Option<TaskId> {
    tasks
        .creation_order()
        .filter(|task| task.state == state)
        .min_by_key(|task| task.last_activated_sequence)
        .map(|task| task.task_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeBinding;

    fn runtime(token: u64) -> RuntimeBinding {
        RuntimeBinding::new(token, format!("cardputerzero-app-{token}.service")).unwrap()
    }

    #[test]
    fn pressure_never_targets_the_foreground_task() {
        let mut tasks = TaskRegistry::new(3).unwrap();
        let first = tasks
            .launch("dev.cardputerzero.first", "1.0.0", runtime(1), None)
            .unwrap()
            .task_id;
        let foreground = tasks
            .launch("dev.cardputerzero.second", "1.0.0", runtime(2), None)
            .unwrap()
            .task_id;
        let plan = plan_resources(&tasks, MemoryPressure::Constrained);
        assert!(plan.contains(&ResourceAction::Freeze { task_id: first }));
        assert!(!plan.contains(&ResourceAction::Freeze {
            task_id: foreground
        }));
    }

    #[test]
    fn critical_pressure_checkpoints_a_frozen_lra_task_one_step_at_a_time() {
        let mut tasks = TaskRegistry::new(3).unwrap();
        let first = tasks
            .launch("dev.cardputerzero.first", "1.0.0", runtime(1), None)
            .unwrap()
            .task_id;
        tasks
            .launch("dev.cardputerzero.second", "1.0.0", runtime(2), None)
            .unwrap();
        tasks
            .launch("dev.cardputerzero.third", "1.0.0", runtime(3), None)
            .unwrap();
        tasks.freeze(first).unwrap();

        let plan = plan_resources(&tasks, MemoryPressure::Critical);
        assert_eq!(
            plan.last(),
            Some(&ResourceAction::CheckpointAndStop { task_id: first })
        );
    }

    #[test]
    fn only_explicit_background_capabilities_may_continue() {
        assert!(BackgroundCapability::Network.may_continue());
        assert!(BackgroundCapability::AudioPlayback.may_continue());
        assert!(!BackgroundCapability::Camera.may_continue());
        assert!(!BackgroundCapability::AudioCapture.may_continue());
        assert!(!BackgroundCapability::GpioOutput.may_continue());
        assert!(!BackgroundCapability::RadioTransmit.may_continue());
    }

    #[test]
    fn switching_revokes_the_previous_foreground_lease() {
        assert_eq!(
            plan_foreground_change(Some(TaskId(7)), TaskId(8)),
            vec![ResourceAction::RevokeForegroundLeases { task_id: TaskId(7) }]
        );
        assert!(plan_foreground_change(Some(TaskId(8)), TaskId(8)).is_empty());
    }
}
