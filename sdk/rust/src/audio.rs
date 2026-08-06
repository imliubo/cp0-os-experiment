use crate::{Error, host_imports};

pub const SAMPLE_RATE_HZ: u32 = 16_000;
pub const CHANNELS: u8 = 1;
pub const MAX_FRAMES: usize = 1024;
pub const MUSIC_SAMPLE_RATE_HZ: u32 = 48_000;
pub const MUSIC_CHANNELS: u8 = 2;
pub const MAX_MUSIC_FRAMES: usize = 720;

pub fn play_pcm_s16le(samples: &[i16]) -> Result<(), Error> {
    if samples.is_empty() || samples.len() > MAX_FRAMES {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host_imports::cp0_audio_play_pcm_s16le(
        samples.as_ptr().cast(),
        core::mem::size_of_val(samples) as u32,
    ))
}

pub fn play_pcm_s16le_stereo_48khz(interleaved_samples: &[i16]) -> Result<(), Error> {
    if interleaved_samples.is_empty()
        || interleaved_samples.len() % MUSIC_CHANNELS as usize != 0
        || interleaved_samples.len() > MAX_MUSIC_FRAMES * MUSIC_CHANNELS as usize
    {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host_imports::cp0_audio_play_pcm_s16le_stereo_48khz(
        interleaved_samples.as_ptr().cast(),
        core::mem::size_of_val(interleaved_samples) as u32,
    ))
}

pub fn capture_pcm_s16le(samples: &mut [i16]) -> Result<usize, Error> {
    if samples.is_empty() || samples.len() > MAX_FRAMES {
        return Err(Error::InvalidArgument);
    }
    let expected_bytes = core::mem::size_of_val(samples);
    let result = host_imports::cp0_audio_capture_pcm_s16le(
        samples.as_mut_ptr().cast(),
        expected_bytes as u32,
    );
    if result < 0 {
        return Error::from_host(result).map(|()| unreachable!());
    }
    let result = result as usize;
    if result != expected_bytes || result % size_of::<i16>() != 0 {
        return Err(Error::Internal);
    }
    Ok(result / size_of::<i16>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_audio_buffers_and_maps_unavailable_host() {
        assert_eq!(play_pcm_s16le(&[]), Err(Error::InvalidArgument));
        assert_eq!(
            play_pcm_s16le(&[0; MAX_FRAMES + 1]),
            Err(Error::InvalidArgument)
        );
        assert_eq!(play_pcm_s16le(&[0; 8]), Err(Error::Unavailable));

        let mut empty = [];
        assert_eq!(capture_pcm_s16le(&mut empty), Err(Error::InvalidArgument));
        let mut samples = [0_i16; 8];
        assert_eq!(capture_pcm_s16le(&mut samples), Err(Error::Unavailable));
    }
}
