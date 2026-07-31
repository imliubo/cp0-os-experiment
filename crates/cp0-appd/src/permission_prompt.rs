use std::fmt;

use cp0_manifest::{AppManifest, Permission};
use serde::{Deserialize, Serialize};

use crate::{Authorization, PermissionChoice, PermissionEngine, PermissionError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionPrompt {
    pub prompt_id: u64,
    pub app_id: String,
    pub app_name: String,
    pub permission: Permission,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionRequestResult {
    Allow,
    Deny,
    Undeclared,
    Prompt(PermissionPrompt),
}

#[derive(Debug)]
pub enum PermissionPromptError {
    Busy(PermissionPrompt),
    NoPendingPrompt,
    StalePrompt,
    Permission(PermissionError),
}

impl fmt::Display for PermissionPromptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy(prompt) => write!(
                formatter,
                "permission prompt {} for {} is already pending",
                prompt.prompt_id, prompt.app_id
            ),
            Self::NoPendingPrompt => formatter.write_str("no permission prompt is pending"),
            Self::StalePrompt => formatter.write_str("permission prompt is stale"),
            Self::Permission(error) => write!(formatter, "cannot resolve permission: {error}"),
        }
    }
}

impl std::error::Error for PermissionPromptError {}

impl From<PermissionError> for PermissionPromptError {
    fn from(error: PermissionError) -> Self {
        Self::Permission(error)
    }
}

#[derive(Debug)]
pub struct PermissionCoordinator {
    engine: PermissionEngine,
    pending: Option<PermissionPrompt>,
    next_prompt_id: u64,
}

impl PermissionCoordinator {
    pub fn new(engine: PermissionEngine) -> Self {
        Self {
            engine,
            pending: None,
            next_prompt_id: 1,
        }
    }

    pub fn request(
        &mut self,
        manifest: &AppManifest,
        permission: Permission,
    ) -> Result<PermissionRequestResult, PermissionPromptError> {
        match self.engine.authorize(manifest, permission) {
            Authorization::Allow => Ok(PermissionRequestResult::Allow),
            Authorization::Deny => Ok(PermissionRequestResult::Deny),
            Authorization::Undeclared => Ok(PermissionRequestResult::Undeclared),
            Authorization::Prompt => {
                if let Some(prompt) = &self.pending {
                    if prompt.app_id == manifest.id && prompt.permission == permission {
                        return Ok(PermissionRequestResult::Prompt(prompt.clone()));
                    }
                    return Err(PermissionPromptError::Busy(prompt.clone()));
                }
                let reason = manifest
                    .permissions
                    .iter()
                    .find(|request| request.name == permission)
                    .expect("authorization prompt requires a declared permission")
                    .reason
                    .clone();
                let prompt = PermissionPrompt {
                    prompt_id: self.allocate_prompt_id(),
                    app_id: manifest.id.clone(),
                    app_name: manifest.name.clone(),
                    permission,
                    reason,
                };
                self.pending = Some(prompt.clone());
                Ok(PermissionRequestResult::Prompt(prompt))
            }
        }
    }

    pub fn pending(&self) -> Option<&PermissionPrompt> {
        self.pending.as_ref()
    }

    pub fn resolve(
        &mut self,
        prompt_id: u64,
        manifest: &AppManifest,
        choice: PermissionChoice,
    ) -> Result<PermissionPrompt, PermissionPromptError> {
        let prompt = self
            .pending
            .as_ref()
            .ok_or(PermissionPromptError::NoPendingPrompt)?;
        if prompt.prompt_id != prompt_id || prompt.app_id != manifest.id {
            return Err(PermissionPromptError::StalePrompt);
        }
        let prompt = prompt.clone();
        self.engine
            .resolve(manifest, prompt.permission, choice)
            .map_err(PermissionPromptError::Permission)?;
        self.pending = None;
        Ok(prompt)
    }

    pub fn clear_app_session(&mut self, app_id: &str) {
        self.engine.clear_session(app_id);
        if self
            .pending
            .as_ref()
            .is_some_and(|prompt| prompt.app_id == app_id)
        {
            self.pending = None;
        }
    }

