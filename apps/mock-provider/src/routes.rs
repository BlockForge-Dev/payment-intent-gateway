use std::time::Duration;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use tokio::time::sleep;

use crate::{
    error::ApiError,
    models::{
        CreateMockPaymentRequest, CreateMockPaymentResponse, DeleteMockPaymentResponse,
        GetMockPaymentStatusResponse, HealthResponse, MockPaymentAdminRecord, MockScenario,
        ResetMockProviderResponse, WebhookReplayResponse,
    },
    scheduler::{dispatch_single_webhook_now, spawn_resolution_task, spawn_webhook_dispatch},
    state::AppState,
};

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

pub async fn list_scenarios() -> Json<Vec<&'static str>> {
    Json(vec![
        "immediate_success",
        "terminal_failure",
        "retryable_infra_error",
        "timeout_after_acceptance",
        "delayed_confirmation",
        "duplicate_webhook",
        "inconsistent_status_check_response",
        "pending_then_resolves",
    ])
}

pub async fn create_mock_payment(
    State(state): State<AppState>,
    Json(body): Json<CreateMockPaymentRequest>,
) -> Result<(StatusCode, Json<CreateMockPaymentResponse>), ApiError> {
    validate_create_request(&body)?;

    if body.scenario == MockScenario::RetryableInfraError {
        return Err(ApiError::ServiceUnavailable(
            "simulated retryable infrastructure error".to_string(),
        ));
    }

    let now = Utc::now();
    let payment = crate::models::SimulatedPayment::new(&body, now);
    let provider_reference = payment.provider_reference.clone();

    {
        let mut store = state.store.write().await;
        store.insert(provider_reference.clone(), payment.clone());
    }

    match body.scenario {
        MockScenario::ImmediateSuccess => {
            if payment.callback_url.is_some() {
                spawn_webhook_dispatch(state.clone(), provider_reference, 0, 1);
            }

            Ok((
                StatusCode::OK,
                Json(CreateMockPaymentResponse::from_payment(&payment, true)),
            ))
        }

        MockScenario::TerminalFailure => Ok((
            StatusCode::OK,
            Json(CreateMockPaymentResponse::from_payment(&payment, true)),
        )),

        MockScenario::TimeoutAfterAcceptance => {
            if payment.callback_url.is_some() {
                spawn_webhook_dispatch(
                    state.clone(),
                    provider_reference,
                    body.resolution_delay_ms.unwrap_or(500),
                    1,
                );
            }

            sleep(Duration::from_millis(
                body.timeout_response_delay_ms.unwrap_or(15_000),
            ))
            .await;

            Ok((
                StatusCode::OK,
                Json(CreateMockPaymentResponse::from_payment(&payment, true)),
            ))
        }

        MockScenario::DelayedConfirmation => {
            spawn_resolution_task(
                state.clone(),
                provider_reference,
                body.resolution_delay_ms.unwrap_or(5_000),
            );

            Ok((
                StatusCode::ACCEPTED,
                Json(CreateMockPaymentResponse::from_payment(&payment, true)),
            ))
        }

        MockScenario::DuplicateWebhook => {
            spawn_resolution_task(
                state.clone(),
                provider_reference,
                body.resolution_delay_ms.unwrap_or(5_000),
            );

            Ok((
                StatusCode::ACCEPTED,
                Json(CreateMockPaymentResponse::from_payment(&payment, true)),
            ))
        }

        MockScenario::InconsistentStatusCheckResponse => Ok((
            StatusCode::ACCEPTED,
            Json(CreateMockPaymentResponse::from_payment(&payment, true)),
        )),

        MockScenario::PendingThenResolves => {
            spawn_resolution_task(
                state.clone(),
                provider_reference,
                body.resolution_delay_ms.unwrap_or(5_000),
            );

            Ok((
                StatusCode::ACCEPTED,
                Json(CreateMockPaymentResponse::from_payment(&payment, true)),
            ))
        }

        MockScenario::RetryableInfraError => unreachable!(),
    }
}

pub async fn get_mock_payment_status(
    State(state): State<AppState>,
    Path(provider_reference): Path<String>,
) -> Result<Json<GetMockPaymentStatusResponse>, ApiError> {
    let response = {
        let mut store = state.store.write().await;
        let payment = store.get_mut(&provider_reference).ok_or_else(|| {
            ApiError::NotFound(format!("mock payment not found: {provider_reference}"))
        })?;

        let visible_status = payment.visible_status_for_read(Utc::now());
        GetMockPaymentStatusResponse::from_payment(payment, visible_status)
    };

    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
pub struct MerchantReferenceQuery {
    pub merchant_reference: String,
}

pub async fn get_mock_payment_status_by_merchant_reference(
    State(state): State<AppState>,
    Query(query): Query<MerchantReferenceQuery>,
) -> Result<Json<GetMockPaymentStatusResponse>, ApiError> {
    let response = {
        let mut store = state.store.write().await;
        let payment = store
            .values_mut()
            .find(|payment| payment.merchant_reference == query.merchant_reference)
            .ok_or_else(|| {
                ApiError::NotFound(format!(
                    "mock payment not found for merchant_reference: {}",
                    query.merchant_reference
                ))
            })?;

        let visible_status = payment.visible_status_for_read(Utc::now());
        GetMockPaymentStatusResponse::from_payment(payment, visible_status)
    };

    Ok(Json(response))
}

pub async fn replay_webhook(
    State(state): State<AppState>,
    Path(provider_reference): Path<String>,
) -> Result<Json<WebhookReplayResponse>, ApiError> {
    let exists = {
        let store = state.store.read().await;
        store.contains_key(&provider_reference)
    };

    if !exists {
        return Err(ApiError::NotFound(format!(
            "mock payment not found: {provider_reference}"
        )));
    }

    let replayed = dispatch_single_webhook_now(state.clone(), &provider_reference).await;

    Ok(Json(WebhookReplayResponse {
        provider_reference,
        replayed,
        note: if replayed {
            "webhook replay attempted".to_string()
        } else {
            "no callback_url configured; nothing replayed".to_string()
        },
    }))
}

pub async fn list_mock_payments(
    State(state): State<AppState>,
) -> Json<Vec<MockPaymentAdminRecord>> {
    let store = state.store.read().await;
    let mut records = store
        .values()
        .map(MockPaymentAdminRecord::from_payment)
        .collect::<Vec<_>>();
    records.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Json(records)
}

pub async fn delete_mock_payment(
    State(state): State<AppState>,
    Path(provider_reference): Path<String>,
) -> Json<DeleteMockPaymentResponse> {
    let deleted = state
        .store
        .write()
        .await
        .remove(&provider_reference)
        .is_some();

    Json(DeleteMockPaymentResponse {
        provider_reference,
        deleted,
        note: if deleted {
            "mock payment removed from provider store".to_string()
        } else {
            "mock payment not found".to_string()
        },
    })
}

pub async fn reset_mock_provider(State(state): State<AppState>) -> Json<ResetMockProviderResponse> {
    let removed_payments = {
        let mut store = state.store.write().await;
        let count = store.len();
        store.clear();
        count
    };

    Json(ResetMockProviderResponse { removed_payments })
}

fn validate_create_request(body: &CreateMockPaymentRequest) -> Result<(), ApiError> {
    if body.merchant_reference.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "merchant_reference is required".to_string(),
        ));
    }

    if body.amount_minor <= 0 {
        return Err(ApiError::BadRequest(
            "amount_minor must be greater than zero".to_string(),
        ));
    }

    if body.currency.trim().is_empty() {
        return Err(ApiError::BadRequest("currency is required".to_string()));
    }

    Ok(())
}
