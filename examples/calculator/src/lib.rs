#![no_std]

#[cfg(not(test))]
use core::panic::PanicInfo;
#[cfg(not(test))]
use cp0_sdk::input;
use cp0_sdk::{
    display::{self, Rect},
    ui::{ButtonStyle, Canvas, color},
};

const FRAME_BYTES: usize = display::WIDTH as usize * display::STANDARD_HEIGHT as usize * 2;
static mut FRAME: [u8; FRAME_BYTES] = [0; FRAME_BYTES];

struct Calculator {
    value: i32,
    accumulator: i32,
    pending: u16,
    entering: bool,
    error: bool,
    selection: u8,
    selection_active: bool,
}

impl Calculator {
    const fn new() -> Self {
        Self {
            value: 0,
            accumulator: 0,
            pending: 0,
            entering: false,
            error: false,
            selection: BUTTON_EQUAL,
            selection_active: false,
        }
    }

    fn key(&mut self, code: u16, character: Option<u8>) {
        if self.navigate(code) {
            return;
        }
        if code == KEY_SPACE
            || ((code == KEY_ENTER || code == KEY_KPENTER) && self.selection_active)
        {
            self.activate_selection();
            self.selection_active = false;
            return;
        }
        if let Some(operator) = operator_for_key(code, character) {
            self.selection_active = false;
            if operator == KEY_EQUAL {
                self.apply();
                self.pending = 0;
            } else {
                self.select_operator(operator);
            }
            return;
        }
        if let Some(digit) = digit_for_character(character) {
            self.selection_active = false;
            self.enter_digit(digit);
            return;
        }
        match code {
            46 => *self = Self::new(),
            14 => {
                self.selection_active = false;
                self.value /= 10;
                self.entering = true;
            }
            KEY_ENTER | KEY_KPENTER => {
                self.selection_active = false;
                self.apply();
                self.pending = 0;
            }
            _ => {}
        }
    }

    fn navigate(&mut self, code: u16) -> bool {
        let row = self.selection / BUTTON_COLUMNS;
        let column = self.selection % BUTTON_COLUMNS;
        self.selection = match code {
            KEY_LEFT => row * BUTTON_COLUMNS + (column + BUTTON_COLUMNS - 1) % BUTTON_COLUMNS,
            KEY_RIGHT => row * BUTTON_COLUMNS + (column + 1) % BUTTON_COLUMNS,
            KEY_UP => ((row + BUTTON_ROWS - 1) % BUTTON_ROWS) * BUTTON_COLUMNS + column,
            KEY_DOWN => ((row + 1) % BUTTON_ROWS) * BUTTON_COLUMNS + column,
            _ => return false,
        };
        self.selection_active = true;
        true
    }

    fn activate_selection(&mut self) {
        match self.selection {
            0 => self.enter_digit(7),
            1 => self.enter_digit(8),
            2 => self.enter_digit(9),
            BUTTON_DIVIDE => self.select_operator(KEY_KPSLASH),
            4 => self.enter_digit(4),
            5 => self.enter_digit(5),
            6 => self.enter_digit(6),
            BUTTON_MULTIPLY => self.select_operator(KEY_KPASTERISK),
            8 => self.enter_digit(1),
            9 => self.enter_digit(2),
            10 => self.enter_digit(3),
            BUTTON_SUBTRACT => self.select_operator(KEY_KPMINUS),
            BUTTON_CLEAR => *self = Self::new(),
            13 => self.enter_digit(0),
            BUTTON_EQUAL => {
                self.apply();
                self.pending = 0;
            }
            BUTTON_ADD => self.select_operator(KEY_KPPLUS),
            _ => {}
        }
    }

    fn enter_digit(&mut self, digit: i32) {
        if self.error || !self.entering {
            self.value = 0;
            self.error = false;
        }
        self.value = self.value.saturating_mul(10).saturating_add(digit);
        self.value = self.value.min(999_999_999);
        self.entering = true;
    }

    fn select_operator(&mut self, operator: u16) {
        self.apply();
        self.pending = operator;
        self.entering = false;
    }

