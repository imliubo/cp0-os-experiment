use std::collections::VecDeque;
use std::fmt;

pub const MAX_PENDING_INTENTS: usize = 8;
pub const MAX_INTENT_PAYLOAD_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingIntent {
    pub intent_id: u64,
    pub sender_app_id: String,
    pub target_app_id: String,
    pub action: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentQueueError {
    InvalidAction,
    PayloadTooLarge,
    Full,
    Exhausted,
}

impl fmt::Display for IntentQueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAction => formatter.write_str("invalid intent action"),
            Self::PayloadTooLarge => formatter.write_str("intent payload is too large"),
            Self::Full => formatter.write_str("intent queue is full"),
            Self::Exhausted => formatter.write_str("intent identifier space is exhausted"),
        }
    }
}

impl std::error::Error for IntentQueueError {}

#[derive(Debug, Default)]
pub struct IntentQueue {
    next_id: u64,
    pending: VecDeque<PendingIntent>,
}

impl IntentQueue {
    pub fn enqueue(
        &mut self,
        sender_app_id: &str,
        target_app_id: &str,
        action: String,
        payload: Vec<u8>,
    ) -> Result<u64, IntentQueueError> {
        if !cp0_manifest::is_valid_intent_action(&action) {
            return Err(IntentQueueError::InvalidAction);
        }
        if payload.len() > MAX_INTENT_PAYLOAD_BYTES {
            return Err(IntentQueueError::PayloadTooLarge);
        }
        if self.pending.len() >= MAX_PENDING_INTENTS {
            return Err(IntentQueueError::Full);
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(IntentQueueError::Exhausted)?;
        let intent_id = self.next_id;
        self.pending.push_back(PendingIntent {
            intent_id,
            sender_app_id: sender_app_id.into(),
            target_app_id: target_app_id.into(),
            action,
            payload,
        });
        Ok(intent_id)
    }

    pub fn take(&mut self, target_app_id: &str) -> Option<PendingIntent> {
        let index = self
            .pending
            .iter()
            .position(|intent| intent.target_app_id == target_app_id)?;
        self.pending.remove(index)
    }

    pub fn cancel(&mut self, intent_id: u64) -> bool {
        let Some(index) = self
            .pending
            .iter()
            .position(|intent| intent.intent_id == intent_id)
        else {
            return false;
        };
        self.pending.remove(index);
        true
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_queue_and_delivers_once_to_the_bound_target() {
        let mut queue = IntentQueue::default();
        for index in 0..MAX_PENDING_INTENTS {
            queue
                .enqueue(
                    "dev.cardputerzero.sender",
                    if index == 0 {
                        "dev.cardputerzero.receiver"
                    } else {
                        "dev.cardputerzero.other"
                    },
                    "dev.cardputerzero.documents.open".into(),
                    vec![index as u8],
                )
                .unwrap();
        }
        assert_eq!(queue.len(), MAX_PENDING_INTENTS);
        assert_eq!(
            queue.enqueue(
                "dev.cardputerzero.sender",
                "dev.cardputerzero.receiver",
                "dev.cardputerzero.documents.open".into(),
                Vec::new(),
            ),
            Err(IntentQueueError::Full)
        );
        let message = queue.take("dev.cardputerzero.receiver").unwrap();
        assert_eq!(message.payload, [0]);
        assert!(queue.take("dev.cardputerzero.receiver").is_none());
    }

    #[test]
    fn cancels_only_the_acknowledgement_bound_message() {
        let mut queue = IntentQueue::default();
        let first = queue
            .enqueue(
                "dev.cardputerzero.sender",
                "dev.cardputerzero.receiver",
                "dev.cardputerzero.documents.open".into(),
                Vec::new(),
            )
            .unwrap();
        let second = queue
            .enqueue(
                "dev.cardputerzero.sender",
                "dev.cardputerzero.receiver",
                "dev.cardputerzero.documents.open".into(),
                Vec::new(),
            )
            .unwrap();
        assert!(queue.cancel(first));
        assert_eq!(
            queue.take("dev.cardputerzero.receiver").unwrap().intent_id,
            second
        );
        assert!(!queue.cancel(first));
    }
}
