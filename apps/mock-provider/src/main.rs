mod error;
mod models;
mod routes;
mod scheduler;
mod state;

use std::{env, net::SocketAddr};

use axum::{
    routing::{get, post},
    Router,
};
use routes::{
    create_mock_payment, delete_mock_payment, get_mock_payment_status,
    get_mock_payment_status_by_merchant_reference, health, list_mock_payments, list_scenarios,
    replay_webhook, reset_mock_provider,
};
use state::AppState;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let bind_addr =
        env::var("MOCK_PROVIDER_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3010".to_string());
    let webhook_secret = env::var("MOCK_PROVIDER_WEBHOOK_SECRET").ok();

    let state = AppState::new(webhook_secret);

    let app = Router::new()
        .route("/health", get(health))
        .route("/mock-provider/scenarios", get(list_scenarios))
        .route("/mock-provider/payments", post(create_mock_payment))
        .route("/mock-provider/admin/payments", get(list_mock_payments))
        .route("/mock-provider/admin/reset", post(reset_mock_provider))
        .route(
            "/mock-provider/payments/by-merchant-reference",
            get(get_mock_payment_status_by_merchant_reference),
        )
        .route(
            "/mock-provider/payments/{provider_reference}",
            get(get_mock_payment_status).delete(delete_mock_payment),
        )
        .route(
            "/mock-provider/payments/{provider_reference}/webhooks/replay",
            post(replay_webhook),
        )
        .with_state(state);

    let addr: SocketAddr = bind_addr.parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("mock provider listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
