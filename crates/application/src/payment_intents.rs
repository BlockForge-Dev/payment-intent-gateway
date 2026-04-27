use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::{FailureClassification, IntentId, IntentState, PaymentIntent};
use persistence::{ComputedReceipt, CreateIntentResult, PostgresPersistence};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{build_operator_receipt, ApplicationError, OperatorReceipt};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePaymentIntentCommand {
    pub merchant_reference: String,
    pub amount_minor: i64,
    pub currency: String,
    pub provider: String,
    pub callback_url: Option<String>,
    pub idempotency_key: String,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum CreatePaymentIntentResult {
    Created(PaymentIntent),
    Existing(PaymentIntent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorIntentList {
    pub generated_at: DateTime<Utc>,
    pub items: Vec<OperatorIntentListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorIntentListItem {
    pub intent_id: IntentId,
    pub merchant_reference: String,
    pub amount_minor: i64,
    pub currency: String,
    pub provider: String,
    pub state: String,
    pub latest_failure_classification: Option<String>,
    pub provider_reference: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub flags: OperatorIntentListFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorIntentListFlags {
    pub has_unknown_outcome: bool,
    pub has_reconciliation_mismatch: bool,
    pub needs_manual_review: bool,
}

#[async_trait]
pub trait PaymentIntentGatewayRepo: Clone + Send + Sync + 'static {
    async fn create_intent_with_idempotency(
        &self,
        intent: &PaymentIntent,
        scope: &str,
        request_fingerprint: &str,
    ) -> Result<CreateIntentResult, persistence::PersistenceError>;

    async fn get_intent_by_id(
        &self,
        intent_id: IntentId,
    ) -> Result<PaymentIntent, persistence::PersistenceError>;

    async fn get_receipt_by_id(
        &self,
        intent_id: IntentId,
    ) -> Result<ComputedReceipt, persistence::PersistenceError>;

    async fn list_intents(
        &self,
        limit: u32,
    ) -> Result<Vec<PaymentIntent>, persistence::PersistenceError>;
}

#[async_trait]
impl PaymentIntentGatewayRepo for PostgresPersistence {
    async fn create_intent_with_idempotency(
        &self,
        intent: &PaymentIntent,
        scope: &str,
        request_fingerprint: &str,
    ) -> Result<CreateIntentResult, persistence::PersistenceError> {
        PostgresPersistence::create_intent_with_idempotency(
            self,
            intent,
            scope,
            request_fingerprint,
        )
        .await
    }

    async fn get_intent_by_id(
        &self,
        intent_id: IntentId,
    ) -> Result<PaymentIntent, persistence::PersistenceError> {
        PostgresPersistence::get_intent_by_id(self, intent_id).await
    }

    async fn get_receipt_by_id(
        &self,
        intent_id: IntentId,
    ) -> Result<ComputedReceipt, persistence::PersistenceError> {
        PostgresPersistence::get_receipt_by_id(self, intent_id).await
    }

    async fn list_intents(
        &self,
        limit: u32,
    ) -> Result<Vec<PaymentIntent>, persistence::PersistenceError> {
        PostgresPersistence::list_intents(self, limit).await
    }
}

#[derive(Debug, Clone)]
pub struct PaymentIntentService<R>
where
    R: PaymentIntentGatewayRepo,
{
    repo: R,
    supported_providers: Vec<String>,
    idempotency_scope: String,
}

impl<R> PaymentIntentService<R>
where
    R: PaymentIntentGatewayRepo,
{
    pub fn new(repo: R) -> Self {
        Self {
            repo,
            supported_providers: vec!["paystack".to_string(), "mockpay".to_string()],
            idempotency_scope: "payment_intents:create".to_string(),
        }
    }

    pub fn with_supported_providers(mut self, providers: Vec<String>) -> Self {
        self.supported_providers = providers
            .into_iter()
            .map(|p| normalize_provider(&p))
            .collect();
        self
    }

    pub async fn create_intent(
        &self,
        command: CreatePaymentIntentCommand,
    ) -> Result<CreatePaymentIntentResult, ApplicationError> {
        let merchant_reference = command.merchant_reference.trim().to_string();
        let currency = command.currency.trim().to_uppercase();
        let provider = normalize_provider(&command.provider);
        let callback_url = normalize_callback_url(command.callback_url)?;
        let idempotency_key = command.idempotency_key.trim().to_string();

        if merchant_reference.is_empty() {
            return Err(ApplicationError::Validation(
                "merchant_reference is required".to_string(),
            ));
        }

        if idempotency_key.is_empty() {
            return Err(ApplicationError::Validation(
                "Idempotency-Key header is required".to_string(),
            ));
        }

        if command.amount_minor <= 0 {
            return Err(ApplicationError::Validation(
                "amount_minor must be greater than zero".to_string(),
            ));
        }

        if !self.supported_providers.iter().any(|p| p == &provider) {
            return Err(ApplicationError::UnsupportedProvider(provider));
        }

        let fingerprint = fingerprint_create_intent_request(
            &merchant_reference,
            command.amount_minor,
            &currency,
            &provider,
            callback_url.as_deref(),
        )?;

        let mut intent = PaymentIntent::new(
            merchant_reference,
            idempotency_key,
            command.amount_minor,
            currency,
            provider,
            command.received_at,
        )?
        .with_callback_url(callback_url);

        intent.validate(command.received_at)?;
        intent.queue(command.received_at)?;

        let result = self
            .repo
            .create_intent_with_idempotency(&intent, &self.idempotency_scope, &fingerprint)
            .await?;

        match result {
            CreateIntentResult::Created(intent) => Ok(CreatePaymentIntentResult::Created(intent)),
            CreateIntentResult::Existing(intent) => Ok(CreatePaymentIntentResult::Existing(intent)),
        }
    }

    pub async fn get_intent(&self, intent_id: IntentId) -> Result<PaymentIntent, ApplicationError> {
        self.repo
            .get_intent_by_id(intent_id)
            .await
            .map_err(Into::into)
    }

    pub async fn get_receipt(
        &self,
        intent_id: IntentId,
    ) -> Result<OperatorReceipt, ApplicationError> {
        let receipt = self.repo.get_receipt_by_id(intent_id).await?;
        Ok(build_operator_receipt(receipt))
    }

    pub async fn list_operator_intents(
        &self,
        requested_limit: Option<u32>,
    ) -> Result<OperatorIntentList, ApplicationError> {
        let limit = requested_limit.unwrap_or(50).clamp(1, 200);
        let intents = self.repo.list_intents(limit).await?;

        let items = intents
            .into_iter()
            .map(|intent| {
                let has_unknown_outcome = matches!(
                    intent.state,
                    IntentState::UnknownOutcome | IntentState::ProviderPending
                );
                let has_reconciliation_mismatch = matches!(
                    intent.latest_failure,
                    Some(FailureClassification::ReconciliationMismatch)
                );
                let needs_manual_review = intent.state == IntentState::ManualReview;

                OperatorIntentListItem {
                    intent_id: intent.id,
                    merchant_reference: intent.merchant_reference.0,
                    amount_minor: intent.money.amount_minor,
                    currency: intent.money.currency,
                    provider: intent.provider.0,
                    state: intent_state_to_api(intent.state).to_string(),
                    latest_failure_classification: intent
                        .latest_failure
                        .as_ref()
                        .map(failure_to_api)
                        .map(str::to_string),
                    provider_reference: intent.provider_reference.map(|reference| reference.0),
                    updated_at: intent.updated_at,
                    flags: OperatorIntentListFlags {
                        has_unknown_outcome,
                        has_reconciliation_mismatch,
                        needs_manual_review,
                    },
                }
            })
            .collect();

        Ok(OperatorIntentList {
            generated_at: Utc::now(),
            items,
        })
    }
}

fn normalize_provider(provider: &str) -> String {
    provider.trim().to_lowercase()
}

fn normalize_callback_url(
    callback_url: Option<String>,
) -> Result<Option<String>, ApplicationError> {
    let Some(callback_url) = callback_url else {
        return Ok(None);
    };

    let trimmed = callback_url.trim();
    if trimmed.is_empty() {
        return Err(ApplicationError::Validation(
            "callback_url cannot be empty".to_string(),
        ));
    }

    let parsed = reqwest::Url::parse(trimmed)
        .map_err(|err| ApplicationError::Validation(format!("callback_url is invalid: {err}")))?;

    match parsed.scheme() {
        "http" | "https" => Ok(Some(parsed.to_string())),
        other => Err(ApplicationError::Validation(format!(
            "callback_url scheme must be http or https, got {other}"
        ))),
    }
}

pub fn fingerprint_create_intent_request(
    merchant_reference: &str,
    amount_minor: i64,
    currency: &str,
    provider: &str,
    callback_url: Option<&str>,
) -> Result<String, ApplicationError> {
    let canonical = json!({
        "merchant_reference": merchant_reference.trim(),
        "amount_minor": amount_minor,
        "currency": currency.trim().to_uppercase(),
        "provider": provider.trim().to_lowercase(),
        "callback_url": callback_url,
    });

    let bytes = serde_json::to_vec(&canonical).map_err(|e| {
        ApplicationError::Validation(format!("failed to canonicalize request: {e}"))
    })?;

    let digest = Sha256::digest(bytes);
    Ok(hex::encode(digest))
}

fn intent_state_to_api(state: IntentState) -> &'static str {
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

fn failure_to_api(failure: &FailureClassification) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use domain::IntentId;
    use persistence::{CreateIntentResult, PersistenceError};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone, Default)]
    struct FakeRepo {
        store: Arc<Mutex<HashMap<(String, String), (String, PaymentIntent)>>>,
        intents: Arc<Mutex<HashMap<IntentId, PaymentIntent>>>,
    }

    #[async_trait]
    impl PaymentIntentGatewayRepo for FakeRepo {
        async fn create_intent_with_idempotency(
            &self,
            intent: &PaymentIntent,
            scope: &str,
            request_fingerprint: &str,
        ) -> Result<CreateIntentResult, PersistenceError> {
            let mut store = self.store.lock().unwrap();
            let mut intents = self.intents.lock().unwrap();

            let key = (scope.to_string(), intent.idempotency_key.0.clone());

            if let Some((existing_fingerprint, existing_intent)) = store.get(&key) {
                if existing_fingerprint != request_fingerprint {
                    return Err(PersistenceError::IdempotencyConflict {
                        scope: scope.to_string(),
                        key: intent.idempotency_key.0.clone(),
                    });
                }

                return Ok(CreateIntentResult::Existing(existing_intent.clone()));
            }

            store.insert(key, (request_fingerprint.to_string(), intent.clone()));
            intents.insert(intent.id, intent.clone());

            Ok(CreateIntentResult::Created(intent.clone()))
        }

        async fn get_intent_by_id(
            &self,
            intent_id: IntentId,
        ) -> Result<PaymentIntent, PersistenceError> {
            let intents = self.intents.lock().unwrap();
            intents
                .get(&intent_id)
                .cloned()
                .ok_or(PersistenceError::IntentNotFound(intent_id))
        }

        async fn get_receipt_by_id(
            &self,
            intent_id: IntentId,
        ) -> Result<ComputedReceipt, PersistenceError> {
            let intents = self.intents.lock().unwrap();
            let intent = intents
                .get(&intent_id)
                .cloned()
                .ok_or(PersistenceError::IntentNotFound(intent_id))?;

            Ok(ComputedReceipt {
                core: intent.to_receipt(),
                provider_events: vec![],
                callback_notifications: vec![],
                callback_deliveries: vec![],
                reconciliation_runs: vec![],
                audit_events: vec![],
            })
        }

        async fn list_intents(&self, limit: u32) -> Result<Vec<PaymentIntent>, PersistenceError> {
            let intents = self.intents.lock().unwrap();
            let mut values = intents.values().cloned().collect::<Vec<_>>();
            values.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
            values.truncate(limit as usize);
            Ok(values)
        }
    }

    fn service() -> PaymentIntentService<FakeRepo> {
        PaymentIntentService::new(FakeRepo::default())
            .with_supported_providers(vec!["paystack".into(), "mockpay".into()])
    }

    #[tokio::test]
    async fn same_idempotency_key_same_payload_returns_existing_lineage() {
        let svc = service();
        let now = Utc::now();

        let cmd = CreatePaymentIntentCommand {
            merchant_reference: "order_123".into(),
            amount_minor: 5000,
            currency: "NGN".into(),
            provider: "paystack".into(),
            callback_url: None,
            idempotency_key: "idem_123".into(),
            received_at: now,
        };

        let first = svc.create_intent(cmd.clone()).await.unwrap();
        let second = svc.create_intent(cmd.clone()).await.unwrap();

        let first_id = match first {
            CreatePaymentIntentResult::Created(intent) => intent.id,
            CreatePaymentIntentResult::Existing(_) => panic!("first call should create"),
        };

        let second_id = match second {
            CreatePaymentIntentResult::Created(_) => panic!("second call should return existing"),
            CreatePaymentIntentResult::Existing(intent) => intent.id,
        };

        assert_eq!(first_id, second_id);
    }

    #[tokio::test]
    async fn same_idempotency_key_different_payload_is_rejected() {
        let svc = service();
        let now = Utc::now();

        let first = CreatePaymentIntentCommand {
            merchant_reference: "order_123".into(),
            amount_minor: 5000,
            currency: "NGN".into(),
            provider: "paystack".into(),
            callback_url: None,
            idempotency_key: "idem_123".into(),
            received_at: now,
        };

        let second = CreatePaymentIntentCommand {
            merchant_reference: "order_123".into(),
            amount_minor: 7000,
            currency: "NGN".into(),
            provider: "paystack".into(),
            callback_url: None,
            idempotency_key: "idem_123".into(),
            received_at: now,
        };

        svc.create_intent(first).await.unwrap();
        let result = svc.create_intent(second).await;

        assert!(matches!(
            result,
            Err(ApplicationError::IdempotencyConflict { .. })
        ));
    }

    #[tokio::test]
    async fn unsupported_provider_is_rejected() {
        let svc = service();
        let now = Utc::now();

        let cmd = CreatePaymentIntentCommand {
            merchant_reference: "order_123".into(),
            amount_minor: 5000,
            currency: "NGN".into(),
            provider: "flutterwave".into(),
            callback_url: None,
            idempotency_key: "idem_123".into(),
            received_at: now,
        };

        let result = svc.create_intent(cmd).await;
        assert!(matches!(
            result,
            Err(ApplicationError::UnsupportedProvider(_))
        ));
    }

    #[tokio::test]
    async fn same_idempotency_key_different_callback_url_is_rejected() {
        let svc = service();
        let now = Utc::now();

        let first = CreatePaymentIntentCommand {
            merchant_reference: "order_123".into(),
            amount_minor: 5000,
            currency: "NGN".into(),
            provider: "paystack".into(),
            callback_url: Some("https://merchant.example/callbacks/a".into()),
            idempotency_key: "idem_123".into(),
            received_at: now,
        };

        let second = CreatePaymentIntentCommand {
            merchant_reference: "order_123".into(),
            amount_minor: 5000,
            currency: "NGN".into(),
            provider: "paystack".into(),
            callback_url: Some("https://merchant.example/callbacks/b".into()),
            idempotency_key: "idem_123".into(),
            received_at: now,
        };

        svc.create_intent(first).await.unwrap();
        let result = svc.create_intent(second).await;

        assert!(matches!(
            result,
            Err(ApplicationError::IdempotencyConflict { .. })
        ));
    }

    #[tokio::test]
    async fn invalid_callback_url_is_rejected() {
        let svc = service();
        let now = Utc::now();

        let cmd = CreatePaymentIntentCommand {
            merchant_reference: "order_123".into(),
            amount_minor: 5000,
            currency: "NGN".into(),
            provider: "paystack".into(),
            callback_url: Some("ftp://merchant.example/callbacks".into()),
            idempotency_key: "idem_123".into(),
            received_at: now,
        };

        let result = svc.create_intent(cmd).await;
        assert!(matches!(result, Err(ApplicationError::Validation(_))));
    }

    #[tokio::test]
    async fn list_operator_intents_surfaces_unknown_and_manual_review_flags() {
        let svc = service();
        let now = Utc::now();

        let first = CreatePaymentIntentCommand {
            merchant_reference: "order_unknown".into(),
            amount_minor: 5000,
            currency: "NGN".into(),
            provider: "mockpay".into(),
            callback_url: None,
            idempotency_key: "idem_unknown".into(),
            received_at: now,
        };

        let second = CreatePaymentIntentCommand {
            merchant_reference: "order_manual".into(),
            amount_minor: 7000,
            currency: "NGN".into(),
            provider: "mockpay".into(),
            callback_url: None,
            idempotency_key: "idem_manual".into(),
            received_at: now + chrono::Duration::seconds(1),
        };

        let unknown_id = match svc.create_intent(first).await.unwrap() {
            CreatePaymentIntentResult::Created(intent) => intent.id,
            CreatePaymentIntentResult::Existing(_) => panic!("intent should be created"),
        };

        let manual_id = match svc.create_intent(second).await.unwrap() {
            CreatePaymentIntentResult::Created(intent) => intent.id,
            CreatePaymentIntentResult::Existing(_) => panic!("intent should be created"),
        };

        let mut unknown_intent = svc.get_intent(unknown_id).await.unwrap();
        unknown_intent.state = IntentState::UnknownOutcome;
        unknown_intent.latest_failure = Some(FailureClassification::UnknownOutcome);
        unknown_intent.updated_at = now + chrono::Duration::seconds(2);

        let mut manual_intent = svc.get_intent(manual_id).await.unwrap();
        manual_intent.state = IntentState::ManualReview;
        manual_intent.latest_failure = Some(FailureClassification::ReconciliationMismatch);
        manual_intent.updated_at = now + chrono::Duration::seconds(3);

        {
            let fake_repo = &svc.repo;
            let mut intents = fake_repo.intents.lock().unwrap();
            intents.insert(unknown_intent.id, unknown_intent.clone());
            intents.insert(manual_intent.id, manual_intent.clone());
        }

        let list = svc.list_operator_intents(Some(10)).await.unwrap();

        assert_eq!(list.items.len(), 2);
        assert_eq!(list.items[0].intent_id, manual_intent.id);
        assert!(list.items[0].flags.needs_manual_review);
        assert!(list.items[0].flags.has_reconciliation_mismatch);
        assert_eq!(list.items[1].intent_id, unknown_intent.id);
        assert!(list.items[1].flags.has_unknown_outcome);
    }
}
