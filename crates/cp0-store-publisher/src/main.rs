use std::env;
use std::error::Error;
use std::time::Duration;

use cp0_store_publisher::{RunOutcome, StorePublisher, connect, migrate};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url =
        env::var("CP0_STORE_DATABASE_URL").map_err(|_| "CP0_STORE_DATABASE_URL is required")?;
    let object_root =
        env::var("CP0_STORE_OBJECT_ROOT").map_err(|_| "CP0_STORE_OBJECT_ROOT is required")?;
    let origin_root =
        env::var("CP0_STORE_ORIGIN_ROOT").map_err(|_| "CP0_STORE_ORIGIN_ROOT is required")?;
    let signing_key =
        env::var("CP0_STORE_SIGNING_KEY").map_err(|_| "CP0_STORE_SIGNING_KEY is required")?;
    let base_url = env::var("CP0_STORE_PUBLIC_BASE_URL")
        .map_err(|_| "CP0_STORE_PUBLIC_BASE_URL is required")?;
    let worker_id =
        env::var("CP0_STORE_PUBLISHER_ID").unwrap_or_else(|_| "publisher-primary".to_owned());
    let once = env::var("CP0_STORE_PUBLISH_ONCE").as_deref() == Ok("1");
    let pool = connect(&database_url, 4).await?;
    migrate(&pool).await?;
    let publisher = StorePublisher::open(
        pool,
        object_root,
        origin_root,
        signing_key,
        base_url,
        worker_id,
    )
    .await?;

    loop {
        let outcome = publisher.run_once().await?;
        if once {
            return Ok(());
        }
        if matches!(outcome, RunOutcome::Idle | RunOutcome::Deferred { .. }) {
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(500)) => {}
                _ = tokio::signal::ctrl_c() => return Ok(()),
            }
        }
    }
}
