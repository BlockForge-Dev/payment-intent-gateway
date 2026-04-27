use chrono::{DateTime, Utc};
use domain::{FailureClassification, IntentId, IntentState, PaymentIntent};
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};

use crate::callback_notifications::schedule_callback_notification_tx;
use crate::{DbPaymentIntentRow, PersistenceError, PostgresPersistence};

impl PostgresPersistence {
    pub async fn list_due_resolution_candidates(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<PaymentIntent>, PersistenceError> {
        let rows: Vec<DbPaymentIntentRow> = sqlx::query_as::<_, DbPaymentIntentRow>(
            r#"
            SELECT
                id,
                merchant_reference,
                amount_minor,
                currency,
                provider,
                callback_url,
                state,
                latest_failure_classification,
                provider_reference,
                next_resolution_at,
                last_resolution_at,
                resolution_attempt_count,
                created_at,
                updated_at
            FROM payment_intents
            WHERE
                state IN ('unknown_outcome', 'provider_pending')
                AND next_resolution_at IS NOT NULL
                AND next_resolution_at <= $1
            ORDER BY next_resolution_at ASC, created_at ASC
            LIMIT $2
            "#,
        )
        .bind(now)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;

        let mut intents = Vec::with_capacity(rows.len());
        for row in rows {
            intents.push(self.get_intent_by_id(row.id).await?);
        }

        Ok(intents)
    }

    pub async fn save_status_check_update(
        &self,
        intent: &PaymentIntent,
        observed_status: Option<&str>,
        raw_summary: Option<Value>,
        note: &str,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool().begin().await?;

        let persisted_state: Option<String> = sqlx::query_scalar(
            r#"
            SELECT state
            FROM payment_intents
            WHERE id = $1
            "#,
        )
        .bind(intent.id)
        .fetch_optional(&mut *tx)
        .await?;

        let persisted_state = persisted_state.ok_or(PersistenceError::IntentNotFound(intent.id))?;

        sqlx::query(
            r#"
            UPDATE payment_intents
            SET
                state = $2,
                latest_failure_classification = $3,
                provider_reference = $4,
                next_resolution_at = $5,
                last_resolution_at = $6,
                resolution_attempt_count = $7,
                updated_at = $8
            WHERE id = $1
            "#,
        )
        .bind(intent.id)
        .bind(state_to_db(intent.state))
        .bind(intent.latest_failure.as_ref().map(failure_to_db))
        .bind(intent.provider_reference.as_ref().map(|p| p.0.as_str()))
        .bind(intent.next_resolution_at)
        .bind(intent.last_resolution_at)
        .bind(intent.resolution_attempt_count as i32)
        .bind(intent.updated_at)
        .execute(&mut *tx)
        .await?;

        if persisted_state != state_to_db(intent.state) {
            insert_audit_event_tx(
                &mut tx,
                Some(intent.id),
                "state_transition",
                json!({
                    "state": state_to_db(intent.state),
                    "note": note,
                }),
                intent.updated_at,
            )
            .await?;

            schedule_callback_notification_tx(&mut tx, intent, intent.updated_at).await?;
        }

        insert_audit_event_tx(
            &mut tx,
            Some(intent.id),
            "status_check_observed",
            json!({
                "observed_status": observed_status,
                "note": note,
                "next_resolution_at": intent.next_resolution_at,
                "last_resolution_at": intent.last_resolution_at,
                "resolution_attempt_count": intent.resolution_attempt_count,
                "provider_reference": intent.provider_reference.as_ref().map(|p| p.0.clone()),
                "raw_summary": raw_summary,
            }),
            intent.updated_at,
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }
}

async fn insert_audit_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    intent_id: Option<IntentId>,
    event_type: &str,
    payload: Value,
    created_at: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO audit_events (intent_id, event_type, payload, created_at)
        VALUES ($1,$2,$3,$4)
        "#,
    )
    .bind(intent_id)
    .bind(event_type)
    .bind(sqlx::types::Json(payload))
    .bind(created_at)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

fn state_to_db(state: IntentState) -> &'static str {
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

fn failure_to_db(failure: &FailureClassification) -> &'static str {
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
