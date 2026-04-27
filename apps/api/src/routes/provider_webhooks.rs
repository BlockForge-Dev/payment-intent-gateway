use application::{
    IngestProviderWebhookCommand, ProviderWebhookIngestionSummary, ProviderWebhookStatus,
};
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{app_state::AppState, error::ApiError};

const MOCKPAY_SIGNATURE_HEADER: &str = "X-Mockpay-Signature";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MockpayWebhookPayload {
    provider_event_id: String,
    provider_reference: Option<String>,
    event_type: String,
    status: ProviderWebhookStatus,
    #[serde(rename = "scenario")]
    _scenario: String,
    merchant_reference: String,
    #[serde(rename = "amount_minor")]
    _amount_minor: i64,
    #[serde(rename = "currency")]
    _currency: String,
    #[serde(rename = "occurred_at")]
    _occurred_at: DateTime<Utc>,
}

pub async fn ingest_provider_webhook(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ProviderWebhookIngestionSummary>, ApiError> {
    let provider = provider.trim().to_lowercase();

    if provider == "mockpay" {
        verify_mockpay_signature(&state, &headers, body.as_ref())?;
        let raw_payload: serde_json::Value =
            serde_json::from_slice(body.as_ref()).map_err(|err| {
                ApiError::BadRequest(format!("invalid mockpay webhook payload: {err}"))
            })?;
        let payload: MockpayWebhookPayload =
            serde_json::from_slice(body.as_ref()).map_err(|err| {
                ApiError::BadRequest(format!("invalid mockpay webhook payload: {err}"))
            })?;

        let summary = state
            .webhook_service
            .ingest(IngestProviderWebhookCommand {
                provider_name: provider,
                provider_event_id: payload.provider_event_id,
                provider_reference: payload.provider_reference,
                merchant_reference: Some(payload.merchant_reference),
                event_type: payload.event_type,
                status: payload.status,
                raw_payload,
                received_at: Utc::now(),
            })
            .await?;

        return Ok(Json(summary));
    }

    Err(ApiError::BadRequest(format!(
        "unsupported provider webhook: {provider}"
    )))
}

fn verify_mockpay_signature(
    state: &AppState,
    headers: &HeaderMap,
    raw_body: &[u8],
) -> Result<(), ApiError> {
    let Some(secret) = state.mock_provider_webhook_secret.as_ref() else {
        return Ok(());
    };

    let provided_signature = headers
        .get(MOCKPAY_SIGNATURE_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;

    let expected_signature = compute_mockpay_signature(secret, raw_body);
    if provided_signature != expected_signature {
        return Err(ApiError::Unauthorized);
    }

    Ok(())
}

pub fn compute_mockpay_signature(secret: &str, raw_body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b".");
    hasher.update(raw_body);
    hex::encode(hasher.finalize())
}
