use std::fmt;
use std::fs::File;
use std::io::BufReader;

use jpeg_decoder::{Decoder, PixelFormat};

use crate::photo_library::{
    PHOTO_FRAME_BYTES, PHOTO_HEIGHT, PHOTO_LIBRARY_ID, PHOTO_LIBRARY_QUOTA_BYTES, PHOTO_WIDTH,
    PhotoImportError, camera_original, original_blob_key,
};
use crate::{StorageClient, StorageClientError};

pub(crate) const MAX_PHOTO_VIEW_ZOOM_LEVEL: u8 = 2;
pub(crate) const MIN_PHOTO_VIEW_PAN: i16 = -1000;
pub(crate) const MAX_PHOTO_VIEW_PAN: i16 = 1000;

#[derive(Debug)]
pub(crate) enum PhotoViewError {
    Library(PhotoImportError),
    Storage(StorageClientError),
    MissingOriginal,
    InvalidJpeg,
}

impl fmt::Display for PhotoViewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Library(error) => write!(formatter, "photo library failed: {error}"),
            Self::Storage(error) => write!(formatter, "photo storage failed: {error}"),
            Self::MissingOriginal => formatter.write_str("photo original is unavailable"),
            Self::InvalidJpeg => formatter.write_str("photo original is not a supported JPEG"),
        }
    }
}

impl std::error::Error for PhotoViewError {}

impl From<PhotoImportError> for PhotoViewError {
    fn from(error: PhotoImportError) -> Self {
        Self::Library(error)
    }
}

impl From<StorageClientError> for PhotoViewError {
    fn from(error: StorageClientError) -> Self {
        Self::Storage(error)
    }
}

#[derive(Debug)]
struct DecodedPhoto {
    width: usize,
    height: usize,
    channels: usize,
    pixels: Vec<u8>,
}

#[derive(Debug, Default)]
pub(crate) struct PhotoViewCache {
    photo_id: u64,
    decoded: Option<DecodedPhoto>,
}

impl PhotoViewCache {
    pub(crate) fn render(
        &mut self,
        storage: &StorageClient,
        request_id: u64,
        photo_id: u64,
        zoom_level: u8,
        pan_x: i16,
        pan_y: i16,
    ) -> Result<Option<Vec<u8>>, PhotoViewError> {
        let Some(original) = camera_original(storage, request_id, photo_id)? else {
            return Ok(None);
        };
        if self.photo_id != photo_id || self.decoded.is_none() {
            let descriptor = storage
                .open_blob(
                    request_id,
                    PHOTO_LIBRARY_ID,
                    PHOTO_LIBRARY_QUOTA_BYTES,
                    &original_blob_key(photo_id),
                    original.jpeg_size_bytes,
                )?
                .ok_or(PhotoViewError::MissingOriginal)?;
            let mut decoder = Decoder::new(BufReader::new(File::from(descriptor)));
            let pixels = decoder.decode().map_err(|_| PhotoViewError::InvalidJpeg)?;
            let info = decoder.info().ok_or(PhotoViewError::InvalidJpeg)?;
            let channels = match info.pixel_format {
                PixelFormat::L8 => 1,
                PixelFormat::RGB24 => 3,
                _ => return Err(PhotoViewError::InvalidJpeg),
            };
            let width = usize::from(info.width);
            let height = usize::from(info.height);
            if info.width != original.width
                || info.height != original.height
                || pixels.len() != width * height * channels
            {
                return Err(PhotoViewError::InvalidJpeg);
            }
            self.photo_id = photo_id;
            self.decoded = Some(DecodedPhoto {
                width,
                height,
                channels,
                pixels,
            });
        }
        let decoded = self.decoded.as_ref().expect("decoded photo was cached");
        Ok(Some(render_view(decoded, zoom_level, pan_x, pan_y)))
    }
}

