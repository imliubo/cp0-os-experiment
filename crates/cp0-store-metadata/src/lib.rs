use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const STORE_LISTING_SCHEMA_VERSION: u32 = 1;
pub const MAX_LISTING_BYTES: usize = 32 * 1024;
pub const MAX_LOCALIZATIONS: usize = 8;
pub const MAX_KEYWORDS: usize = 8;
pub const MAX_SCREENSHOTS: usize = 5;
pub const MAX_ICON_BYTES: u64 = 64 * 1024;
pub const MAX_SCREENSHOT_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoreListing {
    pub schema_version: u32,
    pub app_id: String,
    pub version: String,
    pub default_locale: String,
    pub category: StoreCategory,
    pub age_rating: AgeRating,
    pub privacy_url: String,
    pub support_url: String,
    pub icon: ImageAsset,
    pub screenshots: Vec<ImageAsset>,
    pub localizations: Vec<LocalizedListing>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageAsset {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalizedListing {
    pub locale: String,
    pub name: String,
    pub subtitle: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub release_notes: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StoreCategory {
    DeveloperTools,
    Education,
    Entertainment,
    Games,
    Hardware,
    Media,
    Productivity,
    Utilities,
}

impl StoreCategory {
    pub const ALL: [Self; 8] = [
        Self::DeveloperTools,
        Self::Education,
        Self::Entertainment,
        Self::Games,
        Self::Hardware,
        Self::Media,
        Self::Productivity,
        Self::Utilities,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeveloperTools => "developer-tools",
            Self::Education => "education",
            Self::Entertainment => "entertainment",
            Self::Games => "games",
            Self::Hardware => "hardware",
            Self::Media => "media",
            Self::Productivity => "productivity",
            Self::Utilities => "utilities",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgeRating {
    #[serde(rename = "4+")]
    FourPlus,
    #[serde(rename = "9+")]
    NinePlus,
    #[serde(rename = "12+")]
    TwelvePlus,
    #[serde(rename = "17+")]
    SeventeenPlus,
}

impl AgeRating {
    pub const ALL: [Self; 4] = [
        Self::FourPlus,
        Self::NinePlus,
        Self::TwelvePlus,
        Self::SeventeenPlus,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FourPlus => "4+",
            Self::NinePlus => "9+",
            Self::TwelvePlus => "12+",
            Self::SeventeenPlus => "17+",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubmissionState {
    Draft,
    Uploading,
    Processing,
    ReadyForReview,
    InReview,
    PendingSecondaryReview,
    NeedsChanges,
    Approved,
    Rejected,
    Withdrawn,
}

impl SubmissionState {
    pub const ALL: [Self; 10] = [
        Self::Draft,
        Self::Uploading,
        Self::Processing,
        Self::ReadyForReview,
        Self::InReview,
        Self::PendingSecondaryReview,
        Self::NeedsChanges,
        Self::Approved,
        Self::Rejected,
        Self::Withdrawn,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Uploading => "uploading",
            Self::Processing => "processing",
            Self::ReadyForReview => "ready-for-review",
            Self::InReview => "in-review",
            Self::PendingSecondaryReview => "pending-secondary-review",
            Self::NeedsChanges => "needs-changes",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Withdrawn => "withdrawn",
        }
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Uploading | Self::Withdrawn)
                | (Self::Uploading, Self::Processing | Self::Withdrawn)
                | (
                    Self::Processing,
                    Self::ReadyForReview | Self::NeedsChanges | Self::Rejected | Self::Withdrawn
                )
                | (Self::ReadyForReview, Self::InReview | Self::Withdrawn)
                | (
                    Self::InReview,
                    Self::PendingSecondaryReview
                        | Self::NeedsChanges
                        | Self::Approved
                        | Self::Rejected
                        | Self::Withdrawn
                )
                | (
                    Self::PendingSecondaryReview,
                    Self::InReview | Self::Withdrawn
                )
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseState {
    Ready,
    Scheduled,
    Publishing,
    PublishFailed,
    Published,
    Paused,
    Removed,
}

impl ReleaseState {
    pub const ALL: [Self; 7] = [
        Self::Ready,
        Self::Scheduled,
        Self::Publishing,
        Self::PublishFailed,
        Self::Published,
        Self::Paused,
        Self::Removed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Scheduled => "scheduled",
            Self::Publishing => "publishing",
            Self::PublishFailed => "publish-failed",
            Self::Published => "published",
            Self::Paused => "paused",
            Self::Removed => "removed",
        }
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Ready,
                Self::Scheduled | Self::Publishing | Self::Removed
            ) | (
                Self::Scheduled,
                Self::Ready | Self::Publishing | Self::Removed
            ) | (Self::Publishing, Self::Published | Self::PublishFailed)
                | (Self::PublishFailed, Self::Publishing | Self::Removed)
                | (Self::Published, Self::Paused | Self::Removed)
                | (Self::Paused, Self::Published | Self::Removed)
        )
    }
}

#[derive(Debug)]
pub enum ListingError {
    Io(std::io::Error),
    TooLarge,
    Json(serde_json::Error),
    Invalid(Vec<String>),
}

impl fmt::Display for ListingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "cannot read Store listing: {error}"),
            Self::TooLarge => write!(
                formatter,
                "Store listing exceeds the {MAX_LISTING_BYTES}-byte limit"
            ),
            Self::Json(error) => write!(formatter, "invalid Store listing JSON: {error}"),
            Self::Invalid(errors) => write!(formatter, "{}", errors.join("\n")),
        }
    }
}

