use std::fmt;
use std::fs::File;
use std::io::Read;

use cp0_camera_protocol::{CAMERA_PHOTO_HEIGHT, CAMERA_PHOTO_WIDTH, MAX_CAMERA_JPEG_BYTES};
use sha2::{Digest, Sha256};

use crate::{StorageClient, StorageClientError};

pub(crate) const PHOTO_LIBRARY_ID: &str = cp0_storage_protocol::SYSTEM_PHOTO_LIBRARY_ID;
pub(crate) const PHOTO_LIBRARY_QUOTA_BYTES: u64 =
    cp0_storage_protocol::SYSTEM_PHOTO_LIBRARY_QUOTA_BYTES;
pub(crate) const PHOTO_LIBRARY_HEAD_KEY: &str = "head.v2";
pub(crate) const PHOTO_LIBRARY_LEGACY_INDEX_KEY: &str = "index.v1";
pub(crate) const PHOTO_WIDTH: usize = 320;
pub(crate) const PHOTO_HEIGHT: usize = 170;
pub(crate) const PHOTO_FRAME_BYTES: usize = PHOTO_WIDTH * PHOTO_HEIGHT * 2;

const HEAD_BYTES: usize = 32;
const HEAD_MAGIC: [u8; 4] = *b"CP0H";
const INDEX_PAGE_PHOTOS: usize = 256;
const PAGE_HEADER_BYTES: usize = 16;
const PAGE_BYTES: usize = PAGE_HEADER_BYTES + INDEX_PAGE_PHOTOS * 8;
const PAGE_MAGIC: [u8; 4] = *b"CP0G";
const LEGACY_MAX_PHOTOS: usize = 32;
const LEGACY_HEADER_BYTES: usize = 8;
const LEGACY_MAGIC: [u8; 4] = *b"CP0P";
const CHUNK_BYTES: usize = cp0_storage_protocol::MAX_STORAGE_VALUE_BYTES;
const CHUNK_COUNT: usize = PHOTO_FRAME_BYTES.div_ceil(CHUNK_BYTES);
const MAX_PHOTO_ID: u64 = i64::MAX as u64;
const PHOTO_METADATA_BYTES: usize = 56;
const PHOTO_METADATA_MAGIC: [u8; 4] = *b"CP0M";
const PHOTO_METADATA_VERSION: u8 = 1;
const PHOTO_KIND_CAMERA: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CameraOriginal {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) jpeg_size_bytes: u32,
    pub(crate) captured_milliseconds: u64,
    pub(crate) sha256: [u8; 32],
}

#[derive(Debug)]
pub enum PhotoImportError {
    InvalidFrame,
    InvalidIndex,
    ResourceExhausted,
    Storage(StorageClientError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhotoExportKind {
    CameraJpeg,
    ScreenshotRgb565,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhotoExport {
    pub id: u64,
    pub kind: PhotoExportKind,
    pub width: u16,
    pub height: u16,
    pub size_bytes: u32,
    pub captured_milliseconds: Option<u64>,
    pub sha256: Option<[u8; 32]>,
}

impl fmt::Display for PhotoImportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFrame => formatter.write_str("invalid photo frame"),
            Self::InvalidIndex => formatter.write_str("invalid photo library index"),
            Self::ResourceExhausted => formatter.write_str("photo library index is exhausted"),
            Self::Storage(error) => write!(formatter, "photo library storage failed: {error}"),
        }
    }
}

impl std::error::Error for PhotoImportError {}

