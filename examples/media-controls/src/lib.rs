#![no_std]

#[cfg(not(test))]
use core::panic::PanicInfo;
#[cfg(not(test))]
use cp0_sdk::{
    display::{self, Rect},
    input,
    media::{self, Action, ActionCapabilities, PlaybackState},
    ui::{Canvas, color},
};

#[cfg(not(test))]
const KEY_SPACE: u16 = 57;
#[cfg(not(test))]
const FRAME_BYTES: usize = display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;
#[cfg(not(test))]
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LastAction {
    None,
    PlayPause,
    Previous,
    Next,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Player {
    playing: bool,
    track: u8,
    last_action: LastAction,
}

impl Player {
    const fn new() -> Self {
        Self {
            playing: true,
            track: 1,
            last_action: LastAction::None,
        }
    }

    fn apply(&mut self, action: LastAction) {
        self.last_action = action;
        match action {
            LastAction::PlayPause => self.playing = !self.playing,
            LastAction::Previous => self.track = self.track.saturating_sub(1).max(1),
            LastAction::Next => self.track = self.track.saturating_add(1).min(9),
            LastAction::None => {}
        }
    }

    #[cfg(not(test))]
    const fn playback_state(self) -> PlaybackState {
        if self.playing {
            PlaybackState::Playing
        } else {
            PlaybackState::Paused
        }
    }
}

#[cfg(not(test))]
fn frame() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(FRAME).cast(), FRAME_BYTES) }
}

#[cfg(not(test))]
fn register(player: Player) -> Result<(), cp0_sdk::Error> {
    media::update_session(player.playback_state(), ActionCapabilities::ALL)
}

#[cfg(not(test))]
fn render(player: Player, pixels: &mut [u8]) {
    let mut canvas = Canvas::new(pixels, display::WIDTH, display::STANDARD_HEIGHT).unwrap();
    canvas.clear(color::BACKGROUND);
    canvas.draw_text(106, 10, "MEDIA SESSION", color::MUTED, 1);
    canvas.fill_rect(
        Rect {
            x: 16,
            y: 30,
            width: 288,
            height: 78,
        },
        color::SURFACE,
    );
    canvas.stroke_rect(
        Rect {
            x: 16,
            y: 30,
            width: 288,
            height: 78,
        },
        color::ACCENT,
    );
    canvas.draw_text(
        112,
        42,
        if player.playing { "PLAYING" } else { "PAUSED" },
        if player.playing {
            color::SUCCESS
        } else {
            color::WARNING
        },
        2,
    );
    let track = [b'0' + player.track];
    let track = unsafe { core::str::from_utf8_unchecked(&track) };
    canvas.draw_text(112, 72, "TRACK", color::TEXT, 1);
    canvas.draw_text(154, 72, track, color::ACCENT, 1);
    canvas.draw_text(94, 91, "LAST", color::MUTED, 1);
    canvas.draw_text(130, 91, action_label(player.last_action), color::TEXT, 1);
    canvas.draw_text(91, 126, "FN Q / W / E", color::MUTED, 1);
}

#[cfg(not(test))]
const fn action_label(action: LastAction) -> &'static str {
    match action {
        LastAction::None => "NONE",
        LastAction::PlayPause => "PLAY PAUSE",
        LastAction::Previous => "PREVIOUS",
        LastAction::Next => "NEXT",
    }
}

#[cfg(not(test))]
const fn from_media_action(action: Action) -> LastAction {
    match action {
        Action::PlayPause => LastAction::PlayPause,
        Action::Previous => LastAction::Previous,
        Action::Next => LastAction::Next,
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let pixels = frame();
    let mut player = Player::new();
    if register(player).is_err() {
        return 1;
    }
    let mut dirty = true;
    loop {
        match media::take_action() {
            Ok(Some(action)) => {
                player.apply(from_media_action(action));
                if register(player).is_err() {
                    return 1;
                }
                dirty = true;
            }
            Ok(None) => {}
            Err(_) => return 1,
        }
        if dirty {
            render(player, pixels);
            if display::present_rgb565(pixels, &[]).is_err() {
                return 1;
            }
            dirty = false;
        }
        match input::poll_key_event(50) {
            Ok(Some(event)) if event.pressed && event.code == KEY_SPACE => {
                player.apply(LastAction::PlayPause);
                if register(player).is_err() {
                    return 1;
                }
                dirty = true;
            }
            Ok(_) => {}
            Err(_) => return 1,
        }
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_bounded_media_actions() {
        let mut player = Player::new();
        player.apply(LastAction::Previous);
        assert_eq!(player.track, 1);
        player.apply(LastAction::Next);
        assert_eq!(player.track, 2);
        player.track = 9;
        player.apply(LastAction::Next);
        assert_eq!(player.track, 9);
        player.apply(LastAction::PlayPause);
        assert!(!player.playing);
    }
}
