use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("database error: {0}")] Sqlx(#[from] sqlx::Error),

    #[error("serialization error: {0}")] Serde(#[from] serde_json::Error),

    #[error("payment intent not found: {0}")] IntentNotFound(Uuid),

    #[error("idempotency conflict for scope={scope} key={key}")] IdempotencyConflict {
        scope: String,
        key: String,
    },

    #[error("invalid persisted state: {0}")] InvalidPersistedState(String),

    #[error("invalid persisted failure classification: {0}")] InvalidFailureClassification(String),

    #[error("invalid persisted attempt outcome: {0}")] InvalidAttemptOutcome(String),

    #[error("invalid persisted reconciliation comparison: {0}")] InvalidReconComparison(String),

    #[error("invalid persisted reconciliation decision: {0}")] InvalidReconDecision(String),

    #[error("invariant violation: {0}")] InvariantViolation(String),
}
