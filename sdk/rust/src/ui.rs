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
            let glyph = glyph(character.to_ascii_uppercase());
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

const fn glyph(character: char) -> [u8; 5] {
    match character {
        '0' => [0x3e, 0x51, 0x49, 0x45, 0x3e],
        '1' => [0x00, 0x42, 0x7f, 0x40, 0x00],
        '2' => [0x42, 0x61, 0x51, 0x49, 0x46],
        '3' => [0x21, 0x41, 0x45, 0x4b, 0x31],
        '4' => [0x18, 0x14, 0x12, 0x7f, 0x10],
        '5' => [0x27, 0x45, 0x45, 0x45, 0x39],
        '6' => [0x3c, 0x4a, 0x49, 0x49, 0x30],
        '7' => [0x01, 0x71, 0x09, 0x05, 0x03],
        '8' => [0x36, 0x49, 0x49, 0x49, 0x36],
        '9' => [0x06, 0x49, 0x49, 0x29, 0x1e],
        'A' => [0x7e, 0x11, 0x11, 0x11, 0x7e],
        'B' => [0x7f, 0x49, 0x49, 0x49, 0x36],
        'C' => [0x3e, 0x41, 0x41, 0x41, 0x22],
        'D' => [0x7f, 0x41, 0x41, 0x22, 0x1c],
        'E' => [0x7f, 0x49, 0x49, 0x49, 0x41],
        'F' => [0x7f, 0x09, 0x09, 0x09, 0x01],
        'G' => [0x3e, 0x41, 0x49, 0x49, 0x7a],
        'H' => [0x7f, 0x08, 0x08, 0x08, 0x7f],
        'I' => [0x00, 0x41, 0x7f, 0x41, 0x00],
        'J' => [0x20, 0x40, 0x41, 0x3f, 0x01],
        'K' => [0x7f, 0x08, 0x14, 0x22, 0x41],
        'L' => [0x7f, 0x40, 0x40, 0x40, 0x40],
        'M' => [0x7f, 0x02, 0x0c, 0x02, 0x7f],
        'N' => [0x7f, 0x04, 0x08, 0x10, 0x7f],
        'O' => [0x3e, 0x41, 0x41, 0x41, 0x3e],
        'P' => [0x7f, 0x09, 0x09, 0x09, 0x06],
        'Q' => [0x3e, 0x41, 0x51, 0x21, 0x5e],
        'R' => [0x7f, 0x09, 0x19, 0x29, 0x46],
        'S' => [0x46, 0x49, 0x49, 0x49, 0x31],
        'T' => [0x01, 0x01, 0x7f, 0x01, 0x01],
        'U' => [0x3f, 0x40, 0x40, 0x40, 0x3f],
        'V' => [0x1f, 0x20, 0x40, 0x20, 0x1f],
        'W' => [0x3f, 0x40, 0x38, 0x40, 0x3f],
        'X' => [0x63, 0x14, 0x08, 0x14, 0x63],
        'Y' => [0x07, 0x08, 0x70, 0x08, 0x07],
        'Z' => [0x61, 0x51, 0x49, 0x45, 0x43],
        '+' => [0x08, 0x08, 0x3e, 0x08, 0x08],
        '-' => [0x08, 0x08, 0x08, 0x08, 0x08],
        '*' => [0x14, 0x08, 0x3e, 0x08, 0x14],
        '/' => [0x20, 0x10, 0x08, 0x04, 0x02],
        '=' => [0x14, 0x14, 0x14, 0x14, 0x14],
        '.' => [0x00, 0x60, 0x60, 0x00, 0x00],
        ':' => [0x00, 0x36, 0x36, 0x00, 0x00],
        ' ' => [0; 5],
        _ => [0x02, 0x01, 0x51, 0x09, 0x06],
    }
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
}
