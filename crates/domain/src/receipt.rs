use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    ExecutionAttempt, FailureClassification, IdempotencyKey, IntentId, IntentState,
    MerchantReference, Money, ProviderName, ProviderReference, ReconResult,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptTimelineEntry {
    pub state: IntentState,
    pub at: DateTime<Utc>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentReceipt {
    pub intent_id: IntentId,
    pub merchant_reference: MerchantReference,
    pub idempotency_key: IdempotencyKey,
    pub money: Money,
    pub provider: ProviderName,
    pub callback_url: Option<String>,
    pub provider_reference: Option<ProviderReference>,
    pub current_state: IntentState,
    pub latest_failure: Option<FailureClassification>,
    pub timeline: Vec<ReceiptTimelineEntry>,
    pub attempts: Vec<ExecutionAttempt>,
    pub reconciliation: Option<ReconResult>,
    pub next_resolution_at: Option<DateTime<Utc>>,
    pub last_resolution_at: Option<DateTime<Utc>>,
    pub resolution_attempt_count: u32,
}
