mod app_state;
mod error;
mod routes;

use std::{env, net::SocketAddr, time::Duration};

use application::{PaymentIntentService, ProviderWebhookService};
use axum::{
    routing::{get, post},
    Router,
};
use persistence::{connect, PostgresPersistence};
use tracing::info;

use crate::app_state::AppState;
use crate::routes::payment_intents::{
    create_payment_intent, get_payment_intent, get_payment_intent_receipt, list_payment_intents,
};
use crate::routes::provider_webhooks::ingest_provider_webhook;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let api_bearer_token = env::var("API_BEARER_TOKEN").expect("API_BEARER_TOKEN must be set");
    let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
    let status_check_delay_secs: u64 = env::var("STATUS_CHECK_DELAY_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);
    let mock_provider_webhook_secret = env::var("MOCK_PROVIDER_WEBHOOK_SECRET").ok();

    let pool = connect(&database_url, 10).await?;
    let repo = PostgresPersistence::new(pool);
    let service = PaymentIntentService::new(repo.clone())
        .with_supported_providers(vec!["paystack".into(), "mockpay".into()]);
    let webhook_service =
        ProviderWebhookService::new(repo, Duration::from_secs(status_check_delay_secs))
            .with_supported_providers(vec!["paystack".into(), "mockpay".into()]);

    let state = AppState {
        service,
        webhook_service,
        api_bearer_token,
        mock_provider_webhook_secret,
    };

    let app = Router::new()
        .route(
            "/payment-intents",
            get(list_payment_intents).post(create_payment_intent),
        )
        .route("/payment-intents/{id}", get(get_payment_intent))
        .route(
            "/payment-intents/{id}/receipt",
            get(get_payment_intent_receipt),
        )
        .route(
            "/provider/webhooks/{provider}",
            post(ingest_provider_webhook),
        )
        .with_state(state);

    let addr: SocketAddr = bind_addr.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("payment intent api listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
