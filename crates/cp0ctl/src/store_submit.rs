use std::env;
use std::fs::File;
use std::io::Read;
use std::thread;
use std::time::{Duration, Instant};

use cp0_store_metadata::{ImageAsset, SubmissionState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ureq::config::Config;
use ureq::{Agent, Body};

use crate::store_submission::{ValidatedSubmission, validate_submission_bundle};

const DEFAULT_API: &str = "https://developer.cardputerzero.dev";
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: u64 = 32 * 1024;
const MAX_UPLOAD_CHUNK_BYTES: usize = 256 * 1024;
const MAX_RETRY_ATTEMPTS: usize = 3;

pub fn submit(package_path: &str, listing_path: &str) -> Result<(), String> {
    let submission = validate_submission_bundle(package_path, listing_path)?;
    let base_url = env::var("CP0_STORE_API").unwrap_or_else(|_| DEFAULT_API.into());
    let mut api = HttpSubmissionApi::new(&base_url)?;
    let idempotency_root = new_idempotency_root()?;
    let output = execute_submission(&mut api, &submission, &idempotency_root, &mut thread::sleep)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&output)
            .map_err(|error| format!("cannot encode submission result: {error}"))?
    );
    Ok(())
}

#[derive(Debug)]
enum ApiError {
    Retryable(&'static str),
    Unauthorized,
    Fatal(String),
}

impl ApiError {
    fn message(self) -> String {
        match self {
            Self::Retryable(message) => message.into(),
            Self::Unauthorized => "Store authorization expired".into(),
            Self::Fatal(message) => message,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubmissionHandle {
    submission_id: String,
    etag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct SubmitOutput {
    submission_id: String,
    state: SubmissionState,
    content_sha256: String,
    portal_url: String,
}

trait SubmissionApi {
    fn authorize(&mut self) -> Result<String, ApiError>;

    fn create_submission(
        &mut self,
        token: &str,
        submission: &ValidatedSubmission,
        idempotency_key: &str,
    ) -> Result<SubmissionHandle, ApiError>;

    #[allow(clippy::too_many_arguments)]
    fn upload_chunk(
        &mut self,
        token: &str,
        submission_id: &str,
        part_name: &str,
        offset: u64,
        total: u64,
        chunk_sha256: &str,
        chunk: &[u8],
        etag: &str,
        idempotency_key: &str,
    ) -> Result<String, ApiError>;

    fn finalize_submission(
        &mut self,
        token: &str,
        submission: &ValidatedSubmission,
        handle: &SubmissionHandle,
        idempotency_key: &str,
    ) -> Result<SubmissionState, ApiError>;
}

fn execute_submission<A: SubmissionApi>(
    api: &mut A,
    submission: &ValidatedSubmission,
    idempotency_root: &str,
    sleep: &mut impl FnMut(Duration),
) -> Result<SubmitOutput, String> {
    validate_idempotency_key(idempotency_root)?;
    let mut token = authorize_with_retry(api, sleep)?;
    let create_key = format!("{idempotency_root}-create");
    let mut handle = call_with_retry(api, &mut token, sleep, |api, token| {
        api.create_submission(token, submission, &create_key)
    })?;

    upload_object(
        api,
        &mut token,
        sleep,
        &mut handle,
        "package",
        &submission.package,
        idempotency_root,
        0,
    )?;
    upload_object(
        api,
        &mut token,
        sleep,
        &mut handle,
        "listing",
        &submission.listing,
        idempotency_root,
        1,
    )?;
    for (index, asset) in submission.assets.iter().enumerate() {
        upload_object(
            api,
            &mut token,
            sleep,
            &mut handle,
            &format!("asset-{index}"),
            &asset.encoded,
            idempotency_root,
            index + 2,
        )?;
    }

    let finalize_key = format!("{idempotency_root}-finalize");
    let state = call_with_retry(api, &mut token, sleep, |api, token| {
        api.finalize_submission(token, submission, &handle, &finalize_key)
    })?;
    if state != SubmissionState::Processing {
        return Err("Store returned an invalid state after submission finalization".into());
    }
    Ok(SubmitOutput {
        submission_id: handle.submission_id.clone(),
        state,
        content_sha256: submission.content_sha256.clone(),
        portal_url: format!(
            "{}/submissions/{}",
            api_base_for_output(),
            handle.submission_id
        ),
    })
}

#[allow(clippy::too_many_arguments)]
fn upload_object<A: SubmissionApi>(
    api: &mut A,
    token: &mut String,
    sleep: &mut impl FnMut(Duration),
    handle: &mut SubmissionHandle,
    part_name: &str,
    encoded: &[u8],
    idempotency_root: &str,
    part_index: usize,
) -> Result<(), String> {
    if encoded.is_empty() {
        return Err(format!("Store upload part {part_name} is empty"));
    }
    let total = encoded.len() as u64;
    for (chunk_index, chunk) in encoded.chunks(MAX_UPLOAD_CHUNK_BYTES).enumerate() {
        let offset = (chunk_index * MAX_UPLOAD_CHUNK_BYTES) as u64;
        let chunk_sha256 = cp0_store_protocol::lower_hex(&Sha256::digest(chunk));
        let key = format!("{idempotency_root}-p{part_index}-c{chunk_index}");
        let etag = handle.etag.clone();
        handle.etag = call_with_retry(api, token, sleep, |api, token| {
            api.upload_chunk(
                token,
                &handle.submission_id,
                part_name,
                offset,
                total,
                &chunk_sha256,
                chunk,
                &etag,
                &key,
            )
        })?;
    }
    Ok(())
}

fn authorize_with_retry<A: SubmissionApi>(
    api: &mut A,
    sleep: &mut impl FnMut(Duration),
) -> Result<String, String> {
    for attempt in 0..MAX_RETRY_ATTEMPTS {
        match api.authorize() {
            Ok(token) => return Ok(token),
            Err(ApiError::Retryable(message)) if attempt + 1 < MAX_RETRY_ATTEMPTS => {
                let _ = message;
                sleep(retry_delay(attempt));
            }
            Err(error) => return Err(error.message()),
        }
    }
    unreachable!("retry loop returns on its last attempt")
}

fn call_with_retry<A: SubmissionApi, T>(
    api: &mut A,
    token: &mut String,
    sleep: &mut impl FnMut(Duration),
    mut operation: impl FnMut(&mut A, &str) -> Result<T, ApiError>,
) -> Result<T, String> {
    let mut reauthorized = false;
    for attempt in 0..MAX_RETRY_ATTEMPTS {
        match operation(api, token) {
            Ok(value) => return Ok(value),
            Err(ApiError::Unauthorized) if !reauthorized => {
                *token = authorize_with_retry(api, sleep)?;
                reauthorized = true;
            }
            Err(ApiError::Retryable(message)) if attempt + 1 < MAX_RETRY_ATTEMPTS => {
                let _ = message;
                sleep(retry_delay(attempt));
            }
            Err(error) => return Err(error.message()),
        }
    }
    Err("Store request retry budget was exhausted".into())
}

fn retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(250_u64 << attempt.min(3))
}

#[derive(Debug)]
struct HttpSubmissionApi {
    agent: Agent,
    base_url: String,
}

impl HttpSubmissionApi {
    fn new(base_url: &str) -> Result<Self, String> {
        let base_url = base_url.trim_end_matches('/');
        let uri: ureq::http::Uri = base_url
            .parse()
            .map_err(|_| "Store API origin is invalid".to_owned())?;
        if uri.scheme_str() != Some("https")
            || uri.authority().is_none()
            || uri.path() != "/"
            || uri.query().is_some()
            || !cp0_store_protocol::is_valid_https_url(&format!("{base_url}/v1"))
        {
            return Err("Store API must be bounded HTTPS without credentials or fragments".into());
        }
        let config = Config::builder()
            .https_only(true)
            .proxy(None)
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(true)
            .timeout_global(Some(HTTP_TIMEOUT))
            .max_response_header_size(16 * 1024)
            .max_idle_connections(1)
            .max_idle_connections_per_host(1)
            .build();
        Ok(Self {
            agent: Agent::new_with_config(config),
            base_url: base_url.into(),
        })
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct DeviceCodeRequest<'a> {
    client_id: &'a str,
    scope: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct DeviceTokenRequest<'a> {
    grant_type: &'a str,
    device_code: &'a str,
    client_id: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: u64,
    scope: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProblemResponse {
    #[serde(rename = "type")]
    problem_type: String,
    title: String,
    status: u16,
    code: String,
    request_id: String,
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct CreateSubmissionRequest<'a> {
    version: &'a str,
    package_sha256: &'a str,
    package_bytes: u64,
    listing_sha256: &'a str,
    listing_bytes: u64,
    assets: Vec<&'a ImageAsset>,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct FinalizeSubmissionRequest<'a> {
    content_sha256: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmissionResponse {
    submission_id: String,
    app_id: String,
    version: String,
    revision: u32,
    state: SubmissionState,
    package_sha256: String,
    listing_sha256: String,
    assets: Vec<ImageAsset>,
    resource_version: u64,
    created_unix_seconds: u64,
}

impl SubmissionApi for HttpSubmissionApi {
    fn authorize(&mut self) -> Result<String, ApiError> {
        let response = self
            .agent
            .post(self.endpoint("/oauth/device/code"))
            .send_json(DeviceCodeRequest {
                client_id: "cp0ctl",
                scope: "store.submit",
            })
            .map_err(map_transport_error)?;
        let code: DeviceCodeResponse = decode_json(response, 200)?.0;
        validate_device_code(&code)?;
        eprintln!(
            "Open {} and enter code {} to authorize this submission.",
            code.verification_uri, code.user_code
        );

        let deadline = Instant::now()
            .checked_add(Duration::from_secs(code.expires_in))
            .ok_or_else(|| ApiError::Fatal("Store authorization expiry is invalid".into()))?;
        let mut interval = code.interval;
        loop {
            if Instant::now() >= deadline {
                return Err(ApiError::Fatal("Store authorization expired".into()));
            }
            thread::sleep(Duration::from_secs(interval));
            let mut response = self
                .agent
                .post(self.endpoint("/oauth/token"))
                .send_json(DeviceTokenRequest {
                    grant_type: "urn:ietf:params:oauth:grant-type:device_code",
                    device_code: &code.device_code,
                    client_id: "cp0ctl",
                })
                .map_err(map_transport_error)?;
            let status = response.status().as_u16();
            if status == 200 {
                let token: DeviceTokenResponse = response
                    .body_mut()
                    .with_config()
                    .limit(MAX_RESPONSE_BYTES)
                    .read_json()
                    .map_err(|_| ApiError::Fatal("Store token response is invalid".into()))?;
                validate_token(&token)?;
                return Ok(token.access_token);
            }
            let code = decode_problem_code(&mut response);
            match code.as_deref() {
                Some("authorization-pending") => continue,
                Some("slow-down") => {
                    interval = (interval + 5).min(30);
                }
                Some("access-denied") => {
                    return Err(ApiError::Fatal("Store authorization was denied".into()));
                }
                Some("expired-token") => {
                    return Err(ApiError::Fatal("Store authorization expired".into()));
                }
                _ => return Err(status_error(status, code)),
            }
        }
    }

    fn create_submission(
        &mut self,
        token: &str,
        submission: &ValidatedSubmission,
        idempotency_key: &str,
    ) -> Result<SubmissionHandle, ApiError> {
        let assets = submission
            .assets
            .iter()
            .map(|asset| &asset.descriptor)
            .collect();
        let response = self
            .agent
            .post(self.endpoint(&format!("/v1/apps/{}/submissions", submission.app_id)))
            .header("Authorization", format!("Bearer {token}"))
            .header("Idempotency-Key", idempotency_key)
            .send_json(CreateSubmissionRequest {
                version: &submission.version,
                package_sha256: &submission.package_sha256,
                package_bytes: submission.package.len() as u64,
                listing_sha256: &submission.listing_sha256,
                listing_bytes: submission.listing.len() as u64,
                assets,
            })
            .map_err(map_transport_error)?;
        let (response, etag) = decode_json::<SubmissionResponse>(response, 201)?;
        validate_submission_response(&response, submission, &[SubmissionState::Uploading])?;
        Ok(SubmissionHandle {
            submission_id: response.submission_id,
            etag: etag.ok_or_else(|| ApiError::Fatal("Store response omitted ETag".into()))?,
        })
    }

    fn upload_chunk(
        &mut self,
        token: &str,
        submission_id: &str,
        part_name: &str,
        offset: u64,
        total: u64,
        chunk_sha256: &str,
        chunk: &[u8],
        etag: &str,
        idempotency_key: &str,
    ) -> Result<String, ApiError> {
        let end = offset
            .checked_add(chunk.len() as u64)
            .and_then(|value| value.checked_sub(1))
            .ok_or_else(|| ApiError::Fatal("Store upload range overflow".into()))?;
        let response = self
            .agent
            .put(self.endpoint(&format!(
                "/v1/submissions/{submission_id}/parts/{part_name}"
            )))
            .header("Authorization", format!("Bearer {token}"))
            .header("Idempotency-Key", idempotency_key)
            .header("If-Match", etag)
            .header("Content-SHA256", chunk_sha256)
            .header("Content-Range", format!("bytes {offset}-{end}/{total}"))
            .header("Content-Type", "application/octet-stream")
            .send(chunk)
            .map_err(map_transport_error)?;
        let (_, etag) = decode_empty(response, 204)?;
        etag.ok_or_else(|| ApiError::Fatal("Store upload response omitted ETag".into()))
    }

    fn finalize_submission(
        &mut self,
        token: &str,
        submission: &ValidatedSubmission,
        handle: &SubmissionHandle,
        idempotency_key: &str,
    ) -> Result<SubmissionState, ApiError> {
        let response = self
            .agent
            .post(self.endpoint(&format!(
                "/v1/submissions/{}:finalize",
                handle.submission_id
            )))
            .header("Authorization", format!("Bearer {token}"))
            .header("Idempotency-Key", idempotency_key)
            .header("If-Match", &handle.etag)
            .send_json(FinalizeSubmissionRequest {
                content_sha256: &submission.content_sha256,
            })
            .map_err(map_transport_error)?;
        let (response, _) = decode_json::<SubmissionResponse>(response, 202)?;
        validate_submission_response(&response, submission, &[SubmissionState::Processing])?;
        if response.submission_id != handle.submission_id {
            return Err(ApiError::Fatal(
                "Store finalized a different submission".into(),
            ));
        }
        Ok(response.state)
    }
}

fn validate_submission_response(
    response: &SubmissionResponse,
    submission: &ValidatedSubmission,
    allowed_states: &[SubmissionState],
) -> Result<(), ApiError> {
    let expected_assets = submission
        .assets
        .iter()
        .map(|asset| &asset.descriptor)
        .collect::<Vec<_>>();
    if !is_valid_submission_id(&response.submission_id)
        || response.app_id != submission.app_id
        || response.version != submission.version
        || response.package_sha256 != submission.package_sha256
        || response.listing_sha256 != submission.listing_sha256
        || response.assets.iter().collect::<Vec<_>>() != expected_assets
        || response.revision == 0
        || response.resource_version == 0
        || response.created_unix_seconds == 0
        || !allowed_states.contains(&response.state)
    {
        return Err(ApiError::Fatal(
            "Store submission response does not match the upload".into(),
        ));
    }
    Ok(())
}

fn decode_json<T: for<'de> Deserialize<'de>>(
    mut response: ureq::http::Response<Body>,
    expected_status: u16,
) -> Result<(T, Option<String>), ApiError> {
    let status = response.status().as_u16();
    if status != expected_status {
        let code = decode_problem_code(&mut response);
        return Err(status_error(status, code));
    }
    let etag = response_etag(&response)?;
    let decoded = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_json()
        .map_err(|_| ApiError::Fatal("Store API response is invalid".into()))?;
    Ok((decoded, etag))
}

fn decode_empty(
    mut response: ureq::http::Response<Body>,
    expected_status: u16,
) -> Result<((), Option<String>), ApiError> {
    let status = response.status().as_u16();
    if status != expected_status {
        let code = decode_problem_code(&mut response);
        return Err(status_error(status, code));
    }
    Ok(((), response_etag(&response)?))
}

fn response_etag(response: &ureq::http::Response<Body>) -> Result<Option<String>, ApiError> {
    response
        .headers()
        .get("etag")
        .map(|value| {
            value
                .to_str()
                .ok()
                .filter(|etag| {
                    (3..=64).contains(&etag.len())
                        && etag.starts_with('"')
                        && etag.ends_with('"')
                        && !etag.chars().any(char::is_control)
                })
                .map(str::to_owned)
                .ok_or_else(|| ApiError::Fatal("Store response ETag is invalid".into()))
        })
        .transpose()
}

fn decode_problem_code(response: &mut ureq::http::Response<Body>) -> Option<String> {
    let problem: ProblemResponse = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_json()
        .ok()?;
    let _ = (
        problem.problem_type,
        problem.title,
        problem.status,
        problem.request_id,
        problem.detail,
    );
    is_safe_code(&problem.code).then_some(problem.code)
}

fn status_error(status: u16, code: Option<String>) -> ApiError {
    if status == 401 {
        ApiError::Unauthorized
    } else if status == 429 || matches!(status, 500 | 502 | 503 | 504) {
        ApiError::Retryable("Store API is temporarily unavailable")
    } else {
        ApiError::Fatal(format!(
            "Store API returned HTTP {status} ({})",
            code.as_deref().unwrap_or("unknown-error")
        ))
    }
}

fn map_transport_error(_error: ureq::Error) -> ApiError {
    ApiError::Retryable("Store API transport failed")
}

fn validate_device_code(code: &DeviceCodeResponse) -> Result<(), ApiError> {
    if !(32..=128).contains(&code.device_code.len())
        || code.device_code.chars().any(char::is_control)
        || !(6..=16).contains(&code.user_code.len())
        || code
            .user_code
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && byte != b'-')
        || !cp0_store_protocol::is_valid_https_url(&code.verification_uri)
        || !(60..=900).contains(&code.expires_in)
        || !(5..=30).contains(&code.interval)
    {
        return Err(ApiError::Fatal(
            "Store device authorization response is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_token(token: &DeviceTokenResponse) -> Result<(), ApiError> {
    if !(32..=2048).contains(&token.access_token.len())
        || token.access_token.chars().any(char::is_control)
        || token.token_type != "Bearer"
        || token.scope != "store.submit"
        || !(60..=3600).contains(&token.expires_in)
    {
        return Err(ApiError::Fatal("Store token response is invalid".into()));
    }
    Ok(())
}

fn is_valid_submission_id(value: &str) -> bool {
    value
        .strip_prefix("sub_")
        .is_some_and(|suffix| cp0_store_protocol::is_lower_hex(suffix, 16))
}

fn is_safe_code(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn validate_idempotency_key(value: &str) -> Result<(), String> {
    if !(16..=96).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-'))
    {
        return Err("Store idempotency key is invalid".into());
    }
    Ok(())
}

fn new_idempotency_root() -> Result<String, String> {
    let mut random = [0_u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(|error| format!("cannot read operating-system randomness: {error}"))?;
    Ok(format!("cp0ctl-{}", cp0_store_protocol::lower_hex(&random)))
}

fn api_base_for_output() -> String {
    env::var("CP0_STORE_API")
        .unwrap_or_else(|_| DEFAULT_API.into())
        .trim_end_matches('/')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store_submission::ValidatedAsset;

    #[derive(Debug)]
    struct UploadCall {
        part: String,
        offset: u64,
        total: u64,
        digest: String,
        bytes: usize,
        idempotency_key: String,
    }

    #[derive(Debug, Default)]
    struct MockApi {
        authorize_calls: usize,
        create_calls: usize,
        upload_attempts: usize,
        finalize_calls: usize,
        fail_first_upload: bool,
        expire_first_create_token: bool,
        uploads: Vec<UploadCall>,
        etag_version: u64,
    }

    impl SubmissionApi for MockApi {
        fn authorize(&mut self) -> Result<String, ApiError> {
            self.authorize_calls += 1;
            Ok(format!("token-{:0>32}", self.authorize_calls))
        }

        fn create_submission(
            &mut self,
            _token: &str,
            _submission: &ValidatedSubmission,
            _idempotency_key: &str,
        ) -> Result<SubmissionHandle, ApiError> {
            self.create_calls += 1;
            if self.expire_first_create_token && self.create_calls == 1 {
                return Err(ApiError::Unauthorized);
            }
            self.etag_version = 1;
            Ok(SubmissionHandle {
                submission_id: "sub_11111111111111111111111111111111".into(),
                etag: "\"1\"".into(),
            })
        }

        fn upload_chunk(
            &mut self,
            _token: &str,
            _submission_id: &str,
            part_name: &str,
            offset: u64,
            total: u64,
            chunk_sha256: &str,
            chunk: &[u8],
            _etag: &str,
            idempotency_key: &str,
        ) -> Result<String, ApiError> {
            self.upload_attempts += 1;
            self.uploads.push(UploadCall {
                part: part_name.into(),
                offset,
                total,
                digest: chunk_sha256.into(),
                bytes: chunk.len(),
                idempotency_key: idempotency_key.into(),
            });
            if self.fail_first_upload && self.upload_attempts == 1 {
                return Err(ApiError::Retryable("injected network interruption"));
            }
            self.etag_version += 1;
            Ok(format!("\"{}\"", self.etag_version))
        }

        fn finalize_submission(
            &mut self,
            _token: &str,
            _submission: &ValidatedSubmission,
            _handle: &SubmissionHandle,
            _idempotency_key: &str,
        ) -> Result<SubmissionState, ApiError> {
            self.finalize_calls += 1;
            Ok(SubmissionState::Processing)
        }
    }

    fn submission() -> ValidatedSubmission {
        let package = vec![0x55; MAX_UPLOAD_CHUNK_BYTES + 17];
        let listing = vec![0x22; 97];
        let icon = vec![0x33; 53];
        ValidatedSubmission {
            app_id: "dev.cardputerzero.notes".into(),
            version: "1.2.0".into(),
            package_sha256: cp0_store_protocol::lower_hex(&Sha256::digest(&package)),
            listing_sha256: cp0_store_protocol::lower_hex(&Sha256::digest(&listing)),
            content_sha256: "44".repeat(32),
            package,
            listing,
            assets: vec![ValidatedAsset {
                descriptor: ImageAsset {
                    path: "images/icon.png".into(),
                    sha256: cp0_store_protocol::lower_hex(&Sha256::digest(&icon)),
                    bytes: icon.len() as u64,
                    width: 48,
                    height: 48,
                },
                encoded: icon,
            }],
        }
    }

    #[test]
    fn uploads_bounded_chunks_and_retries_the_same_idempotent_request() {
        let mut api = MockApi {
            fail_first_upload: true,
            ..MockApi::default()
        };
        let mut sleeps = Vec::new();
        let output = execute_submission(
            &mut api,
            &submission(),
            "cp0ctl-11111111111111111111111111111111",
            &mut |duration| sleeps.push(duration),
        )
        .unwrap();

        assert_eq!(output.state, SubmissionState::Processing);
        assert_eq!(api.authorize_calls, 1);
        assert_eq!(api.create_calls, 1);
        assert_eq!(api.finalize_calls, 1);
        assert_eq!(api.uploads.len(), 5);
        assert_eq!(api.uploads[0].part, "package");
        assert_eq!(api.uploads[0].offset, 0);
        assert_eq!(api.uploads[0].bytes, MAX_UPLOAD_CHUNK_BYTES);
        assert_eq!(
            api.uploads[0].idempotency_key,
            api.uploads[1].idempotency_key
        );
        assert_eq!(api.uploads[0].digest, api.uploads[1].digest);
        assert_eq!(api.uploads[2].offset, MAX_UPLOAD_CHUNK_BYTES as u64);
        assert_eq!(api.uploads[2].total, (MAX_UPLOAD_CHUNK_BYTES + 17) as u64);
        assert_eq!(api.uploads[3].part, "listing");
        assert_eq!(api.uploads[4].part, "asset-0");
        assert_eq!(sleeps, [Duration::from_millis(250)]);
    }

    #[test]
    fn reauthorizes_once_without_changing_the_create_request() {
        let mut api = MockApi {
            expire_first_create_token: true,
            ..MockApi::default()
        };
        execute_submission(
            &mut api,
            &submission(),
            "cp0ctl-22222222222222222222222222222222",
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(api.authorize_calls, 2);
        assert_eq!(api.create_calls, 2);
        assert_eq!(api.finalize_calls, 1);
    }

    #[test]
    fn validates_endpoint_tokens_ids_and_response_binding() {
        assert!(HttpSubmissionApi::new("http://developer.example.com").is_err());
        assert!(HttpSubmissionApi::new("https://developer.example.com/api").is_err());
        assert!(HttpSubmissionApi::new("https://developer.example.com?debug=1").is_err());
        assert!(HttpSubmissionApi::new("https://developer.example.com").is_ok());
        assert!(is_valid_submission_id(
            "sub_0123456789abcdef0123456789abcdef"
        ));
        assert!(!is_valid_submission_id("sub_../../escape"));
        assert!(
            validate_token(&DeviceTokenResponse {
                access_token: "x".repeat(32),
                token_type: "Bearer".into(),
                expires_in: 300,
                scope: "store.submit".into(),
            })
            .is_ok()
        );

        let submission = submission();
        let response = SubmissionResponse {
            submission_id: "sub_0123456789abcdef0123456789abcdef".into(),
            app_id: submission.app_id.clone(),
            version: submission.version.clone(),
            revision: 1,
            state: SubmissionState::Uploading,
            package_sha256: submission.package_sha256.clone(),
            listing_sha256: "00".repeat(32),
            assets: submission
                .assets
                .iter()
                .map(|asset| asset.descriptor.clone())
                .collect(),
            resource_version: 1,
            created_unix_seconds: 1,
        };
        assert!(
            validate_submission_response(&response, &submission, &[SubmissionState::Uploading])
                .is_err()
        );
    }
}
