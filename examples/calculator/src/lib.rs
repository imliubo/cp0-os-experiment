#![no_std]

#[cfg(not(test))]
use core::panic::PanicInfo;
use cp0_sdk::{
    display::{self, Rect},
    input,
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
}

impl Calculator {
    const fn new() -> Self {
        Self {
            value: 0,
            accumulator: 0,
            pending: 0,
            entering: false,
            error: false,
        }
    }

    fn key(&mut self, code: u16, modifiers: u8) {
        if let Some(operator) = operator_for_key(code, modifiers) {
            if operator == KEY_EQUAL {
                self.apply();
                self.pending = 0;
            } else {
                self.apply();
                self.pending = operator;
                self.entering = false;
            }
            return;
        }
        if let Some(digit) = digit_for_key(code, modifiers) {
            if self.error || !self.entering {
                self.value = 0;
                self.error = false;
            }
            self.value = self.value.saturating_mul(10).saturating_add(digit);
            self.value = self.value.min(999_999_999);
            self.entering = true;
            return;
        }
        match code {
            46 => *self = Self::new(),
            14 => {
                self.value /= 10;
                self.entering = true;
            }
            28 | 96 => {
                self.apply();
                self.pending = 0;
            }
            _ => {}
        }
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

const KEY_8: u16 = 9;
const KEY_MINUS: u16 = 12;
const KEY_EQUAL: u16 = 13;
const KEY_SLASH: u16 = 53;
const KEY_KPASTERISK: u16 = 55;
const KEY_KPMINUS: u16 = 74;
const KEY_KPPLUS: u16 = 78;
const KEY_KPSLASH: u16 = 98;

fn shifted(modifiers: u8) -> bool {
    modifiers & input::MODIFIER_SHIFT != 0
}

fn operator_for_key(code: u16, modifiers: u8) -> Option<u16> {
    match (code, shifted(modifiers)) {
        (KEY_EQUAL, true) | (KEY_KPPLUS, _) => Some(KEY_KPPLUS),
        (KEY_MINUS, false) | (KEY_KPMINUS, _) => Some(KEY_KPMINUS),
        (KEY_8, true) | (KEY_KPASTERISK, _) => Some(KEY_KPASTERISK),
        (KEY_SLASH, false) | (KEY_KPSLASH, _) => Some(KEY_KPSLASH),
        (KEY_EQUAL, false) => Some(KEY_EQUAL),
        _ => None,
    }
}

fn digit_for_key(code: u16, modifiers: u8) -> Option<i32> {
    if shifted(modifiers) {
        return None;
    }
    match code {
        11 => Some(0),
        2..=10 => Some(i32::from(code - 1)),
        _ => None,
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
        }
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
                calculator.key(event.code, event.modifiers);
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
        let shift = input::MODIFIER_SHIFT;
        let mut calculator = Calculator::new();

        calculator.key(8, 0);
        calculator.key(KEY_8, shift);
        calculator.key(7, 0);
        calculator.key(KEY_EQUAL, 0);
        assert_eq!(calculator.value, 42);

        calculator.key(KEY_MINUS, 0);
        calculator.key(3, 0);
        calculator.key(KEY_EQUAL, 0);
        assert_eq!(calculator.value, 40);

        calculator.key(KEY_SLASH, 0);
        calculator.key(5, 0);
        calculator.key(KEY_EQUAL, 0);
        assert_eq!(calculator.value, 10);

        calculator.key(KEY_EQUAL, shift);
        calculator.key(2, 0);
        calculator.key(KEY_EQUAL, 0);
        assert_eq!(calculator.value, 11);
    }

    #[test]
    fn shifted_number_is_not_entered_as_a_digit() {
        assert_eq!(digit_for_key(KEY_8, input::MODIFIER_SHIFT), None);
        assert_eq!(
            operator_for_key(KEY_8, input::MODIFIER_SHIFT),
            Some(KEY_KPASTERISK)
        );
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_information: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
