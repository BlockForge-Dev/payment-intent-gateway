use async_trait::async_trait;
use chrono::{ DateTime, Utc };
use domain::{ IntentId, PaymentIntent };
use persistence::{ CreateIntentResult, PostgresPersistence };
use serde::{ Deserialize, Serialize };
use serde_json::json;
use sha2::{ Digest, Sha256 };

use crate::ApplicationError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePaymentIntentCommand {
    pub merchant_reference: String,
    pub amount_minor: i64,
    pub currency: String,
    pub provider: String,
    pub idempotency_key: String,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum CreatePaymentIntentResult {
    Created(PaymentIntent),
    Existing(PaymentIntent),
}

#[async_trait]
pub trait PaymentIntentGatewayRepo: Clone + Send + Sync + 'static {
    async fn create_intent_with_idempotency(
        &self,
        intent: &PaymentIntent,
        scope: &str,
        request_fingerprint: &str
    ) -> Result<CreateIntentResult, persistence::PersistenceError>;

    async fn get_intent_by_id(
        &self,
        intent_id: IntentId
    ) -> Result<PaymentIntent, persistence::PersistenceError>;
}

#[async_trait]
impl PaymentIntentGatewayRepo for PostgresPersistence {
    async fn create_intent_with_idempotency(
        &self,
        intent: &PaymentIntent,
        scope: &str,
        request_fingerprint: &str
    ) -> Result<CreateIntentResult, persistence::PersistenceError> {
        PostgresPersistence::create_intent_with_idempotency(
            self,
            intent,
            scope,
            request_fingerprint
        ).await
    }

    async fn get_intent_by_id(
        &self,
        intent_id: IntentId
    ) -> Result<PaymentIntent, persistence::PersistenceError> {
        PostgresPersistence::get_intent_by_id(self, intent_id).await
    }
}

#[derive(Debug, Clone)]
pub struct PaymentIntentService<R> where R: PaymentIntentGatewayRepo {
    repo: R,
    supported_providers: Vec<String>,
    idempotency_scope: String,
}

impl<R> PaymentIntentService<R> where R: PaymentIntentGatewayRepo {
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
        command: CreatePaymentIntentCommand
    ) -> Result<CreatePaymentIntentResult, ApplicationError> {
        let merchant_reference = command.merchant_reference.trim().to_string();
        let currency = command.currency.trim().to_uppercase();
        let provider = normalize_provider(&command.provider);
        let idempotency_key = command.idempotency_key.trim().to_string();

        if merchant_reference.is_empty() {
            return Err(ApplicationError::Validation("merchant_reference is required".to_string()));
        }

        if idempotency_key.is_empty() {
            return Err(
                ApplicationError::Validation("Idempotency-Key header is required".to_string())
            );
        }

        if command.amount_minor <= 0 {
            return Err(
                ApplicationError::Validation("amount_minor must be greater than zero".to_string())
            );
        }

        if !self.supported_providers.iter().any(|p| p == &provider) {
            return Err(ApplicationError::UnsupportedProvider(provider));
        }

        let fingerprint = fingerprint_create_intent_request(
            &merchant_reference,
            command.amount_minor,
            &currency,
            &provider
        )?;

        let mut intent = PaymentIntent::new(
            merchant_reference,
            idempotency_key,
            command.amount_minor,
            currency,
            provider,
            command.received_at
        )?;

        intent.validate(command.received_at)?;
        intent.queue(command.received_at)?;

        let result = self.repo.create_intent_with_idempotency(
            &intent,
            &self.idempotency_scope,
            &fingerprint
        ).await?;

        match result {
            CreateIntentResult::Created(intent) => Ok(CreatePaymentIntentResult::Created(intent)),
            CreateIntentResult::Existing(intent) => Ok(CreatePaymentIntentResult::Existing(intent)),
        }
    }

    pub async fn get_intent(&self, intent_id: IntentId) -> Result<PaymentIntent, ApplicationError> {
        self.repo.get_intent_by_id(intent_id).await.map_err(Into::into)
    }
}

fn normalize_provider(provider: &str) -> String {
    provider.trim().to_lowercase()
}

pub fn fingerprint_create_intent_request(
    merchant_reference: &str,
    amount_minor: i64,
    currency: &str,
    provider: &str
) -> Result<String, ApplicationError> {
    let canonical =
        json!({
        "merchant_reference": merchant_reference.trim(),
        "amount_minor": amount_minor,
        "currency": currency.trim().to_uppercase(),
        "provider": provider.trim().to_lowercase(),
    });

    let bytes = serde_json
        ::to_vec(&canonical)
        .map_err(|e| ApplicationError::Validation(format!("failed to canonicalize request: {e}")))?;

    let digest = Sha256::digest(bytes);
    Ok(hex::encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use domain::IntentId;
    use persistence::{ CreateIntentResult, PersistenceError };
    use std::collections::HashMap;
    use std::sync::{ Arc, Mutex };

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
            request_fingerprint: &str
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
            intent_id: IntentId
        ) -> Result<PaymentIntent, PersistenceError> {
            let intents = self.intents.lock().unwrap();
            intents.get(&intent_id).cloned().ok_or(PersistenceError::IntentNotFound(intent_id))
        }
    }

    fn service() -> PaymentIntentService<FakeRepo> {
        PaymentIntentService::new(FakeRepo::default()).with_supported_providers(
            vec!["paystack".into(), "mockpay".into()]
        )
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
            idempotency_key: "idem_123".into(),
            received_at: now,
        };

        let second = CreatePaymentIntentCommand {
            merchant_reference: "order_123".into(),
            amount_minor: 7000,
            currency: "NGN".into(),
            provider: "paystack".into(),
            idempotency_key: "idem_123".into(),
            received_at: now,
        };

        svc.create_intent(first).await.unwrap();
        let result = svc.create_intent(second).await;

        assert!(matches!(result, Err(ApplicationError::IdempotencyConflict { .. })));
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
            idempotency_key: "idem_123".into(),
            received_at: now,
        };

        let result = svc.create_intent(cmd).await;
        assert!(matches!(result, Err(ApplicationError::UnsupportedProvider(_))));
    }
}