    pub fn reset(
        &mut self,
        manifest: &AppManifest,
        permission: Permission,
    ) -> Result<(), PermissionPromptError> {
        if self
            .pending
            .as_ref()
            .is_some_and(|prompt| prompt.app_id == manifest.id && prompt.permission == permission)
        {
            return Err(PermissionPromptError::Busy(
                self.pending.clone().expect("pending prompt was checked"),
            ));
        }
        self.engine.reset(manifest, permission)?;
        Ok(())
    }

    pub fn reset_app(&mut self, app_id: &str) -> Result<(), PermissionPromptError> {
        if self
            .pending
            .as_ref()
            .is_some_and(|prompt| prompt.app_id == app_id)
        {
            self.pending = None;
        }
        self.engine.reset_app(app_id)?;
        Ok(())
    }

    fn allocate_prompt_id(&mut self) -> u64 {
        let prompt_id = self.next_prompt_id;
        self.next_prompt_id = self.next_prompt_id.wrapping_add(1).max(1);
        prompt_id
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::PermissionStore;

    fn coordinator(name: &str) -> PermissionCoordinator {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(format!("prompt-{name}-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        PermissionCoordinator::new(PermissionEngine::new(path, PermissionStore::default()).unwrap())
    }

    #[test]
    fn creates_one_idempotent_trusted_prompt() {
        let manifest = crate::tests::manifest();
        let mut coordinator = coordinator("idempotent");
        let first = coordinator
            .request(&manifest, Permission::NotificationsPost)
            .unwrap();
        let second = coordinator
            .request(&manifest, Permission::NotificationsPost)
            .unwrap();
        assert_eq!(first, second);
        let PermissionRequestResult::Prompt(prompt) = first else {
            panic!("permission should require a prompt")
        };
        assert_eq!(prompt.app_id, manifest.id);
        assert_eq!(prompt.app_name, manifest.name);
        assert_eq!(prompt.reason, manifest.permissions[0].reason);
        assert_eq!(coordinator.pending(), Some(&prompt));
    }

    #[test]
    fn serializes_prompts_and_rejects_stale_responses() {
        let manifest = crate::tests::manifest();
        let mut coordinator = coordinator("stale");
        let PermissionRequestResult::Prompt(prompt) = coordinator
            .request(&manifest, Permission::NotificationsPost)
            .unwrap()
        else {
            panic!("permission should require a prompt")
        };
        assert!(matches!(
            coordinator.resolve(prompt.prompt_id + 1, &manifest, PermissionChoice::Deny),
            Err(PermissionPromptError::StalePrompt)
        ));
        assert_eq!(coordinator.pending(), Some(&prompt));
    }

    #[test]
    fn applies_choice_and_clears_prompt() {
        let manifest = crate::tests::manifest();
        let mut coordinator = coordinator("resolve");
        let PermissionRequestResult::Prompt(prompt) = coordinator
            .request(&manifest, Permission::NotificationsPost)
            .unwrap()
        else {
            panic!("permission should require a prompt")
        };
        coordinator
            .resolve(prompt.prompt_id, &manifest, PermissionChoice::AllowOnce)
            .unwrap();
        assert!(coordinator.pending().is_none());
        assert_eq!(
            coordinator
                .request(&manifest, Permission::NotificationsPost)
                .unwrap(),
            PermissionRequestResult::Allow
        );
        coordinator.clear_app_session(&manifest.id);
        assert!(matches!(
            coordinator
                .request(&manifest, Permission::NotificationsPost)
                .unwrap(),
            PermissionRequestResult::Prompt(_)
        ));
    }

    #[test]
    fn never_prompts_for_undeclared_permissions() {
        let manifest = crate::tests::manifest();
        let mut coordinator = coordinator("undeclared");
        assert_eq!(
            coordinator
                .request(&manifest, Permission::CameraCapture)
                .unwrap(),
            PermissionRequestResult::Undeclared
        );
        assert!(coordinator.pending().is_none());
    }
}
