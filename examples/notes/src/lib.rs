#![no_std]

#[cfg(not(test))]
use core::panic::PanicInfo;
use cp0_sdk::{
    display::{self, Rect},
    input, storage, system,
    ui::{Canvas, color},
};

const KEY_BACKSPACE: u16 = 14;
const KEY_ENTER: u16 = 28;
const NOTE_KEY: &str = "draft.v1";
const NOTE_BYTES: usize = 192;
const LINE_COLUMNS: usize = 48;
const LINE_COUNT: usize = 11;
const FRAME_BYTES: usize = display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;

#[cfg(not(test))]
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];

struct Note {
    bytes: [u8; NOTE_BYTES],
    length: usize,
}

impl Note {
    const fn empty() -> Self {
        Self {
            bytes: [0; NOTE_BYTES],
            length: 0,
        }
    }

    fn push(&mut self, byte: u8) -> bool {
        if self.length == self.bytes.len() {
            return false;
        }
        self.bytes[self.length] = byte;
        self.length += 1;
        true
    }

    fn backspace(&mut self) -> bool {
        if self.length == 0 {
            return false;
        }
        self.length -= 1;
        self.bytes[self.length] = 0;
        true
    }
}

#[cfg(not(test))]
fn frame() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(FRAME).cast(), FRAME_BYTES) }
}

fn load_note() -> Note {
    let mut note = Note::empty();
    if let Ok(Some(length)) = storage::get(NOTE_KEY, &mut note.bytes) {
        if note.bytes[..length]
            .iter()
            .all(|byte| *byte == b'\n' || (b' '..=b'~').contains(byte))
        {
            note.length = length;
        }
    }
    note
}

fn apply_key(note: &mut Note, event: input::KeyEvent) -> bool {
    if event.code == KEY_BACKSPACE {
        note.backspace()
    } else if event.code == KEY_ENTER {
        note.push(b'\n')
    } else if let Some(character) = event.character {
        note.push(character)
    } else {
        false
    }
}

fn render(note: &Note, pixels: &mut [u8], saved: bool) {
    let mut canvas = Canvas::new(pixels, display::WIDTH, display::STANDARD_HEIGHT).unwrap();
    canvas.clear(color::BACKGROUND);
    canvas.draw_text(10, 8, "NOTES", color::TEXT, 2);
    canvas.draw_text(
        263,
        11,
        if saved { "SAVED" } else { "EDIT" },
        if saved {
            color::SUCCESS
        } else {
            color::WARNING
        },
        1,
    );
    canvas.fill_rect(
        Rect {
            x: 8,
            y: 29,
            width: 304,
            height: 116,
        },
        color::SURFACE,
    );
    canvas.stroke_rect(
        Rect {
            x: 8,
            y: 29,
            width: 304,
            height: 116,
        },
        color::MUTED,
    );

    let mut line = [b' '; LINE_COLUMNS];
    let mut row = 0;
    let mut column = 0;
    for byte in note.bytes[..note.length].iter().copied() {
        if byte == b'\n' || column == LINE_COLUMNS {
            draw_line(&mut canvas, row, &line, column);
            line.fill(b' ');
            row += 1;
            column = 0;
            if row == LINE_COUNT {
                break;
            }
            if byte == b'\n' {
                continue;
            }
        }
        line[column] = byte;
        column += 1;
    }
    if row < LINE_COUNT {
        draw_line(&mut canvas, row, &line, column);
        let cursor_x = 15 + column as u16 * 6;
        let cursor_y = 37 + row as u16 * 10;
        canvas.fill_rect(
            Rect {
                x: cursor_x,
                y: cursor_y,
                width: 2,
                height: 8,
            },
            color::ACCENT,
        );
    }
}

fn draw_line(canvas: &mut Canvas<'_>, row: usize, line: &[u8], length: usize) {
    let text = unsafe { core::str::from_utf8_unchecked(&line[..length]) };
    canvas.draw_text(15, 37 + row as u16 * 10, text, color::TEXT, 1);
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let pixels = frame();
    let mut note = load_note();
    let mut dirty = true;
    let mut needs_save = false;
    let mut save_at = 0;
    let mut saved = true;

    loop {
        let now = system::monotonic_milliseconds();
        if needs_save && now >= save_at {
            saved = if note.length == 0 {
                storage::delete(NOTE_KEY).is_ok()
            } else {
                storage::put(NOTE_KEY, &note.bytes[..note.length]).is_ok()
            };
            needs_save = false;
            dirty = true;
        }
        if dirty {
            render(&note, pixels, saved);
            if display::present_rgb565(pixels, &[]).is_ok() {
                dirty = false;
            }
        }
        match input::poll_key_event(100) {
            Ok(Some(event)) if event.pressed => {
                let changed = apply_key(&mut note, event);
                if changed {
                    saved = false;
                    needs_save = true;
                    save_at = now.saturating_add(600);
                    dirty = true;
                }
            }
            Ok(_) => {}
            Err(_) => return 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumes_system_characters_and_bounds_note() {
        let mut note = Note::empty();
        for character in b"aA!@#$%^&*()~`_-+=[]{};:'\"<>\\|,./? " {
            assert!(apply_key(
                &mut note,
                input::KeyEvent {
                    code: 0,
                    pressed: true,
                    repeated: false,
                    modifiers: 0,
                    character: Some(*character),
                },
            ));
        }
        for _ in 0..NOTE_BYTES {
            if note.length < NOTE_BYTES {
                assert!(note.push(b'x'));
            }
        }
        assert!(!note.push(b'y'));
        assert!(note.backspace());
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
