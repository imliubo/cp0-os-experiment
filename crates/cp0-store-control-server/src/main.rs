use std::env;
use std::error::Error;
use std::net::{IpAddr, SocketAddr};

use cp0_store_control_server::{StoreControlService, connect, migrate, router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url =
        env::var("CP0_STORE_DATABASE_URL").map_err(|_| "CP0_STORE_DATABASE_URL is required")?;
    let listen_addr = env::var("CP0_STORE_LISTEN_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_owned())
        .parse::<SocketAddr>()?;
    require_safe_bind(listen_addr.ip())?;

    let pool = connect(&database_url, 10).await?;
    migrate(&pool).await?;
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    axum::serve(listener, router(StoreControlService::new(pool)))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn require_safe_bind(address: IpAddr) -> Result<(), Box<dyn Error>> {
    if address.is_loopback() || env::var("CP0_STORE_ALLOW_NON_LOOPBACK").as_deref() == Ok("1") {
        return Ok(());
    }
    Err("non-loopback bind requires CP0_STORE_ALLOW_NON_LOOPBACK=1 and external TLS".into())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
