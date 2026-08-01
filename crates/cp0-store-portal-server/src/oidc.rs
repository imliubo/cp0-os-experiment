use std::collections::HashSet;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreJsonWebKeySet, CoreJwsSigningAlgorithm,
};
use openidconnect::{
    AccessTokenHash, AsyncHttpClient, AuthUrl, AuthorizationCode, ClientId, ClientSecret,
    CsrfToken, EndpointNotSet, EndpointSet, HttpRequest, HttpResponse, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
    TokenUrl,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::sha256_hex;

const MAX_AUTHORIZATION_URI_BYTES: usize = 4096;
const MAX_CODE_BYTES: usize = 4096;
const MAX_SUBJECT_BYTES: usize = 1024;
const OIDC_TRANSACTION_SECONDS: i64 = 600;
const MAX_TOKEN_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_TOKEN_REQUEST_BYTES: usize = 16 * 1024;

type ConfiguredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
    EndpointNotSet,
>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthIntent {
    Login,
    StepUp,
    Link,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedIdentity {
    pub issuer: String,
    pub subject: String,
    pub email: String,
    pub email_verified: bool,
    pub mfa_authenticated_unix_seconds: Option<i64>,
}

#[derive(Debug)]
pub enum OidcError {
    InvalidConfiguration,
    InvalidRequest,
    ProviderUnavailable,
    InvalidToken,
}

pub type OidcFuture<'a> =
    Pin<Box<dyn Future<Output = Result<VerifiedIdentity, OidcError>> + Send + 'a>>;

pub trait OidcProvider: Send + Sync {
    fn key(&self) -> &str;
    fn issuer(&self) -> &str;
    fn config_sha256(&self) -> &str;
    fn authorization_uri(
        &self,
        intent: AuthIntent,
        state: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<String, OidcError>;
    fn exchange<'a>(
        &'a self,
        intent: AuthIntent,
        code: &'a str,
        nonce: &'a str,
        pkce_verifier: &'a str,
        now: i64,
    ) -> OidcFuture<'a>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OidcProviderConfig {
    pub key: String,
    pub label: String,
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub client_id: String,
    pub client_secret_env: Option<String>,
    pub redirect_uri: String,
    pub accepted_signing_algorithms: Vec<String>,
    pub accepted_mfa_acr: Vec<String>,
    pub clock_skew_seconds: u16,
    pub jwks: Value,
}

pub struct ProductionOidcProvider {
    key: String,
    issuer: String,
    config_sha256: String,
    client: ConfiguredClient,
    http_client: BoundedTokenHttpClient,
    accepted_signing_algorithms: Vec<CoreJwsSigningAlgorithm>,
    accepted_mfa_acr: HashSet<String>,
    clock_skew_seconds: i64,
}

#[derive(Clone)]
struct BoundedTokenHttpClient {
    client: openidconnect::reqwest::Client,
    token_endpoint: String,
}

#[derive(Debug)]
enum BoundedHttpError {
    InvalidRequest,
    RequestFailed,
    ResponseTooLarge,
    InvalidResponse,
}

impl Display for BoundedHttpError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidRequest => "invalid OIDC HTTP request",
            Self::RequestFailed => "OIDC HTTP request failed",
            Self::ResponseTooLarge => "OIDC HTTP response exceeded limit",
            Self::InvalidResponse => "invalid OIDC HTTP response",
        };
        formatter.write_str(message)
    }
}

impl Error for BoundedHttpError {}

impl<'c> AsyncHttpClient<'c> for BoundedTokenHttpClient {
    type Error = BoundedHttpError;
    type Future = Pin<Box<dyn Future<Output = Result<HttpResponse, Self::Error>> + Send + 'c>>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(async move {
            if request.method() != openidconnect::http::Method::POST
                || request.uri().to_string() != self.token_endpoint
                || request.body().len() > MAX_TOKEN_REQUEST_BYTES
            {
                return Err(BoundedHttpError::InvalidRequest);
            }
            let request: openidconnect::reqwest::Request = request
                .try_into()
                .map_err(|_| BoundedHttpError::InvalidRequest)?;
            let mut response = self
                .client
                .execute(request)
                .await
                .map_err(|_| BoundedHttpError::RequestFailed)?;
            if response
                .content_length()
                .is_some_and(|length| length > MAX_TOKEN_RESPONSE_BYTES as u64)
            {
                return Err(BoundedHttpError::ResponseTooLarge);
            }
            let status = response.status();
            let version = response.version();
            let headers = response.headers().clone();
            let mut body = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| BoundedHttpError::RequestFailed)?
            {
                if body.len() + chunk.len() > MAX_TOKEN_RESPONSE_BYTES {
                    return Err(BoundedHttpError::ResponseTooLarge);
                }
                body.extend_from_slice(&chunk);
            }
            let mut builder = openidconnect::http::Response::builder()
                .status(status)
                .version(version);
            for (name, value) in &headers {
                builder = builder.header(name, value);
            }
            builder
                .body(body)
                .map_err(|_| BoundedHttpError::InvalidResponse)
        })
    }
}

