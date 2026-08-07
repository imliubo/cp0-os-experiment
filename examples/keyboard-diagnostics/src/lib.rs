#![no_std]

#[cfg(not(test))]
use core::panic::PanicInfo;
use cp0_sdk::input::{KeyEvent, MODIFIER_SHIFT};
#[cfg(not(test))]
use cp0_sdk::{
    display::{self, Rect},
    input, storage,
    ui::{Canvas, color},
};

const KEY_BACKSPACE: u16 = 14;
const KEY_ENTER: u16 = 28;
const KEY_A: u16 = 30;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_Z: u16 = 44;
const KEY_RIGHTSHIFT: u16 = 54;

#[cfg(not(test))]
const LOG_KEY: &str = "keyboard-test.log";
const LOG_BYTES: usize = 8192;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestCase {
    prompt: &'static str,
    expected_name: &'static str,
    code: u16,
    shifted: bool,
    ascii: u8,
}

const fn test(
    prompt: &'static str,
    expected_name: &'static str,
    code: u16,
    shifted: bool,
    ascii: u8,
) -> TestCase {
    TestCase {
        prompt,
        expected_name,
        code,
        shifted,
        ascii,
    }
}

// The Sym cases are an exact transcription of the user-provided V0.6 CSV.
const TESTS: [TestCase; 37] = [
    test("PRESS A ONLY", "LOWERCASE A", KEY_A, false, b'a'),
    test("HOLD SHIFT + A", "UPPERCASE A", KEY_A, true, b'A'),
    test(
        "RELEASE SHIFT THEN PRESS A",
        "LOWERCASE A",
        KEY_A,
        false,
        b'a',
    ),
    test("HOLD SHIFT + Z", "UPPERCASE Z", KEY_Z, true, b'Z'),
    test(
        "RELEASE SHIFT THEN PRESS Z",
        "LOWERCASE Z",
        KEY_Z,
        false,
        b'z',
    ),
    test("HOLD SYM + 1", "EXCLAMATION", 26, false, b'!'),
    test("HOLD SYM + 2", "AT SIGN", 27, false, b'@'),
    test("HOLD SYM + 3", "HASH", 39, false, b'#'),
    test("HOLD SYM + 4", "DOLLAR", 40, false, b'$'),
    test("HOLD SYM + 5", "PERCENT", 41, false, b'%'),
    test("HOLD SYM + 6", "CARET", 43, false, b'^'),
    test("HOLD SYM + 7", "AMPERSAND", 51, false, b'&'),
    test("HOLD SYM + 8", "ASTERISK", 52, false, b'*'),
    test("HOLD SYM + 9", "LEFT PAREN", 53, false, b'('),
    test("HOLD SYM + 0", "RIGHT PAREN", 94, false, b')'),
    test("HOLD SYM + Q", "TILDE", 55, false, b'~'),
    test("HOLD SYM + W", "BACKTICK", 69, false, b'`'),
    test("HOLD SYM + E", "UNDERSCORE", 70, false, b'_'),
    test("HOLD SYM + R", "MINUS", 71, false, b'-'),
    test("HOLD SYM + T", "PLUS", 72, false, b'+'),
    test("HOLD SYM + Y", "EQUAL", 73, false, b'='),
    test("HOLD SYM + U", "LEFT BRACKET", 74, false, b'['),
    test("HOLD SYM + I", "RIGHT BRACKET", 75, false, b']'),
    test("HOLD SYM + O", "LEFT BRACE", 76, false, b'{'),
    test("HOLD SYM + P", "RIGHT BRACE", 77, false, b'}'),
    test("HOLD SYM + A", "SEMICOLON", 79, false, b';'),
    test("HOLD SYM + S", "COLON", 80, false, b':'),
    test("HOLD SYM + D", "APOSTROPHE", 81, false, b'\''),
    test("HOLD SYM + G", "DOUBLE QUOTE", 82, false, b'\"'),
    test("HOLD SYM + H", "LESS THAN", 83, false, b'<'),
    test("HOLD SYM + J", "GREATER THAN", 85, false, b'>'),
    test("HOLD SYM + K", "BACKSLASH", 86, false, b'\\'),
    test("HOLD SYM + L", "PIPE", 89, false, b'|'),
    test("HOLD SYM + V", "COMMA", 90, false, b','),
    test("HOLD SYM + B", "DOT", 91, false, b'.'),
    test("HOLD SYM + N", "SLASH", 92, false, b'/'),
    test("HOLD SYM + M", "QUESTION", 93, false, b'?'),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Waiting,
    Review,
    Complete,
}

