use std::fmt;

use cp0_document_protocol::{DocumentSummary, is_valid_document_id};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentPrompt {
    pub prompt_id: u64,
    pub app_id: String,
    pub app_name: String,
    pub documents: Vec<DocumentSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentRequestResult {
    NeedsDocuments,
    Pending(DocumentPrompt),
    Selected(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolvedSelection {
    Selected(String),
    Cancelled,
}

#[derive(Debug)]
pub enum DocumentPromptError {
    Busy(DocumentPrompt),
    NoPendingPrompt,
    StalePrompt,
    InvalidSelection,
    EmptyDocumentList,
}

impl fmt::Display for DocumentPromptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy(prompt) => write!(
                formatter,
                "document prompt {} for {} is already pending",
                prompt.prompt_id, prompt.app_id
            ),
            Self::NoPendingPrompt => formatter.write_str("no document prompt is pending"),
            Self::StalePrompt => formatter.write_str("document prompt is stale"),
            Self::InvalidSelection => formatter.write_str("document selection is invalid"),
            Self::EmptyDocumentList => formatter.write_str("no documents are available"),
        }
    }
}

impl std::error::Error for DocumentPromptError {}

#[derive(Debug, Default)]
pub struct DocumentCoordinator {
    pending: Option<DocumentPrompt>,
    resolved: Option<(String, ResolvedSelection)>,
    next_prompt_id: u64,
}

impl DocumentCoordinator {
    pub fn poll(&mut self, app_id: &str) -> Result<DocumentRequestResult, DocumentPromptError> {
        if let Some((resolved_app, _)) = &self.resolved {
            if resolved_app == app_id {
                let (_, selection) = self.resolved.take().expect("resolved selection exists");
                return Ok(match selection {
                    ResolvedSelection::Selected(document_id) => {
                        DocumentRequestResult::Selected(document_id)
                    }
                    ResolvedSelection::Cancelled => DocumentRequestResult::Cancelled,
                });
            }
        }
        if let Some(prompt) = &self.pending {
            return if prompt.app_id == app_id {
                Ok(DocumentRequestResult::Pending(prompt.clone()))
            } else {
                Err(DocumentPromptError::Busy(prompt.clone()))
            };
        }
        Ok(DocumentRequestResult::NeedsDocuments)
    }

    pub fn request(
        &mut self,
        app_id: &str,
        app_name: &str,
        documents: Vec<DocumentSummary>,
    ) -> Result<DocumentRequestResult, DocumentPromptError> {
        match self.poll(app_id)? {
            DocumentRequestResult::NeedsDocuments => {}
            existing => return Ok(existing),
        }
        if documents.is_empty() {
            return Err(DocumentPromptError::EmptyDocumentList);
        }
        self.next_prompt_id = self.next_prompt_id.wrapping_add(1).max(1);
        let prompt = DocumentPrompt {
            prompt_id: self.next_prompt_id,
            app_id: app_id.into(),
            app_name: app_name.into(),
            documents,
        };
        self.pending = Some(prompt.clone());
        Ok(DocumentRequestResult::Pending(prompt))
    }

    pub fn pending(&self) -> Option<&DocumentPrompt> {
        self.pending.as_ref()
    }

    pub fn resolve(
        &mut self,
        prompt_id: u64,
        document_id: Option<&str>,
    ) -> Result<(String, Option<String>), DocumentPromptError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(DocumentPromptError::NoPendingPrompt)?;
        if pending.prompt_id != prompt_id {
            return Err(DocumentPromptError::StalePrompt);
        }
        let selection = match document_id {
            Some(document_id)
                if is_valid_document_id(document_id)
                    && pending
                        .documents
                        .iter()
                        .any(|document| document.document_id == document_id) =>
            {
                ResolvedSelection::Selected(document_id.into())
            }
            Some(_) => return Err(DocumentPromptError::InvalidSelection),
            None => ResolvedSelection::Cancelled,
        };
        let pending = self.pending.take().expect("pending prompt exists");
        let selected = match &selection {
            ResolvedSelection::Selected(document_id) => Some(document_id.clone()),
            ResolvedSelection::Cancelled => None,
        };
        self.resolved = Some((pending.app_id.clone(), selection));
        Ok((pending.app_id, selected))
    }

    pub fn clear_app(&mut self, app_id: &str) {
        if self
            .pending
            .as_ref()
            .is_some_and(|prompt| prompt.app_id == app_id)
        {
            self.pending = None;
        }
        if self
            .resolved
            .as_ref()
            .is_some_and(|(resolved_app, _)| resolved_app == app_id)
        {
            self.resolved = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(id: &str, name: &str) -> DocumentSummary {
        DocumentSummary {
            document_id: id.into(),
            name: name.into(),
            size_bytes: 5,
        }
    }

    #[test]
    fn only_resolves_an_id_from_the_trusted_snapshot() {
        let id = "00000000000000010000000000000002";
        let mut coordinator = DocumentCoordinator::default();
        let prompt = match coordinator
            .request(
                "dev.cardputerzero.app",
                "App",
                vec![document(id, "one.txt")],
            )
            .unwrap()
        {
            DocumentRequestResult::Pending(prompt) => prompt,
            other => panic!("unexpected result: {other:?}"),
        };
        assert!(matches!(
            coordinator.resolve(prompt.prompt_id, Some("00000000000000030000000000000004")),
            Err(DocumentPromptError::InvalidSelection)
        ));
        coordinator.resolve(prompt.prompt_id, Some(id)).unwrap();
        assert_eq!(
            coordinator.poll("dev.cardputerzero.app").unwrap(),
            DocumentRequestResult::Selected(id.into())
        );
    }

    #[test]
    fn serializes_prompts_and_supports_cancel() {
        let id = "00000000000000010000000000000002";
        let mut coordinator = DocumentCoordinator::default();
        let prompt = match coordinator
            .request(
                "dev.cardputerzero.first",
                "First",
                vec![document(id, "one.txt")],
            )
            .unwrap()
        {
            DocumentRequestResult::Pending(prompt) => prompt,
            other => panic!("unexpected result: {other:?}"),
        };
        assert!(matches!(
            coordinator.poll("dev.cardputerzero.second"),
            Err(DocumentPromptError::Busy(_))
        ));
        coordinator.resolve(prompt.prompt_id, None).unwrap();
        assert_eq!(
            coordinator.poll("dev.cardputerzero.first").unwrap(),
            DocumentRequestResult::Cancelled
        );
    }
}
