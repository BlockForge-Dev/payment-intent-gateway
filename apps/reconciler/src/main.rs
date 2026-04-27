use std::{env, time::Duration};

use application::{MockProviderAdapter, ReconciliationService};
use persistence::{connect, PostgresPersistence};
use tracing::{info, warn};
use uuid::Uuid;

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
    let mock_resolution_delay_ms: u64 = env::var("MOCK_RESOLUTION_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3_000);
    let reconcile_intent_ids = env::var("RECONCILE_INTENT_IDS")
        .expect("RECONCILE_INTENT_IDS must be set to a comma-separated list of UUIDs");

    let intent_ids = reconcile_intent_ids
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(Uuid::parse_str)
        .collect::<Result<Vec<_>, _>>()?;

    if intent_ids.is_empty() {
        warn!("no reconciliation intent ids were provided");
        return Ok(());
    }

    let pool = connect(&database_url, 10).await?;
    let repo = PostgresPersistence::new(pool);
    let provider = MockProviderAdapter::new(
        mock_provider_base_url,
        Duration::from_millis(provider_timeout_ms),
        mock_resolution_delay_ms,
    )?;
    let service = ReconciliationService::new(repo, provider);

    let results = service.reconcile_selected_intents(intent_ids).await?;

    for result in results {
        info!(
            intent_id = %result.intent_id,
            previous_state = %result.previous_state,
            state = %result.state,
            provider_reference = ?result.provider_reference,
            provider_state_seen = %result.provider_state_seen,
            comparison = %result.comparison,
            decision = %result.decision,
            note = %result.note,
            "reconciliation completed"
        );
    }

    Ok(())
}
