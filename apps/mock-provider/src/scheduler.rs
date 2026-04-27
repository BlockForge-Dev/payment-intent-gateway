use std::time::Duration;

use chrono::Utc;
use reqwest::header::HeaderMap;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::time::sleep;
use tracing::{info, warn};

use crate::{
    models::{MockProviderStatus, SimulatedPayment},
    state::AppState,
};

pub fn spawn_resolution_task(state: AppState, provider_reference: String, delay_ms: u64) {
    tokio::spawn(async move {
        sleep(Duration::from_millis(delay_ms)).await;

        let maybe_dispatch = {
            let mut store = state.store.write().await;
            let Some(payment) = store.get_mut(&provider_reference) else {
                return;
            };

            payment.apply_time_progress(Utc::now());

            let callback_url = payment.callback_url.clone();
            let duplicate_count = if payment.webhook_on_resolution {
                payment.webhook_duplicate_count
            } else {
                0
            };
            let payload = webhook_payload(payment);

            Some((callback_url, duplicate_count, payload))
        };

        if let Some((Some(callback_url), duplicate_count, payload)) = maybe_dispatch {
            for idx in 0..duplicate_count {
                let response = state
                    .http_client
                    .post(callback_url.clone())
                    .headers(webhook_signature_headers(&state, &payload))
                    .json(&payload)
                    .send()
                    .await;

                match response {
                    Ok(resp) => {
                        info!(
                            provider_reference = %provider_reference,
                            webhook_attempt = idx + 1,
                            status = %resp.status(),
                            "mock provider webhook sent"
                        );
                    }
                    Err(err) => {
                        warn!(
                            provider_reference = %provider_reference,
                            webhook_attempt = idx + 1,
                            error = %err,
                            "mock provider webhook failed"
                        );
                    }
                }

                if idx + 1 < duplicate_count {
                    sleep(Duration::from_millis(150)).await;
                }
            }
        }
    });
}

pub fn spawn_webhook_dispatch(
    state: AppState,
    provider_reference: String,
    delay_ms: u64,
    duplicate_count: usize,
) {
    tokio::spawn(async move {
        sleep(Duration::from_millis(delay_ms)).await;

        let maybe_payload = {
            let store = state.store.read().await;
            let Some(payment) = store.get(&provider_reference) else {
                return;
            };

            let callback_url = payment.callback_url.clone();
            let payload = webhook_payload(payment);
            Some((callback_url, payload))
        };

        if let Some((Some(callback_url), payload)) = maybe_payload {
            for idx in 0..duplicate_count {
                let response = state
                    .http_client
                    .post(callback_url.clone())
                    .headers(webhook_signature_headers(&state, &payload))
                    .json(&payload)
                    .send()
                    .await;

                match response {
                    Ok(resp) => {
                        info!(
                            provider_reference = %provider_reference,
                            webhook_attempt = idx + 1,
                            status = %resp.status(),
                            "mock provider webhook sent"
                        );
                    }
                    Err(err) => {
                        warn!(
                            provider_reference = %provider_reference,
                            webhook_attempt = idx + 1,
                            error = %err,
                            "mock provider webhook failed"
                        );
                    }
                }

                if idx + 1 < duplicate_count {
                    sleep(Duration::from_millis(150)).await;
                }
            }
        }
    });
}

pub async fn dispatch_single_webhook_now(state: AppState, provider_reference: &str) -> bool {
    let maybe_payload = {
        let store = state.store.read().await;
        let Some(payment) = store.get(provider_reference) else {
            return false;
        };

        let callback_url = payment.callback_url.clone();
        let payload = webhook_payload(payment);
        Some((callback_url, payload))
    };

    let Some((Some(callback_url), payload)) = maybe_payload else {
        return false;
    };

    state
        .http_client
        .post(callback_url)
        .headers(webhook_signature_headers(&state, &payload))
        .json(&payload)
        .send()
        .await
        .is_ok()
}

fn webhook_payload(payment: &SimulatedPayment) -> serde_json::Value {
    json!({
        "provider_event_id": webhook_event_id(payment),
        "provider_reference": payment.provider_reference,
        "event_type": "payment.updated",
        "status": payment.internal_status,
        "scenario": payment.scenario,
        "merchant_reference": payment.merchant_reference,
        "amount_minor": payment.amount_minor,
        "currency": payment.currency,
        "occurred_at": Utc::now(),
    })
}

fn webhook_event_id(payment: &SimulatedPayment) -> String {
    format!(
        "evt_{}_{}",
        payment.provider_reference,
        mock_provider_status_to_api(&payment.internal_status)
    )
}

fn mock_provider_status_to_api(status: &MockProviderStatus) -> &'static str {
    match status {
        MockProviderStatus::Pending => "pending",
        MockProviderStatus::Succeeded => "succeeded",
        MockProviderStatus::FailedTerminal => "failed_terminal",
    }
}

fn webhook_signature_headers(state: &AppState, payload: &serde_json::Value) -> HeaderMap {
    let mut headers = HeaderMap::new();

    let Some(secret) = state.webhook_secret.as_ref() else {
        return headers;
    };

    if let Ok(raw_body) = serde_json::to_vec(payload) {
        if let Ok(signature) = compute_mockpay_signature(secret, &raw_body).parse() {
            headers.insert("X-Mockpay-Signature", signature);
        }
    }

    headers
}

fn compute_mockpay_signature(secret: &str, raw_body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b".");
    hasher.update(raw_body);
    hex::encode(hasher.finalize())
}
