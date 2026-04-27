//This is the lifecycle

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentState {
    Received,
    Validated,
    Rejected,
    Queued,
    Leased,
    Executing,
    ProviderPending,
    RetryScheduled,
    UnknownOutcome,
    Succeeded,
    FailedTerminal,
    Reconciling,
    Reconciled,
    ManualReview,
    DeadLettered,
}

impl IntentState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::FailedTerminal | Self::DeadLettered
        )
    }

    pub fn can_retry(self) -> bool {
        matches!(self, Self::RetryScheduled | Self::Queued)
    }
    pub fn can_begin_execution(self) -> bool {
        matches!(self, Self::Queued | Self::Leased | Self::RetryScheduled)
    }

    pub fn needs_reconciliation(self) -> bool {
        matches!(self, Self::UnknownOutcome | Self::ProviderPending)
    }

    pub fn can_begin_reconciliation(self) -> bool {
        matches!(
            self,
            Self::UnknownOutcome
                | Self::ProviderPending
                | Self::ManualReview
                | Self::Succeeded
                | Self::FailedTerminal
        )
    }
}
