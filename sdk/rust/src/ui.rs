use crate::{Error, display::Rect};

#[derive(Debug)]
pub struct Canvas<'a> {
    pixels: &'a mut [u8],
    width: u16,
    height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonStyle {
    pub background: u16,
    pub foreground: u16,
    pub border: u16,
}

pub mod color {
    pub const BACKGROUND: u16 = 0x0861;
    pub const SURFACE: u16 = 0x2104;
    pub const SURFACE_RAISED: u16 = 0x39c7;
    pub const TEXT: u16 = 0xffff;
    pub const MUTED: u16 = 0x9cf3;
    pub const ACCENT: u16 = 0x05ff;
    pub const SUCCESS: u16 = 0x3666;
    pub const WARNING: u16 = 0xfd20;
    pub const DANGER: u16 = 0xe943;
}

impl ButtonStyle {
    pub const PRIMARY: Self = Self {
        background: color::ACCENT,
        foreground: 0x0021,
        border: color::TEXT,
    };

    pub const SECONDARY: Self = Self {
        background: color::SURFACE_RAISED,
        foreground: color::TEXT,
        border: color::MUTED,
    };

    pub const DANGER: Self = Self {
        background: color::DANGER,
        foreground: color::TEXT,
        border: color::WARNING,
    };
}

impl<'a> Canvas<'a> {
    pub fn new(pixels: &'a mut [u8], width: u16, height: u16) -> Result<Self, Error> {
        let expected = usize::from(width)
            .checked_mul(usize::from(height))
            .and_then(|count| count.checked_mul(2))
            .ok_or(Error::InvalidArgument)?;
        if width == 0 || height == 0 || pixels.len() != expected {
            return Err(Error::InvalidArgument);
        }
        Ok(Self {
            pixels,
            width,
            height,
        })
    }

    pub const fn width(&self) -> u16 {
        self.width
    }

    pub const fn height(&self) -> u16 {
        self.height
    }

    pub fn clear(&mut self, color: u16) {
        self.fill_rect(
            Rect {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            },
            color,
        );
    }

    pub fn pixel(&mut self, x: u16, y: u16, color: u16) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (usize::from(y) * usize::from(self.width) + usize::from(x)) * 2;
        let encoded = color.to_le_bytes();
        self.pixels[offset] = encoded[0];
        self.pixels[offset + 1] = encoded[1];
    }

    pub fn fill_rect(&mut self, rectangle: Rect, color: u16) {
        let right = u32::from(rectangle.x)
            .saturating_add(u32::from(rectangle.width))
            .min(u32::from(self.width));
        let bottom = u32::from(rectangle.y)
            .saturating_add(u32::from(rectangle.height))
            .min(u32::from(self.height));
        let encoded = color.to_le_bytes();
        for y in u32::from(rectangle.y).min(u32::from(self.height))..bottom {
            for x in u32::from(rectangle.x).min(u32::from(self.width))..right {
                let offset = (y as usize * usize::from(self.width) + x as usize) * 2;
                self.pixels[offset] = encoded[0];
                self.pixels[offset + 1] = encoded[1];
            }
        }
    }

    pub fn stroke_rect(&mut self, rectangle: Rect, color: u16) {
        if rectangle.width == 0 || rectangle.height == 0 {
            return;
        }
        self.fill_rect(
            Rect {
                height: 1,
                ..rectangle
            },
            color,
        );
        self.fill_rect(
            Rect {
                y: rectangle.y.saturating_add(rectangle.height - 1),
                height: 1,
                ..rectangle
            },
            color,
        );
        self.fill_rect(
            Rect {
                width: 1,
                ..rectangle
            },
            color,
        );
        self.fill_rect(
            Rect {
                x: rectangle.x.saturating_add(rectangle.width - 1),
                width: 1,
                ..rectangle
            },
            color,
        );
    }

    pub fn draw_text(&mut self, x: u16, y: u16, text: &str, color: u16, scale: u8) {
        if !(1..=4).contains(&scale) {
            return;
        }
        let scale = u16::from(scale);
        let mut cursor = x;
        for character in text.chars() {
            let glyph = glyph(character);
            for (column, bits) in glyph.iter().enumerate() {
                for row in 0..7_u16 {
                    if bits & (1 << row) != 0 {
                        self.fill_rect(
                            Rect {
                                x: cursor.saturating_add(column as u16 * scale),
                                y: y.saturating_add(row * scale),
                                width: scale,
                                height: scale,
                            },
                            color,
                        );
                    }
                }
            }
            cursor = cursor.saturating_add(6 * scale);
            if cursor >= self.width {
                break;
            }
        }
    }

    pub fn button(&mut self, rectangle: Rect, label: &str, style: ButtonStyle) {
        self.fill_rect(rectangle, style.background);
        self.stroke_rect(rectangle, style.border);
        let text_width = label.chars().count().saturating_mul(6) as u16;
        let x = rectangle
            .x
            .saturating_add(rectangle.width.saturating_sub(text_width) / 2);
        let y = rectangle
            .y
            .saturating_add(rectangle.height.saturating_sub(7) / 2);
        self.draw_text(x, y, label, style.foreground, 1);
    }

    pub fn progress(&mut self, rectangle: Rect, value: u16, maximum: u16) {
        self.fill_rect(rectangle, color::SURFACE);
        self.stroke_rect(rectangle, color::MUTED);
        if maximum == 0 || rectangle.width <= 2 || rectangle.height <= 2 {
            return;
        }
        let width =
            u32::from(rectangle.width - 2) * u32::from(value.min(maximum)) / u32::from(maximum);
        self.fill_rect(
            Rect {
                x: rectangle.x.saturating_add(1),
                y: rectangle.y.saturating_add(1),
                width: width as u16,
                height: rectangle.height - 2,
            },
            color::SUCCESS,
        );
    }
}

