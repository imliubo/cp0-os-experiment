use crate::{Error, camera, host_imports, storage};

pub const LIST_PAGE_PHOTOS: usize = 8;
pub const CHUNK_BYTES: usize = storage::MAX_VALUE_BYTES;
pub const CHUNK_COUNT: usize = camera::FRAME_BYTES.div_ceil(CHUNK_BYTES);

const HEAD_KEY: &str = "head.v2";
const LEGACY_INDEX_KEY: &str = "index.v1";
const HEAD_BYTES: usize = 32;
const HEAD_MAGIC: [u8; 4] = *b"CP0H";
const INDEX_PAGE_PHOTOS: usize = 256;
const PAGE_HEADER_BYTES: usize = 16;
const PAGE_BYTES: usize = PAGE_HEADER_BYTES + INDEX_PAGE_PHOTOS * 8;
const PAGE_MAGIC: [u8; 4] = *b"CP0G";
const LEGACY_MAX_PHOTOS: usize = 32;
const LEGACY_HEADER_BYTES: usize = 8;
const LEGACY_BYTES: usize = LEGACY_HEADER_BYTES + LEGACY_MAX_PHOTOS * 8;
const LEGACY_MAGIC: [u8; 4] = *b"CP0P";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Photo {
    pub id: u64,
}

pub const MIN_VIEW_PAN: i16 = -1000;
pub const MAX_VIEW_PAN: i16 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ViewZoom {
    Fit = 0,
    Half = 1,
    Actual = 2,
}

#[derive(Clone, Copy)]
struct Head {
    active_count: u64,
    slot_count: u64,
    last_id: u64,
}

#[derive(Clone, Copy)]
struct IndexPage {
    active_count: u16,
    ids: [u64; INDEX_PAGE_PHOTOS],
}

impl IndexPage {
    const fn empty() -> Self {
        Self {
            active_count: 0,
            ids: [0; INDEX_PAGE_PHOTOS],
        }
    }
}

#[derive(Clone, Copy)]
struct LegacyIndex {
    count: usize,
    ids: [u64; LEGACY_MAX_PHOTOS],
}

impl LegacyIndex {
    const fn empty() -> Self {
        Self {
            count: 0,
            ids: [0; LEGACY_MAX_PHOTOS],
        }
    }
}

pub fn count() -> Result<u64, Error> {
    if let Some(head) = load_v2_head()? {
        return Ok(head.active_count);
    }
    Ok(load_legacy_index()?.map_or(0, |legacy| legacy.count as u64))
}

pub fn list(output: &mut [Photo]) -> Result<usize, Error> {
    list_page(0, output)
}

pub fn list_page(offset: u64, output: &mut [Photo]) -> Result<usize, Error> {
    if output.is_empty() {
        return Ok(0);
    }
    let Some(head) = load_v2_head()? else {
        return list_legacy(offset, output);
    };
    let mut skipped = 0_u64;
    let mut written = 0;
    let page_count = head.slot_count.div_ceil(INDEX_PAGE_PHOTOS as u64);
    for page_number in 0..page_count {
        let page = load_page(page_number as u32)?.ok_or(Error::Internal)?;
        let slots = page_slots(&head, page_number);
        let visible = page.ids[..slots].iter().filter(|id| **id != 0).count() as u64;
        if skipped + visible <= offset {
            skipped += visible;
            continue;
        }
        for id in &page.ids[..slots] {
            if *id == 0 {
                continue;
            }
            if skipped < offset {
                skipped += 1;
                continue;
            }
            output[written] = Photo { id: *id };
            written += 1;
            if written == output.len() {
                return Ok(written);
            }
        }
    }
    Ok(written)
}

pub fn save_rgb565(pixels: &[u16], suggested_id: u64) -> Result<Photo, Error> {
    if pixels.len() != camera::PIXEL_COUNT {
        return Err(Error::InvalidArgument);
    }
    let bytes =
        unsafe { core::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), camera::FRAME_BYTES) };
    let result =
        host_imports::cp0_photos_import_rgb565(bytes.as_ptr(), bytes.len() as u32, suggested_id);
    if result < 0 {
        return Error::from_host(result as i32).map(|()| unreachable!());
    }
    let id = result as u64;
    if id == 0 {
        Err(Error::Internal)
    } else {
        Ok(Photo { id })
    }
}

pub fn load_rgb565(photo: Photo, pixels: &mut [u16]) -> Result<(), Error> {
    if photo.id == 0 || pixels.len() != camera::PIXEL_COUNT {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host_imports::cp0_photos_load_rgb565(
        photo.id,
        pixels.as_mut_ptr().cast(),
        camera::FRAME_BYTES as u32,
    ))
}

