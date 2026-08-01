use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use cp0_store_portal_server::{OidcProvider, OidcProviderConfig, ProductionOidcProvider};
use cp0_store_workforce_server::{WorkforceSecrets, WorkforceService, connect, migrate, router};
use serde::Deserialize;

const MAX_CONFIG_BYTES: u64 = 128 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkforceConfig {
    review: AudienceConfig,
    operations: AudienceConfig,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AudienceConfig {
    allowed_origin: String,
    post_login_uri: String,
    providers: Vec<OidcProviderConfig>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url = required_env("CP0_WORKFORCE_DATABASE_URL")?;
    let config_path = required_env("CP0_WORKFORCE_CONFIG")?;
    let config = load_config(&config_path)?;
    validate_redirect_uris(&config)?;
    let secrets = WorkforceSecrets::from_base64(
        &required_env("CP0_WORKFORCE_CSRF_KEY")?,
        &required_env("CP0_WORKFORCE_NONCE_KEY")?,
        &required_env("CP0_WORKFORCE_PKCE_KEY")?,
        &required_env("CP0_WORKFORCE_SUBJECT_KEY")?,
        &required_env("CP0_WORKFORCE_CONTROL_TOKEN_KEY")?,
    )
    .map_err(|_| "Workforce keys must be distinct 32-byte base64url values")?;

    let review = config.review;
    let operations = config.operations;
    let review_providers = build_providers(review.providers)?;
    let operations_providers = build_providers(operations.providers)?;
    let listen_addr = env::var("CP0_WORKFORCE_LISTEN_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8791".to_owned())
        .parse::<SocketAddr>()?;
    require_safe_bind(listen_addr.ip())?;

    let pool = connect(&database_url, 10).await?;
    migrate(&pool).await?;
    let service = WorkforceService::new(
        pool,
        secrets,
        review_providers,
        operations_providers,
        review.allowed_origin,
        review.post_login_uri,
        operations.allowed_origin,
        operations.post_login_uri,
    )
    .map_err(|_| "invalid Workforce service configuration")?;
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    axum::serve(listener, router(service))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn load_config(path: &str) -> Result<WorkforceConfig, Box<dyn Error>> {
    let mut file = File::open(path)?;
    let mut encoded = Vec::new();
    file.by_ref()
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut encoded)?;
    if encoded.len() as u64 > MAX_CONFIG_BYTES {
        return Err("Workforce config exceeds 128 KiB".into());
    }
    Ok(serde_json::from_slice(&encoded)?)
}

fn build_providers(
    configs: Vec<OidcProviderConfig>,
) -> Result<Vec<Arc<dyn OidcProvider>>, Box<dyn Error>> {
    let mut providers: Vec<Arc<dyn OidcProvider>> = Vec::with_capacity(configs.len());
    for config in configs {
        let client_secret = config
            .client_secret_env
            .as_deref()
            .map(required_provider_secret)
            .transpose()?;
        providers.push(Arc::new(
            ProductionOidcProvider::new(config, client_secret)
                .map_err(|_| "invalid OIDC provider configuration")?,
        ));
    }
    Ok(providers)
}

fn required_env(name: &str) -> Result<String, Box<dyn Error>> {
    env::var(name).map_err(|_| format!("{name} is required").into())
}

fn required_provider_secret(name: &str) -> Result<String, Box<dyn Error>> {
    if !valid_environment_name(name) {
        return Err("invalid OIDC client secret environment variable name".into());
    }
    required_env(name)
}

fn valid_environment_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z'))
        && value.len() <= 64
        && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn validate_redirect_uris(config: &WorkforceConfig) -> Result<(), Box<dyn Error>> {
    let review_origin = validate_audience_redirects(&config.review, "review")?;
    let operations_origin = validate_audience_redirects(&config.operations, "operations")?;
    if review_origin == operations_origin {
        return Err("Review and Operations must use different origins".into());
    }
    Ok(())
}

fn validate_audience_redirects(
    config: &AudienceConfig,
    callback_prefix: &str,
) -> Result<String, Box<dyn Error>> {
    let origin = url::Url::parse(&config.allowed_origin)?;
    if origin.scheme() != "https"
        || origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.path() != "/"
        || origin.query().is_some()
        || origin.fragment().is_some()
    {
        return Err("allowed_origin must be a bare HTTPS origin".into());
    }
    let origin = origin.origin().ascii_serialization();
    let expected = format!("{origin}/{callback_prefix}/auth/callback");
    if config
        .providers
        .iter()
        .any(|provider| provider.redirect_uri != expected)
    {
        return Err(format!(
            "every {callback_prefix} OIDC redirect_uri must be the exact Workforce callback"
        )
        .into());
    }
    Ok(origin)
}

fn require_safe_bind(address: IpAddr) -> Result<(), Box<dyn Error>> {
    if address.is_loopback() || env::var("CP0_WORKFORCE_ALLOW_NON_LOOPBACK").as_deref() == Ok("1") {
        return Ok(());
    }
    Err("non-loopback bind requires CP0_WORKFORCE_ALLOW_NON_LOOPBACK=1 and external TLS".into())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(redirect_uri: &str) -> OidcProviderConfig {
        OidcProviderConfig {
            key: "primary".to_owned(),
            label: "Primary identity".to_owned(),
            issuer: "https://identity.example.com".to_owned(),
            authorization_endpoint: "https://identity.example.com/authorize".to_owned(),
            token_endpoint: "https://identity.example.com/token".to_owned(),
            client_id: "cardputerzero-workforce".to_owned(),
            client_secret_env: None,
            redirect_uri: redirect_uri.to_owned(),
            accepted_signing_algorithms: vec!["RS256".to_owned()],
            accepted_mfa_acr: vec!["urn:example:acr:mfa".to_owned()],
            clock_skew_seconds: 60,
            jwks: serde_json::json!({"keys": []}),
        }
    }

    fn config() -> WorkforceConfig {
        WorkforceConfig {
            review: AudienceConfig {
                allowed_origin: "https://review.cardputerzero.dev".to_owned(),
                post_login_uri: "https://review.cardputerzero.dev/queue".to_owned(),
                providers: vec![provider(
                    "https://review.cardputerzero.dev/review/auth/callback",
                )],
            },
            operations: AudienceConfig {
                allowed_origin: "https://operations.cardputerzero.dev".to_owned(),
                post_login_uri: "https://operations.cardputerzero.dev/console".to_owned(),
                providers: vec![provider(
                    "https://operations.cardputerzero.dev/operations/auth/callback",
                )],
            },
        }
    }

    #[test]
    fn environment_names_are_closed() {
        assert!(valid_environment_name("CP0_OIDC_REVIEW_SECRET"));
        assert!(!valid_environment_name("lowercase"));
        assert!(!valid_environment_name("CP0-BAD"));
    }

    #[test]
    fn audience_callbacks_and_origins_are_exact() {
        let mut value = config();
        validate_redirect_uris(&value).unwrap();
        value.operations.allowed_origin = value.review.allowed_origin.clone();
        assert!(validate_redirect_uris(&value).is_err());

        let mut value = config();
        value.review.providers[0].redirect_uri.push('/');
        assert!(validate_redirect_uris(&value).is_err());
    }
}
