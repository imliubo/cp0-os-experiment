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

const KEY_1: u16 = 2;
const KEY_0: u16 = 11;
const KEY_MINUS: u16 = 12;
const KEY_EQUAL: u16 = 13;
const KEY_BACKSPACE: u16 = 14;
const KEY_Q: u16 = 16;
const KEY_W: u16 = 17;
const KEY_E: u16 = 18;
const KEY_R: u16 = 19;
const KEY_T: u16 = 20;
const KEY_Y: u16 = 21;
const KEY_U: u16 = 22;
const KEY_I: u16 = 23;
const KEY_O: u16 = 24;
const KEY_P: u16 = 25;
const KEY_ENTER: u16 = 28;
const KEY_A: u16 = 30;
const KEY_S: u16 = 31;
const KEY_D: u16 = 32;
const KEY_G: u16 = 34;
const KEY_H: u16 = 35;
const KEY_J: u16 = 36;
const KEY_K: u16 = 37;
const KEY_L: u16 = 38;
const KEY_SEMICOLON: u16 = 39;
const KEY_APOSTROPHE: u16 = 40;
const KEY_GRAVE: u16 = 41;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_BACKSLASH: u16 = 43;
const KEY_Z: u16 = 44;
const KEY_V: u16 = 47;
const KEY_B: u16 = 48;
const KEY_N: u16 = 49;
const KEY_M: u16 = 50;
const KEY_COMMA: u16 = 51;
const KEY_DOT: u16 = 52;
const KEY_SLASH: u16 = 53;
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
    test("HOLD SYM + 1", "EXCLAMATION", KEY_1, true, b'!'),
    test("HOLD SYM + 2", "AT SIGN", KEY_1 + 1, true, b'@'),
    test("HOLD SYM + 3", "HASH", KEY_1 + 2, true, b'#'),
    test("HOLD SYM + 4", "DOLLAR", KEY_1 + 3, true, b'$'),
    test("HOLD SYM + 5", "PERCENT", KEY_1 + 4, true, b'%'),
    test("HOLD SYM + 6", "CARET", KEY_1 + 5, true, b'^'),
    test("HOLD SYM + 7", "AMPERSAND", KEY_1 + 6, true, b'&'),
    test("HOLD SYM + 8", "ASTERISK", KEY_1 + 7, true, b'*'),
    test("HOLD SYM + 9", "LEFT PAREN", KEY_1 + 8, true, b'('),
    test("HOLD SYM + 0", "RIGHT PAREN", KEY_0, true, b')'),
    test("HOLD SYM + Q", "TILDE", KEY_GRAVE, true, b'~'),
    test("HOLD SYM + W", "BACKTICK", KEY_GRAVE, false, b'`'),
    test("HOLD SYM + E", "UNDERSCORE", KEY_MINUS, true, b'_'),
    test("HOLD SYM + R", "MINUS", KEY_MINUS, false, b'-'),
    test("HOLD SYM + T", "PLUS", KEY_EQUAL, true, b'+'),
    test("HOLD SYM + Y", "EQUAL", KEY_EQUAL, false, b'='),
    test("HOLD SYM + U", "LEFT BRACKET", 26, false, b'['),
    test("HOLD SYM + I", "RIGHT BRACKET", 27, false, b']'),
    test("HOLD SYM + O", "LEFT BRACE", 26, true, b'{'),
    test("HOLD SYM + P", "RIGHT BRACE", 27, true, b'}'),
    test("HOLD SYM + A", "SEMICOLON", KEY_SEMICOLON, false, b';'),
    test("HOLD SYM + S", "COLON", KEY_SEMICOLON, true, b':'),
    test("HOLD SYM + D", "APOSTROPHE", KEY_APOSTROPHE, false, b'\''),
    test("HOLD SYM + G", "DOUBLE QUOTE", KEY_APOSTROPHE, true, b'\"'),
    test("HOLD SYM + H", "LESS THAN", KEY_COMMA, true, b'<'),
    test("HOLD SYM + J", "GREATER THAN", KEY_DOT, true, b'>'),
    test("HOLD SYM + K", "BACKSLASH", KEY_BACKSLASH, false, b'\\'),
    test("HOLD SYM + L", "PIPE", KEY_BACKSLASH, true, b'|'),
    test("HOLD SYM + V", "COMMA", KEY_COMMA, false, b','),
    test("HOLD SYM + B", "DOT", KEY_DOT, false, b'.'),
    test("HOLD SYM + N", "SLASH", KEY_SLASH, false, b'/'),
    test("HOLD SYM + M", "QUESTION", KEY_SLASH, true, b'?'),
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
        let actual_ascii = us_ascii(event.code, event.modifiers);
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

