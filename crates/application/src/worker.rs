use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::{IntentId, PaymentIntent};
use persistence::{LeasedPaymentIntent, PersistenceError, PostgresPersistence};
use uuid::Uuid;

use crate::ApplicationError;

#[async_trait]
pub trait WorkerLeaseRepo: Clone + Send + Sync + 'static {
    async fn lease_next_available_intent(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_for: Duration,
    ) -> Result<Option<LeasedPaymentIntent>, PersistenceError>;

    async fn renew_lease(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        lease_for: Duration,
    ) -> Result<LeasedPaymentIntent, PersistenceError>;

    async fn return_lease_to_queue(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        available_at: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<PaymentIntent, PersistenceError>;

    async fn schedule_retry_from_lease(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        available_at: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<PaymentIntent, PersistenceError>;

    async fn mark_leased_as_executing(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<PaymentIntent, PersistenceError>;
}

#[async_trait]
impl WorkerLeaseRepo for PostgresPersistence {
    async fn lease_next_available_intent(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_for: Duration,
    ) -> Result<Option<LeasedPaymentIntent>, PersistenceError> {
        PostgresPersistence::lease_next_available_intent(self, worker_id, now, lease_for).await
    }

    async fn renew_lease(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        lease_for: Duration,
    ) -> Result<LeasedPaymentIntent, PersistenceError> {
        PostgresPersistence::renew_lease(self, intent_id, lease_token, now, lease_for).await
    }

    async fn return_lease_to_queue(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        available_at: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<PaymentIntent, PersistenceError> {
        PostgresPersistence::return_lease_to_queue(
            self,
            intent_id,
            lease_token,
            now,
            available_at,
            note,
        )
        .await
    }

    async fn schedule_retry_from_lease(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        available_at: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<PaymentIntent, PersistenceError> {
        PostgresPersistence::schedule_retry_from_lease(
            self,
            intent_id,
            lease_token,
            now,
            available_at,
            note,
        )
        .await
    }

    async fn mark_leased_as_executing(
        &self,
        intent_id: IntentId,
        lease_token: Uuid,
        now: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<PaymentIntent, PersistenceError> {
        PostgresPersistence::mark_leased_as_executing(self, intent_id, lease_token, now, note).await
    }
}

#[derive(Debug, Clone)]
pub struct WorkerLeaseService<R>
where
    R: WorkerLeaseRepo,
{
    repo: R,
    worker_id: String,
    lease_for: Duration,
}

impl<R> WorkerLeaseService<R>
where
    R: WorkerLeaseRepo,
{
    pub fn new(repo: R, worker_id: impl Into<String>, lease_for: Duration) -> Self {
        Self {
            repo,
            worker_id: worker_id.into(),
            lease_for,
        }
    }

    pub async fn poll_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<LeasedPaymentIntent>, ApplicationError> {
        self.repo
            .lease_next_available_intent(&self.worker_id, now, self.lease_for)
            .await
            .map_err(Into::into)
    }

    pub async fn renew(
        &self,
        leased: &LeasedPaymentIntent,
        now: DateTime<Utc>,
    ) -> Result<LeasedPaymentIntent, ApplicationError> {
        self.repo
            .renew_lease(leased.intent.id, leased.lease_token, now, self.lease_for)
            .await
            .map_err(Into::into)
    }

    pub async fn release_without_execution(
        &self,
        leased: &LeasedPaymentIntent,
        now: DateTime<Utc>,
        available_at: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<PaymentIntent, ApplicationError> {
        self.repo
            .return_lease_to_queue(
                leased.intent.id,
                leased.lease_token,
                now,
                available_at,
                note,
            )
            .await
            .map_err(Into::into)
    }

    pub async fn schedule_retry(
        &self,
        leased: &LeasedPaymentIntent,
        now: DateTime<Utc>,
        available_at: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<PaymentIntent, ApplicationError> {
        self.repo
            .schedule_retry_from_lease(
                leased.intent.id,
                leased.lease_token,
                now,
                available_at,
                note,
            )
            .await
            .map_err(Into::into)
    }

    pub async fn mark_executing(
        &self,
        leased: &LeasedPaymentIntent,
        now: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<PaymentIntent, ApplicationError> {
        self.repo
            .mark_leased_as_executing(leased.intent.id, leased.lease_token, now, note)
            .await
            .map_err(Into::into)
    }
}
