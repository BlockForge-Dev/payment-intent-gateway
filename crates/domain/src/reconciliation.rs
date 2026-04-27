use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{EvidenceSource, IntentState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconComparison {
    Match,
    Mismatch,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconDecision {
    ConfirmSucceeded,
    ConfirmFailedTerminal,
    KeepUnknown,
    EscalateManualReview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconResult {
    pub compared_at: DateTime<Utc>,
    pub internal_state: IntentState,
    pub provider_state: String,
    pub comparison: ReconComparison,
    pub decision: ReconDecision,
    pub evidence: EvidenceSource,
    pub note: Option<String>,
}