impl std::error::Error for ListingError {}

pub fn load_and_validate(path: impl AsRef<Path>) -> Result<StoreListing, ListingError> {
    let path = path.as_ref();
    let metadata = std::fs::metadata(path).map_err(ListingError::Io)?;
    if metadata.len() > MAX_LISTING_BYTES as u64 {
        return Err(ListingError::TooLarge);
    }
    let encoded = std::fs::read(path).map_err(ListingError::Io)?;
    parse_and_validate(&encoded)
}

pub fn parse_and_validate(encoded: &[u8]) -> Result<StoreListing, ListingError> {
    if encoded.len() > MAX_LISTING_BYTES {
        return Err(ListingError::TooLarge);
    }
    let listing: StoreListing = serde_json::from_slice(encoded).map_err(ListingError::Json)?;
    validate(&listing).map_err(ListingError::Invalid)?;
    Ok(listing)
}

pub fn validate(listing: &StoreListing) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if listing.schema_version != STORE_LISTING_SCHEMA_VERSION {
        errors.push(format!(
            "schema_version must be {STORE_LISTING_SCHEMA_VERSION}"
        ));
    }
    if !cp0_manifest::is_valid_app_id(&listing.app_id) {
        errors.push("app_id must be a lowercase reverse-domain application ID".into());
    }
    if !cp0_manifest::is_valid_app_version(&listing.version) {
        errors.push("version must be a valid three-part semantic version".into());
    }
    if !is_valid_locale(&listing.default_locale) {
        errors.push("default_locale must be a canonical supported locale tag".into());
    }
    for (field, url) in [
        ("privacy_url", &listing.privacy_url),
        ("support_url", &listing.support_url),
    ] {
        if !is_valid_https_url(url) {
            errors.push(format!(
                "{field} must be bounded HTTPS without credentials or fragments"
            ));
        }
    }

    validate_asset(&listing.icon, "icon", MAX_ICON_BYTES, (48, 48), &mut errors);
    if !(1..=MAX_SCREENSHOTS).contains(&listing.screenshots.len()) {
        errors.push(format!(
            "screenshots must contain between 1 and {MAX_SCREENSHOTS} images"
        ));
    }
    let mut asset_paths = BTreeSet::from([listing.icon.path.as_str()]);
    for (index, screenshot) in listing.screenshots.iter().enumerate() {
        validate_asset(
            screenshot,
            &format!("screenshots[{index}]"),
            MAX_SCREENSHOT_BYTES,
            (320, 170),
            &mut errors,
        );
        if !asset_paths.insert(&screenshot.path) {
            errors.push(format!(
                "screenshots[{index}].path duplicates another image asset"
            ));
        }
    }

    if !(1..=MAX_LOCALIZATIONS).contains(&listing.localizations.len()) {
        errors.push(format!(
            "localizations must contain between 1 and {MAX_LOCALIZATIONS} entries"
        ));
    }
    let mut previous_locale: Option<&str> = None;
    let mut has_default = false;
    for (index, localization) in listing.localizations.iter().enumerate() {
        let field = format!("localizations[{index}]");
        if !is_valid_locale(&localization.locale) {
            errors.push(format!("{field}.locale is not canonical"));
        }
        if previous_locale.is_some_and(|previous| previous >= localization.locale.as_str()) {
            errors.push("localizations must be unique and sorted by locale".into());
        }
        previous_locale = Some(&localization.locale);
        has_default |= localization.locale == listing.default_locale;
        validate_inline(
            &localization.name,
            1,
            32,
            &format!("{field}.name"),
            &mut errors,
        );
        validate_inline(
            &localization.subtitle,
            1,
            48,
            &format!("{field}.subtitle"),
            &mut errors,
        );
        validate_prose(
            &localization.description,
            1,
            1024,
            &format!("{field}.description"),
            &mut errors,
        );
        validate_prose(
            &localization.release_notes,
            1,
            512,
            &format!("{field}.release_notes"),
            &mut errors,
        );
        if localization.keywords.len() > MAX_KEYWORDS {
            errors.push(format!(
                "{field}.keywords must contain at most {MAX_KEYWORDS} entries"
            ));
        }
        let mut previous_keyword: Option<&str> = None;
        for keyword in &localization.keywords {
            validate_inline(keyword, 1, 24, &format!("{field}.keyword"), &mut errors);
            if keyword.len() > 48 {
                errors.push(format!("{field}.keyword exceeds 48 bytes"));
            }
            if previous_keyword.is_some_and(|previous| previous >= keyword.as_str()) {
                errors.push(format!(
                    "{field}.keywords must be unique and sorted lexicographically"
                ));
            }
            previous_keyword = Some(keyword);
        }
    }
    if !has_default {
        errors.push("default_locale must have a matching localization".into());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn validate_png_structure(
    encoded: &[u8],
    expected_width: u16,
    expected_height: u16,
) -> Result<(), String> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if encoded.len() < SIGNATURE.len() || &encoded[..SIGNATURE.len()] != SIGNATURE {
        return Err("image does not have a PNG signature".into());
    }

    let mut offset = SIGNATURE.len();
    let mut seen_header = false;
    let mut seen_data = false;
    let mut data_ended = false;
    while offset < encoded.len() {
        let header_end = offset
            .checked_add(8)
            .filter(|end| *end <= encoded.len())
            .ok_or_else(|| "PNG chunk header is truncated".to_owned())?;
        let length = u32::from_be_bytes(encoded[offset..offset + 4].try_into().unwrap()) as usize;
        let data_end = header_end
            .checked_add(length)
            .filter(|end| {
                end.checked_add(4)
                    .is_some_and(|crc_end| crc_end <= encoded.len())
            })
            .ok_or_else(|| "PNG chunk length is outside the file".to_owned())?;
        let crc_end = data_end + 4;
        let kind: [u8; 4] = encoded[offset + 4..header_end].try_into().unwrap();
        if !kind.iter().all(u8::is_ascii_alphabetic) {
            return Err("PNG chunk type is invalid".into());
        }
        let expected_crc = u32::from_be_bytes(encoded[data_end..crc_end].try_into().unwrap());
        if png_crc32(&encoded[offset + 4..data_end]) != expected_crc {
            return Err("PNG chunk CRC does not match".into());
        }
        let data = &encoded[header_end..data_end];

        match &kind {
            b"IHDR" => {
                if seen_header || offset != SIGNATURE.len() || data.len() != 13 {
                    return Err("PNG must contain exactly one leading IHDR chunk".into());
                }
                let width = u32::from_be_bytes(data[0..4].try_into().unwrap());
                let height = u32::from_be_bytes(data[4..8].try_into().unwrap());
                if width != u32::from(expected_width) || height != u32::from(expected_height) {
                    return Err(format!(
                        "PNG dimensions must be {expected_width}x{expected_height}"
                    ));
                }
                let valid_depth = matches!(
                    (data[8], data[9]),
                    (1 | 2 | 4 | 8 | 16, 0) | (8 | 16, 2) | (1 | 2 | 4 | 8, 3) | (8 | 16, 4 | 6)
                );
                if !valid_depth || data[10] != 0 || data[11] != 0 || !matches!(data[12], 0 | 1) {
                    return Err("PNG IHDR encoding fields are invalid".into());
                }
                seen_header = true;
            }
            b"PLTE" => {
                if !seen_header
                    || seen_data
                    || data.is_empty()
                    || data.len() > 768
                    || data.len() % 3 != 0
                {
                    return Err("PNG palette chunk is invalid or out of order".into());
                }
            }
            b"IDAT" => {
                if !seen_header || data_ended {
                    return Err("PNG image data chunks are invalid or non-contiguous".into());
                }
                seen_data = true;
            }
            b"IEND" => {
                if !seen_header || !seen_data || !data.is_empty() || crc_end != encoded.len() {
                    return Err("PNG IEND chunk is invalid or not final".into());
                }
                return Ok(());
            }
            _ => {
                if kind[0].is_ascii_uppercase() {
                    return Err("PNG contains an unsupported critical chunk".into());
                }
                if seen_data {
                    data_ended = true;
                }
            }
        }
        if &kind != b"IDAT" && seen_data {
            data_ended = true;
        }
        offset = crc_end;
    }
    Err("PNG is missing a final IEND chunk".into())
}

fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn validate_asset(
    asset: &ImageAsset,
    field: &str,
    max_bytes: u64,
    expected_dimensions: (u16, u16),
    errors: &mut Vec<String>,
) {
    if !is_valid_asset_path(&asset.path) {
        errors.push(format!("{field}.path must be a safe relative PNG path"));
    }
    if !is_lower_hex(&asset.sha256, 32) {
        errors.push(format!("{field}.sha256 must be lowercase SHA-256"));
    }
    if !(1..=max_bytes).contains(&asset.bytes) {
        errors.push(format!("{field}.bytes must be between 1 and {max_bytes}"));
    }
    if (asset.width, asset.height) != expected_dimensions {
        errors.push(format!(
            "{field} dimensions must be {}x{}",
            expected_dimensions.0, expected_dimensions.1
        ));
    }
}

fn validate_inline(value: &str, min: usize, max: usize, field: &str, errors: &mut Vec<String>) {
    let chars = value.chars().count();
    if !(min..=max).contains(&chars) || value.trim() != value || value.chars().any(char::is_control)
    {
        errors.push(format!(
            "{field} must contain {min}-{max} safe inline characters"
        ));
    }
}

fn validate_prose(value: &str, min: usize, max: usize, field: &str, errors: &mut Vec<String>) {
    let chars = value.chars().count();
    if !(min..=max).contains(&chars)
        || value.trim() != value
        || value
            .chars()
            .any(|character| character.is_control() && character != '\n')
    {
        errors.push(format!(
            "{field} must contain {min}-{max} safe prose characters"
        ));
    }
}

