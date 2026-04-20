use application::{ CreatePaymentIntentCommand, CreatePaymentIntentResult };
use axum::{
    extract::{ Path, State },
    http::{ header::AUTHORIZATION, HeaderMap, StatusCode },
    Json,
};
use chrono::Utc;
use domain::{ FailureClassification, IntentState, PaymentIntent };
use serde::{ Deserialize, Serialize };
use uuid::Uuid;

use crate::{ app_state::AppState, error::ApiError };

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePaymentIntentRequest {
    pub merchant_reference: String,
    pub amount_minor: i64,
    pub currency: String,
    pub provider: String,
}

#[derive(Debug, Serialize)]
pub struct PaymentIntentResponse {
    pub intent_id: Uuid,
    pub merchant_reference: String,
    pub amount_minor: i64,
    pub currency: String,
    pub provider: String,
    pub state: String,
    pub provider_reference: Option<String>,
    pub latest_failure_classification: Option<String>,
    pub idempotency_status: String,
    pub receipt_url: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub async fn create_payment_intent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreatePaymentIntentRequest>
) -> Result<(StatusCode, Json<PaymentIntentResponse>), ApiError> {
    authenticate(&headers, &state.api_bearer_token)?;
    let idempotency_key = extract_idempotency_key(&headers)?;

    let command = CreatePaymentIntentCommand {
        merchant_reference: body.merchant_reference,
        amount_minor: body.amount_minor,
        currency: body.currency,
        provider: body.provider,
        idempotency_key,
        received_at: Utc::now(),
    };

    let result = state.service.create_intent(command).await?;

    match result {
        CreatePaymentIntentResult::Created(intent) =>
            Ok((StatusCode::CREATED, Json(to_response(intent, "created")))),
        CreatePaymentIntentResult::Existing(intent) =>
            Ok((StatusCode::OK, Json(to_response(intent, "existing")))),
    }
}

pub async fn get_payment_intent(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>
) -> Result<Json<PaymentIntentResponse>, ApiError> {
    authenticate(&headers, &state.api_bearer_token)?;
    let intent = state.service.get_intent(id).await?;
    Ok(Json(to_response(intent, "queried")))
}

fn to_response(intent: PaymentIntent, idempotency_status: &str) -> PaymentIntentResponse {
    PaymentIntentResponse {
        intent_id: intent.id,
        merchant_reference: intent.merchant_reference.0,
        amount_minor: intent.money.amount_minor,
        currency: intent.money.currency,
        provider: intent.provider.0,
        state: intent_state_to_api(intent.state).to_string(),
        provider_reference: intent.provider_reference.map(|p| p.0),
        latest_failure_classification: intent.latest_failure.map(|f| failure_to_api(&f).to_string()),
        idempotency_status: idempotency_status.to_string(),
        receipt_url: format!("/payment-intents/{}/receipt", intent.id),
        created_at: intent.created_at,
        updated_at: intent.updated_at,
    }
}

fn intent_state_to_api(state: IntentState) -> &'static str {
    match state {
        IntentState::Received => "received",
        IntentState::Validated => "validated",
        IntentState::Rejected => "rejected",
        IntentState::Queued => "queued",
        IntentState::Leased => "leased",
        IntentState::Executing => "executing",
        IntentState::ProviderPending => "provider_pending",
        IntentState::RetryScheduled => "retry_scheduled",
        IntentState::UnknownOutcome => "unknown_outcome",
        IntentState::Succeeded => "succeeded",
        IntentState::FailedTerminal => "failed_terminal",
        IntentState::Reconciling => "reconciling",
        IntentState::Reconciled => "reconciled",
        IntentState::ManualReview => "manual_review",
        IntentState::DeadLettered => "dead_lettered",
    }
}

fn failure_to_api(failure: &FailureClassification) -> &'static str {
    match failure {
        FailureClassification::Validation => "validation",
        FailureClassification::DuplicateRequest => "duplicate_request",
        FailureClassification::RetryableInfrastructure => "retryable_infrastructure",
        FailureClassification::TerminalProvider => "terminal_provider",
        FailureClassification::UnknownOutcome => "unknown_outcome",
        FailureClassification::CallbackDelivery => "callback_delivery",
        FailureClassification::ReconciliationMismatch => "reconciliation_mismatch",
    }
}

fn authenticate(headers: &HeaderMap, expected_token: &str) -> Result<(), ApiError> {
    let auth = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;

    let expected = format!("Bearer {}", expected_token);
    if auth != expected {
        return Err(ApiError::Unauthorized);
    }

    Ok(())
}

fn extract_idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let key = headers
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::BadRequest("Idempotency-Key header is required".to_string()))?;

    Ok(key.to_string())
}
