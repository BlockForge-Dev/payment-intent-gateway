use chrono::{DateTime, Utc};
use domain::{
    AttemptOutcome, EvidenceSource, ExecutionAttempt, FailureClassification, IdempotencyKey,
    IntentId, IntentState, MerchantReference, Money, PaymentIntent, PaymentReceipt, ProviderName,
    ProviderReference, ReceiptTimelineEntry, ReconComparison, ReconDecision, ReconResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::types::Json;
use sqlx::{PgPool, Postgres, Transaction};

use crate::callback_notifications::schedule_callback_notification_tx;
use crate::error::PersistenceError;
use crate::rows::{
    DbAuditEventRow, DbCallbackDeliveryRow, DbCallbackNotificationRow, DbExecutionAttemptRow,
    DbIdempotencyKeyRow, DbPaymentIntentRow, DbProviderEventRow, DbReconciliationRunRow,
};

#[derive(Debug, Clone)]
pub struct PostgresPersistence {
    pool: PgPool,
}

#[derive(Debug, Clone)]
pub enum CreateIntentResult {
    Created(PaymentIntent),
    Existing(PaymentIntent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveProviderEventInput {
    pub provider_name: String,
    pub provider_event_id: String,
    pub intent_id: Option<IntentId>,
    pub provider_reference: Option<String>,
    pub event_type: String,
    pub raw_payload: Value,
    pub dedup_hash: String,
    pub received_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveCallbackDeliveryInput {
    pub intent_id: IntentId,
    pub destination_url: String,
    pub attempt_no: i32,
    pub payload: Value,
    pub http_status_code: Option<i32>,
    pub delivery_result: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub retry_count: i32,
    pub response_body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveReconciliationRunInput {
    pub intent_id: IntentId,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub provider_status_seen: String,
    pub raw_provider_summary: Option<Value>,
    pub internal_status_seen: IntentState,
    pub comparison: ReconComparison,
    pub decision: ReconDecision,
    pub evidence: EvidenceSource,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredProviderEvent {
    pub provider_name: String,
    pub provider_event_id: String,
    pub intent_id: Option<IntentId>,
    pub provider_reference: Option<String>,
    pub event_type: String,
    pub raw_payload: Value,
    pub dedup_hash: String,
    pub received_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCallbackDelivery {
    pub intent_id: IntentId,
    pub destination_url: String,
    pub attempt_no: i32,
    pub payload: Value,
    pub http_status_code: Option<i32>,
    pub delivery_result: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub retry_count: i32,
    pub response_body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCallbackNotification {
    pub event_key: String,
    pub intent_id: IntentId,
    pub destination_url: String,
    pub target_state: String,
    pub payload: Value,
    pub status: String,
    pub next_attempt_at: DateTime<Utc>,
    pub attempt_count: i32,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub last_http_status_code: Option<i32>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredReconciliationRun {
    pub intent_id: IntentId,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub provider_status_seen: String,
    pub internal_status_seen: IntentState,
    pub comparison: ReconComparison,
    pub decision: ReconDecision,
    pub evidence: EvidenceSource,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAuditEvent {
    pub intent_id: Option<IntentId>,
    pub event_type: String,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedReceipt {
    pub core: PaymentReceipt,
    pub provider_events: Vec<StoredProviderEvent>,
    pub callback_notifications: Vec<StoredCallbackNotification>,
    pub callback_deliveries: Vec<StoredCallbackDelivery>,
    pub reconciliation_runs: Vec<StoredReconciliationRun>,
    pub audit_events: Vec<StoredAuditEvent>,
}

impl PostgresPersistence {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn create_intent_with_idempotency(
        &self,
        intent: &PaymentIntent,
        scope: &str,
        request_fingerprint: &str,
    ) -> Result<CreateIntentResult, PersistenceError> {
        let mut tx = self.pool.begin().await?;

        let existing = sqlx::query_as::<_, DbIdempotencyKeyRow>(
            r#"
            SELECT scope, idempotency_key, intent_id, request_fingerprint, created_at
            FROM idempotency_keys
            WHERE scope = $1 AND idempotency_key = $2
            "#,
        )
        .bind(scope)
        .bind(intent.idempotency_key.0.as_str())
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(existing) = existing {
            if existing.request_fingerprint != request_fingerprint {
                return Err(PersistenceError::IdempotencyConflict {
                    scope: scope.to_string(),
                    key: existing.idempotency_key,
                });
            }

            let existing_intent = self
                .load_intent_by_id_tx(&mut tx, existing.intent_id)
                .await?;
            return Ok(CreateIntentResult::Existing(existing_intent));
        }

        sqlx::query(
            r#"
            INSERT INTO payment_intents (
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
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
            "#,
        )
        .bind(intent.id)
        .bind(intent.merchant_reference.0.as_str())
        .bind(intent.money.amount_minor)
        .bind(intent.money.currency.as_str())
        .bind(intent.provider.0.as_str())
        .bind(intent.callback_url.as_deref())
        .bind(state_to_db(intent.state))
        .bind(intent.latest_failure.as_ref().map(failure_to_db))
        .bind(intent.provider_reference.as_ref().map(|p| p.0.as_str()))
        .bind(intent.next_resolution_at)
        .bind(intent.last_resolution_at)
        .bind(intent.resolution_attempt_count as i32)
        .bind(intent.created_at)
        .bind(intent.updated_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO idempotency_keys (
                scope,
                idempotency_key,
                intent_id,
                request_fingerprint,
                created_at
            )
            VALUES ($1,$2,$3,$4,$5)
            "#,
        )
        .bind(scope)
        .bind(intent.idempotency_key.0.as_str())
        .bind(intent.id)
        .bind(request_fingerprint)
        .bind(intent.created_at)
        .execute(&mut *tx)
        .await?;

        for entry in &intent.timeline {
            self.insert_state_transition_audit_tx(&mut tx, intent.id, entry)
                .await?;
        }

        self.insert_audit_event_tx(
            &mut tx,
            Some(intent.id),
            "intent_created",
            json!({
                "merchant_reference": intent.merchant_reference.0,
                "provider": intent.provider.0,
                "amount_minor": intent.money.amount_minor,
                "currency": intent.money.currency
            }),
            intent.created_at,
        )
        .await?;

        tx.commit().await?;
        Ok(CreateIntentResult::Created(intent.clone()))
    }

    pub async fn get_intent_by_id(
        &self,
        intent_id: IntentId,
    ) -> Result<PaymentIntent, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let intent = self.load_intent_by_id_tx(&mut tx, intent_id).await?;
        tx.commit().await?;
        Ok(intent)
    }

    pub async fn list_intents(&self, limit: u32) -> Result<Vec<PaymentIntent>, PersistenceError> {
        let mut tx = self.pool.begin().await?;

        let intent_ids = sqlx::query_scalar::<_, IntentId>(
            r#"
            SELECT id
            FROM payment_intents
            ORDER BY updated_at DESC
            LIMIT $1
            "#,
        )
        .bind(i64::from(limit))
        .fetch_all(&mut *tx)
        .await?;

        let mut intents = Vec::with_capacity(intent_ids.len());
        for intent_id in intent_ids {
            intents.push(self.load_intent_by_id_tx(&mut tx, intent_id).await?);
        }

        tx.commit().await?;
        Ok(intents)
    }

    pub async fn save_attempt_started(
        &self,
        intent: &PaymentIntent,
        request_payload_snapshot: Value,
    ) -> Result<(), PersistenceError> {
        let attempt = intent.attempts.last().ok_or_else(|| {
            PersistenceError::InvariantViolation(
                "attempt start requested but no current attempt exists".to_string(),
            )
        })?;

        let latest_transition = intent.timeline.last().ok_or_else(|| {
            PersistenceError::InvariantViolation("payment intent timeline is empty".to_string())
        })?;

        let mut tx = self.pool.begin().await?;

        self.update_payment_intent_header_tx(&mut tx, intent)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO execution_attempts (
                intent_id,
                attempt_no,
                started_at,
                ended_at,
                request_payload_snapshot,
                outcome_kind,
                raw_provider_response_summary,
                error_category,
                result_reason,
                provider_reference,
                note
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
            "#,
        )
        .bind(intent.id)
        .bind(i32::try_from(attempt.attempt_no).map_err(|_| {
            PersistenceError::InvariantViolation("attempt number overflow".to_string())
        })?)
        .bind(attempt.started_at)
        .bind(attempt.ended_at)
        .bind(Json(request_payload_snapshot))
        .bind(Option::<String>::None)
        .bind(Option::<Json<Value>>::None)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(attempt.provider_reference.as_ref().map(|p| p.0.as_str()))
        .bind(attempt.note.as_deref())
        .execute(&mut *tx)
        .await?;

        self.insert_state_transition_audit_tx(&mut tx, intent.id, latest_transition)
            .await?;
        schedule_callback_notification_tx(&mut tx, intent, intent.updated_at).await?;

        self.insert_audit_event_tx(
            &mut tx,
            Some(intent.id),
            "execution_attempt_started",
            json!({
                "attempt_no": attempt.attempt_no,
                "started_at": attempt.started_at,
            }),
            attempt.started_at,
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn save_attempt_finished(
        &self,
        intent: &PaymentIntent,
        raw_provider_response_summary: Option<Value>,
        retry_available_at: Option<DateTime<Utc>>,
    ) -> Result<(), PersistenceError> {
        let attempt = intent.attempts.last().ok_or_else(|| {
            PersistenceError::InvariantViolation(
                "attempt finish requested but no current attempt exists".to_string(),
            )
        })?;

        let outcome = attempt.outcome.as_ref().ok_or_else(|| {
            PersistenceError::InvariantViolation(
                "attempt finish requested but attempt has no outcome".to_string(),
            )
        })?;

        let latest_transition = intent.timeline.last().ok_or_else(|| {
            PersistenceError::InvariantViolation("payment intent timeline is empty".to_string())
        })?;

        let (outcome_kind, error_category, result_reason) = attempt_outcome_to_db(outcome);

        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            UPDATE payment_intents
            SET
                state = $2,
                latest_failure_classification = $3,
                provider_reference = $4,
                available_at = COALESCE($5, available_at),
                next_resolution_at = $6,
                last_resolution_at = $7,
                resolution_attempt_count = $8,
                updated_at = $9
            WHERE id = $1
            "#,
        )
        .bind(intent.id)
        .bind(state_to_db(intent.state))
        .bind(intent.latest_failure.as_ref().map(failure_to_db))
        .bind(intent.provider_reference.as_ref().map(|p| p.0.as_str()))
        .bind(retry_available_at)
        .bind(intent.next_resolution_at)
        .bind(intent.last_resolution_at)
        .bind(intent.resolution_attempt_count as i32)
        .bind(intent.updated_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE execution_attempts
            SET
                ended_at = $3,
                outcome_kind = $4,
                raw_provider_response_summary = $5,
                error_category = $6,
                result_reason = $7,
                provider_reference = $8,
                note = $9
            WHERE intent_id = $1 AND attempt_no = $2
            "#,
        )
        .bind(intent.id)
        .bind(i32::try_from(attempt.attempt_no).map_err(|_| {
            PersistenceError::InvariantViolation("attempt number overflow".to_string())
        })?)
        .bind(attempt.ended_at)
        .bind(outcome_kind)
        .bind(raw_provider_response_summary.map(Json))
        .bind(error_category)
        .bind(result_reason)
        .bind(attempt.provider_reference.as_ref().map(|p| p.0.as_str()))
        .bind(attempt.note.as_deref())
        .execute(&mut *tx)
        .await?;

        self.insert_state_transition_audit_tx(&mut tx, intent.id, latest_transition)
            .await?;
        schedule_callback_notification_tx(&mut tx, intent, intent.updated_at).await?;

        self.insert_audit_event_tx(
            &mut tx,
            Some(intent.id),
            "execution_attempt_finished",
            json!({
                "attempt_no": attempt.attempt_no,
                "outcome_kind": outcome_kind,
                "ended_at": attempt.ended_at,
                "provider_reference": attempt.provider_reference.as_ref().map(|p| p.0.clone()),
                "retry_available_at": retry_available_at,
                "next_resolution_at": intent.next_resolution_at,
                "last_resolution_at": intent.last_resolution_at,
                "resolution_attempt_count": intent.resolution_attempt_count,
            }),
            attempt.ended_at.unwrap_or(intent.updated_at),
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn save_provider_event(
        &self,
        input: SaveProviderEventInput,
    ) -> Result<bool, PersistenceError> {
        let rows_affected = sqlx::query(
            r#"
            INSERT INTO provider_events (
                provider_name,
                provider_event_id,
                intent_id,
                provider_reference,
                event_type,
                raw_payload,
                dedup_hash,
                received_at,
                processed_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            ON CONFLICT (provider_name, dedup_hash) DO NOTHING
            "#,
        )
        .bind(input.provider_name.as_str())
        .bind(input.provider_event_id.as_str())
        .bind(input.intent_id)
        .bind(input.provider_reference.as_deref())
        .bind(input.event_type.as_str())
        .bind(Json(input.raw_payload.clone()))
        .bind(input.dedup_hash.as_str())
        .bind(input.received_at)
        .bind(input.processed_at)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if rows_affected == 1 {
            self.insert_audit_event(
                input.intent_id,
                "provider_event_recorded",
                json!({
                    "provider_name": input.provider_name,
                    "provider_event_id": input.provider_event_id,
                    "event_type": input.event_type,
                    "dedup_hash": input.dedup_hash
                }),
                input.received_at,
            )
            .await?;
            return Ok(true);
        }

        Ok(false)
    }

    pub async fn find_intent_for_provider_event(
        &self,
        provider_name: &str,
        provider_reference: Option<&str>,
        merchant_reference: Option<&str>,
    ) -> Result<Option<PaymentIntent>, PersistenceError> {
        let mut tx = self.pool.begin().await?;

        let by_provider_reference = match provider_reference {
            Some(provider_reference) => {
                sqlx::query_scalar::<_, IntentId>(
                    r#"
                SELECT id
                FROM payment_intents
                WHERE provider = $1 AND provider_reference = $2
                ORDER BY updated_at DESC
                LIMIT 1
                "#,
                )
                .bind(provider_name)
                .bind(provider_reference)
                .fetch_optional(&mut *tx)
                .await?
            }
            None => None,
        };

        let intent_id = match (by_provider_reference, merchant_reference) {
            (Some(intent_id), _) => Some(intent_id),
            (None, Some(merchant_reference)) => {
                sqlx::query_scalar::<_, IntentId>(
                    r#"
                SELECT id
                FROM payment_intents
                WHERE provider = $1 AND merchant_reference = $2
                ORDER BY created_at DESC
                LIMIT 1
                "#,
                )
                .bind(provider_name)
                .bind(merchant_reference)
                .fetch_optional(&mut *tx)
                .await?
            }
            (None, None) => None,
        };

        let intent = match intent_id {
            Some(intent_id) => Some(self.load_intent_by_id_tx(&mut tx, intent_id).await?),
            None => None,
        };

        tx.commit().await?;
        Ok(intent)
    }

    pub async fn persist_provider_webhook_effect(
        &self,
        input: SaveProviderEventInput,
        intent: Option<&PaymentIntent>,
        previous_state: Option<IntentState>,
        audit_event_type: &str,
        audit_payload: Value,
    ) -> Result<bool, PersistenceError> {
        let mut tx = self.pool.begin().await?;

        let rows_affected = sqlx::query(
            r#"
            INSERT INTO provider_events (
                provider_name,
                provider_event_id,
                intent_id,
                provider_reference,
                event_type,
                raw_payload,
                dedup_hash,
                received_at,
                processed_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            ON CONFLICT (provider_name, dedup_hash) DO NOTHING
            "#,
        )
        .bind(input.provider_name.as_str())
        .bind(input.provider_event_id.as_str())
        .bind(input.intent_id)
        .bind(input.provider_reference.as_deref())
        .bind(input.event_type.as_str())
        .bind(Json(input.raw_payload.clone()))
        .bind(input.dedup_hash.as_str())
        .bind(input.received_at)
        .bind(input.processed_at)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        if rows_affected == 0 {
            tx.commit().await?;
            return Ok(false);
        }

        if let Some(intent) = intent {
            if previous_state.is_some() && previous_state != Some(intent.state) {
                self.update_payment_intent_header_tx(&mut tx, intent)
                    .await?;

                if let Some(transition) = intent.timeline.last() {
                    self.insert_state_transition_audit_tx(&mut tx, intent.id, transition)
                        .await?;
                    schedule_callback_notification_tx(&mut tx, intent, input.received_at).await?;
                }
            } else if intent.provider_reference.is_some() {
                sqlx::query(
                    r#"
                    UPDATE payment_intents
                    SET
                        provider_reference = COALESCE(provider_reference, $2),
                        updated_at = GREATEST(updated_at, $3)
                    WHERE id = $1
                    "#,
                )
                .bind(intent.id)
                .bind(
                    intent
                        .provider_reference
                        .as_ref()
                        .map(|reference| reference.0.as_str()),
                )
                .bind(input.received_at)
                .execute(&mut *tx)
                .await?;
            }
        }

        self.insert_audit_event_tx(
            &mut tx,
            input.intent_id,
            audit_event_type,
            audit_payload,
            input.received_at,
        )
        .await?;

        tx.commit().await?;
        Ok(true)
    }

    pub async fn save_callback_delivery(
        &self,
        input: SaveCallbackDeliveryInput,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin().await?;

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
        .bind(Json(input.payload.clone()))
        .bind(input.http_status_code)
        .bind(input.delivery_result.as_str())
        .bind(input.started_at)
        .bind(input.ended_at)
        .bind(input.retry_count)
        .bind(input.response_body.as_deref())
        .execute(&mut *tx)
        .await?;

        self.insert_audit_event_tx(
            &mut tx,
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

        tx.commit().await?;
        Ok(())
    }

    pub async fn save_reconciliation_run(
        &self,
        intent: &PaymentIntent,
        input: SaveReconciliationRunInput,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO reconciliation_runs (
                intent_id,
                started_at,
                ended_at,
                provider_status_seen,
                internal_status_seen,
                comparison_result,
                decision,
                evidence,
                notes
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            "#,
        )
        .bind(input.intent_id)
        .bind(input.started_at)
        .bind(input.ended_at)
        .bind(input.provider_status_seen.as_str())
        .bind(state_to_db(input.internal_status_seen))
        .bind(recon_comparison_to_db(input.comparison))
        .bind(recon_decision_to_db(input.decision))
        .bind(Json(serde_json::to_value(&input.evidence)?))
        .bind(input.note.as_deref())
        .execute(&mut *tx)
        .await?;

        self.update_payment_intent_header_tx(&mut tx, intent)
            .await?;

        if let Some(transition) = intent.timeline.last() {
            self.insert_state_transition_audit_tx(&mut tx, intent.id, transition)
                .await?;
            schedule_callback_notification_tx(&mut tx, intent, input.ended_at).await?;
        }

        self.insert_audit_event_tx(
            &mut tx,
            Some(intent.id),
            "reconciliation_run_recorded",
            json!({
                "provider_status_seen": input.provider_status_seen,
                "raw_provider_summary": input.raw_provider_summary,
                "internal_status_seen": state_to_db(input.internal_status_seen),
                "comparison_result": recon_comparison_to_db(input.comparison),
                "decision": recon_decision_to_db(input.decision),
            }),
            input.ended_at,
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_receipt_by_id(
        &self,
        intent_id: IntentId,
    ) -> Result<ComputedReceipt, PersistenceError> {
        let intent = self.get_intent_by_id(intent_id).await?;
        let core = intent.to_receipt();

        let provider_events = sqlx::query_as::<_, DbProviderEventRow>(
            r#"
            SELECT
                provider_name,
                provider_event_id,
                intent_id,
                provider_reference,
                event_type,
                raw_payload,
                dedup_hash,
                received_at,
                processed_at
            FROM provider_events
            WHERE intent_id = $1
               OR ($2::text IS NOT NULL AND provider_reference = $2)
            ORDER BY received_at ASC
            "#,
        )
        .bind(intent_id)
        .bind(intent.provider_reference.as_ref().map(|p| p.0.as_str()))
        .fetch_all(&self.pool)
        .await?;

        let callback_deliveries = sqlx::query_as::<_, DbCallbackDeliveryRow>(
            r#"
            SELECT
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
            FROM callback_deliveries
            WHERE intent_id = $1
            ORDER BY started_at ASC
            "#,
        )
        .bind(intent_id)
        .fetch_all(&self.pool)
        .await?;

        let callback_notifications = sqlx::query_as::<_, DbCallbackNotificationRow>(
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
            WHERE intent_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(intent_id)
        .fetch_all(&self.pool)
        .await?;

        let reconciliation_runs = sqlx::query_as::<_, DbReconciliationRunRow>(
            r#"
            SELECT
                intent_id,
                started_at,
                ended_at,
                provider_status_seen,
                internal_status_seen,
                comparison_result,
                decision,
                evidence,
                notes
            FROM reconciliation_runs
            WHERE intent_id = $1
            ORDER BY ended_at ASC
            "#,
        )
        .bind(intent_id)
        .fetch_all(&self.pool)
        .await?;

        let audit_events = sqlx::query_as::<_, DbAuditEventRow>(
            r#"
            SELECT intent_id, event_type, payload, created_at
            FROM audit_events
            WHERE intent_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(intent_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(ComputedReceipt {
            core,
            provider_events: provider_events
                .into_iter()
                .map(|row| StoredProviderEvent {
                    provider_name: row.provider_name,
                    provider_event_id: row.provider_event_id,
                    intent_id: row.intent_id,
                    provider_reference: row.provider_reference,
                    event_type: row.event_type,
                    raw_payload: row.raw_payload.0,
                    dedup_hash: row.dedup_hash,
                    received_at: row.received_at,
                    processed_at: row.processed_at,
                })
                .collect(),
            callback_notifications: callback_notifications
                .into_iter()
                .map(|row| StoredCallbackNotification {
                    event_key: row.event_key,
                    intent_id: row.intent_id,
                    destination_url: row.destination_url,
                    target_state: row.target_state,
                    payload: row.payload.0,
                    status: row.status,
                    next_attempt_at: row.next_attempt_at,
                    attempt_count: row.attempt_count,
                    last_attempt_at: row.last_attempt_at,
                    delivered_at: row.delivered_at,
                    last_http_status_code: row.last_http_status_code,
                    last_error: row.last_error,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                })
                .collect(),
            callback_deliveries: callback_deliveries
                .into_iter()
                .map(|row| StoredCallbackDelivery {
                    intent_id: row.intent_id,
                    destination_url: row.destination_url,
                    attempt_no: row.attempt_no,
                    payload: row.payload.0,
                    http_status_code: row.http_status_code,
                    delivery_result: row.delivery_result,
                    started_at: row.started_at,
                    ended_at: row.ended_at,
                    retry_count: row.retry_count,
                    response_body: row.response_body,
                })
                .collect(),
            reconciliation_runs: reconciliation_runs
                .into_iter()
                .map(map_reconciliation_run_row)
                .collect::<Result<Vec<_>, _>>()?,
            audit_events: audit_events
                .into_iter()
                .map(|row| StoredAuditEvent {
                    intent_id: row.intent_id,
                    event_type: row.event_type,
                    payload: row.payload.0,
                    created_at: row.created_at,
                })
                .collect(),
        })
    }

    async fn load_intent_by_id_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        intent_id: IntentId,
    ) -> Result<PaymentIntent, PersistenceError> {
        let intent_row = sqlx::query_as::<_, DbPaymentIntentRow>(
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
            WHERE id = $1
            "#,
        )
        .bind(intent_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(PersistenceError::IntentNotFound(intent_id))?;

        let attempt_rows = sqlx::query_as::<_, DbExecutionAttemptRow>(
            r#"
            SELECT
                intent_id,
                attempt_no,
                started_at,
                ended_at,
                request_payload_snapshot,
                outcome_kind,
                raw_provider_response_summary,
                error_category,
                result_reason,
                provider_reference,
                note
            FROM execution_attempts
            WHERE intent_id = $1
            ORDER BY attempt_no ASC
            "#,
        )
        .bind(intent_id)
        .fetch_all(&mut **tx)
        .await?;

        let recon_row = sqlx::query_as::<_, DbReconciliationRunRow>(
            r#"
            SELECT
                intent_id,
                started_at,
                ended_at,
                provider_status_seen,
                internal_status_seen,
                comparison_result,
                decision,
                evidence,
                notes
            FROM reconciliation_runs
            WHERE intent_id = $1
            ORDER BY ended_at DESC
            LIMIT 1
            "#,
        )
        .bind(intent_id)
        .fetch_optional(&mut **tx)
        .await?;

        let audit_rows = sqlx::query_as::<_, DbAuditEventRow>(
            r#"
            SELECT intent_id, event_type, payload, created_at
            FROM audit_events
            WHERE intent_id = $1
            ORDER BY created_at ASC
            "#,
        )
        .bind(intent_id)
        .fetch_all(&mut **tx)
        .await?;

        let attempts = attempt_rows
            .into_iter()
            .map(map_attempt_row)
            .collect::<Result<Vec<_>, _>>()?;

        let timeline = audit_rows
            .iter()
            .filter(|row| row.event_type == "state_transition")
            .map(map_timeline_row)
            .collect::<Result<Vec<_>, _>>()?;

        let reconciliation = match recon_row {
            Some(row) => Some(map_recon_row(row)?),
            None => None,
        };

        Ok(PaymentIntent {
            id: intent_row.id,
            merchant_reference: MerchantReference(intent_row.merchant_reference),
            idempotency_key: self.load_idempotency_key_tx(tx, intent_id).await?,
            money: Money::new(intent_row.amount_minor, intent_row.currency),
            provider: ProviderName(intent_row.provider),
            callback_url: intent_row.callback_url,
            provider_reference: intent_row.provider_reference.map(ProviderReference),
            state: state_from_db(&intent_row.state)?,
            latest_failure: match intent_row.latest_failure_classification {
                Some(value) => Some(failure_from_db(&value)?),
                None => None,
            },
            attempts,
            reconciliation,
            timeline,
            next_resolution_at: intent_row.next_resolution_at,
            last_resolution_at: intent_row.last_resolution_at,
            resolution_attempt_count: intent_row.resolution_attempt_count as u32,
            created_at: intent_row.created_at,
            updated_at: intent_row.updated_at,
        })
    }

    async fn load_idempotency_key_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        intent_id: IntentId,
    ) -> Result<IdempotencyKey, PersistenceError> {
        let row = sqlx::query_as::<_, DbIdempotencyKeyRow>(
            r#"
            SELECT scope, idempotency_key, intent_id, request_fingerprint, created_at
            FROM idempotency_keys
            WHERE intent_id = $1
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .bind(intent_id)
        .fetch_one(&mut **tx)
        .await?;

        Ok(IdempotencyKey(row.idempotency_key))
    }

    async fn update_payment_intent_header_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        intent: &PaymentIntent,
    ) -> Result<(), PersistenceError> {
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
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    async fn insert_state_transition_audit_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        intent_id: IntentId,
        entry: &ReceiptTimelineEntry,
    ) -> Result<(), PersistenceError> {
        self.insert_audit_event_tx(
            tx,
            Some(intent_id),
            "state_transition",
            json!({
                "state": state_to_db(entry.state),
                "note": entry.note
            }),
            entry.at,
        )
        .await
    }

    async fn insert_audit_event(
        &self,
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
        .bind(Json(payload))
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn insert_audit_event_tx(
        &self,
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
        .bind(Json(payload))
        .bind(created_at)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}

fn map_attempt_row(row: DbExecutionAttemptRow) -> Result<ExecutionAttempt, PersistenceError> {
    let outcome = match row.outcome_kind {
        None => None,
        Some(kind) => Some(attempt_outcome_from_db(
            &kind,
            row.error_category.as_deref(),
            row.result_reason.as_deref(),
        )?),
    };

    Ok(ExecutionAttempt {
        attempt_no: row.attempt_no as u32,
        started_at: row.started_at,
        ended_at: row.ended_at,
        outcome,
        provider_reference: row.provider_reference.map(ProviderReference),
        note: row.note,
    })
}

fn map_timeline_row(row: &DbAuditEventRow) -> Result<ReceiptTimelineEntry, PersistenceError> {
    let payload = &row.payload.0;
    let state = payload
        .get("state")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            PersistenceError::InvariantViolation(
                "state_transition audit event missing state".to_string(),
            )
        })?;

    let note = payload
        .get("note")
        .and_then(Value::as_str)
        .map(|value| value.to_string());

    Ok(ReceiptTimelineEntry {
        state: state_from_db(state)?,
        at: row.created_at,
        note,
    })
}

fn map_recon_row(row: DbReconciliationRunRow) -> Result<ReconResult, PersistenceError> {
    let evidence: EvidenceSource = serde_json::from_value(row.evidence.0)?;

    Ok(ReconResult {
        compared_at: row.ended_at,
        internal_state: state_from_db(&row.internal_status_seen)?,
        provider_state: row.provider_status_seen,
        comparison: recon_comparison_from_db(&row.comparison_result)?,
        decision: recon_decision_from_db(&row.decision)?,
        evidence,
        note: row.notes,
    })
}

fn map_reconciliation_run_row(
    row: DbReconciliationRunRow,
) -> Result<StoredReconciliationRun, PersistenceError> {
    let evidence: EvidenceSource = serde_json::from_value(row.evidence.0)?;

    Ok(StoredReconciliationRun {
        intent_id: row.intent_id,
        started_at: row.started_at,
        ended_at: row.ended_at,
        provider_status_seen: row.provider_status_seen,
        internal_status_seen: state_from_db(&row.internal_status_seen)?,
        comparison: recon_comparison_from_db(&row.comparison_result)?,
        decision: recon_decision_from_db(&row.decision)?,
        evidence,
        note: row.notes,
    })
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

fn state_from_db(value: &str) -> Result<IntentState, PersistenceError> {
    match value {
        "received" => Ok(IntentState::Received),
        "validated" => Ok(IntentState::Validated),
        "rejected" => Ok(IntentState::Rejected),
        "queued" => Ok(IntentState::Queued),
        "leased" => Ok(IntentState::Leased),
        "executing" => Ok(IntentState::Executing),
        "provider_pending" => Ok(IntentState::ProviderPending),
        "retry_scheduled" => Ok(IntentState::RetryScheduled),
        "unknown_outcome" => Ok(IntentState::UnknownOutcome),
        "succeeded" => Ok(IntentState::Succeeded),
        "failed_terminal" => Ok(IntentState::FailedTerminal),
        "reconciling" => Ok(IntentState::Reconciling),
        "reconciled" => Ok(IntentState::Reconciled),
        "manual_review" => Ok(IntentState::ManualReview),
        "dead_lettered" => Ok(IntentState::DeadLettered),
        other => Err(PersistenceError::InvalidPersistedState(other.to_string())),
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

fn failure_from_db(value: &str) -> Result<FailureClassification, PersistenceError> {
    match value {
        "validation" => Ok(FailureClassification::Validation),
        "duplicate_request" => Ok(FailureClassification::DuplicateRequest),
        "retryable_infrastructure" => Ok(FailureClassification::RetryableInfrastructure),
        "terminal_provider" => Ok(FailureClassification::TerminalProvider),
        "unknown_outcome" => Ok(FailureClassification::UnknownOutcome),
        "callback_delivery" => Ok(FailureClassification::CallbackDelivery),
        "reconciliation_mismatch" => Ok(FailureClassification::ReconciliationMismatch),
        other => Err(PersistenceError::InvalidFailureClassification(
            other.to_string(),
        )),
    }
}

fn attempt_outcome_to_db(
    outcome: &AttemptOutcome,
) -> (&'static str, Option<&'static str>, Option<String>) {
    match outcome {
        AttemptOutcome::Succeeded => ("succeeded", None, None),
        AttemptOutcome::ProviderPending => ("provider_pending", None, None),
        AttemptOutcome::RetryableFailure {
            classification,
            reason,
        } => (
            "retryable_failure",
            Some(failure_to_db(classification)),
            Some(reason.clone()),
        ),
        AttemptOutcome::TerminalFailure {
            classification,
            reason,
        } => (
            "terminal_failure",
            Some(failure_to_db(classification)),
            Some(reason.clone()),
        ),
        AttemptOutcome::UnknownOutcome {
            classification,
            reason,
        } => (
            "unknown_outcome",
            Some(failure_to_db(classification)),
            Some(reason.clone()),
        ),
    }
}

fn attempt_outcome_from_db(
    outcome_kind: &str,
    error_category: Option<&str>,
    result_reason: Option<&str>,
) -> Result<AttemptOutcome, PersistenceError> {
    match outcome_kind {
        "succeeded" => Ok(AttemptOutcome::Succeeded),
        "provider_pending" => Ok(AttemptOutcome::ProviderPending),
        "retryable_failure" => Ok(AttemptOutcome::RetryableFailure {
            classification: failure_from_db(error_category.ok_or_else(|| {
                PersistenceError::InvariantViolation(
                    "retryable_failure missing error_category".to_string(),
                )
            })?)?,
            reason: result_reason.unwrap_or_default().to_string(),
        }),
        "terminal_failure" => Ok(AttemptOutcome::TerminalFailure {
            classification: failure_from_db(error_category.ok_or_else(|| {
                PersistenceError::InvariantViolation(
                    "terminal_failure missing error_category".to_string(),
                )
            })?)?,
            reason: result_reason.unwrap_or_default().to_string(),
        }),
        "unknown_outcome" => Ok(AttemptOutcome::UnknownOutcome {
            classification: failure_from_db(error_category.ok_or_else(|| {
                PersistenceError::InvariantViolation(
                    "unknown_outcome missing error_category".to_string(),
                )
            })?)?,
            reason: result_reason.unwrap_or_default().to_string(),
        }),
        other => Err(PersistenceError::InvalidAttemptOutcome(other.to_string())),
    }
}

fn recon_comparison_to_db(comparison: ReconComparison) -> &'static str {
    match comparison {
        ReconComparison::Match => "match",
        ReconComparison::Mismatch => "mismatch",
        ReconComparison::Unresolved => "unresolved",
    }
}

fn recon_comparison_from_db(value: &str) -> Result<ReconComparison, PersistenceError> {
    match value {
        "match" => Ok(ReconComparison::Match),
        "mismatch" => Ok(ReconComparison::Mismatch),
        "unresolved" => Ok(ReconComparison::Unresolved),
        other => Err(PersistenceError::InvalidReconComparison(other.to_string())),
    }
}

fn recon_decision_to_db(decision: ReconDecision) -> &'static str {
    match decision {
        ReconDecision::ConfirmSucceeded => "confirm_succeeded",
        ReconDecision::ConfirmFailedTerminal => "confirm_failed_terminal",
        ReconDecision::KeepUnknown => "keep_unknown",
        ReconDecision::EscalateManualReview => "escalate_manual_review",
    }
}

impl PostgresPersistence {
    pub async fn save_attempt_started_from_lease(
        &self,
        intent: &PaymentIntent,
        lease_token: uuid::Uuid,
        request_payload_snapshot: Value,
    ) -> Result<(), PersistenceError> {
        let attempt = intent.attempts.last().ok_or_else(|| {
            PersistenceError::InvariantViolation(
                "attempt start requested but no current attempt exists".to_string(),
            )
        })?;

        let latest_transition = intent.timeline.last().ok_or_else(|| {
            PersistenceError::InvariantViolation("payment intent timeline is empty".to_string())
        })?;

        let mut tx = self.pool.begin().await?;

        let updated = sqlx::query(
            r#"
            UPDATE payment_intents
            SET
                state = $3,
                latest_failure_classification = $4,
                provider_reference = $5,
                next_resolution_at = $6,
                last_resolution_at = $7,
                resolution_attempt_count = $8,
                lease_owner = NULL,
                lease_token = NULL,
                lease_expires_at = NULL,
                updated_at = $9
            WHERE
                id = $1
                AND state = 'leased'
                AND lease_token = $2
            "#,
        )
        .bind(intent.id)
        .bind(lease_token)
        .bind(state_to_db(intent.state))
        .bind(intent.latest_failure.as_ref().map(failure_to_db))
        .bind(intent.provider_reference.as_ref().map(|p| p.0.as_str()))
        .bind(intent.next_resolution_at)
        .bind(intent.last_resolution_at)
        .bind(intent.resolution_attempt_count as i32)
        .bind(intent.updated_at)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        if updated == 0 {
            return Err(PersistenceError::LeaseNotHeld(intent.id));
        }

        sqlx::query(
            r#"
            INSERT INTO execution_attempts (
                intent_id,
                attempt_no,
                started_at,
                ended_at,
                request_payload_snapshot,
                outcome_kind,
                raw_provider_response_summary,
                error_category,
                result_reason,
                provider_reference,
                note
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
            "#,
        )
        .bind(intent.id)
        .bind(i32::try_from(attempt.attempt_no).map_err(|_| {
            PersistenceError::InvariantViolation("attempt number overflow".to_string())
        })?)
        .bind(attempt.started_at)
        .bind(attempt.ended_at)
        .bind(Json(request_payload_snapshot))
        .bind(Option::<String>::None)
        .bind(Option::<Json<Value>>::None)
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(attempt.provider_reference.as_ref().map(|p| p.0.as_str()))
        .bind(attempt.note.as_deref())
        .execute(&mut *tx)
        .await?;

        self.insert_state_transition_audit_tx(&mut tx, intent.id, latest_transition)
            .await?;

        self.insert_audit_event_tx(
            &mut tx,
            Some(intent.id),
            "execution_attempt_started",
            json!({
                "attempt_no": attempt.attempt_no,
                "started_at": attempt.started_at,
                "lease_token": lease_token,
            }),
            attempt.started_at,
        )
        .await?;

        tx.commit().await?;
        Ok(())
    }
}

fn recon_decision_from_db(value: &str) -> Result<ReconDecision, PersistenceError> {
    match value {
        "confirm_succeeded" => Ok(ReconDecision::ConfirmSucceeded),
        "confirm_failed_terminal" => Ok(ReconDecision::ConfirmFailedTerminal),
        "keep_unknown" => Ok(ReconDecision::KeepUnknown),
        "escalate_manual_review" => Ok(ReconDecision::EscalateManualReview),
        other => Err(PersistenceError::InvalidReconDecision(other.to_string())),
    }
}
