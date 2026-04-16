use chrono::{ DateTime, Utc };
use domain::IntentId;
use serde_json::Value;
use sqlx::types::Json;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct DbPaymentIntentRow {
    pub id: IntentId,
    pub merchant_reference: String,
    pub amount_minor: i64,
    pub currency: String,
    pub provider: String,
    pub state: String,
    pub latest_failure_classification: Option<String>,
    pub provider_reference: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DbIdempotencyKeyRow {
    pub scope: String,
    pub idempotency_key: String,
    pub intent_id: IntentId,
    pub request_fingerprint: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DbExecutionAttemptRow {
    pub intent_id: IntentId,
    pub attempt_no: i32,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub request_payload_snapshot: Json<Value>,
    pub outcome_kind: Option<String>,
    pub raw_provider_response_summary: Option<Json<Value>>,
    pub error_category: Option<String>,
    pub result_reason: Option<String>,
    pub provider_reference: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DbProviderEventRow {
    pub provider_name: String,
    pub provider_event_id: String,
    pub intent_id: Option<IntentId>,
    pub provider_reference: Option<String>,
    pub event_type: String,
    pub raw_payload: Json<Value>,
    pub dedup_hash: String,
    pub received_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DbCallbackDeliveryRow {
    pub intent_id: IntentId,
    pub destination_url: String,
    pub attempt_no: i32,
    pub payload: Json<Value>,
    pub http_status_code: Option<i32>,
    pub delivery_result: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub retry_count: i32,
    pub response_body: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DbAuditEventRow {
    pub intent_id: Option<IntentId>,
    pub event_type: String,
    pub payload: Json<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DbReconciliationRunRow {
    pub intent_id: IntentId,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub provider_status_seen: String,
    pub internal_status_seen: String,
    pub comparison_result: String,
    pub decision: String,
    pub evidence: Json<Value>,
    pub notes: Option<String>,
}