    fn apply(&mut self) {
        if self.error {
            return;
        }
        if self.pending == 0 {
            self.accumulator = self.value;
        } else {
            self.accumulator = match self.pending {
                12 | 74 => self.accumulator.saturating_sub(self.value),
                13 | 78 => self.accumulator.saturating_add(self.value),
                55 => self.accumulator.saturating_mul(self.value),
                98 if self.value != 0 => self.accumulator / self.value,
                98 => {
                    self.error = true;
                    0
                }
                _ => self.value,
            };
            self.value = self.accumulator;
        }
        self.entering = false;
    }
}

const KEY_EQUAL: u16 = 13;
const KEY_KPASTERISK: u16 = 55;
const KEY_KPMINUS: u16 = 74;
const KEY_KPPLUS: u16 = 78;
const KEY_KPSLASH: u16 = 98;
const KEY_KPENTER: u16 = 96;
const KEY_ENTER: u16 = 28;
const KEY_SPACE: u16 = 57;
const KEY_UP: u16 = 103;
const KEY_LEFT: u16 = 105;
const KEY_RIGHT: u16 = 106;
const KEY_DOWN: u16 = 108;

const BUTTON_COLUMNS: u8 = 4;
const BUTTON_ROWS: u8 = 4;
const BUTTON_DIVIDE: u8 = 3;
const BUTTON_MULTIPLY: u8 = 7;
const BUTTON_SUBTRACT: u8 = 11;
const BUTTON_CLEAR: u8 = 12;
const BUTTON_EQUAL: u8 = 14;
const BUTTON_ADD: u8 = 15;

fn operator_for_key(code: u16, character: Option<u8>) -> Option<u16> {
    match (code, character) {
        (KEY_KPPLUS, _) | (_, Some(b'+')) => Some(KEY_KPPLUS),
        (KEY_KPMINUS, _) | (_, Some(b'-')) => Some(KEY_KPMINUS),
        (KEY_KPASTERISK, _) | (_, Some(b'*')) => Some(KEY_KPASTERISK),
        (KEY_KPSLASH, _) | (_, Some(b'/')) => Some(KEY_KPSLASH),
        (KEY_EQUAL, _) | (_, Some(b'=')) => Some(KEY_EQUAL),
        _ => None,
    }
}

fn digit_for_character(character: Option<u8>) -> Option<i32> {
    character
        .filter(u8::is_ascii_digit)
        .map(|byte| i32::from(byte - b'0'))
}

fn operator_label(operator: u16) -> &'static str {
    match operator {
        KEY_KPMINUS => "-",
        KEY_KPPLUS => "+",
        KEY_KPASTERISK => "*",
        KEY_KPSLASH => "/",
        _ => "",
    }
}

fn frame() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(FRAME).cast(), FRAME_BYTES) }
}

fn render(calculator: &Calculator, pixels: &mut [u8]) {
    let mut canvas = Canvas::new(pixels, display::WIDTH, display::STANDARD_HEIGHT).unwrap();
    canvas.clear(color::BACKGROUND);
    canvas.fill_rect(
        Rect {
            x: 8,
            y: 7,
            width: 304,
            height: 39,
        },
        color::SURFACE,
    );
    canvas.stroke_rect(
        Rect {
            x: 8,
            y: 7,
            width: 304,
            height: 39,
        },
        color::ACCENT,
    );
    let mut number = [0_u8; 12];
    let text = if calculator.error {
        "ERROR"
    } else {
        format_i32(calculator.value, &mut number)
    };
    let text_width = text.len() as u16 * 12;
    canvas.draw_text(
        302_u16.saturating_sub(text_width),
        18,
        text,
        if calculator.error {
            color::DANGER
        } else {
            color::TEXT
        },
        2,
    );
    if calculator.pending != 0 {
        canvas.draw_text(18, 18, operator_label(calculator.pending), color::ACCENT, 2);
    }

    let labels = [
        ["7", "8", "9", "/"],
        ["4", "5", "6", "*"],
        ["1", "2", "3", "-"],
        ["C", "0", "=", "+"],
    ];
    for (row, labels) in labels.iter().enumerate() {
        for (column, label) in labels.iter().enumerate() {
            let operator = column == 3 || *label == "=";
            canvas.button(
                Rect {
                    x: 8 + column as u16 * 77,
                    y: 52 + row as u16 * 24,
                    width: 72,
                    height: 20,
                },
                label,
                if *label == "C" {
                    ButtonStyle::DANGER
                } else if operator {
                    ButtonStyle::PRIMARY
                } else {
                    ButtonStyle::SECONDARY
                },
            );
            let button = (row * usize::from(BUTTON_COLUMNS) + column) as u8;
            if (calculator.selection_active && calculator.selection == button)
                || calculator.pending != 0 && operator_for_button(button) == calculator.pending
            {
                canvas.stroke_rect(
                    Rect {
                        x: 7 + column as u16 * 77,
                        y: 51 + row as u16 * 24,
                        width: 74,
                        height: 22,
                    },
                    color::ACCENT,
                );
            }
        }
    }
}