fn us_ascii(code: u16, modifiers: u8) -> Option<u8> {
    let shift = modifiers & MODIFIER_SHIFT != 0;
    let value = match code {
        KEY_1..=KEY_0 => {
            const PLAIN: &[u8; 10] = b"1234567890";
            const SHIFTED: &[u8; 10] = b"!@#$%^&*()";
            let index = usize::from(code - KEY_1);
            if shift { SHIFTED[index] } else { PLAIN[index] }
        }
        KEY_Q => letter(b'q', shift),
        KEY_W => letter(b'w', shift),
        KEY_E => letter(b'e', shift),
        KEY_R => letter(b'r', shift),
        KEY_T => letter(b't', shift),
        KEY_Y => letter(b'y', shift),
        KEY_U => letter(b'u', shift),
        KEY_I => letter(b'i', shift),
        KEY_O => letter(b'o', shift),
        KEY_P => letter(b'p', shift),
        KEY_A => letter(b'a', shift),
        KEY_S => letter(b's', shift),
        KEY_D => letter(b'd', shift),
        33 => letter(b'f', shift),
        KEY_G => letter(b'g', shift),
        KEY_H => letter(b'h', shift),
        KEY_J => letter(b'j', shift),
        KEY_K => letter(b'k', shift),
        KEY_L => letter(b'l', shift),
        KEY_Z => letter(b'z', shift),
        45 => letter(b'x', shift),
        46 => letter(b'c', shift),
        KEY_V => letter(b'v', shift),
        KEY_B => letter(b'b', shift),
        KEY_N => letter(b'n', shift),
        KEY_M => letter(b'm', shift),
        KEY_MINUS => {
            if shift {
                b'_'
            } else {
                b'-'
            }
        }
        KEY_EQUAL => {
            if shift {
                b'+'
            } else {
                b'='
            }
        }
        26 => {
            if shift {
                b'{'
            } else {
                b'['
            }
        }
        27 => {
            if shift {
                b'}'
            } else {
                b']'
            }
        }
        KEY_SEMICOLON => {
            if shift {
                b':'
            } else {
                b';'
            }
        }
        KEY_APOSTROPHE => {
            if shift {
                b'\"'
            } else {
                b'\''
            }
        }
        KEY_GRAVE => {
            if shift {
                b'~'
            } else {
                b'`'
            }
        }
        KEY_BACKSLASH => {
            if shift {
                b'|'
            } else {
                b'\\'
            }
        }
        KEY_COMMA => {
            if shift {
                b'<'
            } else {
                b','
            }
        }
        KEY_DOT => {
            if shift {
                b'>'
            } else {
                b'.'
            }
        }
        KEY_SLASH => {
            if shift {
                b'?'
            } else {
                b'/'
            }
        }
        _ => return None,
    };
    Some(value)
}

const fn letter(lowercase: u8, shifted: bool) -> u8 {
    if shifted { lowercase - 32 } else { lowercase }
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

    fn event(code: u16, modifiers: u8) -> KeyEvent {
        KeyEvent {
            code,
            pressed: true,
            repeated: false,
            modifiers,
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
    fn case_mapping_uses_only_the_shift_modifier() {
        assert_eq!(us_ascii(KEY_A, 0), Some(b'a'));
        assert_eq!(us_ascii(KEY_A, MODIFIER_SHIFT), Some(b'A'));
        assert_eq!(us_ascii(KEY_Z, 0), Some(b'z'));
        assert_eq!(us_ascii(KEY_Z, MODIFIER_SHIFT), Some(b'Z'));
    }

    #[test]
    fn review_requires_explicit_confirmation_or_retry() {
        let mut state = Diagnostics::new();
        assert!(state.handle(event(KEY_A, 0)));
        assert_eq!(state.phase, Phase::Review);
        assert!(state.captured_matches);

        assert!(state.handle(event(KEY_BACKSPACE, 0)));
        assert_eq!(state.phase, Phase::Waiting);
        assert_eq!(state.index, 0);

        assert!(state.handle(event(KEY_A, 0)));
        assert!(state.handle(event(KEY_ENTER, 0)));
        assert_eq!(state.phase, Phase::Waiting);
        assert_eq!((state.index, state.confirmed, state.passed), (1, 1, 1));
    }

    #[test]
    fn modifier_events_are_logged_but_not_captured() {
        let mut state = Diagnostics::new();
        assert!(!state.handle(event(KEY_LEFTSHIFT, MODIFIER_SHIFT)));
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
        assert!(state.handle(event(KEY_A, MODIFIER_SHIFT)));
        assert!(!state.captured_matches);
        assert!(state.handle(event(KEY_ENTER, 0)));
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
                state.handle(event(KEY_LEFTSHIFT, MODIFIER_SHIFT));
            }
            assert!(state.handle(event(
                case.code,
                if case.shifted { MODIFIER_SHIFT } else { 0 },
            )));
            state.handle(KeyEvent {
                code: case.code,
                pressed: false,
                repeated: false,
                modifiers: if case.shifted { MODIFIER_SHIFT } else { 0 },
            });
            if case.shifted {
                state.handle(KeyEvent {
                    code: KEY_LEFTSHIFT,
                    pressed: false,
                    repeated: false,
                    modifiers: 0,
                });
            }
            assert!(state.handle(event(KEY_ENTER, 0)));
            state.handle(KeyEvent {
                code: KEY_ENTER,
                pressed: false,
                repeated: false,
                modifiers: 0,
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
