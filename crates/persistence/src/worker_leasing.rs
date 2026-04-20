use std::time::Duration as StdDuration;

use chrono::{ DateTime, Duration, Utc };
use domain::{ IntentId, PaymentIntent };
use serde::{ Deserialize, Serialize };
use serde_json::{ json, Value };
use sqlx::{ FromRow, Postgres, Transaction };
use uuid::Uuid;

use crate::{ PersistenceError, PostgresPersistence };

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeasedPaymentIntent {
    pub intent: PaymentIntent,
    pub lease_token: Uuid,
    pub worker_id: String,
    pub leased_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
struct LeaseClaimRow {
    intent_id: Uuid,
    previous_state: String,
    lease_owner: Option<String>,
    lease_token: Option<Uuid>,
    leased_at: Option<DateTime<Utc>>,
    lease_expires_at: Option<DateTime<Utc>>,
}

impl PostgresPersistence {
    pub async fn lease_next_available_intent(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_for: StdDuration
    ) -> Result<Option<LeasedPaymentIntent>, PersistenceError> {
        if worker_id.trim().is_empty() {
            return Err(PersistenceError::EmptyWorkerId);
        }
        if lease_for.is_zero() {
            return Err(PersistenceError::InvalidLeaseDuration);
        }

        let lease_expires_at =
            now +
            Duration::from_std(lease_for).map_err(|_| PersistenceError::InvalidLeaseDuration)?;

        let lease_token = Uuid::new_v4();
        let mut tx = self.pool().begin().await?;

        let row = sqlx
            ::query_as::<_, LeaseClaimRow>(
                r#"
            WITH candidate AS (
                SELECT
                    id,
                    state AS previous_state,
                    created_at,
                    available_at,
                    lease_expires_at
                FROM payment_intents
                WHERE
                    (
                        state IN ('queued', 'retry_scheduled')
                        AND available_at <= $1
                    )
                    OR
                    (
                        state = 'leased'
                        AND lease_expires_at IS NOT NULL
                        AND lease_expires_at <= $1
                    )
                ORDER BY
                    CASE
                        WHEN state = 'leased' THEN lease_expires_at
                        ELSE available_at
                    END ASC,
                    created_at ASC
                FOR UPDATE SKIP LOCKED
                LIMIT 1
            )
            UPDATE payment_intents p
            SET
                state = 'leased',
                lease_owner = $2,
                lease_token = $3,
                lease_expires_at = $4,
                last_leased_at = $1,
                updated_at = $1
            FROM candidate
            WHERE p.id = candidate.id
            RETURNING
                p.id AS intent_id,
                candidate.previous_state,
                p.lease_owner,
                p.lease_token,
                p.last_leased_at AS leased_at,
                p.lease_expires_at
            "#
            )
            .bind(now)
            .bind(worker_id)
            .bind(lease_token)
            .bind(lease_expires_at)
            .fetch_optional(&mut *tx).await?;

        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };

        insert_audit_event_tx(
            &mut tx,
            Some(row.intent_id),
            "state_transition",
            json!({
                "state": "leased",
                "note": format!("lease acquired by worker {}", worker_id),
            }),
            now
        ).await?;

        insert_audit_event_tx(
            &mut tx,
            Some(row.intent_id),
            "lease_acquired",
            json!({
                "worker_id": worker_id,
                "lease_token": lease_token,
                "leased_at": now,
                "lease_expires_at": lease_expires_at,
                "previous_state": row.previous_state,
            }),
            now
        ).await?;

        tx.commit().await?;

        let intent = self.get_intent_by_id(row.intent_id).await?;

        Ok(
            Some(LeasedPaymentIntent {
                intent,
                lease_token: row.lease_token
                    .or(Some(lease_token))
                    .ok_or_else(|| {
                        PersistenceError::InvariantViolation(
                            "lease_token missing after claim".to_string()
                        )
                    })?,
                worker_id: row.lease_owner.unwrap_or_else(|| worker_id.to_string()),
                leased_at: row.leased_at.unwrap_or(now),
                lease_expires_at: row.lease_expires_at.unwrap_or(lease_expires_at),
            })
        )
    }

    pub async fn renew_lease(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        lease_for: StdDuration
    ) -> Result<LeasedPaymentIntent, PersistenceError> {
        if lease_for.is_zero() {
            return Err(PersistenceError::InvalidLeaseDuration);
        }

        let new_expiry =
            now +
            Duration::from_std(lease_for).map_err(|_| PersistenceError::InvalidLeaseDuration)?;

        let mut tx = self.pool().begin().await?;

        let row = sqlx
            ::query_as::<_, LeaseClaimRow>(
                r#"
            UPDATE payment_intents
            SET
                lease_expires_at = $3,
                updated_at = $2
            WHERE
                id = $1
                AND state = 'leased'
                AND lease_token = $4
                AND lease_expires_at IS NOT NULL
                AND lease_expires_at > $2
            RETURNING
                id AS intent_id,
                state AS previous_state,
                lease_owner,
                lease_token,
                last_leased_at AS leased_at,
                lease_expires_at
            "#
            )
            .bind(intent_id)
            .bind(now)
            .bind(new_expiry)
            .bind(lease_token)
            .fetch_optional(&mut *tx).await?;

        let Some(row) = row else {
            return Err(PersistenceError::LeaseNotHeld(intent_id));
        };

        insert_audit_event_tx(
            &mut tx,
            Some(intent_id),
            "lease_renewed",
            json!({
                "lease_token": lease_token,
                "renewed_at": now,
                "lease_expires_at": new_expiry,
            }),
            now
        ).await?;

        tx.commit().await?;

        let intent = self.get_intent_by_id(intent_id).await?;
        Ok(LeasedPaymentIntent {
            intent,
            lease_token,
            worker_id: row.lease_owner.unwrap_or_default(),
            leased_at: row.leased_at.unwrap_or(now),
            lease_expires_at: row.lease_expires_at.unwrap_or(new_expiry),
        })
    }

    pub async fn return_lease_to_queue(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        available_at: DateTime<Utc>,
        note: Option<String>
    ) -> Result<PaymentIntent, PersistenceError> {
        let mut tx = self.pool().begin().await?;

        let updated = sqlx
            ::query(
                r#"
            UPDATE payment_intents
            SET
                state = 'queued',
                available_at = $4,
                lease_owner = NULL,
                lease_token = NULL,
                lease_expires_at = NULL,
                updated_at = $3
            WHERE
                id = $1
                AND state = 'leased'
                AND lease_token = $2
            "#
            )
            .bind(intent_id)
            .bind(lease_token)
            .bind(now)
            .bind(available_at)
            .execute(&mut *tx).await?
            .rows_affected();

        if updated == 0 {
            return Err(PersistenceError::LeaseNotHeld(intent_id));
        }

        insert_audit_event_tx(
            &mut tx,
            Some(intent_id),
            "state_transition",
            json!({
                "state": "queued",
                "note": note.clone().unwrap_or_else(|| "lease released back to queue".to_string()),
            }),
            now
        ).await?;

        insert_audit_event_tx(
            &mut tx,
            Some(intent_id),
            "lease_released",
            json!({
                "lease_token": lease_token,
                "released_at": now,
                "available_at": available_at,
                "reason": note,
            }),
            now
        ).await?;

        tx.commit().await?;
        self.get_intent_by_id(intent_id).await
    }

    pub async fn schedule_retry_from_lease(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        available_at: DateTime<Utc>,
        note: Option<String>
    ) -> Result<PaymentIntent, PersistenceError> {
        let mut tx = self.pool().begin().await?;

        let updated = sqlx
            ::query(
                r#"
            UPDATE payment_intents
            SET
                state = 'retry_scheduled',
                available_at = $4,
                lease_owner = NULL,
                lease_token = NULL,
                lease_expires_at = NULL,
                updated_at = $3
            WHERE
                id = $1
                AND state = 'leased'
                AND lease_token = $2
            "#
            )
            .bind(intent_id)
            .bind(lease_token)
            .bind(now)
            .bind(available_at)
            .execute(&mut *tx).await?
            .rows_affected();

        if updated == 0 {
            return Err(PersistenceError::LeaseNotHeld(intent_id));
        }

        insert_audit_event_tx(
            &mut tx,
            Some(intent_id),
            "state_transition",
            json!({
                "state": "retry_scheduled",
                "note": note.clone().unwrap_or_else(|| "retry scheduled from lease".to_string()),
            }),
            now
        ).await?;

        insert_audit_event_tx(
            &mut tx,
            Some(intent_id),
            "retry_scheduled",
            json!({
                "lease_token": lease_token,
                "scheduled_at": now,
                "available_at": available_at,
                "reason": note,
            }),
            now
        ).await?;

        tx.commit().await?;
        self.get_intent_by_id(intent_id).await
    }

    pub async fn mark_leased_as_executing(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        note: Option<String>
    ) -> Result<PaymentIntent, PersistenceError> {
        let mut tx = self.pool().begin().await?;

        let updated = sqlx
            ::query(
                r#"
            UPDATE payment_intents
            SET
                state = 'executing',
                lease_owner = NULL,
                lease_token = NULL,
                lease_expires_at = NULL,
                updated_at = $3
            WHERE
                id = $1
                AND state = 'leased'
                AND lease_token = $2
            "#
            )
            .bind(intent_id)
            .bind(lease_token)
            .bind(now)
            .execute(&mut *tx).await?
            .rows_affected();

        if updated == 0 {
            return Err(PersistenceError::LeaseNotHeld(intent_id));
        }

        insert_audit_event_tx(
            &mut tx,
            Some(intent_id),
            "state_transition",
            json!({
                "state": "executing",
                "note": note.clone().unwrap_or_else(|| "lease consumed by execution".to_string()),
            }),
            now
        ).await?;

        insert_audit_event_tx(
            &mut tx,
            Some(intent_id),
            "execution_claimed_from_lease",
            json!({
                "lease_token": lease_token,
                "claimed_at": now,
                "reason": note,
            }),
            now
        ).await?;

        tx.commit().await?;
        self.get_intent_by_id(intent_id).await
    }
}

async fn insert_audit_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    intent_id: Option<IntentId>,
    event_type: &str,
    payload: Value,
    created_at: DateTime<Utc>
) -> Result<(), PersistenceError> {
    sqlx
        ::query(
            r#"
        INSERT INTO audit_events (intent_id, event_type, payload, created_at)
        VALUES ($1,$2,$3,$4)
        "#
        )
        .bind(intent_id)
        .bind(event_type)
        .bind(sqlx::types::Json(payload))
        .bind(created_at)
        .execute(&mut **tx).await?;

    Ok(())
}
