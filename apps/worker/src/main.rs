use std::{ env, time::Duration };

use application::WorkerLeaseService;
use chrono::{ Duration as ChronoDuration, Utc };
use persistence::{ connect, PostgresPersistence };
use tracing::{ info, warn };

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber
        ::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())
        )
        .init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let worker_id = env::var("WORKER_ID").unwrap_or_else(|_| "worker-1".to_string());
    let lease_secs: u64 = env
        ::var("LEASE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let poll_interval_ms: u64 = env
        ::var("POLL_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    let requeue_delay_secs: i64 = env
        ::var("M4_REQUEUE_DELAY_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);

    let pool = connect(&database_url, 10).await?;
    let repo = PostgresPersistence::new(pool);
    let service = WorkerLeaseService::new(repo, worker_id.clone(), Duration::from_secs(lease_secs));

    info!(
        worker_id = %worker_id,
        lease_secs = lease_secs,
        poll_interval_ms = poll_interval_ms,
        "worker foundation started"
    );

    let mut interval = tokio::time::interval(Duration::from_millis(poll_interval_ms));

    loop {
        interval.tick().await;

        let now = Utc::now();
        match service.poll_once(now).await {
            Ok(Some(leased)) => {
                info!(
                    intent_id = %leased.intent.id,
                    worker_id = %leased.worker_id,
                    lease_token = %leased.lease_token,
                    lease_expires_at = %leased.lease_expires_at,
                    "lease acquired"
                );

                // Milestone 4 behavior only:
                // We are proving safe acquisition and release, not provider execution yet.
                // So we return the lease back to queue after a short delay.
                let next_available = now + ChronoDuration::seconds(requeue_delay_secs);

                let released = service.release_without_execution(
                    &leased,
                    Utc::now(),
                    next_available,
                    Some("milestone 4 foundation release; execution not wired yet".to_string())
                ).await?;

                info!(
                    intent_id = %released.id,
                    state = ?released.state,
                    next_available_at = %next_available,
                    "lease released back to queue"
                );
            }
            Ok(None) => {}
            Err(err) => {
                warn!(error = %err, "worker poll failed");
            }
        }
    }
}