struct LogBuffer {
    bytes: [u8; LOG_BYTES],
    len: usize,
    truncated: bool,
}

impl LogBuffer {
    const fn new() -> Self {
        Self {
            bytes: [0; LOG_BYTES],
            len: 0,
            truncated: false,
        }
    }

    fn push(&mut self, value: u8) {
        if self.len < self.bytes.len() {
            self.bytes[self.len] = value;
            self.len += 1;
        } else {
            self.truncated = true;
        }
    }

    fn text(&mut self, value: &str) {
        for byte in value.bytes() {
            self.push(byte);
        }
    }

    fn number(&mut self, value: u16) {
        let mut digits = [0_u8; 5];
        let mut cursor = digits.len();
        let mut value = value;
        if value == 0 {
            self.push(b'0');
            return;
        }
        while value > 0 {
            cursor -= 1;
            digits[cursor] = b'0' + (value % 10) as u8;
            value /= 10;
        }
        for digit in &digits[cursor..] {
            self.push(*digit);
        }
    }

    fn slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

struct Diagnostics {
    index: usize,
    phase: Phase,
    captured: Option<KeyEvent>,
    captured_ascii: Option<u8>,
    captured_matches: bool,
    passed: u16,
    confirmed: u16,
    attempts: u16,
    sequence: u16,
    #[cfg(not(test))]
    storage_failed: bool,
    log: LogBuffer,
}

impl Diagnostics {
    fn new() -> Self {
        let mut state = Self {
            index: 0,
            phase: Phase::Waiting,
            captured: None,
            captured_ascii: None,
            captured_matches: false,
            passed: 0,
            confirmed: 0,
            attempts: 0,
            sequence: 0,
            #[cfg(not(test))]
            storage_failed: false,
            log: LogBuffer::new(),
        };
        state.log.text("CP0K,1,");
        state.log.number(TESTS.len() as u16);
        state.log.push(b'\n');
        state.begin_step();
        state
    }

    fn begin_step(&mut self) {
        let case = TESTS[self.index];
        self.log.text("S,");
        self.log.number((self.index + 1) as u16);
        self.log.push(b',');
        self.log.text(case.prompt);
        self.log.push(b',');
        self.log.number(case.code);
        self.log.push(b',');
        self.log.push(if case.shifted { b'1' } else { b'0' });
        self.log.push(b',');
        self.log.number(u16::from(case.ascii));
        self.log.push(b'\n');
    }

    fn log_event(&mut self, event: KeyEvent) {
        self.sequence = self.sequence.saturating_add(1);
        self.log.text("E,");
        self.log.number(self.sequence);
        self.log.push(b',');
        self.log.number((self.index + 1) as u16);
        self.log.push(b',');
        self.log.number(event.code);
        self.log.push(b',');
        self.log.push(if event.pressed { b'1' } else { b'0' });
        self.log.push(b',');
        self.log.push(if event.repeated { b'1' } else { b'0' });
        self.log.push(b',');
        self.log.number(u16::from(event.modifiers));
        self.log.push(b'\n');
    }

    fn capture(&mut self, event: KeyEvent) {
        let case = TESTS[self.index];
        let actual_ascii = event.character;
        let clean_modifiers = event.modifiers & !MODIFIER_SHIFT == 0;
        let shifted = event.modifiers & MODIFIER_SHIFT != 0;
        self.captured_matches = event.code == case.code
            && shifted == case.shifted
            && clean_modifiers
            && actual_ascii == Some(case.ascii);
        self.captured = Some(event);
        self.captured_ascii = actual_ascii;
        self.attempts = self.attempts.saturating_add(1);
        self.phase = Phase::Review;
        self.log.text("C,");
        self.log.number((self.index + 1) as u16);
        self.log.push(b',');
        self.log.number(event.code);
        self.log.push(b',');
        self.log.number(u16::from(event.modifiers));
        self.log.push(b',');
        if let Some(ascii) = actual_ascii {
            self.log.number(u16::from(ascii));
        } else {
            self.log.text("none");
        }
        self.log.push(b',');
        self.log
            .push(if self.captured_matches { b'1' } else { b'0' });
        self.log.push(b'\n');
    }

