use std::fmt;

use serde::{Deserialize, Serialize};

pub const METRICS_SCHEMA_VERSION: u32 = 1;
pub const WEEK_SECONDS: u64 = 7 * 24 * 60 * 60;
pub const WEEK_OFFSET_SECONDS: u64 = 4 * 24 * 60 * 60;
pub const MAX_METRIC_RECORDS: usize = 64;
pub const MAX_METRICS_REPORT_BYTES: usize = 32 * 1024;
pub const MAX_WEEKLY_INSTALLS: u8 = 8;
pub const MAX_WEEKLY_LAUNCHES: u16 = 4096;
pub const METRICS_PRIVACY_THRESHOLD: u32 = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AggregateMetricsReport {
    pub schema_version: u32,
    pub batch_id: String,
    pub week_start_unix_seconds: u64,
    pub records: Vec<AppMetricRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppMetricRecord {
    pub app_id: String,
    pub version: String,
    pub installs: u8,
    pub launches: u16,
    pub crashes: u16,
}

#[derive(Debug)]
pub enum MetricsError {
    Json(serde_json::Error),
    Invalid(&'static str),
}

impl fmt::Display for MetricsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "invalid metrics JSON: {error}"),
            Self::Invalid(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for MetricsError {}

impl From<serde_json::Error> for MetricsError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl AggregateMetricsReport {
    pub fn validate(&self) -> Result<(), MetricsError> {
        if self.schema_version != METRICS_SCHEMA_VERSION {
            return Err(MetricsError::Invalid(
                "metrics schema version is unsupported",
            ));
        }
        if !is_valid_batch_id(&self.batch_id) {
            return Err(MetricsError::Invalid("metrics batch ID is invalid"));
        }
        if !is_week_start(self.week_start_unix_seconds) {
            return Err(MetricsError::Invalid("metrics week boundary is invalid"));
        }
        if !(1..=MAX_METRIC_RECORDS).contains(&self.records.len()) {
            return Err(MetricsError::Invalid(
                "metrics record count is outside limits",
            ));
        }
        let mut previous: Option<(&str, &str)> = None;
        for record in &self.records {
            record.validate()?;
            let identity = (record.app_id.as_str(), record.version.as_str());
            if previous.is_some_and(|previous| previous >= identity) {
                return Err(MetricsError::Invalid(
                    "metrics records are not unique canonical order",
                ));
            }
            previous = Some(identity);
        }
        Ok(())
    }
}

impl AppMetricRecord {
    pub fn validate(&self) -> Result<(), MetricsError> {
        if !cp0_manifest::is_valid_app_id(&self.app_id)
            || !cp0_manifest::is_valid_app_version(&self.version)
        {
            return Err(MetricsError::Invalid(
                "metrics application identity is invalid",
            ));
        }
        if self.installs > MAX_WEEKLY_INSTALLS
            || self.launches > MAX_WEEKLY_LAUNCHES
            || self.crashes > self.launches
            || (self.installs == 0 && self.launches == 0 && self.crashes == 0)
        {
            return Err(MetricsError::Invalid("metrics counters are outside limits"));
        }
        Ok(())
    }
}

pub fn decode_report(encoded: &[u8]) -> Result<AggregateMetricsReport, MetricsError> {
    if encoded.is_empty() || encoded.len() > MAX_METRICS_REPORT_BYTES {
        return Err(MetricsError::Invalid(
            "metrics report size is outside limits",
        ));
    }
    let report: AggregateMetricsReport = serde_json::from_slice(encoded)?;
    report.validate()?;
    if encode_report(&report)?.len() > MAX_METRICS_REPORT_BYTES {
        return Err(MetricsError::Invalid("metrics report exceeds its bound"));
    }
    Ok(report)
}

pub fn encode_report(report: &AggregateMetricsReport) -> Result<Vec<u8>, MetricsError> {
    report.validate()?;
    let encoded = serde_json::to_vec(report)?;
    if encoded.len() > MAX_METRICS_REPORT_BYTES {
        return Err(MetricsError::Invalid("metrics report exceeds its bound"));
    }
    Ok(encoded)
}

pub fn week_start(unix_seconds: u64) -> u64 {
    if unix_seconds < WEEK_OFFSET_SECONDS {
        return 0;
    }
    unix_seconds - ((unix_seconds - WEEK_OFFSET_SECONDS) % WEEK_SECONDS)
}

pub fn is_week_start(unix_seconds: u64) -> bool {
    unix_seconds >= WEEK_OFFSET_SECONDS && (unix_seconds - WEEK_OFFSET_SECONDS) % WEEK_SECONDS == 0
}

pub fn is_valid_batch_id(value: &str) -> bool {
    value
        .strip_prefix("batch_")
        .is_some_and(|digest| digest.len() == 32 && digest.bytes().all(is_lower_hex))
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> AggregateMetricsReport {
        AggregateMetricsReport {
            schema_version: METRICS_SCHEMA_VERSION,
            batch_id: "batch_0123456789abcdef0123456789abcdef".into(),
            week_start_unix_seconds: WEEK_OFFSET_SECONDS + WEEK_SECONDS,
            records: vec![
                AppMetricRecord {
                    app_id: "dev.cardputerzero.alpha".into(),
                    version: "1.0.0".into(),
                    installs: 1,
                    launches: 4,
                    crashes: 1,
                },
                AppMetricRecord {
                    app_id: "dev.cardputerzero.beta".into(),
                    version: "2.0.0".into(),
                    installs: 0,
                    launches: 2,
                    crashes: 0,
                },
            ],
        }
    }

    #[test]
    fn strict_report_round_trips_without_device_identity_or_timestamps() {
        let report = report();
        let encoded = encode_report(&report).unwrap();
        assert_eq!(decode_report(&encoded).unwrap(), report);
        let text = String::from_utf8(encoded).unwrap();
        assert!(!text.contains("device") && !text.contains("occurred") && !text.contains("stack"));

        let mut value = serde_json::to_value(report).unwrap();
        value["device_id"] = serde_json::Value::String("forbidden".into());
        assert!(decode_report(&serde_json::to_vec(&value).unwrap()).is_err());
    }

    #[test]
    fn rejects_duplicate_unsorted_empty_and_impossible_counters() {
        let mut invalid = report();
        invalid.records.swap(0, 1);
        assert!(invalid.validate().is_err());

        let mut invalid = report();
        invalid.records.push(invalid.records[1].clone());
        assert!(invalid.validate().is_err());

        let mut invalid = report();
        invalid.records.clear();
        assert!(invalid.validate().is_err());

        let mut invalid = report();
        invalid.records[0].launches = 0;
        assert!(invalid.validate().is_err());

        let mut invalid = report();
        invalid.records[0].crashes = invalid.records[0].launches + 1;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn accepts_only_monday_utc_week_boundaries_and_ephemeral_batch_ids() {
        assert_eq!(week_start(WEEK_OFFSET_SECONDS), WEEK_OFFSET_SECONDS);
        assert_eq!(
            week_start(WEEK_OFFSET_SECONDS + WEEK_SECONDS + 123),
            WEEK_OFFSET_SECONDS + WEEK_SECONDS
        );
        assert!(is_week_start(WEEK_OFFSET_SECONDS + 10 * WEEK_SECONDS));
        assert!(!is_week_start(WEEK_OFFSET_SECONDS + 1));
        assert!(is_valid_batch_id("batch_0123456789abcdef0123456789abcdef"));
        assert!(!is_valid_batch_id("batch_0123456789ABCDEF0123456789ABCDEF"));
    }
}