impl From<StorageClientError> for PhotoImportError {
    fn from(error: StorageClientError) -> Self {
        Self::Storage(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Head {
    active_count: u64,
    slot_count: u64,
    last_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
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

trait PhotoStorage {
    fn put(&self, request_id: u64, key: &str, value: &[u8]) -> Result<(), StorageClientError>;
    fn get(&self, request_id: u64, key: &str) -> Result<Option<Vec<u8>>, StorageClientError>;
    fn delete(&self, request_id: u64, key: &str) -> Result<bool, StorageClientError>;
    fn put_blob_chunk(
        &self,
        request_id: u64,
        key: &str,
        offset: usize,
        total_bytes: usize,
        value: &[u8],
    ) -> Result<(), StorageClientError>;
    fn delete_blob(&self, request_id: u64, key: &str) -> Result<bool, StorageClientError>;
}

impl PhotoStorage for StorageClient {
    fn put(&self, request_id: u64, key: &str, value: &[u8]) -> Result<(), StorageClientError> {
        StorageClient::put(
            self,
            request_id,
            PHOTO_LIBRARY_ID,
            PHOTO_LIBRARY_QUOTA_BYTES,
            key,
            value,
        )
        .map(|_| ())
    }

    fn get(&self, request_id: u64, key: &str) -> Result<Option<Vec<u8>>, StorageClientError> {
        StorageClient::get(
            self,
            request_id,
            PHOTO_LIBRARY_ID,
            PHOTO_LIBRARY_QUOTA_BYTES,
            key,
        )
    }

    fn delete(&self, request_id: u64, key: &str) -> Result<bool, StorageClientError> {
        StorageClient::delete(
            self,
            request_id,
            PHOTO_LIBRARY_ID,
            PHOTO_LIBRARY_QUOTA_BYTES,
            key,
        )
    }

    fn put_blob_chunk(
        &self,
        request_id: u64,
        key: &str,
        offset: usize,
        total_bytes: usize,
        value: &[u8],
    ) -> Result<(), StorageClientError> {
        StorageClient::put_blob_chunk(
            self,
            request_id,
            PHOTO_LIBRARY_ID,
            PHOTO_LIBRARY_QUOTA_BYTES,
            key,
            u32::try_from(offset).expect("photo blob offset fits u32"),
            u32::try_from(total_bytes).expect("photo blob size fits u32"),
            value,
        )
        .map(|_| ())
    }

    fn delete_blob(&self, request_id: u64, key: &str) -> Result<bool, StorageClientError> {
        StorageClient::delete_blob(
            self,
            request_id,
            PHOTO_LIBRARY_ID,
            PHOTO_LIBRARY_QUOTA_BYTES,
            key,
        )
    }
}

pub(crate) fn import_screenshot(
    storage: &StorageClient,
    request_id: u64,
    frame: &[u8],
    suggested_id: u64,
) -> Result<u64, PhotoImportError> {
    import_frame(storage, request_id, frame, suggested_id)
}

pub(crate) fn import_app_photo(
    storage: &StorageClient,
    request_id: u64,
    frame: &[u8],
    suggested_id: u64,
) -> Result<u64, PhotoImportError> {
    import_frame(storage, request_id, frame, suggested_id)
}

pub(crate) fn import_camera_photo(
    storage: &StorageClient,
    request_id: u64,
    thumbnail: &[u8],
    jpeg: &[u8],
    captured_milliseconds: u64,
) -> Result<u64, PhotoImportError> {
    import_frame_with_original(
        storage,
        request_id,
        thumbnail,
        Some((jpeg, captured_milliseconds)),
    )
}

pub(crate) fn remove_photo(
    storage: &StorageClient,
    request_id: u64,
    id: u64,
) -> Result<bool, PhotoImportError> {
    remove_frame(storage, request_id, id)
}

pub(crate) fn photo_is_active(
    storage: &StorageClient,
    request_id: u64,
    id: u64,
) -> Result<bool, PhotoImportError> {
    photo_is_active_in(storage, request_id, id)
}

pub(crate) fn load_legacy_photo(
    storage: &StorageClient,
    request_id: u64,
    id: u64,
) -> Result<Option<Vec<u8>>, PhotoImportError> {
    if id == 0 || id > MAX_PHOTO_ID {
        return Err(PhotoImportError::InvalidFrame);
    }
    let mut frame = vec![0_u8; PHOTO_FRAME_BYTES];
    for chunk in 0..CHUNK_COUNT {
        let start = chunk * CHUNK_BYTES;
        let end = (start + CHUNK_BYTES).min(frame.len());
        let Some(value) = storage.get(
            request_id,
            PHOTO_LIBRARY_ID,
            PHOTO_LIBRARY_QUOTA_BYTES,
            &legacy_chunk_key(id, chunk),
        )?
        else {
            return Ok(None);
        };
        if value.len() != end - start {
            return Err(PhotoImportError::InvalidFrame);
        }
        frame[start..end].copy_from_slice(&value);
    }
    Ok(Some(frame))
}

pub(crate) fn photo_blob_key(id: u64) -> String {
    blob_key(id)
}

pub(crate) fn camera_original(
    storage: &StorageClient,
    request_id: u64,
    id: u64,
) -> Result<Option<CameraOriginal>, PhotoImportError> {
    if id == 0 || id > MAX_PHOTO_ID {
        return Err(PhotoImportError::InvalidFrame);
    }
    let Some(metadata) = storage.get(
        request_id,
        PHOTO_LIBRARY_ID,
        PHOTO_LIBRARY_QUOTA_BYTES,
        &metadata_key(id),
    )?
    else {
        return Ok(None);
    };
    decode_camera_metadata(&metadata).map(Some)
}

pub fn list_photo_exports(
    storage: &StorageClient,
    request_id: u64,
) -> Result<Vec<PhotoExport>, PhotoImportError> {
    let ids = if let Some(encoded_head) = storage.get(
        request_id,
        PHOTO_LIBRARY_ID,
        PHOTO_LIBRARY_QUOTA_BYTES,
        PHOTO_LIBRARY_HEAD_KEY,
    )? {
        let head = decode_head(&encoded_head)?;
        let page_count = head.slot_count.div_ceil(INDEX_PAGE_PHOTOS as u64);
        let mut ids = Vec::new();
        for page_number in 0..page_count {
            let page_number =
                u32::try_from(page_number).map_err(|_| PhotoImportError::ResourceExhausted)?;
            let page = load_page(storage, request_id, page_number)?
                .ok_or(PhotoImportError::InvalidIndex)?;
            let slots = page_slots(&head, u64::from(page_number));
            ids.extend(page.ids[..slots].iter().copied().filter(|id| *id != 0));
        }
        if ids.len() as u64 != head.active_count {
            return Err(PhotoImportError::InvalidIndex);
        }
        ids
    } else {
        match storage.get(
            request_id,
            PHOTO_LIBRARY_ID,
            PHOTO_LIBRARY_QUOTA_BYTES,
            PHOTO_LIBRARY_LEGACY_INDEX_KEY,
        )? {
            Some(encoded) => {
                let index = decode_legacy_index(&encoded)?;
                index.ids[..index.count].to_vec()
            }
            None => Vec::new(),
        }
    };

    ids.into_iter()
        .map(|id| {
            if let Some(original) = camera_original(storage, request_id, id)? {
                Ok(PhotoExport {
                    id,
                    kind: PhotoExportKind::CameraJpeg,
                    width: original.width,
                    height: original.height,
                    size_bytes: original.jpeg_size_bytes,
                    captured_milliseconds: Some(original.captured_milliseconds),
                    sha256: Some(original.sha256),
                })
            } else {
                Ok(PhotoExport {
                    id,
                    kind: PhotoExportKind::ScreenshotRgb565,
                    width: PHOTO_WIDTH as u16,
                    height: PHOTO_HEIGHT as u16,
                    size_bytes: PHOTO_FRAME_BYTES as u32,
                    captured_milliseconds: None,
                    sha256: None,
                })
            }
        })
        .collect()
}

pub fn read_photo_export(
    storage: &StorageClient,
    request_id: u64,
    photo: &PhotoExport,
) -> Result<Vec<u8>, PhotoImportError> {
    let key = match photo.kind {
        PhotoExportKind::CameraJpeg => original_blob_key(photo.id),
        PhotoExportKind::ScreenshotRgb565 => photo_blob_key(photo.id),
    };
    let descriptor = storage.open_blob(
        request_id,
        PHOTO_LIBRARY_ID,
        PHOTO_LIBRARY_QUOTA_BYTES,
        &key,
        photo.size_bytes,
    )?;
    let mut bytes = if let Some(descriptor) = descriptor {
        let mut file = File::from(descriptor);
        let mut bytes = Vec::with_capacity(photo.size_bytes as usize);
        file.read_to_end(&mut bytes)
            .map_err(StorageClientError::Io)?;
        bytes
    } else if photo.kind == PhotoExportKind::ScreenshotRgb565 {
        load_legacy_photo(storage, request_id, photo.id)?.ok_or(PhotoImportError::InvalidFrame)?
    } else {
        return Err(PhotoImportError::InvalidFrame);
    };
    if bytes.len() != photo.size_bytes as usize {
        bytes.clear();
        return Err(PhotoImportError::InvalidFrame);
    }
    if let Some(expected) = photo.sha256 {
        if Sha256::digest(&bytes).as_slice() != expected {
            bytes.clear();
            return Err(PhotoImportError::InvalidFrame);
        }
    }
    Ok(bytes)
}

fn photo_is_active_in(
    storage: &impl PhotoStorage,
    request_id: u64,
    id: u64,
) -> Result<bool, PhotoImportError> {
    if id == 0 || id > MAX_PHOTO_ID {
        return Err(PhotoImportError::InvalidFrame);
    }
    let Some(encoded_head) = storage.get(request_id, PHOTO_LIBRARY_HEAD_KEY)? else {
        let legacy = match storage.get(request_id, PHOTO_LIBRARY_LEGACY_INDEX_KEY)? {
            Some(encoded) => decode_legacy_index(&encoded)?,
            None => return Ok(false),
        };
        return Ok(legacy.ids[..legacy.count].contains(&id));
    };
    let head = decode_head(&encoded_head)?;
    let page_count = head.slot_count.div_ceil(INDEX_PAGE_PHOTOS as u64);
    for page_number in 0..page_count {
        let page_number_u32 =
            u32::try_from(page_number).map_err(|_| PhotoImportError::ResourceExhausted)?;
        let page = load_page(storage, request_id, page_number_u32)?
            .ok_or(PhotoImportError::InvalidIndex)?;
        let slots = page_slots(&head, page_number);
        for entry in &page.ids[..slots] {
            if *entry == id {
                return Ok(true);
            }
            if *entry > id {
                return Ok(false);
            }
        }
    }
    Ok(false)
}

fn import_frame(
    storage: &impl PhotoStorage,
    request_id: u64,
    frame: &[u8],
    _suggested_id: u64,
) -> Result<u64, PhotoImportError> {
    import_frame_with_original(storage, request_id, frame, None)
}

fn import_frame_with_original(
    storage: &impl PhotoStorage,
    request_id: u64,
    frame: &[u8],
    original: Option<(&[u8], u64)>,
) -> Result<u64, PhotoImportError> {
    if frame.len() != PHOTO_FRAME_BYTES {
        return Err(PhotoImportError::InvalidFrame);
    }
    if let Some((jpeg, _)) = original {
        if jpeg.is_empty()
            || jpeg.len() > MAX_CAMERA_JPEG_BYTES
            || !jpeg.starts_with(&[0xff, 0xd8])
            || !jpeg.ends_with(&[0xff, 0xd9])
        {
            return Err(PhotoImportError::InvalidFrame);
        }
    }
    let mut head = load_head_for_update(storage, request_id)?;
    let next_id = head
        .last_id
        .checked_add(1)
        .ok_or(PhotoImportError::ResourceExhausted)?;
    // IDs are broker-owned. The ABI keeps the hint for SDK 1.0 compatibility,
    // but an App must not be able to exhaust the shared namespace.
    let id = next_id.max(1);
    if id > MAX_PHOTO_ID {
        return Err(PhotoImportError::ResourceExhausted);
    }
    let page_number = u32::try_from(head.slot_count / INDEX_PAGE_PHOTOS as u64)
        .map_err(|_| PhotoImportError::ResourceExhausted)?;
    let position = (head.slot_count % INDEX_PAGE_PHOTOS as u64) as usize;
    let old_page = if position == 0 {
        None
    } else {
        let mut page =
            load_page(storage, request_id, page_number)?.ok_or(PhotoImportError::InvalidIndex)?;
        canonicalize_page(&mut page, position);
        Some(page)
    };
    let mut page = old_page.clone().unwrap_or_else(IndexPage::empty);

    if let Err(error) = put_blob(storage, request_id, &blob_key(id), frame) {
        cleanup_photo(storage, request_id, id);
        return Err(error);
    }
    if let Some((jpeg, captured_milliseconds)) = original {
        if let Err(error) = put_blob(storage, request_id, &original_blob_key(id), jpeg) {
            cleanup_photo(storage, request_id, id);
            return Err(error);
        }
        let metadata = encode_camera_metadata(jpeg, captured_milliseconds);
        if let Err(error) = storage.put(request_id, &metadata_key(id), &metadata) {
            cleanup_photo(storage, request_id, id);
            return Err(error.into());
        }
    }

    page.ids[position] = id;
    page.active_count = page
        .active_count
        .checked_add(1)
        .ok_or(PhotoImportError::InvalidIndex)?;
    if let Err(error) = store_page(storage, request_id, page_number, &page) {
        cleanup_photo(storage, request_id, id);
        return Err(error);
    }
    head.active_count = head
        .active_count
        .checked_add(1)
        .ok_or(PhotoImportError::ResourceExhausted)?;
    head.slot_count = head
        .slot_count
        .checked_add(1)
        .ok_or(PhotoImportError::ResourceExhausted)?;
    head.last_id = id;
    if let Err(error) = store_head(storage, request_id, &head) {
        rollback_page(storage, request_id, page_number, old_page.as_ref());
        cleanup_photo(storage, request_id, id);
        return Err(error);
    }
    Ok(id)
}

fn remove_frame(
    storage: &impl PhotoStorage,
    request_id: u64,
    id: u64,
) -> Result<bool, PhotoImportError> {
    if id == 0 || id > MAX_PHOTO_ID {
        return Err(PhotoImportError::InvalidFrame);
    }
    let mut head = load_head_for_update(storage, request_id)?;
    let page_count = head.slot_count.div_ceil(INDEX_PAGE_PHOTOS as u64);
    for page_number in 0..page_count {
        let page_number =
            u32::try_from(page_number).map_err(|_| PhotoImportError::ResourceExhausted)?;
        let mut page =
            load_page(storage, request_id, page_number)?.ok_or(PhotoImportError::InvalidIndex)?;
        let slots = page_slots(&head, u64::from(page_number));
        canonicalize_page(&mut page, slots);
        let Some(position) = page.ids[..slots].iter().position(|entry| *entry == id) else {
            continue;
        };
        let old_page = page.clone();
        page.ids[position] = 0;
        page.active_count = page
            .active_count
            .checked_sub(1)
            .ok_or(PhotoImportError::InvalidIndex)?;
        store_page(storage, request_id, page_number, &page)?;
        head.active_count = head
            .active_count
            .checked_sub(1)
            .ok_or(PhotoImportError::InvalidIndex)?;
        if let Err(error) = store_head(storage, request_id, &head) {
            let _ = store_page(storage, request_id, page_number, &old_page);
            return Err(error);
        }
        delete_photo_storage(storage, request_id, id)?;
        return Ok(true);
    }
    Ok(false)
}

fn load_head_for_update(
    storage: &impl PhotoStorage,
    request_id: u64,
) -> Result<Head, PhotoImportError> {
    if let Some(encoded) = storage.get(request_id, PHOTO_LIBRARY_HEAD_KEY)? {
        let mut head = decode_head(&encoded)?;
        reconcile_head(storage, request_id, &mut head)?;
        return Ok(head);
    }
    let legacy = match storage.get(request_id, PHOTO_LIBRARY_LEGACY_INDEX_KEY)? {
        Some(encoded) => decode_legacy_index(&encoded)?,
        None => LegacyIndex::empty(),
    };
    migrate_legacy(storage, request_id, &legacy)
}

fn reconcile_head(
    storage: &impl PhotoStorage,
    request_id: u64,
    head: &mut Head,
) -> Result<(), PhotoImportError> {
    let page_count = head.slot_count.div_ceil(INDEX_PAGE_PHOTOS as u64);
    let mut active_count = 0_u64;
    let mut previous_id = 0_u64;
    for page_number in 0..page_count {
        let page_number_u32 =
            u32::try_from(page_number).map_err(|_| PhotoImportError::ResourceExhausted)?;
        let mut page = load_page(storage, request_id, page_number_u32)?
            .ok_or(PhotoImportError::InvalidIndex)?;
        let slots = page_slots(head, page_number);
        canonicalize_page(&mut page, slots);
        for id in &page.ids[..slots] {
            if *id == 0 {
                continue;
            }
            if *id <= previous_id || *id > head.last_id {
                return Err(PhotoImportError::InvalidIndex);
            }
            previous_id = *id;
            active_count = active_count
                .checked_add(1)
                .ok_or(PhotoImportError::ResourceExhausted)?;
        }
    }
    head.active_count = active_count;
    Ok(())
}

fn canonicalize_page(page: &mut IndexPage, committed_slots: usize) {
    page.ids[committed_slots..].fill(0);
    page.active_count = page.ids[..committed_slots]
        .iter()
        .filter(|id| **id != 0)
        .count() as u16;
}

fn migrate_legacy(
    storage: &impl PhotoStorage,
    request_id: u64,
    legacy: &LegacyIndex,
) -> Result<Head, PhotoImportError> {
    let head = Head {
        active_count: legacy.count as u64,
        slot_count: legacy.count as u64,
        last_id: legacy
            .count
            .checked_sub(1)
            .map_or(0, |last| legacy.ids[last]),
    };
    if legacy.count != 0 {
        let mut page = IndexPage::empty();
        page.active_count = legacy.count as u16;
        page.ids[..legacy.count].copy_from_slice(&legacy.ids[..legacy.count]);
        store_page(storage, request_id, 0, &page)?;
        if let Err(error) = store_head(storage, request_id, &head) {
            let _ = storage.delete(request_id, &page_key(0));
            return Err(error);
        }
    } else {
        store_head(storage, request_id, &head)?;
    }
    Ok(head)
}

fn store_head(
    storage: &impl PhotoStorage,
    request_id: u64,
    head: &Head,
) -> Result<(), PhotoImportError> {
    storage
        .put(request_id, PHOTO_LIBRARY_HEAD_KEY, &encode_head(head))
        .map_err(PhotoImportError::Storage)
}

fn encode_head(head: &Head) -> [u8; HEAD_BYTES] {
    let mut encoded = [0_u8; HEAD_BYTES];
    encoded[..4].copy_from_slice(&HEAD_MAGIC);
    encoded[4] = 2;
    encoded[8..16].copy_from_slice(&head.active_count.to_le_bytes());
    encoded[16..24].copy_from_slice(&head.slot_count.to_le_bytes());
    encoded[24..32].copy_from_slice(&head.last_id.to_le_bytes());
    encoded
}

fn decode_head(encoded: &[u8]) -> Result<Head, PhotoImportError> {
    if encoded.len() != HEAD_BYTES
        || encoded[..4] != HEAD_MAGIC
        || encoded[4] != 2
        || encoded[5..8] != [0; 3]
    {
        return Err(PhotoImportError::InvalidIndex);
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
        return Err(PhotoImportError::InvalidIndex);
    }
    Ok(head)
}

fn load_page(
    storage: &impl PhotoStorage,
    request_id: u64,
    page_number: u32,
) -> Result<Option<IndexPage>, PhotoImportError> {
    storage
        .get(request_id, &page_key(page_number))?
        .map(|encoded| decode_page(page_number, &encoded))
        .transpose()
}

fn store_page(
    storage: &impl PhotoStorage,
    request_id: u64,
    page_number: u32,
    page: &IndexPage,
) -> Result<(), PhotoImportError> {
    storage
        .put(
            request_id,
            &page_key(page_number),
            &encode_page(page_number, page),
        )
        .map_err(PhotoImportError::Storage)
}

fn rollback_page(
    storage: &impl PhotoStorage,
    request_id: u64,
    page_number: u32,
    old_page: Option<&IndexPage>,
) {
    if let Some(page) = old_page {
        let _ = store_page(storage, request_id, page_number, page);
    } else {
        let _ = storage.delete(request_id, &page_key(page_number));
    }
}

fn encode_page(page_number: u32, page: &IndexPage) -> Vec<u8> {
    let mut encoded = vec![0_u8; PAGE_BYTES];
    encoded[..4].copy_from_slice(&PAGE_MAGIC);
    encoded[4] = 2;
    encoded[6..8].copy_from_slice(&page.active_count.to_le_bytes());
    encoded[8..12].copy_from_slice(&page_number.to_le_bytes());
    for (position, id) in page.ids.iter().enumerate() {
        let start = PAGE_HEADER_BYTES + position * 8;
        encoded[start..start + 8].copy_from_slice(&id.to_le_bytes());
    }
    encoded
}

fn decode_page(page_number: u32, encoded: &[u8]) -> Result<IndexPage, PhotoImportError> {
    if encoded.len() != PAGE_BYTES
        || encoded[..4] != PAGE_MAGIC
        || encoded[4] != 2
        || encoded[5] != 0
        || u32::from_le_bytes(encoded[8..12].try_into().unwrap()) != page_number
        || encoded[12..16] != [0; 4]
    {
        return Err(PhotoImportError::InvalidIndex);
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
                return Err(PhotoImportError::InvalidIndex);
            }
            previous = id;
            actual = actual
                .checked_add(1)
                .ok_or(PhotoImportError::InvalidIndex)?;
        }
        page.ids[position] = id;
    }
    if actual != page.active_count {
        return Err(PhotoImportError::InvalidIndex);
    }
    Ok(page)
}

fn decode_legacy_index(encoded: &[u8]) -> Result<LegacyIndex, PhotoImportError> {
    if encoded.len() < LEGACY_HEADER_BYTES
        || encoded[..4] != LEGACY_MAGIC
        || encoded[4] != 1
        || encoded[6] != 0
        || encoded[7] != 0
    {
        return Err(PhotoImportError::InvalidIndex);
    }
    let count = encoded[5] as usize;
    if count > LEGACY_MAX_PHOTOS || encoded.len() != LEGACY_HEADER_BYTES + count * 8 {
        return Err(PhotoImportError::InvalidIndex);
    }
    let mut index = LegacyIndex::empty();
    index.count = count;
    for position in 0..count {
        let start = LEGACY_HEADER_BYTES + position * 8;
        let id = u64::from_le_bytes(encoded[start..start + 8].try_into().unwrap());
        if id == 0 || (position > 0 && id <= index.ids[position - 1]) {
            return Err(PhotoImportError::InvalidIndex);
        }
        index.ids[position] = id;
    }
    Ok(index)
}

fn page_key(page_number: u32) -> String {
    format!("index.v2.{page_number:08x}")
}

fn blob_key(id: u64) -> String {
    format!("p{id:016x}.rgb565")
}

pub(crate) fn original_blob_key(id: u64) -> String {
    format!("p{id:016x}.jpg")
}

fn metadata_key(id: u64) -> String {
    format!("p{id:016x}.meta")
}

fn legacy_chunk_key(id: u64, chunk: usize) -> String {
    format!("p{id:016x}.c{chunk:02}")
}

fn page_slots(head: &Head, page_number: u64) -> usize {
    head.slot_count
        .saturating_sub(page_number * INDEX_PAGE_PHOTOS as u64)
        .min(INDEX_PAGE_PHOTOS as u64) as usize
}

fn put_blob(
    storage: &impl PhotoStorage,
    request_id: u64,
    key: &str,
    value: &[u8],
) -> Result<(), PhotoImportError> {
    if value.is_empty() || value.len() > cp0_storage_protocol::MAX_STORAGE_BLOB_BYTES {
        return Err(PhotoImportError::InvalidFrame);
    }
    for (chunk, bytes) in value.chunks(CHUNK_BYTES).enumerate() {
        storage.put_blob_chunk(request_id, key, chunk * CHUNK_BYTES, value.len(), bytes)?;
    }
    Ok(())
}

fn encode_camera_metadata(jpeg: &[u8], captured_milliseconds: u64) -> [u8; PHOTO_METADATA_BYTES] {
    let mut metadata = [0_u8; PHOTO_METADATA_BYTES];
    metadata[..4].copy_from_slice(&PHOTO_METADATA_MAGIC);
    metadata[4] = PHOTO_METADATA_VERSION;
    metadata[5] = PHOTO_KIND_CAMERA;
    metadata[8..10].copy_from_slice(&CAMERA_PHOTO_WIDTH.to_le_bytes());
    metadata[10..12].copy_from_slice(&CAMERA_PHOTO_HEIGHT.to_le_bytes());
    metadata[12..16].copy_from_slice(&(jpeg.len() as u32).to_le_bytes());
    metadata[16..24].copy_from_slice(&captured_milliseconds.to_le_bytes());
    metadata[24..].copy_from_slice(&Sha256::digest(jpeg));
    metadata
}

fn decode_camera_metadata(metadata: &[u8]) -> Result<CameraOriginal, PhotoImportError> {
    if metadata.len() != PHOTO_METADATA_BYTES
        || metadata[..4] != PHOTO_METADATA_MAGIC
        || metadata[4] != PHOTO_METADATA_VERSION
        || metadata[5] != PHOTO_KIND_CAMERA
        || metadata[6..8] != [0; 2]
    {
        return Err(PhotoImportError::InvalidFrame);
    }
    let original = CameraOriginal {
        width: u16::from_le_bytes(metadata[8..10].try_into().unwrap()),
        height: u16::from_le_bytes(metadata[10..12].try_into().unwrap()),
        jpeg_size_bytes: u32::from_le_bytes(metadata[12..16].try_into().unwrap()),
        captured_milliseconds: u64::from_le_bytes(metadata[16..24].try_into().unwrap()),
        sha256: metadata[24..].try_into().unwrap(),
    };
    if original.width != CAMERA_PHOTO_WIDTH
        || original.height != CAMERA_PHOTO_HEIGHT
        || original.jpeg_size_bytes == 0
        || original.jpeg_size_bytes as usize > MAX_CAMERA_JPEG_BYTES
    {
        return Err(PhotoImportError::InvalidFrame);
    }
    Ok(original)
}

fn delete_photo_storage(
    storage: &impl PhotoStorage,
    request_id: u64,
    id: u64,
) -> Result<bool, StorageClientError> {
    let mut existed = storage.delete_blob(request_id, &blob_key(id))?;
    existed |= storage.delete_blob(request_id, &original_blob_key(id))?;
    existed |= storage.delete(request_id, &metadata_key(id))?;
    for chunk in 0..CHUNK_COUNT {
        existed |= storage.delete(request_id, &legacy_chunk_key(id, chunk))?;
    }
    Ok(existed)
}

fn cleanup_photo(storage: &impl PhotoStorage, request_id: u64, id: u64) {
    let _ = delete_photo_storage(storage, request_id, id);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use cp0_storage_protocol::StorageErrorCode;

    use super::*;

    #[derive(Default)]
    struct MemoryStorage {
        values: Mutex<BTreeMap<String, Vec<u8>>>,
        fail_put_key: Mutex<Option<String>>,
    }

    impl PhotoStorage for MemoryStorage {
        fn put(&self, _request_id: u64, key: &str, value: &[u8]) -> Result<(), StorageClientError> {
            if self.fail_put_key.lock().unwrap().as_deref() == Some(key) {
                return Err(StorageClientError::Service(StorageErrorCode::Internal));
            }
            self.values
                .lock()
                .unwrap()
                .insert(key.into(), value.to_vec());
            Ok(())
        }

        fn get(&self, _request_id: u64, key: &str) -> Result<Option<Vec<u8>>, StorageClientError> {
            Ok(self.values.lock().unwrap().get(key).cloned())
        }

        fn delete(&self, _request_id: u64, key: &str) -> Result<bool, StorageClientError> {
            Ok(self.values.lock().unwrap().remove(key).is_some())
        }

        fn put_blob_chunk(
            &self,
            _request_id: u64,
            key: &str,
            offset: usize,
            total_bytes: usize,
            value: &[u8],
        ) -> Result<(), StorageClientError> {
            if self.fail_put_key.lock().unwrap().as_deref() == Some(key) {
                return Err(StorageClientError::Service(StorageErrorCode::Internal));
            }
            let mut values = self.values.lock().unwrap();
            let frame = values.entry(key.into()).or_default();
            if frame.len() != offset {
                return Err(StorageClientError::Service(StorageErrorCode::Internal));
            }
            frame.extend_from_slice(value);
            if frame.len() > total_bytes {
                return Err(StorageClientError::Service(StorageErrorCode::Internal));
            }
            Ok(())
        }

        fn delete_blob(&self, _request_id: u64, key: &str) -> Result<bool, StorageClientError> {
            Ok(self.values.lock().unwrap().remove(key).is_some())
        }
    }

    #[test]
    fn imports_exact_frame_and_commits_v2_head_last() {
        let storage = MemoryStorage::default();
        let frame = vec![0x5a; PHOTO_FRAME_BYTES];
        assert_eq!(import_frame(&storage, 7, &frame, 42).unwrap(), 1);
        let values = storage.values.lock().unwrap();
        assert_eq!(values["p0000000000000001.rgb565"].len(), PHOTO_FRAME_BYTES);
        assert_eq!(
            decode_head(&values[PHOTO_LIBRARY_HEAD_KEY])
                .unwrap()
                .active_count,
            1
        );
        let page = decode_page(0, &values[&page_key(0)]).unwrap();
        assert_eq!(page.ids[0], 1);
    }

    #[test]
    fn stores_and_removes_720p_camera_original_with_thumbnail_metadata() {
        let storage = MemoryStorage::default();
        let thumbnail = vec![0x5a; PHOTO_FRAME_BYTES];
        let jpeg = vec![0xff, 0xd8, 1, 2, 3, 4, 0xff, 0xd9];
        assert_eq!(
            import_frame_with_original(&storage, 9, &thumbnail, Some((&jpeg, 1234))).unwrap(),
            1
        );
        {
            let values = storage.values.lock().unwrap();
            assert_eq!(values[&blob_key(1)], thumbnail);
            assert_eq!(values[&original_blob_key(1)], jpeg);
            let metadata = &values[&metadata_key(1)];
            assert_eq!(metadata.len(), PHOTO_METADATA_BYTES);
            assert_eq!(metadata[..4], PHOTO_METADATA_MAGIC);
            assert_eq!(
                u16::from_le_bytes(metadata[8..10].try_into().unwrap()),
                CAMERA_PHOTO_WIDTH
            );
            assert_eq!(
                u16::from_le_bytes(metadata[10..12].try_into().unwrap()),
                CAMERA_PHOTO_HEIGHT
            );
            assert_eq!(
                u64::from_le_bytes(metadata[16..24].try_into().unwrap()),
                1234
            );
        }
        assert!(remove_frame(&storage, 10, 1).unwrap());
        let values = storage.values.lock().unwrap();
        assert!(!values.contains_key(&blob_key(1)));
        assert!(!values.contains_key(&original_blob_key(1)));
        assert!(!values.contains_key(&metadata_key(1)));
    }

    #[test]
    fn rolls_back_uncommitted_frame_when_head_commit_fails() {
        let storage = MemoryStorage::default();
        *storage.fail_put_key.lock().unwrap() = Some(PHOTO_LIBRARY_HEAD_KEY.into());
        let frame = vec![0x33; PHOTO_FRAME_BYTES];
        assert!(matches!(
            import_frame(&storage, 8, &frame, 9),
            Err(PhotoImportError::Storage(_))
        ));
        assert!(storage.values.lock().unwrap().is_empty());
    }

    #[test]
    fn keeps_more_than_thirty_two_frames_without_eviction() {
        let storage = MemoryStorage::default();
        let frame = vec![0x11; PHOTO_FRAME_BYTES];
        for id in 1..=33 {
            assert_eq!(import_frame(&storage, id, &frame, id).unwrap(), id);
        }
        let values = storage.values.lock().unwrap();
        let head = decode_head(&values[PHOTO_LIBRARY_HEAD_KEY]).unwrap();
        assert_eq!(head.active_count, 33);
        assert_eq!(head.slot_count, 33);
        assert!(values.contains_key("p0000000000000001.rgb565"));
        assert!(values.contains_key("p0000000000000021.rgb565"));
    }

    #[test]
    fn spans_index_pages_and_keeps_tombstones_until_explicit_deletion() {
        let storage = MemoryStorage::default();
        let frame = vec![0x22; PHOTO_FRAME_BYTES];
        for id in 1..=260 {
            assert_eq!(import_frame(&storage, id, &frame, id).unwrap(), id);
        }
        assert!(remove_frame(&storage, 300, 2).unwrap());
        assert!(!remove_frame(&storage, 301, 2).unwrap());

        let values = storage.values.lock().unwrap();
        let head = decode_head(&values[PHOTO_LIBRARY_HEAD_KEY]).unwrap();
        assert_eq!(head.active_count, 259);
        assert_eq!(head.slot_count, 260);
        let first = decode_page(0, &values[&page_key(0)]).unwrap();
        let second = decode_page(1, &values[&page_key(1)]).unwrap();
        assert_eq!(first.active_count, 255);
        assert_eq!(first.ids[0], 1);
        assert_eq!(first.ids[1], 0);
        assert_eq!(first.ids[255], 256);
        assert_eq!(second.active_count, 4);
        assert_eq!(&second.ids[..4], &[257, 258, 259, 260]);
        assert!(values.contains_key("p0000000000000001.rgb565"));
        assert!(!values.contains_key("p0000000000000002.rgb565"));
    }

    #[test]
    fn recovers_page_writes_left_ahead_of_the_committed_head() {
        let storage = MemoryStorage::default();
        let frame = vec![0x66; PHOTO_FRAME_BYTES];
        assert_eq!(import_frame(&storage, 1, &frame, 1).unwrap(), 1);

        {
            let mut values = storage.values.lock().unwrap();
            let mut page = decode_page(0, &values[&page_key(0)]).unwrap();
            page.ids[1] = 999;
            page.active_count = 2;
            values.insert(page_key(0), encode_page(0, &page));
            values.insert(blob_key(999), frame.clone());
        }

        assert_eq!(import_frame(&storage, 2, &frame, u64::MAX).unwrap(), 2);
        let values = storage.values.lock().unwrap();
        let head = decode_head(&values[PHOTO_LIBRARY_HEAD_KEY]).unwrap();
        let page = decode_page(0, &values[&page_key(0)]).unwrap();
        assert_eq!(head.active_count, 2);
        assert_eq!(head.slot_count, 2);
        assert_eq!(&page.ids[..2], &[1, 2]);
        assert!(values.contains_key(&blob_key(1)));
    }

    #[test]
    fn reconciles_a_tombstone_committed_before_its_head_count() {
        let storage = MemoryStorage::default();
        let frame = vec![0x77; PHOTO_FRAME_BYTES];
        assert_eq!(import_frame(&storage, 1, &frame, 1).unwrap(), 1);
        assert_eq!(import_frame(&storage, 2, &frame, 2).unwrap(), 2);

        {
            let mut values = storage.values.lock().unwrap();
            let mut page = decode_page(0, &values[&page_key(0)]).unwrap();
            page.ids[0] = 0;
            page.active_count = 1;
            values.insert(page_key(0), encode_page(0, &page));
        }

        assert_eq!(import_frame(&storage, 3, &frame, 3).unwrap(), 3);
        let values = storage.values.lock().unwrap();
        let head = decode_head(&values[PHOTO_LIBRARY_HEAD_KEY]).unwrap();
        let page = decode_page(0, &values[&page_key(0)]).unwrap();
        assert_eq!(head.active_count, 2);
        assert_eq!(head.slot_count, 3);
        assert_eq!(&page.ids[..3], &[0, 2, 3]);
    }

    #[test]
    fn migrates_v1_index_without_removing_legacy_data() {
        let storage = MemoryStorage::default();
        let mut legacy = vec![0_u8; LEGACY_HEADER_BYTES + 2 * 8];
        legacy[..4].copy_from_slice(&LEGACY_MAGIC);
        legacy[4] = 1;
        legacy[5] = 2;
        legacy[8..16].copy_from_slice(&7_u64.to_le_bytes());
        legacy[16..24].copy_from_slice(&9_u64.to_le_bytes());
        storage
            .values
            .lock()
            .unwrap()
            .insert(PHOTO_LIBRARY_LEGACY_INDEX_KEY.into(), legacy.clone());
        let frame = vec![0x44; PHOTO_FRAME_BYTES];
        assert_eq!(import_frame(&storage, 12, &frame, 8).unwrap(), 10);
        let values = storage.values.lock().unwrap();
        assert_eq!(values[PHOTO_LIBRARY_LEGACY_INDEX_KEY], legacy);
        assert_eq!(
            decode_head(&values[PHOTO_LIBRARY_HEAD_KEY])
                .unwrap()
                .active_count,
            3
        );
        let page = decode_page(0, &values[&page_key(0)]).unwrap();
        assert_eq!(&page.ids[..3], &[7, 9, 10]);
    }
}