    fn confirm(&mut self) {
        self.confirmed = self.confirmed.saturating_add(1);
        if self.captured_matches {
            self.passed = self.passed.saturating_add(1);
        }
        self.log.text("K,");
        self.log.number((self.index + 1) as u16);
        self.log.push(b',');
        self.log
            .push(if self.captured_matches { b'1' } else { b'0' });
        self.log.push(b'\n');
        if self.index + 1 == TESTS.len() {
            self.phase = Phase::Complete;
            self.log.text("D,");
            self.log.number(self.confirmed);
            self.log.push(b',');
            self.log.number(self.passed);
            self.log.push(b',');
            self.log.number(self.attempts);
            self.log.push(b',');
            self.log.push(if self.log.truncated { b'1' } else { b'0' });
            self.log.push(b'\n');
        } else {
            self.index += 1;
            self.phase = Phase::Waiting;
            self.captured = None;
            self.captured_ascii = None;
            self.captured_matches = false;
            self.begin_step();
        }
    }

    fn retry(&mut self) {
        self.log.text("R,");
        self.log.number((self.index + 1) as u16);
        self.log.push(b'\n');
        self.phase = Phase::Waiting;
        self.captured = None;
        self.captured_ascii = None;
        self.captured_matches = false;
    }

    fn handle(&mut self, event: KeyEvent) -> bool {
        self.log_event(event);
        if !event.pressed {
            return false;
        }
        match self.phase {
            Phase::Waiting if !is_modifier(event.code) => {
                self.capture(event);
                true
            }
            Phase::Waiting => false,
            Phase::Review if event.code == KEY_ENTER => {
                self.confirm();
                true
            }
            Phase::Review if event.code == KEY_BACKSPACE => {
                self.retry();
                true
            }
            Phase::Review | Phase::Complete => false,
        }
    }
}

const fn is_modifier(code: u16) -> bool {
    code == KEY_LEFTSHIFT || code == KEY_RIGHTSHIFT
}

#[cfg(not(test))]
struct TextLine {
    bytes: [u8; 64],
    len: usize,
}

#[cfg(not(test))]
impl TextLine {
    const fn new() -> Self {
        Self {
            bytes: [0; 64],
            len: 0,
        }
    }

    fn text(&mut self, value: &str) {
        for byte in value.bytes() {
            if self.len == self.bytes.len() {
                break;
            }
            self.bytes[self.len] = byte;
            self.len += 1;
        }
    }

    fn number(&mut self, value: u16) {
        let mut log = LogBuffer::new();
        log.number(value);
        for byte in log.slice() {
            if self.len == self.bytes.len() {
                break;
            }
            self.bytes[self.len] = *byte;
            self.len += 1;
        }
    }

    fn as_str(&self) -> &str {
        // TextLine only accepts ASCII literals and decimal digits.
        unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.len]) }
    }
}

#[cfg(not(test))]
fn ascii_name(value: Option<u8>) -> &'static str {
    match value {
        Some(b'a') => "LOWERCASE A",
        Some(b'A') => "UPPERCASE A",
        Some(b'z') => "LOWERCASE Z",
        Some(b'Z') => "UPPERCASE Z",
        Some(b'!') => "EXCLAMATION",
        Some(b'@') => "AT SIGN",
        Some(b'#') => "HASH",
        Some(b'$') => "DOLLAR",
        Some(b'%') => "PERCENT",
        Some(b'^') => "CARET",
        Some(b'&') => "AMPERSAND",
        Some(b'*') => "ASTERISK",
        Some(b'(') => "LEFT PAREN",
        Some(b')') => "RIGHT PAREN",
        Some(b'~') => "TILDE",
        Some(b'`') => "BACKTICK",
        Some(b'_') => "UNDERSCORE",
        Some(b'-') => "MINUS",
        Some(b'+') => "PLUS",
        Some(b'=') => "EQUAL",
        Some(b'[') => "LEFT BRACKET",
        Some(b']') => "RIGHT BRACKET",
        Some(b'{') => "LEFT BRACE",
        Some(b'}') => "RIGHT BRACE",
        Some(b';') => "SEMICOLON",
        Some(b':') => "COLON",
        Some(b'\'') => "APOSTROPHE",
        Some(b'\"') => "DOUBLE QUOTE",
        Some(b'<') => "LESS THAN",
        Some(b'>') => "GREATER THAN",
        Some(b'\\') => "BACKSLASH",
        Some(b'|') => "PIPE",
        Some(b',') => "COMMA",
        Some(b'.') => "DOT",
        Some(b'/') => "SLASH",
        Some(b'?') => "QUESTION",
        Some(_) => "OTHER ASCII",
        None => "NO ASCII MAPPING",
    }
}

