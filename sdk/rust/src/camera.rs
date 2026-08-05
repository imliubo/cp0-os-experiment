use crate::{Error, host_imports, photos::Photo};

pub const WIDTH: u16 = 320;
pub const HEIGHT: u16 = 170;
pub const PIXEL_COUNT: usize = WIDTH as usize * HEIGHT as usize;
pub const FRAME_BYTES: usize = PIXEL_COUNT * size_of::<u16>();
pub const PREVIEW_FPS: u16 = 30;
pub const PHOTO_WIDTH: u16 = 1280;
pub const PHOTO_HEIGHT: u16 = 720;

pub fn capture_rgb565(pixels: &mut [u16]) -> Result<(), Error> {
    if pixels.len() != PIXEL_COUNT {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host_imports::cp0_camera_capture_rgb565(
        pixels.as_mut_ptr().cast(),
        core::mem::size_of_val(pixels) as u32,
    ))
}

pub fn capture_photo() -> Result<Photo, Error> {
    let result = host_imports::cp0_camera_capture_photo();
    if result < 0 {
        return Error::from_host(result as i32).map(|()| unreachable!());
    }
    if result == 0 {
        Err(Error::Internal)
    } else {
        Ok(Photo { id: result as u64 })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn requires_one_exact_frame_and_maps_unavailable_host() {
        let mut short = [0_u16; 1];
        assert_eq!(capture_rgb565(&mut short), Err(Error::InvalidArgument));
        let mut frame = std::vec![0_u16; PIXEL_COUNT];
        assert_eq!(capture_rgb565(&mut frame), Err(Error::Unavailable));
        assert_eq!(capture_photo(), Err(Error::Unavailable));
        assert_eq!((PREVIEW_FPS, PHOTO_WIDTH, PHOTO_HEIGHT), (30, 1280, 720));
    }
}