fn operator_for_button(button: u8) -> u16 {
    match button {
        BUTTON_DIVIDE => KEY_KPSLASH,
        BUTTON_MULTIPLY => KEY_KPASTERISK,
        BUTTON_SUBTRACT => KEY_KPMINUS,
        BUTTON_ADD => KEY_KPPLUS,
        _ => 0,
    }
}

fn format_i32(value: i32, buffer: &mut [u8; 12]) -> &str {
    let negative = value < 0;
    let mut magnitude = i64::from(value).unsigned_abs();
    let mut cursor = buffer.len();
    if magnitude == 0 {
        cursor -= 1;
        buffer[cursor] = b'0';
    }
    while magnitude > 0 && cursor > usize::from(negative) {
        cursor -= 1;
        buffer[cursor] = b'0' + (magnitude % 10) as u8;
        magnitude /= 10;
    }
    if negative {
        cursor -= 1;
        buffer[cursor] = b'-';
    }
    unsafe { core::str::from_utf8_unchecked(&buffer[cursor..]) }
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main() -> i32 {
    let pixels = frame();
    let mut calculator = Calculator::new();
    let mut dirty = true;
    loop {
        if dirty {
            render(&calculator, pixels);
            if display::present_rgb565(pixels, &[]).is_ok() {
                dirty = false;
            }
        }
        match input::poll_key_event(250) {
            Ok(Some(event)) if event.pressed => {
                calculator.key(event.code, event.character);
                dirty = true;
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
    fn cardputer_symbol_keys_drive_all_operations() {
        let mut calculator = Calculator::new();

        calculator.key(8, Some(b'7'));
        calculator.key(9, Some(b'*'));
        calculator.key(7, Some(b'6'));
        calculator.key(KEY_EQUAL, Some(b'='));
        assert_eq!(calculator.value, 42);

        calculator.key(12, Some(b'-'));
        calculator.key(3, Some(b'2'));
        calculator.key(KEY_EQUAL, Some(b'='));
        assert_eq!(calculator.value, 40);

        calculator.key(53, Some(b'/'));
        calculator.key(5, Some(b'4'));
        calculator.key(KEY_EQUAL, Some(b'='));
        assert_eq!(calculator.value, 10);

        calculator.key(KEY_EQUAL, Some(b'+'));
        calculator.key(2, Some(b'1'));
        calculator.key(KEY_EQUAL, Some(b'='));
        assert_eq!(calculator.value, 11);
    }

    #[test]
    fn system_symbol_is_not_entered_as_a_digit() {
        assert_eq!(digit_for_character(Some(b'*')), None);
        assert_eq!(operator_for_key(9, Some(b'*')), Some(KEY_KPASTERISK));
    }

    #[test]
    fn arrow_navigation_enters_operators_without_symbol_layer() {
        let mut calculator = Calculator::new();

        calculator.key(2, Some(b'1'));
        calculator.key(KEY_RIGHT, None);
        calculator.key(KEY_ENTER, None);
        assert_eq!(calculator.pending, KEY_KPPLUS);
        assert_eq!(operator_label(calculator.pending), "+");

        calculator.key(3, Some(b'2'));
        calculator.key(KEY_ENTER, None);
        assert_eq!(calculator.value, 3);
        assert_eq!(calculator.pending, 0);
    }

    #[test]
    fn on_screen_keypad_reaches_every_operation() {
        let mut calculator = Calculator::new();

        for (button, operator) in [
            (BUTTON_DIVIDE, KEY_KPSLASH),
            (BUTTON_MULTIPLY, KEY_KPASTERISK),
            (BUTTON_SUBTRACT, KEY_KPMINUS),
            (BUTTON_ADD, KEY_KPPLUS),
        ] {
            calculator.selection = button;
            calculator.activate_selection();
            assert_eq!(calculator.pending, operator);
            assert_eq!(operator_for_button(button), operator);
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
