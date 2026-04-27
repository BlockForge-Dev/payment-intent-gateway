use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use domain::{FailureClassification, IntentId, IntentState, PaymentIntent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    DbCallbackNotificationRow, PersistenceError, PostgresPersistence, SaveCallbackDeliveryInput,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeasedCallbackNotification {
    pub notification_id: i64,
    pub event_key: String,
    pub intent_id: IntentId,
    pub destination_url: String,
    pub target_state: String,
    pub payload: Value,
    pub attempt_count: i32,
    pub lease_token: Uuid,
    pub worker_id: String,
    pub leased_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy)]
pub enum CallbackDeliveryDisposition {
    Delivered,
    RetryScheduled,
    DeadLettered,
}

#[derive(Debug, Clone)]
pub struct FinalizeCallbackDeliveryAttemptInput {
    pub notification_id: i64,
    pub lease_token: Uuid,
    pub finished_at: DateTime<Utc>,
    pub disposition: CallbackDeliveryDisposition,
    pub http_status_code: Option<i32>,
    pub response_body: Option<String>,
    pub error_message: Option<String>,
    pub retry_at: Option<DateTime<Utc>>,
}

impl PostgresPersistence {
    pub async fn lease_next_due_callback_notification(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_for: StdDuration,
    ) -> Result<Option<LeasedCallbackNotification>, PersistenceError> {
        if worker_id.trim().is_empty() {
            return Err(PersistenceError::EmptyWorkerId);
        }
        if lease_for.is_zero() {
            return Err(PersistenceError::InvalidLeaseDuration);
        }

        let lease_expires_at = now
            + Duration::from_std(lease_for).map_err(|_| PersistenceError::InvalidLeaseDuration)?;
        let lease_token = Uuid::new_v4();
        let mut tx = self.pool().begin().await?;

        let row = sqlx::query_as::<_, DbCallbackNotificationRow>(
            r#"
            WITH candidate AS (
                SELECT
                    id,
                    event_key,
                    intent_id,
                    destination_url,
                    target_state,
                    payload,
                    status,
                    next_attempt_at,
                    attempt_count,
                    last_attempt_at,
                    delivered_at,
                    last_http_status_code,
                    last_error,
                    lease_owner,
                    lease_token,
                    lease_expires_at,
                    created_at,
                    updated_at
                FROM callback_notifications
                WHERE
                    (
                        status IN ('scheduled', 'retry_scheduled')
                        AND next_attempt_at <= $1
                    )
                    OR
                    (
                        status = 'delivering'
                        AND lease_expires_at IS NOT NULL
                        AND lease_expires_at <= $1
                    )
                ORDER BY
                    CASE
                        WHEN status = 'delivering' THEN lease_expires_at
                        ELSE next_attempt_at
                    END ASC,
                    created_at ASC
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            UPDATE callback_notifications c
            SET
                status = 'delivering',
                lease_owner = $2,
                lease_token = $3,
                lease_expires_at = $4,
                updated_at = $1
            FROM candidate
            WHERE c.id = candidate.id
            RETURNING
                c.id,
                c.event_key,
                c.intent_id,
                c.destination_url,
                c.target_state,
                c.payload,
                c.status,
                c.next_attempt_at,
                c.attempt_count,
                c.last_attempt_at,
                c.delivered_at,
                c.last_http_status_code,
                c.last_error,
                c.lease_owner,
                c.lease_token,
                c.lease_expires_at,
                c.created_at,
                c.updated_at
            "#,
        )
        .bind(now)
        .bind(worker_id)
        .bind(lease_token)
        .bind(lease_expires_at)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };

        insert_audit_event_tx(
            &mut tx,
            Some(row.intent_id),
            "callback_notification_leased",
            json!({
                "notification_id": row.id,
                "event_key": row.event_key,
                "destination_url": row.destination_url,
                "target_state": row.target_state,
                "worker_id": worker_id,
                "lease_token": lease_token,
                "lease_expires_at": lease_expires_at,
            }),
            now,
        )
        .await?;

        tx.commit().await?;

        Ok(Some(LeasedCallbackNotification {
            notification_id: row.id,
            event_key: row.event_key,
            intent_id: row.intent_id,
            destination_url: row.destination_url,
            target_state: row.target_state,
            payload: row.payload.0,
            attempt_count: row.attempt_count,
            lease_token: row.lease_token.unwrap_or(lease_token),
            worker_id: row.lease_owner.unwrap_or_else(|| worker_id.to_string()),
            leased_at: now,
            lease_expires_at: row.lease_expires_at.unwrap_or(lease_expires_at),
        }))
    }

