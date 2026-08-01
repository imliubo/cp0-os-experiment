use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use cp0_store_portal_server::{
    OidcProvider, OidcProviderConfig, PortalSecrets, PortalService, ProductionOidcProvider,
    connect, migrate, router,
};
use serde::Deserialize;

const MAX_CONFIG_BYTES: u64 = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PortalConfig {
    allowed_origin: String,
    post_login_uri: String,
    providers: Vec<OidcProviderConfig>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url =
        env::var("CP0_PORTAL_DATABASE_URL").map_err(|_| "CP0_PORTAL_DATABASE_URL is required")?;
    let config_path = env::var("CP0_PORTAL_CONFIG").map_err(|_| "CP0_PORTAL_CONFIG is required")?;
    let config = load_config(&config_path)?;
    validate_redirect_uris(&config)?;
    let secrets = PortalSecrets::from_base64(
        &required_env("CP0_PORTAL_CSRF_KEY")?,
        &required_env("CP0_PORTAL_NONCE_KEY")?,
        &required_env("CP0_PORTAL_PKCE_KEY")?,
        &required_env("CP0_PORTAL_SUBJECT_KEY")?,
    )
    .map_err(|_| "Portal keys must be distinct 32-byte base64url values")?;
    let mut providers: Vec<Arc<dyn OidcProvider>> = Vec::with_capacity(config.providers.len());
    for provider in config.providers {
        let client_secret = provider
            .client_secret_env
            .as_deref()
            .map(required_provider_secret)
            .transpose()?;
        providers.push(Arc::new(
            ProductionOidcProvider::new(provider, client_secret)
                .map_err(|_| "invalid OIDC provider configuration")?,
        ));
    }
    let listen_addr = env::var("CP0_PORTAL_LISTEN_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8790".to_owned())
        .parse::<SocketAddr>()?;
    require_safe_bind(listen_addr.ip())?;
    let pool = connect(&database_url, 10).await?;
    migrate(&pool).await?;
    let service = PortalService::new(
        pool,
        secrets,
        providers,
        config.allowed_origin,
        config.post_login_uri,
    )
    .map_err(|_| "invalid Portal service configuration")?;
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    axum::serve(listener, router(service))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn load_config(path: &str) -> Result<PortalConfig, Box<dyn Error>> {
    let mut file = File::open(path)?;
    let mut encoded = Vec::new();
    file.by_ref()
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut encoded)?;
    if encoded.len() as u64 > MAX_CONFIG_BYTES {
        return Err("Portal config exceeds 64 KiB".into());
    }
    Ok(serde_json::from_slice(&encoded)?)
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

fn validate_redirect_uris(config: &PortalConfig) -> Result<(), Box<dyn Error>> {
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
    let expected = format!(
        "{}/portal/auth/callback",
        origin.origin().ascii_serialization()
    );
    if config
        .providers
        .iter()
        .any(|provider| provider.redirect_uri != expected)
    {
        return Err("every OIDC redirect_uri must be the exact Portal callback".into());
    }
    Ok(())
}

fn require_safe_bind(address: IpAddr) -> Result<(), Box<dyn Error>> {
    if address.is_loopback() || env::var("CP0_PORTAL_ALLOW_NON_LOOPBACK").as_deref() == Ok("1") {
        return Ok(());
    }
    Err("non-loopback bind requires CP0_PORTAL_ALLOW_NON_LOOPBACK=1 and external TLS".into())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_names_are_closed() {
        assert!(valid_environment_name("CP0_OIDC_PRIMARY_SECRET"));
        assert!(!valid_environment_name("lowercase"));
        assert!(!valid_environment_name("CP0-BAD"));
    }
}