fn is_valid_asset_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 128 || path.contains('\\') || !path.ends_with(".png") {
        return false;
    }
    let path = path.strip_suffix(".png").unwrap();
    path.split('/').all(|part| {
        !part.is_empty()
            && part
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    })
}

fn is_valid_locale(locale: &str) -> bool {
    if locale.len() > 16 {
        return false;
    }
    let mut parts = locale.split('-');
    let Some(language) = parts.next() else {
        return false;
    };
    if !(2..=3).contains(&language.len()) || !language.bytes().all(|byte| byte.is_ascii_lowercase())
    {
        return false;
    }
    let mut next = parts.next();
    if next.is_some_and(|part| part.len() == 4) {
        let script = next.unwrap();
        if !script.as_bytes()[0].is_ascii_uppercase()
            || !script.as_bytes()[1..].iter().all(u8::is_ascii_lowercase)
        {
            return false;
        }
        next = parts.next();
    }
    if let Some(region) = next {
        let valid_region = (region.len() == 2
            && region.bytes().all(|byte| byte.is_ascii_uppercase()))
            || (region.len() == 3 && region.bytes().all(|byte| byte.is_ascii_digit()));
        if !valid_region {
            return false;
        }
    }
    parts.next().is_none()
}

fn is_valid_https_url(url: &str) -> bool {
    url.len() <= 2048
        && url.starts_with("https://")
        && !url.contains('@')
        && !url.contains('#')
        && !url
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        && url[8..]
            .split('/')
            .next()
            .is_some_and(|host| host.contains('.'))
}

