use std::{env, time::Duration};

use application::{ExecutionAttemptService, MockProviderAdapter, WorkerLeaseService};
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
    let worker_id = env::var("WORKER_ID").unwrap_or_else(|_| "worker-1".to_string());
    let lease_secs: u64 = env::var("LEASE_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let poll_interval_ms: u64 = env::var("POLL_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);
    let retry_delay_secs: u64 = env::var("RETRY_DELAY_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let status_check_delay_secs: u64 = env::var("STATUS_CHECK_DELAY_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let mock_provider_base_url =
        env::var("MOCK_PROVIDER_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:3010".to_string());
    let gateway_provider_webhook_url = env::var("GATEWAY_PROVIDER_WEBHOOK_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3000/provider/webhooks/mockpay".to_string());
    let provider_timeout_ms: u64 = env::var("PROVIDER_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2_000);
    let mock_resolution_delay_ms: u64 = env::var("MOCK_RESOLUTION_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3_000);
    let pre_execution_delay_ms: u64 = env::var("WORKER_PRE_EXECUTION_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let exit_after_first_lease = env_flag("WORKER_EXIT_AFTER_FIRST_LEASE");

    let pool = connect(&database_url, 10).await?;
    let repo = PostgresPersistence::new(pool);

    let lease_service = WorkerLeaseService::new(
        repo.clone(),
        worker_id.clone(),
        Duration::from_secs(lease_secs),
    );

    let provider = MockProviderAdapter::new(
        mock_provider_base_url,
        Duration::from_millis(provider_timeout_ms),
        mock_resolution_delay_ms,
    )?
    .with_webhook_callback_url(Some(gateway_provider_webhook_url));

    let execution_service = ExecutionAttemptService::new(
        repo.clone(),
        provider,
        Duration::from_secs(retry_delay_secs),
        Duration::from_secs(status_check_delay_secs),
    );

    info!(
        worker_id = %worker_id,
        lease_secs = lease_secs,
        poll_interval_ms = poll_interval_ms,
        retry_delay_secs = retry_delay_secs,
        status_check_delay_secs = status_check_delay_secs,
        pre_execution_delay_ms = pre_execution_delay_ms,
        exit_after_first_lease = exit_after_first_lease,
        "worker started"
    );

    let mut interval = tokio::time::interval(Duration::from_millis(poll_interval_ms));

    loop {
        interval.tick().await;

        let now = Utc::now();
        match lease_service.poll_once(now).await {
            Ok(Some(leased)) => {
                info!(
                    intent_id = %leased.intent.id,
                    lease_token = %leased.lease_token,
                    merchant_reference = %leased.intent.merchant_reference.0,
                    "leased intent for execution"
                );

                if exit_after_first_lease {
                    warn!(
                        intent_id = %leased.intent.id,
                        lease_token = %leased.lease_token,
                        "exiting immediately after lease acquisition to simulate a worker crash"
                    );
                    std::process::exit(86);
                }

                if pre_execution_delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(pre_execution_delay_ms)).await;
                }

                match execution_service
                    .execute_leased_intent(leased, Utc::now())
                    .await
                {
                    Ok(summary) => {
                        info!(
                            intent_id = %summary.intent_id,
                            state = %summary.state,
                            provider_reference = ?summary.provider_reference,
                            retry_available_at = ?summary.retry_available_at,
                            next_resolution_at = ?summary.next_resolution_at,
                            note = %summary.outcome_note,
                            "execution attempt finished"
                        );
                    }
                    Err(err) => {
                        warn!(error = %err, "execution attempt failed unexpectedly");
                    }
                }
            }
            Ok(None) => {}
            Err(err) => {
                warn!(error = %err, "worker poll failed");
            }
        }
    }
}

fn env_flag(key: &str) -> bool {
    matches!(
        env::var(key)
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}