pub const fn rgb565(red: u8, green: u8, blue: u8) -> u16 {
    (((red as u16) >> 3) << 11) | (((green as u16) >> 2) << 5) | ((blue as u16) >> 3)
}

const FIRST_PRINTABLE_ASCII: u32 = 0x20;
const LAST_PRINTABLE_ASCII: u32 = 0x7e;
const UNKNOWN_GLYPH: [u8; 5] = [0x02, 0x01, 0x51, 0x09, 0x06];

// Column-major 5x7 printable ASCII, indexed from space (0x20) through '~'.
const FONT: [[u8; 5]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00],
    [0x00, 0x00, 0x5f, 0x00, 0x00],
    [0x00, 0x07, 0x00, 0x07, 0x00],
    [0x14, 0x7f, 0x14, 0x7f, 0x14],
    [0x24, 0x2a, 0x7f, 0x2a, 0x12],
    [0x23, 0x13, 0x08, 0x64, 0x62],
    [0x36, 0x49, 0x55, 0x22, 0x50],
    [0x00, 0x05, 0x03, 0x00, 0x00],
    [0x00, 0x1c, 0x22, 0x41, 0x00],
    [0x00, 0x41, 0x22, 0x1c, 0x00],
    [0x14, 0x08, 0x3e, 0x08, 0x14],
    [0x08, 0x08, 0x3e, 0x08, 0x08],
    [0x00, 0x50, 0x30, 0x00, 0x00],
    [0x08, 0x08, 0x08, 0x08, 0x08],
    [0x00, 0x60, 0x60, 0x00, 0x00],
    [0x20, 0x10, 0x08, 0x04, 0x02],
    [0x3e, 0x51, 0x49, 0x45, 0x3e],
    [0x00, 0x42, 0x7f, 0x40, 0x00],
    [0x42, 0x61, 0x51, 0x49, 0x46],
    [0x21, 0x41, 0x45, 0x4b, 0x31],
    [0x18, 0x14, 0x12, 0x7f, 0x10],
    [0x27, 0x45, 0x45, 0x45, 0x39],
    [0x3c, 0x4a, 0x49, 0x49, 0x30],
    [0x01, 0x71, 0x09, 0x05, 0x03],
    [0x36, 0x49, 0x49, 0x49, 0x36],
    [0x06, 0x49, 0x49, 0x29, 0x1e],
    [0x00, 0x36, 0x36, 0x00, 0x00],
    [0x00, 0x56, 0x36, 0x00, 0x00],
    [0x08, 0x14, 0x22, 0x41, 0x00],
    [0x14, 0x14, 0x14, 0x14, 0x14],
    [0x00, 0x41, 0x22, 0x14, 0x08],
    [0x02, 0x01, 0x51, 0x09, 0x06],
    [0x32, 0x49, 0x79, 0x41, 0x3e],
    [0x7e, 0x11, 0x11, 0x11, 0x7e],
    [0x7f, 0x49, 0x49, 0x49, 0x36],
    [0x3e, 0x41, 0x41, 0x41, 0x22],
    [0x7f, 0x41, 0x41, 0x22, 0x1c],
    [0x7f, 0x49, 0x49, 0x49, 0x41],
    [0x7f, 0x09, 0x09, 0x09, 0x01],
    [0x3e, 0x41, 0x49, 0x49, 0x7a],
    [0x7f, 0x08, 0x08, 0x08, 0x7f],
    [0x00, 0x41, 0x7f, 0x41, 0x00],
    [0x20, 0x40, 0x41, 0x3f, 0x01],
    [0x7f, 0x08, 0x14, 0x22, 0x41],
    [0x7f, 0x40, 0x40, 0x40, 0x40],
    [0x7f, 0x02, 0x0c, 0x02, 0x7f],
    [0x7f, 0x04, 0x08, 0x10, 0x7f],
    [0x3e, 0x41, 0x41, 0x41, 0x3e],
    [0x7f, 0x09, 0x09, 0x09, 0x06],
    [0x3e, 0x41, 0x51, 0x21, 0x5e],
    [0x7f, 0x09, 0x19, 0x29, 0x46],
    [0x46, 0x49, 0x49, 0x49, 0x31],
    [0x01, 0x01, 0x7f, 0x01, 0x01],
    [0x3f, 0x40, 0x40, 0x40, 0x3f],
    [0x1f, 0x20, 0x40, 0x20, 0x1f],
    [0x3f, 0x40, 0x38, 0x40, 0x3f],
    [0x63, 0x14, 0x08, 0x14, 0x63],
    [0x07, 0x08, 0x70, 0x08, 0x07],
    [0x61, 0x51, 0x49, 0x45, 0x43],
    [0x00, 0x7f, 0x41, 0x41, 0x00],
    [0x02, 0x04, 0x08, 0x10, 0x20],
    [0x00, 0x41, 0x41, 0x7f, 0x00],
    [0x04, 0x02, 0x01, 0x02, 0x04],
    [0x40, 0x40, 0x40, 0x40, 0x40],
    [0x00, 0x01, 0x02, 0x04, 0x00],
    [0x20, 0x54, 0x54, 0x54, 0x78],
    [0x7f, 0x48, 0x44, 0x44, 0x38],
    [0x38, 0x44, 0x44, 0x44, 0x20],
    [0x38, 0x44, 0x44, 0x48, 0x7f],
    [0x38, 0x54, 0x54, 0x54, 0x18],
    [0x08, 0x7e, 0x09, 0x01, 0x02],
    [0x0c, 0x52, 0x52, 0x52, 0x3e],
    [0x7f, 0x08, 0x04, 0x04, 0x78],
    [0x00, 0x44, 0x7d, 0x40, 0x00],
    [0x20, 0x40, 0x44, 0x3d, 0x00],
    [0x7f, 0x10, 0x28, 0x44, 0x00],
    [0x00, 0x41, 0x7f, 0x40, 0x00],
    [0x7c, 0x04, 0x18, 0x04, 0x78],
    [0x7c, 0x08, 0x04, 0x04, 0x78],
    [0x38, 0x44, 0x44, 0x44, 0x38],
    [0x7c, 0x14, 0x14, 0x14, 0x08],
    [0x08, 0x14, 0x14, 0x18, 0x7c],
    [0x7c, 0x08, 0x04, 0x04, 0x08],
    [0x48, 0x54, 0x54, 0x54, 0x20],
    [0x04, 0x3f, 0x44, 0x40, 0x20],
    [0x3c, 0x40, 0x40, 0x20, 0x7c],
    [0x1c, 0x20, 0x40, 0x20, 0x1c],
    [0x3c, 0x40, 0x30, 0x40, 0x3c],
    [0x44, 0x28, 0x10, 0x28, 0x44],
    [0x0c, 0x50, 0x50, 0x50, 0x3c],
    [0x44, 0x64, 0x54, 0x4c, 0x44],
    [0x00, 0x08, 0x36, 0x41, 0x00],
    [0x00, 0x00, 0x7f, 0x00, 0x00],
    [0x00, 0x41, 0x36, 0x08, 0x00],
    [0x08, 0x04, 0x08, 0x10, 0x08],
];

