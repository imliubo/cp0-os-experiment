use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

pub const MEDIA_ACTION_PLAY_PAUSE: u8 = 1 << 0;
pub const MEDIA_ACTION_PREVIOUS: u8 = 1 << 1;
pub const MEDIA_ACTION_NEXT: u8 = 1 << 2;
pub const MEDIA_ACTION_ALL: u8 =
    MEDIA_ACTION_PLAY_PAUSE | MEDIA_ACTION_PREVIOUS | MEDIA_ACTION_NEXT;
pub const MAX_PENDING_MEDIA_ACTIONS: usize = 4;

pub const fn valid_media_session_update(state: MediaPlaybackState, supported_actions: u8) -> bool {
    if supported_actions & !MEDIA_ACTION_ALL != 0 {
        return false;
    }
    match state {
        MediaPlaybackState::Inactive => supported_actions == 0,
        MediaPlaybackState::Paused | MediaPlaybackState::Playing => supported_actions != 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaPlaybackState {
    Inactive,
    Paused,
    Playing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaAction {
    PlayPause,
    Previous,
    Next,
}

impl MediaAction {
    pub const fn capability(self) -> u8 {
        match self {
            Self::PlayPause => MEDIA_ACTION_PLAY_PAUSE,
            Self::Previous => MEDIA_ACTION_PREVIOUS,
            Self::Next => MEDIA_ACTION_NEXT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSessionError {
    Unavailable,
    Unsupported,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MediaSession {
    app_id: String,
    runtime_token: u64,
    state: MediaPlaybackState,
    supported_actions: u8,
    actions: VecDeque<MediaAction>,
}

#[derive(Debug, Default)]
pub struct MediaSessionBroker {
    session: Option<MediaSession>,
}

impl MediaSessionBroker {
    pub fn update(
        &mut self,
        app_id: &str,
        runtime_token: u64,
        state: MediaPlaybackState,
        supported_actions: u8,
    ) {
        if state == MediaPlaybackState::Inactive {
            self.clear_app(app_id);
            return;
        }

        let session = self.session.get_or_insert_with(|| MediaSession {
            app_id: app_id.into(),
            runtime_token,
            state,
            supported_actions,
            actions: VecDeque::new(),
        });
        if session.app_id != app_id || session.runtime_token != runtime_token {
            *session = MediaSession {
                app_id: app_id.into(),
                runtime_token,
                state,
                supported_actions,
                actions: VecDeque::new(),
            };
            return;
        }
        session.state = state;
        session.supported_actions = supported_actions;
        session
            .actions
            .retain(|action| supported_actions & action.capability() != 0);
    }

    pub fn dispatch(
        &mut self,
        active_app_id: &str,
        runtime_token: u64,
        action: MediaAction,
    ) -> Result<(), MediaSessionError> {
        let session = self
            .session
            .as_mut()
            .filter(|session| {
                session.app_id == active_app_id && session.runtime_token == runtime_token
            })
            .ok_or(MediaSessionError::Unavailable)?;
        if session.supported_actions & action.capability() == 0 {
            return Err(MediaSessionError::Unsupported);
        }
        if session.actions.len() >= MAX_PENDING_MEDIA_ACTIONS {
            return Err(MediaSessionError::Full);
        }
        session.actions.push_back(action);
        Ok(())
    }

    pub fn take(&mut self, app_id: &str, runtime_token: u64) -> Option<MediaAction> {
        self.session
            .as_mut()
            .filter(|session| session.app_id == app_id && session.runtime_token == runtime_token)
            .and_then(|session| session.actions.pop_front())
    }

    pub fn clear_runtime(&mut self, app_id: &str, runtime_token: u64) {
        if self.session.as_ref().is_some_and(|session| {
            session.app_id == app_id && session.runtime_token == runtime_token
        }) {
            self.session = None;
        }
    }

    pub fn clear_app(&mut self, app_id: &str) {
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.app_id == app_id)
        {
            self.session = None;
        }
    }

    pub fn clear(&mut self) {
        self.session = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_only_supported_actions_to_the_bound_identity() {
        let mut broker = MediaSessionBroker::default();
        broker.update(
            "dev.cardputerzero.player",
            7,
            MediaPlaybackState::Playing,
            MEDIA_ACTION_PLAY_PAUSE | MEDIA_ACTION_NEXT,
        );
        assert_eq!(
            broker.dispatch("dev.cardputerzero.other", 7, MediaAction::Next),
            Err(MediaSessionError::Unavailable)
        );
        assert_eq!(
            broker.dispatch("dev.cardputerzero.player", 7, MediaAction::Previous),
            Err(MediaSessionError::Unsupported)
        );
        assert_eq!(
            broker.dispatch("dev.cardputerzero.player", 8, MediaAction::Next),
            Err(MediaSessionError::Unavailable)
        );
        assert_eq!(
            broker.dispatch("dev.cardputerzero.player", 7, MediaAction::Next),
            Ok(())
        );
        assert_eq!(broker.take("dev.cardputerzero.other", 7), None);
        assert_eq!(
            broker.take("dev.cardputerzero.player", 7),
            Some(MediaAction::Next)
        );
    }

    #[test]
    fn bounds_queue_and_purges_removed_capabilities() {
        let mut broker = MediaSessionBroker::default();
        broker.update(
            "dev.cardputerzero.player",
            7,
            MediaPlaybackState::Paused,
            MEDIA_ACTION_ALL,
        );
        for _ in 0..MAX_PENDING_MEDIA_ACTIONS {
            assert_eq!(
                broker.dispatch("dev.cardputerzero.player", 7, MediaAction::Next),
                Ok(())
            );
        }
        assert_eq!(
            broker.dispatch("dev.cardputerzero.player", 7, MediaAction::Next),
            Err(MediaSessionError::Full)
        );
        broker.update(
            "dev.cardputerzero.player",
            7,
            MediaPlaybackState::Playing,
            MEDIA_ACTION_PLAY_PAUSE,
        );
        assert_eq!(broker.take("dev.cardputerzero.player", 7), None);
    }

    #[test]
    fn replacement_and_lifecycle_clear_never_leak_actions() {
        let mut broker = MediaSessionBroker::default();
        broker.update(
            "dev.cardputerzero.first",
            7,
            MediaPlaybackState::Playing,
            MEDIA_ACTION_ALL,
        );
        broker
            .dispatch("dev.cardputerzero.first", 7, MediaAction::Previous)
            .unwrap();
        broker.update(
            "dev.cardputerzero.second",
            8,
            MediaPlaybackState::Paused,
            MEDIA_ACTION_PLAY_PAUSE,
        );
        assert_eq!(broker.take("dev.cardputerzero.first", 7), None);
        assert_eq!(broker.take("dev.cardputerzero.second", 8), None);
        broker.clear_app("dev.cardputerzero.first");
        assert_eq!(
            broker.dispatch("dev.cardputerzero.second", 8, MediaAction::PlayPause),
            Ok(())
        );
        broker.update(
            "dev.cardputerzero.second",
            8,
            MediaPlaybackState::Inactive,
            0,
        );
        assert_eq!(
            broker.dispatch("dev.cardputerzero.second", 8, MediaAction::PlayPause),
            Err(MediaSessionError::Unavailable)
        );
    }

    #[test]
    fn runtime_tokens_isolate_restarts_of_the_same_application() {
        let mut broker = MediaSessionBroker::default();
        broker.update(
            "dev.cardputerzero.player",
            7,
            MediaPlaybackState::Playing,
            MEDIA_ACTION_ALL,
        );
        broker
            .dispatch("dev.cardputerzero.player", 7, MediaAction::Next)
            .unwrap();
        assert_eq!(broker.take("dev.cardputerzero.player", 8), None);
        broker.clear_runtime("dev.cardputerzero.player", 8);
        assert_eq!(
            broker.take("dev.cardputerzero.player", 7),
            Some(MediaAction::Next)
        );
        broker.clear_runtime("dev.cardputerzero.player", 7);
        assert_eq!(broker.take("dev.cardputerzero.player", 7), None);
    }

    #[test]
    fn validates_registration_state_and_capability_mask() {
        assert!(valid_media_session_update(MediaPlaybackState::Inactive, 0));
        assert!(valid_media_session_update(
            MediaPlaybackState::Playing,
            MEDIA_ACTION_ALL
        ));
        assert!(!valid_media_session_update(
            MediaPlaybackState::Inactive,
            MEDIA_ACTION_PLAY_PAUSE
        ));
        assert!(!valid_media_session_update(MediaPlaybackState::Paused, 0));
        assert!(!valid_media_session_update(
            MediaPlaybackState::Playing,
            1 << 7
        ));
    }
}
