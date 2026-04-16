//This is very important. Invalid transitions should fail loudly.

use crate::state::IntentState;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("Invalid state transition from {from:?} to {to:?}")] InvalidStateTransition {
        from: IntentState,
        to: IntentState,
    },

    #[error("terminal state cannot be retried: {0:?}")] TerminalStateNotRetryable(IntentState),

    #[error("attempt number must increase monotonically")]
    InvalidAttemptNumber,

    #[error("evidence is required to resolve unknown outcome")]
    EvidenceRequiredForUnknownResolution,

    #[error("callback delivery failure must not alter execution truth")]
    CallbackFailureCannotMutateExecutionTruth,

    #[error("merchant reference cannot be empty")]
    EmptyMerchantReference,

    #[error("idempotency key cannot be empty")]
    EmptyIdempotencyKey,
    #[error("provider  cannot be empty")]
    EmptyProvider,

    #[error("currency cannot be empty")]
    EmptyCurrency,

    #[error("amount must be greater than zero")]
    InvalidAmount,
}