const fn glyph(character: char) -> [u8; 5] {
    let code = character as u32;
    if code < FIRST_PRINTABLE_ASCII || code > LAST_PRINTABLE_ASCII {
        return UNKNOWN_GLYPH;
    }
    FONT[(code - FIRST_PRINTABLE_ASCII) as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_clipped_components_without_allocating() {
        let mut pixels = [0_u8; 32 * 17 * 2];
        let mut canvas = Canvas::new(&mut pixels, 32, 17).unwrap();
        canvas.clear(color::BACKGROUND);
        canvas.button(
            Rect {
                x: 2,
                y: 2,
                width: 28,
                height: 13,
            },
            "OK",
            ButtonStyle::PRIMARY,
        );
        canvas.fill_rect(
            Rect {
                x: 31,
                y: 16,
                width: 20,
                height: 20,
            },
            color::DANGER,
        );
        assert!(pixels.iter().any(|byte| *byte != 0));
    }

    #[test]
    fn rejects_mismatched_frame_size() {
        assert!(matches!(
            Canvas::new(&mut [0; 4], 2, 2),
            Err(Error::InvalidArgument)
        ));
    }

    fn rendered(character: char) -> [u8; 6 * 7 * 2] {
        let mut pixels = [0_u8; 6 * 7 * 2];
        let mut canvas = Canvas::new(&mut pixels, 6, 7).unwrap();
        let mut encoded = [0_u8; 4];
        canvas.draw_text(0, 0, character.encode_utf8(&mut encoded), 0xffff, 1);
        pixels
    }

    #[test]
    fn renders_printable_ascii_without_losing_case_or_symbols() {
        for lowercase in b'a'..=b'z' {
            assert_ne!(
                rendered(char::from(lowercase)),
                rendered(char::from(lowercase.to_ascii_uppercase())),
                "case-insensitive glyph for {}",
                char::from(lowercase)
            );
        }

        let question = rendered('?');
        for character in "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~".chars() {
            if character != '?' {
                assert_ne!(
                    rendered(character),
                    question,
                    "missing glyph for {character}"
                );
            }
        }
        assert_eq!(glyph('?'), UNKNOWN_GLYPH);
        assert_eq!(glyph('\n'), UNKNOWN_GLYPH);
    }
}
