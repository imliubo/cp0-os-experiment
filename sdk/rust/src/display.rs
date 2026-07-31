use core::mem::size_of;

use crate::{Error, host_imports};

pub const WIDTH: u16 = 320;
pub const STANDARD_HEIGHT: u16 = 150;
pub const IMMERSIVE_HEIGHT: u16 = 170;
pub const MAX_DAMAGE_RECTS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

pub fn dimensions() -> (u16, u16) {
    let packed = host_imports::cp0_display_dimensions();
    (packed as u16, (packed >> 16) as u16)
}

pub fn present_rgb565(pixels: &[u8], damage: &[Rect]) -> Result<(), Error> {
    let (width, height) = dimensions();
    let expected_bytes = usize::from(width)
        .checked_mul(usize::from(height))
        .and_then(|pixels| pixels.checked_mul(2))
        .ok_or(Error::InvalidArgument)?;
    if width != WIDTH
        || !matches!(height, STANDARD_HEIGHT | IMMERSIVE_HEIGHT)
        || pixels.len() != expected_bytes
        || damage.len() > MAX_DAMAGE_RECTS
        || damage.iter().any(|rectangle| {
            rectangle.width == 0
                || rectangle.height == 0
                || u32::from(rectangle.x) + u32::from(rectangle.width) > u32::from(width)
                || u32::from(rectangle.y) + u32::from(rectangle.height) > u32::from(height)
        })
    {
        return Err(Error::InvalidArgument);
    }

    let damage_bytes = damage
        .len()
        .checked_mul(size_of::<Rect>())
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or(Error::InvalidArgument)?;
    Error::from_host(host_imports::cp0_present_rgb565(
        pixels.as_ptr(),
        pixels.len() as u32,
        damage.as_ptr(),
        damage_bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_frame_and_damage_before_host_call() {
        let frame = [0_u8; WIDTH as usize * STANDARD_HEIGHT as usize * 2];
        assert_eq!(dimensions(), (WIDTH, STANDARD_HEIGHT));
        assert_eq!(present_rgb565(&frame, &[]), Err(Error::Unavailable));
        assert_eq!(
            present_rgb565(&frame[..8], &[]),
            Err(Error::InvalidArgument)
        );
        assert_eq!(
            present_rgb565(
                &frame,
                &[Rect {
                    x: 319,
                    y: 0,
                    width: 2,
                    height: 1,
                }],
            ),
            Err(Error::InvalidArgument)
        );
    }
}
