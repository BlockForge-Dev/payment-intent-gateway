use std::{env, time::Duration};

use application::{CallbackDeliveryService, HttpCallbackDispatcher};
use chrono::Utc;
use persistence::{connect, PostgresPersistence};
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let worker_id =
        env::var("CALLBACK_WORKER_ID").unwrap_or_else(|_| "callback-worker-1".to_string());
    let lease_secs: u64 = env::var("CALLBACK_LEASE_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(30);
    let poll_interval_ms: u64 = env::var("CALLBACK_POLL_INTERVAL_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1000);
    let retry_delay_secs: u64 = env::var("CALLBACK_RETRY_DELAY_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(15);
    let max_attempts: i32 = env::var("CALLBACK_MAX_ATTEMPTS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(5);
    let timeout_ms: u64 = env::var("CALLBACK_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3_000);
    let signing_secret = env::var("CALLBACK_SIGNING_SECRET").ok();

    let pool = connect(&database_url, 10).await?;
    let repo = PostgresPersistence::new(pool);
    let dispatcher = HttpCallbackDispatcher::new(Duration::from_millis(timeout_ms))?;
    let service = CallbackDeliveryService::new(
        repo,
        dispatcher,
        worker_id.clone(),
        Duration::from_secs(lease_secs),
        Duration::from_secs(retry_delay_secs),
        max_attempts,
    )
    .with_signing_secret(signing_secret);

    info!(
        worker_id = %worker_id,
        lease_secs = lease_secs,
        poll_interval_ms = poll_interval_ms,
        retry_delay_secs = retry_delay_secs,
        max_attempts = max_attempts,
        timeout_ms = timeout_ms,
        "callback worker started"
    );

    let mut interval = tokio::time::interval(Duration::from_millis(poll_interval_ms));

    loop {
        interval.tick().await;

        match service.poll_once(Utc::now()).await {
            Ok(Some(summary)) => {
                info!(
                    notification_id = summary.notification_id,
                    intent_id = %summary.intent_id,
                    destination_url = %summary.destination_url,
                    target_state = %summary.target_state,
                    attempt_no = summary.attempt_no,
                    outcome = %summary.outcome,
                    http_status_code = ?summary.http_status_code,
                    retry_at = ?summary.retry_at,
                    note = %summary.note,
                    "processed callback notification"
                );
            }
            Ok(None) => {}
            Err(err) => {
                warn!(error = %err, "callback worker poll failed");
            }
        }
    }
}
