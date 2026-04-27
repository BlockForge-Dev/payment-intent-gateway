use std::{env, time::Duration};

use application::{MockProviderAdapter, UnknownOutcomeResolutionService};
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
    let mock_provider_base_url =
        env::var("MOCK_PROVIDER_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3010".to_string());
    let provider_timeout_ms: u64 = env::var("PROVIDER_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);
    let status_check_delay_secs: u64 = env::var("STATUS_CHECK_DELAY_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let status_check_max_attempts: u32 = env::var("STATUS_CHECK_MAX_ATTEMPTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let status_check_batch_size: i64 = env::var("STATUS_CHECK_BATCH_SIZE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let status_check_poll_interval_ms: u64 = env::var("STATUS_CHECK_POLL_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    let mock_resolution_delay_ms: u64 = env::var("MOCK_RESOLUTION_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3_000);

    let pool = connect(&database_url, 10).await?;
    let repo = PostgresPersistence::new(pool);

    let provider = MockProviderAdapter::new(
        mock_provider_base_url,
        Duration::from_millis(provider_timeout_ms),
        mock_resolution_delay_ms,
    )?;

    let service = UnknownOutcomeResolutionService::new(
        repo,
        provider,
        Duration::from_secs(status_check_delay_secs),
        status_check_max_attempts,
    );

    info!(
        status_check_delay_secs = status_check_delay_secs,
        status_check_max_attempts = status_check_max_attempts,
        status_check_batch_size = status_check_batch_size,
        "unknown outcome resolver started"
    );

    let mut interval = tokio::time::interval(Duration::from_millis(status_check_poll_interval_ms));

    loop {
        interval.tick().await;

        match service
            .process_due_candidates(Utc::now(), status_check_batch_size)
            .await
        {
            Ok(results) => {
                for result in results {
                    info!(
                        intent_id = %result.intent_id,
                        state = %result.state,
                        provider_reference = ?result.provider_reference,
                        next_resolution_at = ?result.next_resolution_at,
                        resolution_attempt_count = result.resolution_attempt_count,
                        note = %result.note,
                        "processed ambiguous intent"
                    );
                }
            }
            Err(err) => {
                warn!(error = %err, "resolver pass failed");
            }
        }
    }
}