    pub async fn finalize_callback_delivery_attempt(
        &self,
        input: FinalizeCallbackDeliveryAttemptInput,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool().begin().await?;

        let notification = sqlx::query_as::<_, DbCallbackNotificationRow>(
            r#"
            SELECT
                id,
                event_key,
                intent_id,
                destination_url,
                target_state,
                payload,
                status,
                next_attempt_at,
                attempt_count,
                last_attempt_at,
                delivered_at,
                last_http_status_code,
                last_error,
                lease_owner,
                lease_token,
                lease_expires_at,
                created_at,
                updated_at
            FROM callback_notifications
            WHERE
                id = $1
                AND status = 'delivering'
                AND lease_token = $2
            FOR UPDATE
            "#,
        )
        .bind(input.notification_id)
        .bind(input.lease_token)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(notification) = notification else {
            return Err(PersistenceError::InvariantViolation(format!(
                "callback notification {} is not currently leased with the provided token",
                input.notification_id
            )));
        };

        let next_attempt_no = notification.attempt_count + 1;
        let retry_count = notification.attempt_count;
        let delivery_result = disposition_to_delivery_result(input.disposition);

        let save_delivery = SaveCallbackDeliveryInput {
            intent_id: notification.intent_id,
            destination_url: notification.destination_url.clone(),
            attempt_no: next_attempt_no,
            payload: notification.payload.0.clone(),
            http_status_code: input.http_status_code,
            delivery_result: delivery_result.to_string(),
            started_at: notification.updated_at,
            ended_at: Some(input.finished_at),
            retry_count,
            response_body: input.response_body.clone(),
        };

        insert_callback_delivery_tx(&mut tx, save_delivery).await?;

        let (status, next_attempt_at, delivered_at, audit_event_type) = match input.disposition {
            CallbackDeliveryDisposition::Delivered => (
                "delivered",
                None,
                Some(input.finished_at),
                "callback_delivery_succeeded",
            ),
            CallbackDeliveryDisposition::RetryScheduled => (
                "retry_scheduled",
                input.retry_at,
                None,
                "callback_delivery_retry_scheduled",
            ),
            CallbackDeliveryDisposition::DeadLettered => (
                "dead_lettered",
                None,
                None,
                "callback_delivery_dead_lettered",
            ),
        };

        sqlx::query(
            r#"
            UPDATE callback_notifications
            SET
                status = $2,
                next_attempt_at = COALESCE($3, next_attempt_at),
                attempt_count = $4,
                last_attempt_at = $5,
                delivered_at = $6,
                last_http_status_code = $7,
                last_error = $8,
                lease_owner = NULL,
                lease_token = NULL,
                lease_expires_at = NULL,
                updated_at = $5
            WHERE id = $1
            "#,
        )
        .bind(notification.id)
        .bind(status)
        .bind(next_attempt_at)
        .bind(next_attempt_no)
        .bind(input.finished_at)
        .bind(delivered_at)
        .bind(input.http_status_code)
        .bind(input.error_message.as_deref())
        .execute(&mut *tx)
        .await?;

        insert_audit_event_tx(
            &mut tx,
            Some(notification.intent_id),
            audit_event_type,
            json!({
                "notification_id": notification.id,
                "event_key": notification.event_key,
                "destination_url": notification.destination_url,
                "target_state": notification.target_state,
                "attempt_no": next_attempt_no,
                "delivery_result": delivery_result,
                "http_status_code": input.http_status_code,
                "retry_at": next_attempt_at,
                "error_message": input.error_message,
            }),
            input.finished_at,
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }
}

pub(crate) async fn schedule_callback_notification_tx(
    tx: &mut Transaction<'_, Postgres>,
    intent: &PaymentIntent,
    scheduled_at: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    let Some(destination_url) = intent.callback_url.as_deref() else {
        return Ok(());
    };

    if !is_callback_worthy_state(intent.state) {
        return Ok(());
    }

    let transition = intent.timeline.last().ok_or_else(|| {
        PersistenceError::InvariantViolation(
            "payment intent timeline is empty while scheduling callback".to_string(),
        )
    })?;

    let event_key = format!(
        "{}:{}:{}",
        intent.id,
        state_to_db(intent.state),
        transition.at.to_rfc3339()
    );

    let payload = build_callback_payload(intent, transition.at);

    let rows_affected = sqlx::query(
        r#"
        INSERT INTO callback_notifications (
            event_key,
            intent_id,
            destination_url,
            target_state,
            payload,
            status,
            next_attempt_at,
            attempt_count,
            created_at,
            updated_at
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        ON CONFLICT (event_key) DO NOTHING
        "#,
    )
    .bind(event_key.as_str())
    .bind(intent.id)
    .bind(destination_url)
    .bind(state_to_db(intent.state))
    .bind(sqlx::types::Json(payload.clone()))
    .bind("scheduled")
    .bind(scheduled_at)
    .bind(0_i32)
    .bind(scheduled_at)
    .bind(scheduled_at)
    .execute(&mut **tx)
    .await?
    .rows_affected();

    if rows_affected == 0 {
        return Ok(());
    }

    insert_audit_event_tx(
        tx,
        Some(intent.id),
        "callback_notification_scheduled",
        json!({
            "event_key": event_key,
            "destination_url": destination_url,
            "target_state": state_to_db(intent.state),
            "scheduled_at": scheduled_at,
        }),
        scheduled_at,
    )
    .await?;

    Ok(())
}

fn build_callback_payload(intent: &PaymentIntent, occurred_at: DateTime<Utc>) -> Value {
    json!({
        "intent_id": intent.id,
        "merchant_reference": intent.merchant_reference.0,
        "amount_minor": intent.money.amount_minor,
        "currency": intent.money.currency,
        "provider": intent.provider.0,
        "state": state_to_db(intent.state),
        "latest_failure_classification": intent.latest_failure.as_ref().map(failure_to_db),
        "provider_reference": intent.provider_reference.as_ref().map(|reference| reference.0.clone()),
        "callback_url": intent.callback_url,
        "occurred_at": occurred_at,
        "receipt_url": format!("/payment-intents/{}/receipt", intent.id),
    })
}

fn is_callback_worthy_state(state: IntentState) -> bool {
    matches!(
        state,
        IntentState::ProviderPending
            | IntentState::UnknownOutcome
            | IntentState::Succeeded
            | IntentState::FailedTerminal
            | IntentState::Reconciled
            | IntentState::ManualReview
            | IntentState::DeadLettered
    )
}

fn disposition_to_delivery_result(disposition: CallbackDeliveryDisposition) -> &'static str {
    match disposition {
        CallbackDeliveryDisposition::Delivered => "delivered",
        CallbackDeliveryDisposition::RetryScheduled => "retry_scheduled",
        CallbackDeliveryDisposition::DeadLettered => "dead_lettered",
    }
}

async fn insert_callback_delivery_tx(
    tx: &mut Transaction<'_, Postgres>,
    input: SaveCallbackDeliveryInput,
) -> Result<(), PersistenceError> {
    sqlx::query(
        r#"
        INSERT INTO callback_deliveries (
            intent_id,
            destination_url,
            attempt_no,
            payload,
            http_status_code,
            delivery_result,
            started_at,
            ended_at,
            retry_count,
            response_body
        )
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
        "#,
    )
    .bind(input.intent_id)
    .bind(input.destination_url.as_str())
    .bind(input.attempt_no)
    .bind(sqlx::types::Json(input.payload))
    .bind(input.http_status_code)
    .bind(input.delivery_result.as_str())
    .bind(input.started_at)
    .bind(input.ended_at)
    .bind(input.retry_count)
    .bind(input.response_body.as_deref())
    .execute(&mut **tx)
    .await?;

    insert_audit_event_tx(
        tx,
        Some(input.intent_id),
        "callback_delivery_recorded",
        json!({
            "destination_url": input.destination_url,
            "attempt_no": input.attempt_no,
            "delivery_result": input.delivery_result,
            "http_status_code": input.http_status_code,
            "retry_count": input.retry_count
        }),
        input.started_at,
    )
    .await?;

    Ok(())
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