impl ProductionOidcProvider {
    pub fn new(
        config: OidcProviderConfig,
        client_secret: Option<String>,
    ) -> Result<Self, OidcError> {
        validate_provider_config(&config, client_secret.as_deref())?;
        let config_sha256 =
            sha256_hex(&serde_json::to_vec(&config).map_err(|_| OidcError::InvalidConfiguration)?);
        validate_public_jwks(&config.jwks, &config.accepted_signing_algorithms)?;
        let jwks: CoreJsonWebKeySet = serde_json::from_value(config.jwks.clone())
            .map_err(|_| OidcError::InvalidConfiguration)?;
        let mut client = CoreClient::new(
            ClientId::new(config.client_id.clone()),
            IssuerUrl::new(config.issuer.clone()).map_err(|_| OidcError::InvalidConfiguration)?,
            jwks,
        );
        if let Some(secret) = client_secret {
            client = client.set_client_secret(ClientSecret::new(secret));
        }
        let client = client
            .set_auth_uri(
                AuthUrl::new(config.authorization_endpoint.clone())
                    .map_err(|_| OidcError::InvalidConfiguration)?,
            )
            .set_token_uri(
                TokenUrl::new(config.token_endpoint.clone())
                    .map_err(|_| OidcError::InvalidConfiguration)?,
            )
            .set_redirect_uri(
                RedirectUrl::new(config.redirect_uri.clone())
                    .map_err(|_| OidcError::InvalidConfiguration)?,
            );
        let accepted_signing_algorithms = config
            .accepted_signing_algorithms
            .iter()
            .map(|value| parse_safe_signing_algorithm(value))
            .collect::<Result<Vec<_>, _>>()?;
        let http_client = openidconnect::reqwest::ClientBuilder::new()
            .redirect(openidconnect::reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| OidcError::InvalidConfiguration)?;
        Ok(Self {
            key: config.key,
            issuer: config.issuer,
            config_sha256,
            client,
            http_client: BoundedTokenHttpClient {
                client: http_client,
                token_endpoint: config.token_endpoint,
            },
            accepted_signing_algorithms,
            accepted_mfa_acr: config.accepted_mfa_acr.into_iter().collect(),
            clock_skew_seconds: i64::from(config.clock_skew_seconds),
        })
    }