pub fn load_view_rgb565(
    photo: Photo,
    zoom: ViewZoom,
    pan_x: i16,
    pan_y: i16,
    pixels: &mut [u16],
) -> Result<(), Error> {
    if photo.id == 0
        || !(MIN_VIEW_PAN..=MAX_VIEW_PAN).contains(&pan_x)
        || !(MIN_VIEW_PAN..=MAX_VIEW_PAN).contains(&pan_y)
        || pixels.len() != camera::PIXEL_COUNT
    {
        return Err(Error::InvalidArgument);
    }
    Error::from_host(host_imports::cp0_photos_load_view_rgb565(
        photo.id,
        zoom as u32,
        i32::from(pan_x),
        i32::from(pan_y),
        pixels.as_mut_ptr().cast(),
        camera::FRAME_BYTES as u32,
    ))
}

pub fn delete(photo: Photo) -> Result<bool, Error> {
    if photo.id == 0 {
        return Err(Error::InvalidArgument);
    }
    match host_imports::cp0_photos_remove(photo.id) {
        0 => Ok(false),
        1 => Ok(true),
        value if value < 0 => Error::from_host(value).map(|()| unreachable!()),
        _ => Err(Error::Internal),
    }
}

fn load_v2_head() -> Result<Option<Head>, Error> {
    let mut encoded = [0_u8; HEAD_BYTES];
    let value = get(HEAD_KEY, &mut encoded)?;
    value
        .map(|length| decode_head(&encoded[..length]))
        .transpose()
}

fn load_legacy_index() -> Result<Option<LegacyIndex>, Error> {
    let mut encoded = [0_u8; LEGACY_BYTES];
    get(LEGACY_INDEX_KEY, &mut encoded)?
        .map(|length| decode_legacy_index(&encoded[..length]))
        .transpose()
}

fn list_legacy(offset: u64, output: &mut [Photo]) -> Result<usize, Error> {
    let Some(index) = load_legacy_index()? else {
        return Ok(0);
    };
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(index.count);
    let count = (index.count - start).min(output.len());
    for (target, id) in output
        .iter_mut()
        .zip(index.ids[start..start + count].iter())
    {
        *target = Photo { id: *id };
    }
    Ok(count)
}

#[cfg(test)]
fn encode_head(head: &Head, output: &mut [u8; HEAD_BYTES]) {
    output[..4].copy_from_slice(&HEAD_MAGIC);
    output[4] = 2;
    output[5..8].fill(0);
    output[8..16].copy_from_slice(&head.active_count.to_le_bytes());
    output[16..24].copy_from_slice(&head.slot_count.to_le_bytes());
    output[24..32].copy_from_slice(&head.last_id.to_le_bytes());
}

fn decode_head(encoded: &[u8]) -> Result<Head, Error> {
    if encoded.len() != HEAD_BYTES
        || encoded[..4] != HEAD_MAGIC
        || encoded[4] != 2
        || encoded[5..8] != [0; 3]
    {
        return Err(Error::Internal);
    }
    let head = Head {
        active_count: u64::from_le_bytes(encoded[8..16].try_into().unwrap()),
        slot_count: u64::from_le_bytes(encoded[16..24].try_into().unwrap()),
        last_id: u64::from_le_bytes(encoded[24..32].try_into().unwrap()),
    };
    if head.active_count > head.slot_count
        || (head.slot_count == 0) != (head.last_id == 0)
        || (head.active_count != 0 && head.last_id == 0)
    {
        return Err(Error::Internal);
    }
    Ok(head)
}

fn load_page(page_number: u32) -> Result<Option<IndexPage>, Error> {
    let key = page_key(page_number);
    let mut encoded = [0_u8; PAGE_BYTES];
    get(key.as_str(), &mut encoded)?
        .map(|length| decode_page(page_number, &encoded[..length]))
        .transpose()
}

#[cfg(test)]
fn encode_page(page_number: u32, page: &IndexPage, output: &mut [u8; PAGE_BYTES]) {
    output[..4].copy_from_slice(&PAGE_MAGIC);
    output[4] = 2;
    output[5] = 0;
    output[6..8].copy_from_slice(&page.active_count.to_le_bytes());
    output[8..12].copy_from_slice(&page_number.to_le_bytes());
    output[12..16].fill(0);
    for (position, id) in page.ids.iter().enumerate() {
        let start = PAGE_HEADER_BYTES + position * 8;
        output[start..start + 8].copy_from_slice(&id.to_le_bytes());
    }
}