fn render_view(photo: &DecodedPhoto, zoom_level: u8, pan_x: i16, pan_y: i16) -> Vec<u8> {
    let (display_width, display_height) = match zoom_level {
        0 => fit_dimensions(photo.width, photo.height, PHOTO_WIDTH, PHOTO_HEIGHT),
        1 => (photo.width.div_ceil(2), photo.height.div_ceil(2)),
        _ => (photo.width, photo.height),
    };
    let origin_x = pan_origin(display_width.saturating_sub(PHOTO_WIDTH), pan_x);
    let origin_y = pan_origin(display_height.saturating_sub(PHOTO_HEIGHT), pan_y);
    let letterbox_x = PHOTO_WIDTH.saturating_sub(display_width) / 2;
    let letterbox_y = PHOTO_HEIGHT.saturating_sub(display_height) / 2;
    let mut output = vec![0_u8; PHOTO_FRAME_BYTES];
    for target_y in 0..PHOTO_HEIGHT {
        if target_y < letterbox_y || target_y >= letterbox_y + display_height {
            continue;
        }
        let display_y = origin_y + target_y - letterbox_y;
        let source_y = (display_y * photo.height / display_height).min(photo.height - 1);
        for target_x in 0..PHOTO_WIDTH {
            if target_x < letterbox_x || target_x >= letterbox_x + display_width {
                continue;
            }
            let display_x = origin_x + target_x - letterbox_x;
            let source_x = (display_x * photo.width / display_width).min(photo.width - 1);
            let source = (source_y * photo.width + source_x) * photo.channels;
            let (red, green, blue) = if photo.channels == 1 {
                let luminance = photo.pixels[source];
                (luminance, luminance, luminance)
            } else {
                (
                    photo.pixels[source],
                    photo.pixels[source + 1],
                    photo.pixels[source + 2],
                )
            };
            let rgb565 = (u16::from(red & 0xf8) << 8)
                | (u16::from(green & 0xfc) << 3)
                | u16::from(blue >> 3);
            let target = (target_y * PHOTO_WIDTH + target_x) * 2;
            output[target..target + 2].copy_from_slice(&rgb565.to_le_bytes());
        }
    }
    output
}

fn fit_dimensions(
    source_width: usize,
    source_height: usize,
    target_width: usize,
    target_height: usize,
) -> (usize, usize) {
    if source_width * target_height > source_height * target_width {
        (
            target_width,
            (source_height * target_width / source_width).max(1),
        )
    } else {
        (
            (source_width * target_height / source_height).max(1),
            target_height,
        )
    }
}

fn pan_origin(maximum: usize, pan: i16) -> usize {
    let normalized = i32::from(pan.clamp(MIN_PHOTO_VIEW_PAN, MAX_PHOTO_VIEW_PAN))
        - i32::from(MIN_PHOTO_VIEW_PAN);
    maximum * normalized as usize
        / usize::try_from(i32::from(MAX_PHOTO_VIEW_PAN) - i32::from(MIN_PHOTO_VIEW_PAN)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_preserves_aspect_ratio_and_centers_letterboxing() {
        assert_eq!(fit_dimensions(1280, 720, 320, 170), (302, 170));
        assert_eq!(fit_dimensions(720, 1280, 320, 170), (95, 170));
    }

    #[test]
    fn normalized_pan_reaches_both_edges() {
        assert_eq!(pan_origin(960, -1000), 0);
        assert_eq!(pan_origin(960, 0), 480);
        assert_eq!(pan_origin(960, 1000), 960);
    }

    #[test]
    fn actual_size_view_uses_pan_to_select_source_pixels() {
        let mut source = vec![0_u8; 640 * 170 * 3];
        source[..3].copy_from_slice(&[255, 0, 0]);
        let last = (640 - 320) * 3;
        source[last..last + 3].copy_from_slice(&[0, 255, 0]);
        let photo = DecodedPhoto {
            width: 640,
            height: 170,
            channels: 3,
            pixels: source,
        };
        let left = render_view(&photo, 2, -1000, 0);
        let right = render_view(&photo, 2, 1000, 0);
        assert_eq!(&left[..2], &0xf800_u16.to_le_bytes());
        assert_eq!(&right[..2], &0x07e0_u16.to_le_bytes());
    }
}