    async fn exchange_inner(
        &self,
        intent: AuthIntent,
        code: &str,
        nonce: &str,
        pkce_verifier: &str,
        now: i64,
    ) -> Result<VerifiedIdentity, OidcError> {
        if !(16..=MAX_CODE_BYTES).contains(&code.len())
            || code.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(OidcError::InvalidRequest);
        }
        let authorization_code = AuthorizationCode::new(code.to_owned());
        let token_response = self
            .client
            .exchange_code(authorization_code)
            .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_owned()))
            .request_async(&self.http_client)
            .await
            .map_err(|_| OidcError::ProviderUnavailable)?;
        let id_token = token_response.id_token().ok_or(OidcError::InvalidToken)?;
        let signing_algorithm = id_token
            .signing_alg()
            .map_err(|_| OidcError::InvalidToken)?;
        if !self.accepted_signing_algorithms.contains(signing_algorithm) {
            return Err(OidcError::InvalidToken);
        }
        let verifier = self.client.id_token_verifier();
        let claims = id_token
            .claims(&verifier, &Nonce::new(nonce.to_owned()))
            .map_err(|_| OidcError::InvalidToken)?;
        if let Some(expected_access_token_hash) = claims.access_token_hash() {
            let actual_access_token_hash = AccessTokenHash::from_token(
                token_response.access_token(),
                signing_algorithm,
                id_token
                    .signing_key(&verifier)
                    .map_err(|_| OidcError::InvalidToken)?,
            )
            .map_err(|_| OidcError::InvalidToken)?;
            if actual_access_token_hash != *expected_access_token_hash {
                return Err(OidcError::InvalidToken);
            }
        }
        let issued = claims.issue_time().timestamp();
        if issued < now - OIDC_TRANSACTION_SECONDS - self.clock_skew_seconds
            || issued > now + self.clock_skew_seconds
        {
            return Err(OidcError::InvalidToken);
        }
        let subject = claims.subject().as_str();
        if subject.is_empty() || subject.len() > MAX_SUBJECT_BYTES {
            return Err(OidcError::InvalidToken);
        }
        let email_verified = claims.email_verified().unwrap_or(false);
        let email = claims.email().ok_or(OidcError::InvalidToken)?.as_str();
        if !email_verified {
            return Err(OidcError::InvalidToken);
        }
        let email = normalize_verified_email(email)?;
        let auth_time = claims.auth_time().map(|value| value.timestamp());
        if auth_time.is_some_and(|value| value < 1 || value > now + self.clock_skew_seconds) {
            return Err(OidcError::InvalidToken);
        }
        let mfa_acr = claims
            .auth_context_ref()
            .map(|value| value.as_str())
            .is_some_and(|value| self.accepted_mfa_acr.contains(value));
        let mfa_authenticated_unix_seconds = auth_time.filter(|_| mfa_acr);
        if intent == AuthIntent::StepUp
            && !mfa_authenticated_unix_seconds
                .is_some_and(|value| value >= now - 300 && value <= now + self.clock_skew_seconds)
        {
            return Err(OidcError::InvalidToken);
        }
        Ok(VerifiedIdentity {
            issuer: self.issuer.clone(),
            subject: subject.to_owned(),
            email,
            email_verified,
            mfa_authenticated_unix_seconds,
        })
    }
}

impl OidcProvider for ProductionOidcProvider {
    fn key(&self) -> &str {
        &self.key
    }

    fn issuer(&self) -> &str {
        &self.issuer
    }

    fn config_sha256(&self) -> &str {
        &self.config_sha256
    }

    fn authorization_uri(
        &self,
        intent: AuthIntent,
        state: &str,
        nonce: &str,
        pkce_verifier: &str,
    ) -> Result<String, OidcError> {
        let verifier = PkceCodeVerifier::new(pkce_verifier.to_owned());
        let state = state.to_owned();
        let nonce = nonce.to_owned();
        let request = self
            .client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                move || CsrfToken::new(state),
                move || Nonce::new(nonce),
            )
            .add_scope(Scope::new("email".to_owned()))
            .set_pkce_challenge(PkceCodeChallenge::from_code_verifier_sha256(&verifier));
        let request = if intent == AuthIntent::StepUp {
            request
                .add_extra_param("prompt", "login")
                .add_extra_param("max_age", "0")
        } else {
            request
        };
        let uri = request.url().0.to_string();
        if uri.len() > MAX_AUTHORIZATION_URI_BYTES {
            return Err(OidcError::InvalidConfiguration);
        }
        Ok(uri)
    }

    fn exchange<'a>(
        &'a self,
        intent: AuthIntent,
        code: &'a str,
        nonce: &'a str,
        pkce_verifier: &'a str,
        now: i64,
    ) -> OidcFuture<'a> {
        Box::pin(self.exchange_inner(intent, code, nonce, pkce_verifier, now))
    }
}

fn validate_provider_config(
    config: &OidcProviderConfig,
    client_secret: Option<&str>,
) -> Result<(), OidcError> {
    if !valid_provider_key(&config.key)
        || config.label.trim() != config.label
        || !(1..=80).contains(&config.label.len())
        || !(1..=256).contains(&config.client_id.len())
        || config.client_id.contains(['\r', '\n'])
        || config.clock_skew_seconds > 300
        || !(1..=4).contains(&config.accepted_signing_algorithms.len())
        || config
            .accepted_signing_algorithms
            .iter()
            .collect::<HashSet<_>>()
            .len()
            != config.accepted_signing_algorithms.len()
        || config.accepted_mfa_acr.len() > 8
        || config.accepted_mfa_acr.iter().any(|value| {
            value.trim() != value
                || value.is_empty()
                || value.len() > 256
                || value.contains(['\r', '\n'])
        })
        || config.accepted_mfa_acr.iter().collect::<HashSet<_>>().len()
            != config.accepted_mfa_acr.len()
    {
        return Err(OidcError::InvalidConfiguration);
    }
    validate_exact_https_url(&config.issuer, true)?;
    validate_exact_https_url(&config.authorization_endpoint, true)?;
    validate_exact_https_url(&config.token_endpoint, true)?;
    validate_exact_https_url(&config.redirect_uri, true)?;
    if config.client_secret_env.is_some() != client_secret.is_some()
        || client_secret.is_some_and(|value| {
            value.len() < 16 || value.len() > 4096 || value.contains(['\r', '\n'])
        })
    {
        return Err(OidcError::InvalidConfiguration);
    }
    Ok(())
}