#[cfg(not(test))]
const FRAME_BYTES: usize = display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;
#[cfg(not(test))]
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];

#[cfg(not(test))]
fn frame() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(FRAME).cast(), FRAME_BYTES) }
}

#[cfg(not(test))]
fn save(state: &mut Diagnostics) {
    if storage::put(LOG_KEY, state.log.slice()).is_err() {
        state.storage_failed = true;
    }
}

#[cfg(not(test))]
fn render(state: &Diagnostics, pixels: &mut [u8]) {
    let mut canvas = Canvas::new(pixels, display::WIDTH, display::STANDARD_HEIGHT).unwrap();
    canvas.clear(color::BACKGROUND);
    canvas.draw_text(8, 7, "KEYBOARD DIAGNOSTICS", color::TEXT, 1);

    let mut progress = TextLine::new();
    progress.text("STEP ");
    progress.number((state.index + 1) as u16);
    progress.text(" / ");
    progress.number(TESTS.len() as u16);
    canvas.draw_text(240, 7, progress.as_str(), color::MUTED, 1);
    canvas.progress(
        Rect {
            x: 8,
            y: 19,
            width: 304,
            height: 6,
        },
        state.confirmed,
        TESTS.len() as u16,
    );

    if state.phase == Phase::Complete {
        canvas.draw_text(73, 44, "TEST COMPLETE", color::SUCCESS, 2);
        let mut result = TextLine::new();
        result.text("MATCHED ");
        result.number(state.passed);
        result.text(" / ");
        result.number(TESTS.len() as u16);
        canvas.draw_text(100, 79, result.as_str(), color::TEXT, 1);
        canvas.draw_text(
            106,
            99,
            if state.storage_failed {
                "LOG SAVE FAILED"
            } else {
                "LOG SAVED"
            },
            if state.storage_failed {
                color::DANGER
            } else {
                color::ACCENT
            },
            1,
        );
        canvas.draw_text(50, 128, "USE HOME TO EXIT", color::MUTED, 1);
        return;
    }

    let case = TESTS[state.index];
    canvas.fill_rect(
        Rect {
            x: 8,
            y: 32,
            width: 304,
            height: 42,
        },
        color::SURFACE,
    );
    canvas.stroke_rect(
        Rect {
            x: 8,
            y: 32,
            width: 304,
            height: 42,
        },
        color::ACCENT,
    );
    canvas.draw_text(16, 40, case.prompt, color::TEXT, 1);
    let mut expected = TextLine::new();
    expected.text("EXPECT ");
    expected.text(case.expected_name);
    expected.text(" ASCII ");
    expected.number(u16::from(case.ascii));
    canvas.draw_text(16, 57, expected.as_str(), color::MUTED, 1);

    match state.phase {
        Phase::Waiting => {
            canvas.draw_text(82, 91, "WAITING FOR INPUT", color::WARNING, 1);
            canvas.draw_text(37, 128, "PRESS THE PROMPTED KEY", color::MUTED, 1);
        }
        Phase::Review => {
            let event = state.captured.unwrap();
            let mut received = TextLine::new();
            received.text("CODE ");
            received.number(event.code);
            received.text("  MODS ");
            received.number(u16::from(event.modifiers));
            canvas.draw_text(16, 84, received.as_str(), color::TEXT, 1);
            let mut actual = TextLine::new();
            actual.text("CHAR ");
            actual.text(ascii_name(state.captured_ascii));
            actual.text(" ASCII ");
            if let Some(value) = state.captured_ascii {
                actual.number(u16::from(value));
            } else {
                actual.text("NONE");
            }
            canvas.draw_text(16, 99, actual.as_str(), color::TEXT, 1);
            canvas.draw_text(
                16,
                114,
                if state.captured_matches {
                    "MATCH"
                } else {
                    "MISMATCH"
                },
                if state.captured_matches {
                    color::SUCCESS
                } else {
                    color::DANGER
                },
                1,
            );
            canvas.draw_text(16, 135, "ENTER CONFIRM", color::ACCENT, 1);
            canvas.draw_text(173, 135, "BACKSPACE RETRY", color::MUTED, 1);
        }
        Phase::Complete => unreachable!(),
    }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let pixels = frame();
    let mut state = Diagnostics::new();
    save(&mut state);
    let mut dirty = true;
    loop {
        if dirty {
            render(&state, pixels);
            if display::present_rgb565(pixels, &[]).is_err() {
                return 1;
            }
            dirty = false;
        }
        match input::poll_key_event(250) {
            Ok(Some(event)) => {
                if state.handle(event) {
                    save(&mut state);
                    dirty = true;
                }
            }
            Ok(None) => {}
            Err(_) => {
                state.log.text("X,input_poll\n");
                save(&mut state);
                return 1;
            }
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

    fn event(code: u16, modifiers: u8, character: Option<u8>) -> KeyEvent {
        KeyEvent {
            code,
            pressed: true,
            repeated: false,
            modifiers,
            character,
        }
    }

    #[test]
    fn sym_table_matches_the_32_documented_combinations() {
        let expected = b"!@#$%^&*()~`_-+=[]{};:'\"<>\\|,./?";
        assert_eq!(TESTS.len(), 37);
        for (case, expected) in TESTS[5..].iter().zip(expected) {
            assert_eq!(case.ascii, *expected);
        }
    }

    #[test]
    fn diagnostics_uses_the_system_character() {
        let mut state = Diagnostics::new();
        assert!(state.handle(event(KEY_A, 0, Some(b'x'))));
        assert_eq!(state.captured_ascii, Some(b'x'));
        assert!(!state.captured_matches);
    }

    #[test]
    fn review_requires_explicit_confirmation_or_retry() {
        let mut state = Diagnostics::new();
        assert!(state.handle(event(KEY_A, 0, Some(b'a'))));
        assert_eq!(state.phase, Phase::Review);
        assert!(state.captured_matches);

        assert!(state.handle(event(KEY_BACKSPACE, 0, None)));
        assert_eq!(state.phase, Phase::Waiting);
        assert_eq!(state.index, 0);

        assert!(state.handle(event(KEY_A, 0, Some(b'a'))));
        assert!(state.handle(event(KEY_ENTER, 0, None)));
        assert_eq!(state.phase, Phase::Waiting);
        assert_eq!((state.index, state.confirmed, state.passed), (1, 1, 1));
    }

    #[test]
    fn modifier_events_are_logged_but_not_captured() {
        let mut state = Diagnostics::new();
        assert!(!state.handle(event(KEY_LEFTSHIFT, MODIFIER_SHIFT, None)));
        assert_eq!(state.phase, Phase::Waiting);
        assert!(state.captured.is_none());
        assert!(
            core::str::from_utf8(state.log.slice())
                .unwrap()
                .contains(",42,1,0,1")
        );
    }

    #[test]
    fn mismatch_is_preserved_after_user_confirmation() {
        let mut state = Diagnostics::new();
        assert!(state.handle(event(KEY_A, MODIFIER_SHIFT, Some(b'A'))));
        assert!(!state.captured_matches);
        assert!(state.handle(event(KEY_ENTER, 0, None)));
        assert_eq!((state.confirmed, state.passed), (1, 0));
        assert!(
            core::str::from_utf8(state.log.slice())
                .unwrap()
                .contains("C,1,30,1,65,0")
        );
    }

    #[test]
    fn complete_physical_event_sequence_fits_one_storage_value() {
        let mut state = Diagnostics::new();
        for case in TESTS {
            if case.shifted {
                state.handle(event(KEY_LEFTSHIFT, MODIFIER_SHIFT, None));
            }
            assert!(state.handle(event(
                case.code,
                if case.shifted { MODIFIER_SHIFT } else { 0 },
                Some(case.ascii),
            )));
            state.handle(KeyEvent {
                code: case.code,
                pressed: false,
                repeated: false,
                modifiers: if case.shifted { MODIFIER_SHIFT } else { 0 },
                character: None,
            });
            if case.shifted {
                state.handle(KeyEvent {
                    code: KEY_LEFTSHIFT,
                    pressed: false,
                    repeated: false,
                    modifiers: 0,
                    character: None,
                });
            }
            assert!(state.handle(event(KEY_ENTER, 0, None)));
            state.handle(KeyEvent {
                code: KEY_ENTER,
                pressed: false,
                repeated: false,
                modifiers: 0,
                character: None,
            });
        }
        assert_eq!(state.phase, Phase::Complete);
        assert_eq!((state.confirmed, state.passed), (37, 37));
        assert!(!state.log.truncated);
        assert!(state.log.len < LOG_BYTES);
        assert!(
            core::str::from_utf8(state.log.slice())
                .unwrap()
                .contains("D,37,37,37,0")
        );
    }
}
