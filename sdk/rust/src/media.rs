use core::ops::{BitOr, BitOrAssign};

use crate::{Error, host_imports};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PlaybackState {
    Inactive = 0,
    Paused = 1,
    Playing = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    PlayPause,
    Previous,
    Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionCapabilities(u32);

impl ActionCapabilities {
    pub const NONE: Self = Self(0);
    pub const PLAY_PAUSE: Self = Self(1 << 0);
    pub const PREVIOUS: Self = Self(1 << 1);
    pub const NEXT: Self = Self(1 << 2);
    pub const ALL: Self = Self(Self::PLAY_PAUSE.0 | Self::PREVIOUS.0 | Self::NEXT.0);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    const fn bits(self) -> u32 {
        self.0
    }
}

impl BitOr for ActionCapabilities {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for ActionCapabilities {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

pub fn update_session(
    state: PlaybackState,
    supported_actions: ActionCapabilities,
) -> Result<(), Error> {
    if (state == PlaybackState::Inactive) != (supported_actions == ActionCapabilities::NONE) {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host_imports::cp0_media_session_update(
        state as u32,
        supported_actions.bits(),
    ))
}

pub fn take_action() -> Result<Option<Action>, Error> {
    match host_imports::cp0_media_take_action() {
        0 => Ok(None),
        1 => Ok(Some(Action::PlayPause)),
        2 => Ok(Some(Action::Previous)),
        3 => Ok(Some(Action::Next)),
        value if value < 0 => Error::from_host(value).map(|()| None),
        _ => Err(Error::Internal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_media_session_and_maps_unavailable_host() {
        let supported = ActionCapabilities::PLAY_PAUSE | ActionCapabilities::NEXT;
        assert!(supported.contains(ActionCapabilities::PLAY_PAUSE));
        assert!(!supported.contains(ActionCapabilities::PREVIOUS));
        assert_eq!(
            update_session(PlaybackState::Inactive, supported),
            Err(Error::InvalidArgument)
        );
        assert_eq!(
            update_session(PlaybackState::Playing, ActionCapabilities::NONE),
            Err(Error::InvalidArgument)
        );
        assert_eq!(
            update_session(PlaybackState::Playing, supported),
            Err(Error::Unavailable)
        );
        assert_eq!(take_action(), Err(Error::Unavailable));
    }
}