fn is_lower_hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes * 2
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_listing() -> StoreListing {
        StoreListing {
            schema_version: STORE_LISTING_SCHEMA_VERSION,
            app_id: "dev.cardputerzero.notes".into(),
            version: "1.2.0".into(),
            default_locale: "en-US".into(),
            category: StoreCategory::Productivity,
            age_rating: AgeRating::FourPlus,
            privacy_url: "https://example.com/privacy".into(),
            support_url: "https://example.com/support".into(),
            icon: ImageAsset {
                path: "images/icon.png".into(),
                sha256: "11".repeat(32),
                bytes: 4096,
                width: 48,
                height: 48,
            },
            screenshots: vec![ImageAsset {
                path: "images/screen-1.png".into(),
                sha256: "22".repeat(32),
                bytes: 32_000,
                width: 320,
                height: 170,
            }],
            localizations: vec![LocalizedListing {
                locale: "en-US".into(),
                name: "Notes".into(),
                subtitle: "Fast notes for the small screen".into(),
                description: "Capture and organize short notes.\nWorks fully offline.".into(),
                keywords: vec!["notes".into(), "productivity".into()],
                release_notes: "Initial release.".into(),
            }],
        }
    }

    fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        encoded.extend_from_slice(&(data.len() as u32).to_be_bytes());
        encoded.extend_from_slice(kind);
        encoded.extend_from_slice(data);
        encoded.extend_from_slice(&png_crc32(&encoded[4..]).to_be_bytes());
        encoded
    }

    fn structural_png(width: u16, height: u16) -> Vec<u8> {
        let mut encoded = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        header.extend_from_slice(&u32::from(width).to_be_bytes());
        header.extend_from_slice(&u32::from(height).to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        encoded.extend_from_slice(&png_chunk(b"IHDR", &header));
        encoded.extend_from_slice(&png_chunk(b"IDAT", &[0x78, 0x01]));
        encoded.extend_from_slice(&png_chunk(b"IEND", &[]));
        encoded
    }

    #[test]
    fn strict_listing_round_trips() {
        let listing = valid_listing();
        validate(&listing).unwrap();
        let encoded = serde_json::to_vec(&listing).unwrap();
        assert_eq!(parse_and_validate(&encoded).unwrap(), listing);

        let mut value = serde_json::to_value(valid_listing()).unwrap();
        value["unexpected"] = serde_json::Value::Bool(true);
        assert!(parse_and_validate(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn rejects_ambiguous_localizations_and_assets() {
        let mut listing = valid_listing();
        listing.default_locale = "en-usa".into();
        listing.screenshots[0].path = listing.icon.path.clone();
        listing.localizations[0].keywords = vec!["notes".into(), "notes".into()];
        let errors = validate(&listing).unwrap_err().join("\n");
        assert!(errors.contains("default_locale"));
        assert!(errors.contains("duplicates another image asset"));
        assert!(errors.contains("keywords must be unique"));
    }

    #[test]
    fn locale_and_text_boundaries_are_closed() {
        for valid in ["en", "en-US", "zh-Hans", "zh-Hans-CN", "es-419"] {
            assert!(is_valid_locale(valid), "{valid}");
        }
        for invalid in ["EN", "en-us", "zh-hans-CN", "en-US-extra", "x", ""] {
            assert!(!is_valid_locale(invalid), "{invalid}");
        }

        let mut listing = valid_listing();
        listing.localizations[0].description = "bad\rtext".into();
        assert!(validate(&listing).is_err());
        listing.localizations[0].description = "界".repeat(1024);
        assert!(validate(&listing).is_ok());
        listing.localizations[0].description.push('界');
        assert!(validate(&listing).is_err());
    }

    #[test]
    fn json_schema_enums_match_rust() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../../schemas/store-listing-v1.schema.json"
        ))
        .unwrap();
        let categories = schema["properties"]["category"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        let rust_categories = StoreCategory::ALL
            .iter()
            .map(|category| category.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(categories, rust_categories);

        let ratings = schema["properties"]["age_rating"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        let rust_ratings = AgeRating::ALL
            .iter()
            .map(|rating| rating.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(ratings, rust_ratings);
    }

    #[test]
    fn control_api_states_match_rust_and_reject_unknown_values() {
        let api: serde_json::Value = serde_json::from_str(include_str!(
            "../../../schemas/store-control-v1.openapi.json"
        ))
        .unwrap();
        let submission_states = api["components"]["schemas"]["SubmissionState"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            submission_states,
            SubmissionState::ALL
                .iter()
                .map(|state| state.as_str())
                .collect::<Vec<_>>()
        );
        let release_states = api["components"]["schemas"]["ReleaseState"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            release_states,
            ReleaseState::ALL
                .iter()
                .map(|state| state.as_str())
                .collect::<Vec<_>>()
        );
        assert!(serde_json::from_str::<SubmissionState>("\"future\"").is_err());
        assert!(serde_json::from_str::<ReleaseState>("\"future\"").is_err());
    }

    #[test]
    fn submission_and_release_transitions_are_closed() {
        assert!(SubmissionState::Draft.can_transition_to(SubmissionState::Uploading));
        assert!(SubmissionState::Processing.can_transition_to(SubmissionState::Withdrawn));
        assert!(SubmissionState::InReview.can_transition_to(SubmissionState::Approved));
        assert!(
            SubmissionState::InReview.can_transition_to(SubmissionState::PendingSecondaryReview)
        );
        assert!(
            SubmissionState::PendingSecondaryReview.can_transition_to(SubmissionState::InReview)
        );
        assert!(!SubmissionState::Approved.can_transition_to(SubmissionState::ReadyForReview));
        assert!(!SubmissionState::NeedsChanges.can_transition_to(SubmissionState::Uploading));
        assert!(!SubmissionState::Draft.can_transition_to(SubmissionState::Draft));

        assert!(ReleaseState::Ready.can_transition_to(ReleaseState::Publishing));
        assert!(ReleaseState::Publishing.can_transition_to(ReleaseState::PublishFailed));
        assert!(ReleaseState::PublishFailed.can_transition_to(ReleaseState::Publishing));
        assert!(ReleaseState::Paused.can_transition_to(ReleaseState::Published));
        assert!(!ReleaseState::Removed.can_transition_to(ReleaseState::Published));
        assert!(!ReleaseState::Ready.can_transition_to(ReleaseState::Published));
    }

    #[test]
    fn validates_png_structure_dimensions_order_and_crc() {
        let valid = structural_png(48, 48);
        validate_png_structure(&valid, 48, 48).unwrap();
        assert!(validate_png_structure(&valid, 320, 170).is_err());

        let mut corrupt = valid.clone();
        corrupt[30] ^= 1;
        assert!(validate_png_structure(&corrupt, 48, 48).is_err());

        let mut trailing = valid;
        trailing.push(0);
        assert!(validate_png_structure(&trailing, 48, 48).is_err());
    }
}
