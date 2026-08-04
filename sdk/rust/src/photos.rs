use crate::{Error, camera, host_imports, storage};

pub const MAX_PHOTOS: usize = 32;
pub const CHUNK_BYTES: usize = storage::MAX_VALUE_BYTES;
pub const CHUNK_COUNT: usize = camera::FRAME_BYTES.div_ceil(CHUNK_BYTES);

const INDEX_KEY: &str = "index.v1";
const INDEX_HEADER_BYTES: usize = 8;
const INDEX_BYTES: usize = INDEX_HEADER_BYTES + MAX_PHOTOS * 8;
const INDEX_MAGIC: [u8; 4] = *b"CP0P";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Photo {
    pub id: u64,
}

#[derive(Clone, Copy)]
struct Index {
    count: usize,
    ids: [u64; MAX_PHOTOS],
}

impl Index {
    const fn empty() -> Self {
        Self {
            count: 0,
            ids: [0; MAX_PHOTOS],
        }
    }
}

pub fn list(output: &mut [Photo]) -> Result<usize, Error> {
    let index = load_index(false)?;
    let count = index.count.min(output.len());
    for (target, id) in output.iter_mut().zip(index.ids[..count].iter()) {
        *target = Photo { id: *id };
    }
    Ok(count)
}

pub fn save_rgb565(pixels: &[u16], suggested_id: u64) -> Result<Photo, Error> {
    if pixels.len() != camera::PIXEL_COUNT {
        return Err(Error::InvalidArgument);
    }
    let mut index = load_index(true)?;
    let newest = index.count.checked_sub(1).map_or(0, |last| index.ids[last]);
    let id = suggested_id.max(newest.saturating_add(1)).max(1);
    let bytes =
        unsafe { core::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), camera::FRAME_BYTES) };

    let mut written = 0;
    for chunk in 0..CHUNK_COUNT {
        let start = chunk * CHUNK_BYTES;
        let end = (start + CHUNK_BYTES).min(bytes.len());
        let key = chunk_key(id, chunk);
        if let Err(error) = put(key.as_str(), &bytes[start..end]) {
            cleanup_chunks(id, written);
            return Err(error);
        }
        written += 1;
    }

    let removed = if index.count == MAX_PHOTOS {
        let removed = index.ids[0];
        index.ids.copy_within(1..MAX_PHOTOS, 0);
        index.count -= 1;
        Some(removed)
    } else {
        None
    };
    index.ids[index.count] = id;
    index.count += 1;
    if let Err(error) = store_index(&index) {
        cleanup_chunks(id, CHUNK_COUNT);
        return Err(error);
    }
    if let Some(removed) = removed {
        cleanup_chunks(removed, CHUNK_COUNT);
    }
    Ok(Photo { id })
}

pub fn load_rgb565(photo: Photo, pixels: &mut [u16]) -> Result<(), Error> {
    if photo.id == 0 || pixels.len() != camera::PIXEL_COUNT {
        return Err(Error::InvalidArgument);
    }
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(pixels.as_mut_ptr().cast::<u8>(), camera::FRAME_BYTES)
    };
    for chunk in 0..CHUNK_COUNT {
        let start = chunk * CHUNK_BYTES;
        let end = (start + CHUNK_BYTES).min(bytes.len());
        let key = chunk_key(photo.id, chunk);
        match get(key.as_str(), &mut bytes[start..end])? {
            Some(length) if length == end - start => {}
            _ => return Err(Error::Internal),
        }
    }
    Ok(())
}

pub fn delete(photo: Photo) -> Result<bool, Error> {
    if photo.id == 0 {
        return Err(Error::InvalidArgument);
    }
    let mut index = load_index(true)?;
    let Some(position) = index.ids[..index.count]
        .iter()
        .position(|id| *id == photo.id)
    else {
        return Ok(false);
    };
    index.ids.copy_within(position + 1..index.count, position);
    index.count -= 1;
    index.ids[index.count] = 0;
    store_index(&index)?;
    cleanup_chunks(photo.id, CHUNK_COUNT);
    Ok(true)
}

fn load_index(for_update: bool) -> Result<Index, Error> {
    let mut encoded = [0_u8; INDEX_BYTES];
    let value = if for_update {
        get_index_for_update(&mut encoded)
    } else {
        get(INDEX_KEY, &mut encoded)
    }?;
    let Some(length) = value else {
        return Ok(Index::empty());
    };
    decode_index(&encoded[..length])
}

fn store_index(index: &Index) -> Result<(), Error> {
    let mut encoded = [0_u8; INDEX_BYTES];
    let length = encode_index(index, &mut encoded);
    put(INDEX_KEY, &encoded[..length])
}

