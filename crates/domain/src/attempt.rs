use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AttemptNumber, AttemptOutcome, ProviderReference};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionAttempt {
    pub attempt_no: AttemptNumber,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub outcome: Option<AttemptOutcome>,
    pub provider_reference: Option<ProviderReference>,
    pub note: Option<String>,
}

impl ExecutionAttempt {
    pub fn started(attempt_no: AttemptNumber, started_at: DateTime<Utc>) -> Self {
        Self {
            attempt_no,
            started_at,
            ended_at: None,
            outcome: None,
            provider_reference: None,
            note: None,
        }
    }

    pub fn finish(
        mut self,
        ended_at: DateTime<Utc>,
        outcome: AttemptOutcome,
        provider_reference: Option<ProviderReference>,
        note: Option<String>,
    ) -> Self {
        self.ended_at = Some(ended_at);
        self.outcome = Some(outcome);
        self.provider_reference = provider_reference;
        self.note = note;
        self
    }
}
