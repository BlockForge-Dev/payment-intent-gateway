use serde::{ Deserialize, Serialize };

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureClassification {
    Validation,
    DuplicateRequest,
    RetryableInfrastructure,
    TerminalProvider,
    UnknownOutcome,
    CallbackDelivery,
    ReconciliationMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttemptOutcome {
    Succeeded,
    RetryableFailure {
        classification: FailureClassification,
        reason: String,
    },
    TerminalFailure {
        classification: FailureClassification,
        reason: String,
    },
    ProviderPending,
    UnknownOutcome {
        classification: FailureClassification,
        reason: String,
    },
}