fn encode_index(index: &Index, output: &mut [u8; INDEX_BYTES]) -> usize {
    output[..4].copy_from_slice(&INDEX_MAGIC);
    output[4] = 1;
    output[5] = index.count as u8;
    output[6] = 0;
    output[7] = 0;
    for (position, id) in index.ids[..index.count].iter().enumerate() {
        let start = INDEX_HEADER_BYTES + position * 8;
        output[start..start + 8].copy_from_slice(&id.to_le_bytes());
    }
    INDEX_HEADER_BYTES + index.count * 8
}

fn decode_index(encoded: &[u8]) -> Result<Index, Error> {
    if encoded.len() < INDEX_HEADER_BYTES
        || encoded[..4] != INDEX_MAGIC
        || encoded[4] != 1
        || encoded[6] != 0
        || encoded[7] != 0
    {
        return Err(Error::Internal);
    }
    let count = encoded[5] as usize;
    if count > MAX_PHOTOS || encoded.len() != INDEX_HEADER_BYTES + count * 8 {
        return Err(Error::Internal);
    }
    let mut index = Index::empty();
    index.count = count;
    for position in 0..count {
        let start = INDEX_HEADER_BYTES + position * 8;
        let id = u64::from_le_bytes(encoded[start..start + 8].try_into().unwrap());
        if id == 0 || (position > 0 && id <= index.ids[position - 1]) {
            return Err(Error::Internal);
        }
        index.ids[position] = id;
    }
    Ok(index)
}

struct ChunkKey {
    bytes: [u8; 21],
}

impl ChunkKey {
    fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.bytes) }
    }
}

fn chunk_key(id: u64, chunk: usize) -> ChunkKey {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0_u8; 21];
    bytes[0] = b'p';
    for digit in 0..16 {
        bytes[1 + digit] = HEX[((id >> ((15 - digit) * 4)) & 0xf) as usize];
    }
    bytes[17] = b'.';
    bytes[18] = b'c';
    bytes[19] = b'0' + (chunk / 10) as u8;
    bytes[20] = b'0' + (chunk % 10) as u8;
    ChunkKey { bytes }
}

fn cleanup_chunks(id: u64, count: usize) {
    for chunk in 0..count {
        let key = chunk_key(id, chunk);
        let _ = delete_key(key.as_str());
    }
}

fn put(key: &str, value: &[u8]) -> Result<(), Error> {
    Error::from_host(host_imports::cp0_photos_put(
        key.as_ptr(),
        key.len() as u32,
        value.as_ptr(),
        value.len() as u32,
    ))
}

fn get(key: &str, value: &mut [u8]) -> Result<Option<usize>, Error> {
    let result = host_imports::cp0_photos_get(
        key.as_ptr(),
        key.len() as u32,
        value.as_mut_ptr(),
        value.len() as u32,
    );
    if result < 0 {
        return Error::from_host(result).map(|()| None);
    }
    let length = result as usize;
    if length == 0 {
        Ok(None)
    } else if length <= value.len() {
        Ok(Some(length))
    } else {
        Err(Error::Internal)
    }
}

fn get_index_for_update(value: &mut [u8]) -> Result<Option<usize>, Error> {
    let result = host_imports::cp0_photos_index_get(value.as_mut_ptr(), value.len() as u32);
    if result < 0 {
        return Error::from_host(result).map(|()| None);
    }
    let length = result as usize;
    if length == 0 {
        Ok(None)
    } else if length <= value.len() {
        Ok(Some(length))
    } else {
        Err(Error::Internal)
    }
}

fn delete_key(key: &str) -> Result<bool, Error> {
    match host_imports::cp0_photos_delete(key.as_ptr(), key.len() as u32) {
        0 => Ok(false),
        1 => Ok(true),
        value if value < 0 => Error::from_host(value).map(|()| unreachable!()),
        _ => Err(Error::Internal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_round_trip_is_bounded_and_sorted() {
        let mut index = Index::empty();
        index.count = 3;
        index.ids[..3].copy_from_slice(&[7, 9, 42]);
        let mut encoded = [0_u8; INDEX_BYTES];
        let length = encode_index(&index, &mut encoded);
        let decoded = decode_index(&encoded[..length]).unwrap();
        assert_eq!(decoded.count, 3);
        assert_eq!(&decoded.ids[..3], &[7, 9, 42]);
        encoded[INDEX_HEADER_BYTES + 8..INDEX_HEADER_BYTES + 16]
            .copy_from_slice(&7_u64.to_le_bytes());
        assert_eq!(
            decode_index(&encoded[..length]).err(),
            Some(Error::Internal)
        );
    }

    #[test]
    fn chunk_keys_are_stable_and_native_host_is_unavailable() {
        assert_eq!(chunk_key(0x12ab, 13).as_str(), "p00000000000012ab.c13");
        let mut photos = [Photo { id: 0 }; MAX_PHOTOS];
        assert_eq!(list(&mut photos), Err(Error::Unavailable));
    }
}