fn validate_exact_https_url(value: &str, allow_path: bool) -> Result<(), OidcError> {
    let parsed = Url::parse(value).map_err(|_| OidcError::InvalidConfiguration)?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || (!allow_path && parsed.path() != "/")
    {
        return Err(OidcError::InvalidConfiguration);
    }
    Ok(())
}

fn parse_safe_signing_algorithm(value: &str) -> Result<CoreJwsSigningAlgorithm, OidcError> {
    match value {
        "RS256" => Ok(CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256),
        "RS384" => Ok(CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha384),
        "RS512" => Ok(CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha512),
        "PS256" => Ok(CoreJwsSigningAlgorithm::RsaSsaPssSha256),
        "PS384" => Ok(CoreJwsSigningAlgorithm::RsaSsaPssSha384),
        "PS512" => Ok(CoreJwsSigningAlgorithm::RsaSsaPssSha512),
        "EdDSA" => Ok(CoreJwsSigningAlgorithm::EdDsa),
        _ => Err(OidcError::InvalidConfiguration),
    }
}

fn validate_public_jwks(jwks: &Value, accepted_algorithms: &[String]) -> Result<(), OidcError> {
    let keys = jwks
        .as_object()
        .and_then(|object| object.get("keys"))
        .and_then(Value::as_array)
        .ok_or(OidcError::InvalidConfiguration)?;
    if !(1..=32).contains(&keys.len()) {
        return Err(OidcError::InvalidConfiguration);
    }
    for key in keys {
        let key = key.as_object().ok_or(OidcError::InvalidConfiguration)?;
        if ["d", "p", "q", "dp", "dq", "qi", "oth", "k"]
            .iter()
            .any(|field| key.contains_key(*field))
            || !matches!(
                key.get("kty").and_then(Value::as_str),
                Some("RSA" | "EC" | "OKP")
            )
            || key
                .get("use")
                .and_then(Value::as_str)
                .is_some_and(|value| value != "sig")
            || key
                .get("alg")
                .and_then(Value::as_str)
                .is_some_and(|value| !accepted_algorithms.iter().any(|allowed| allowed == value))
        {
            return Err(OidcError::InvalidConfiguration);
        }
    }
    Ok(())
}

fn normalize_verified_email(value: &str) -> Result<String, OidcError> {
    let normalized = value.trim().to_ascii_lowercase();
    let mut parts = normalized.split('@');
    if !value.is_ascii()
        || value.trim() != value
        || !(3..=254).contains(&normalized.len())
        || parts.next().is_none_or(str::is_empty)
        || parts.next().is_none_or(str::is_empty)
        || parts.next().is_some()
        || normalized.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(OidcError::InvalidToken);
    }
    Ok(normalized)
}

fn valid_provider_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && value.len() <= 32
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_algorithms_and_urls() {
        assert!(parse_safe_signing_algorithm("RS256").is_ok());
        assert!(parse_safe_signing_algorithm("HS256").is_err());
        assert!(parse_safe_signing_algorithm("none").is_err());
        assert!(validate_exact_https_url("https://identity.example.com", false).is_ok());
        assert!(validate_exact_https_url("https://identity.example.com/tenant", true).is_ok());
        assert!(validate_exact_https_url("http://identity.example.com/token", true).is_err());
        assert!(
            validate_public_jwks(
                &serde_json::json!({"keys": [{"kty": "oct", "k": "secret"}]}),
                &["RS256".to_owned()]
            )
            .is_err()
        );
    }

    #[test]
    fn normalizes_only_bounded_verified_email_shapes() {
        assert_eq!(
            normalize_verified_email("Person@Example.COM").unwrap(),
            "person@example.com"
        );
        assert!(normalize_verified_email("missing-at").is_err());
        assert!(normalize_verified_email("a@@example.com").is_err());
        assert!(normalize_verified_email("u@example.com\n").is_err());
    }
}