fn decode_page(page_number: u32, encoded: &[u8]) -> Result<IndexPage, Error> {
    if encoded.len() != PAGE_BYTES
        || encoded[..4] != PAGE_MAGIC
        || encoded[4] != 2
        || encoded[5] != 0
        || u32::from_le_bytes(encoded[8..12].try_into().unwrap()) != page_number
        || encoded[12..16] != [0; 4]
    {
        return Err(Error::Internal);
    }
    let mut page = IndexPage::empty();
    page.active_count = u16::from_le_bytes(encoded[6..8].try_into().unwrap());
    let mut actual = 0_u16;
    let mut previous = 0_u64;
    for position in 0..INDEX_PAGE_PHOTOS {
        let start = PAGE_HEADER_BYTES + position * 8;
        let id = u64::from_le_bytes(encoded[start..start + 8].try_into().unwrap());
        if id != 0 {
            if id <= previous {
                return Err(Error::Internal);
            }
            previous = id;
            actual = actual.checked_add(1).ok_or(Error::Internal)?;
        }
        page.ids[position] = id;
    }
    if actual != page.active_count {
        return Err(Error::Internal);
    }
    Ok(page)
}

fn decode_legacy_index(encoded: &[u8]) -> Result<LegacyIndex, Error> {
    if encoded.len() < LEGACY_HEADER_BYTES
        || encoded[..4] != LEGACY_MAGIC
        || encoded[4] != 1
        || encoded[6] != 0
        || encoded[7] != 0
    {
        return Err(Error::Internal);
    }
    let count = encoded[5] as usize;
    if count > LEGACY_MAX_PHOTOS || encoded.len() != LEGACY_HEADER_BYTES + count * 8 {
        return Err(Error::Internal);
    }
    let mut index = LegacyIndex::empty();
    index.count = count;
    for position in 0..count {
        let start = LEGACY_HEADER_BYTES + position * 8;
        let id = u64::from_le_bytes(encoded[start..start + 8].try_into().unwrap());
        if id == 0 || (position > 0 && id <= index.ids[position - 1]) {
            return Err(Error::Internal);
        }
        index.ids[position] = id;
    }
    Ok(index)
}

fn page_slots(head: &Head, page_number: u64) -> usize {
    head.slot_count
        .saturating_sub(page_number * INDEX_PAGE_PHOTOS as u64)
        .min(INDEX_PAGE_PHOTOS as u64) as usize
}

struct PageKey {
    bytes: [u8; 17],
}

impl PageKey {
    fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.bytes) }
    }
}

fn page_key(page_number: u32) -> PageKey {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = *b"index.v2.00000000";
    for digit in 0..8 {
        bytes[9 + digit] = HEX[((page_number >> ((7 - digit) * 4)) & 0xf) as usize];
    }
    PageKey { bytes }
}

#[cfg(test)]
struct ChunkKey {
    bytes: [u8; 21],
}

#[cfg(test)]
impl ChunkKey {
    fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.bytes) }
    }
}

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_head_and_page_round_trip_with_tombstones() {
        let head = Head {
            active_count: 2,
            slot_count: 3,
            last_id: 42,
        };
        let mut encoded_head = [0_u8; HEAD_BYTES];
        encode_head(&head, &mut encoded_head);
        let decoded_head = decode_head(&encoded_head).unwrap();
        assert_eq!(decoded_head.active_count, 2);
        assert_eq!(decoded_head.slot_count, 3);

        let mut page = IndexPage::empty();
        page.active_count = 2;
        page.ids[..3].copy_from_slice(&[7, 0, 42]);
        let mut encoded_page = [0_u8; PAGE_BYTES];
        encode_page(5, &page, &mut encoded_page);
        let decoded_page = decode_page(5, &encoded_page).unwrap();
        assert_eq!(decoded_page.active_count, 2);
        assert_eq!(&decoded_page.ids[..3], &[7, 0, 42]);
        assert!(decode_page(4, &encoded_page).is_err());
    }

    #[test]
    fn legacy_index_decode_and_keys_are_stable() {
        let mut encoded = [0_u8; LEGACY_HEADER_BYTES + 3 * 8];
        encoded[..4].copy_from_slice(&LEGACY_MAGIC);
        encoded[4] = 1;
        encoded[5] = 3;
        for (position, id) in [7_u64, 9, 42].iter().enumerate() {
            let start = LEGACY_HEADER_BYTES + position * 8;
            encoded[start..start + 8].copy_from_slice(&id.to_le_bytes());
        }
        assert_eq!(decode_legacy_index(&encoded).unwrap().count, 3);
        assert_eq!(page_key(0x12ab).as_str(), "index.v2.000012ab");
        assert_eq!(chunk_key(0x12ab, 13).as_str(), "p00000000000012ab.c13");
    }

    #[test]
    fn native_host_is_unavailable() {
        let mut photos = [Photo { id: 0 }; LIST_PAGE_PHOTOS];
        assert_eq!(list(&mut photos), Err(Error::Unavailable));
        let mut frame = [0_u16; camera::PIXEL_COUNT];
        assert_eq!(
            load_view_rgb565(Photo { id: 1 }, ViewZoom::Actual, 0, 0, &mut frame),
            Err(Error::Unavailable)
        );
        assert_eq!(
            load_view_rgb565(Photo { id: 1 }, ViewZoom::Fit, 1001, 0, &mut frame),
            Err(Error::InvalidArgument)
        );
    }
}
