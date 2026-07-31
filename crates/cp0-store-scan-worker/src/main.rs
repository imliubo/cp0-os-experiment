use std::env;
use std::error::Error;
use std::time::Duration;

use cp0_store_scan_worker::{RunOutcome, ScanWorker, connect, migrate};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let database_url =
        env::var("CP0_STORE_DATABASE_URL").map_err(|_| "CP0_STORE_DATABASE_URL is required")?;
    let object_root =
        env::var("CP0_STORE_OBJECT_ROOT").map_err(|_| "CP0_STORE_OBJECT_ROOT is required")?;
    let worker_id =
        env::var("CP0_STORE_SCAN_WORKER_ID").unwrap_or_else(|_| "scanner-primary".to_owned());
    let once = env::var("CP0_STORE_SCAN_ONCE").as_deref() == Ok("1");
    let pool = connect(&database_url, 4).await?;
    migrate(&pool).await?;
    let worker = ScanWorker::open(pool, object_root, worker_id).await?;

    loop {
        let outcome = worker.run_once().await?;
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
