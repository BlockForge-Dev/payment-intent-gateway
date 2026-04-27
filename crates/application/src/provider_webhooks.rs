use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use domain::{EvidenceSource, IntentId, IntentState, PaymentIntent, ProviderReference};
use persistence::{PersistenceError, PostgresPersistence, SaveProviderEventInput};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::ApplicationError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderWebhookStatus {
    Pending,
    Succeeded,
    FailedTerminal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestProviderWebhookCommand {
    pub provider_name: String,
    pub provider_event_id: String,
    pub provider_reference: Option<String>,
    pub merchant_reference: Option<String>,
    pub event_type: String,
    pub status: ProviderWebhookStatus,
    pub raw_payload: Value,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderWebhookIngestionSummary {
    pub duplicate: bool,
    pub intent_id: Option<IntentId>,
    pub state: Option<String>,
    pub provider_reference: Option<String>,
    pub note: String,
}

#[async_trait]
pub trait ProviderWebhookRepo: Clone + Send + Sync + 'static {
    async fn find_intent_for_provider_event(
        &self,
        provider_name: &str,
        provider_reference: Option<&str>,
        merchant_reference: Option<&str>,
    ) -> Result<Option<PaymentIntent>, PersistenceError>;

    async fn persist_provider_webhook_effect(
        &self,
        input: SaveProviderEventInput,
        intent: Option<&PaymentIntent>,
        previous_state: Option<IntentState>,
        audit_event_type: &str,
        audit_payload: Value,
    ) -> Result<bool, PersistenceError>;
}

#[async_trait]
impl ProviderWebhookRepo for PostgresPersistence {
    async fn find_intent_for_provider_event(
        &self,
        provider_name: &str,
        provider_reference: Option<&str>,
        merchant_reference: Option<&str>,
    ) -> Result<Option<PaymentIntent>, PersistenceError> {
        PostgresPersistence::find_intent_for_provider_event(
            self,
            provider_name,
            provider_reference,
            merchant_reference,
        )
        .await
    }

    async fn persist_provider_webhook_effect(
        &self,
        input: SaveProviderEventInput,
        intent: Option<&PaymentIntent>,
        previous_state: Option<IntentState>,
        audit_event_type: &str,
        audit_payload: Value,
    ) -> Result<bool, PersistenceError> {
        PostgresPersistence::persist_provider_webhook_effect(
            self,
            input,
            intent,
            previous_state,
            audit_event_type,
            audit_payload,
        )
        .await
    }
}

#[derive(Debug, Clone)]
pub struct ProviderWebhookService<R>
where
    R: ProviderWebhookRepo,
{
    repo: R,
    supported_providers: Vec<String>,
    follow_up_delay: StdDuration,
}

impl<R> ProviderWebhookService<R>
where
    R: ProviderWebhookRepo,
{
    pub fn new(repo: R, follow_up_delay: StdDuration) -> Self {
        Self {
            repo,
            supported_providers: vec!["paystack".to_string(), "mockpay".to_string()],
            follow_up_delay,
        }
    }

    pub fn with_supported_providers(mut self, providers: Vec<String>) -> Self {
        self.supported_providers = providers
            .into_iter()
            .map(|provider| normalize_provider(&provider))
            .collect();
        self
    }

    pub async fn ingest(
        &self,
        command: IngestProviderWebhookCommand,
    ) -> Result<ProviderWebhookIngestionSummary, ApplicationError> {
        let provider_name = normalize_provider(&command.provider_name);
        if !self
            .supported_providers
            .iter()
            .any(|provider| provider == &provider_name)
        {
            return Err(ApplicationError::UnsupportedProvider(provider_name));
        }

        let provider_event_id = command.provider_event_id.trim().to_string();
        if provider_event_id.is_empty() {
            return Err(ApplicationError::Validation(
                "provider_event_id is required".to_string(),
            ));
        }

        let event_type = command.event_type.trim().to_string();
        if event_type.is_empty() {
            return Err(ApplicationError::Validation(
                "event_type is required".to_string(),
            ));
        }

        let provider_reference = command
            .provider_reference
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());

        let merchant_reference = command
            .merchant_reference
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string());

        let mut intent = self
            .repo
            .find_intent_for_provider_event(
                &provider_name,
                provider_reference.as_deref(),
                merchant_reference.as_deref(),
            )
            .await?;

        let previous_state = intent.as_ref().map(|intent| intent.state);

        let note = if let Some(intent) = intent.as_mut() {
            if intent.provider_reference.is_none() {
                if let Some(provider_reference) = &provider_reference {
                    intent.provider_reference = Some(ProviderReference(provider_reference.clone()));
                }
            }

            apply_provider_webhook_to_intent(
                intent,
                command.status,
                &provider_event_id,
                command.received_at,
                self.follow_up_delay,
            )?
        } else {
            "provider webhook recorded without a matching internal intent".to_string()
        };

        let state_after = intent
            .as_ref()
            .map(|intent| intent_state_to_api(intent.state).to_string());
        let provider_reference_after = intent
            .as_ref()
            .and_then(|intent| {
                intent
                    .provider_reference
                    .as_ref()
                    .map(|reference| reference.0.clone())
            })
            .or(provider_reference.clone());

        let audit_event_type = match (intent.as_ref(), previous_state) {
            (None, _) => "provider_webhook_unmatched",
            (Some(intent), Some(previous_state)) if intent.state != previous_state => {
                "provider_webhook_applied"
            }
            _ => "provider_webhook_recorded",
        };

        let save_input = SaveProviderEventInput {
            provider_name: provider_name.clone(),
            provider_event_id: provider_event_id.clone(),
            intent_id: intent.as_ref().map(|intent| intent.id),
            provider_reference: provider_reference_after.clone(),
            event_type: event_type.clone(),
            raw_payload: command.raw_payload.clone(),
            dedup_hash: compute_provider_event_dedup_hash(&provider_name, &provider_event_id),
            received_at: command.received_at,
            processed_at: Some(command.received_at),
        };

        let audit_payload = json!({
            "provider_name": provider_name,
            "provider_event_id": provider_event_id,
            "event_type": event_type,
            "observed_status": webhook_status_to_api(command.status),
            "merchant_reference": merchant_reference,
            "provider_reference": provider_reference_after,
            "state_before": previous_state.map(intent_state_to_api),
            "state_after": state_after,
            "note": note,
        });

        let inserted = self
            .repo
            .persist_provider_webhook_effect(
                save_input,
                intent.as_ref(),
                previous_state,
                audit_event_type,
                audit_payload,
            )
            .await?;

        if !inserted {
            return Ok(ProviderWebhookIngestionSummary {
                duplicate: true,
                intent_id: intent.as_ref().map(|intent| intent.id),
                state: state_after,
                provider_reference: provider_reference_after,
                note: "duplicate provider webhook ignored".to_string(),
            });
        }

        Ok(ProviderWebhookIngestionSummary {
            duplicate: false,
            intent_id: intent.as_ref().map(|intent| intent.id),
            state: state_after,
            provider_reference: provider_reference_after,
            note,
        })
    }
}

pub fn compute_provider_event_dedup_hash(provider_name: &str, provider_event_id: &str) -> String {
    let digest = Sha256::digest(format!(
        "{}:{}",
        normalize_provider(provider_name),
        provider_event_id.trim()
    ));
    hex::encode(digest)
}

fn apply_provider_webhook_to_intent(
    intent: &mut PaymentIntent,
    status: ProviderWebhookStatus,
    provider_event_id: &str,
    received_at: DateTime<Utc>,
    follow_up_delay: StdDuration,
) -> Result<String, ApplicationError> {
    let already_known_note = match status {
        ProviderWebhookStatus::Succeeded => {
            "provider webhook recorded after success was already known".to_string()
        }
        ProviderWebhookStatus::FailedTerminal => {
            "provider webhook recorded after terminal failure was already known".to_string()
        }
        ProviderWebhookStatus::Pending => {
            "provider webhook recorded while the intent was already pending".to_string()
        }
    };

    match status {
        ProviderWebhookStatus::Succeeded => {
            if intent.state.needs_reconciliation() {
                let note = "provider webhook confirmed success".to_string();
                intent.resolve_unknown_with_evidence(
                    received_at,
                    IntentState::Succeeded,
                    EvidenceSource::ProviderWebhook {
                        event_id: provider_event_id.to_string(),
                    },
                    Some(note.clone()),
                )?;
                return Ok(note);
            }

            if intent.state == IntentState::Succeeded || intent.state == IntentState::Reconciled {
                return Ok(already_known_note);
            }

            Ok(format!(
                "provider webhook recorded without state change because current state is {}",
                intent_state_to_api(intent.state)
            ))
        }
        ProviderWebhookStatus::FailedTerminal => {
            if intent.state.needs_reconciliation() {
                let note = "provider webhook confirmed terminal failure".to_string();
                intent.resolve_unknown_with_evidence(
                    received_at,
                    IntentState::FailedTerminal,
                    EvidenceSource::ProviderWebhook {
                        event_id: provider_event_id.to_string(),
                    },
                    Some(note.clone()),
                )?;
                return Ok(note);
            }

            if intent.state == IntentState::FailedTerminal
                || intent.state == IntentState::ManualReview
            {
                return Ok(already_known_note);
            }

            Ok(format!(
                "provider webhook recorded without state change because current state is {}",
                intent_state_to_api(intent.state)
            ))
        }
        ProviderWebhookStatus::Pending => {
            let next_resolution_at = received_at
                + Duration::from_std(follow_up_delay).map_err(|_| {
                    ApplicationError::Validation(
                        "invalid provider webhook follow-up delay".to_string(),
                    )
                })?;

            if intent.state == IntentState::UnknownOutcome {
                intent.resolve_unknown_with_evidence(
                    received_at,
                    IntentState::ProviderPending,
                    EvidenceSource::ProviderWebhook {
                        event_id: provider_event_id.to_string(),
                    },
                    Some("provider webhook confirmed provider-side pending status".to_string()),
                )?;
                intent.schedule_status_check(received_at, next_resolution_at)?;
                return Ok("provider webhook confirmed provider-side pending status".to_string());
            }

            if intent.state == IntentState::ProviderPending {
                intent.schedule_status_check(received_at, next_resolution_at)?;
                return Ok(already_known_note);
            }

            Ok(format!(
                "provider webhook recorded without state change because current state is {}",
                intent_state_to_api(intent.state)
            ))
        }
    }
}

fn normalize_provider(provider_name: &str) -> String {
    provider_name.trim().to_lowercase()
}

fn webhook_status_to_api(status: ProviderWebhookStatus) -> &'static str {
    match status {
        ProviderWebhookStatus::Pending => "pending",
        ProviderWebhookStatus::Succeeded => "succeeded",
        ProviderWebhookStatus::FailedTerminal => "failed_terminal",
    }
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

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        sync::{Arc, Mutex},
    };

    use super::*;
    use chrono::Utc;
    use domain::{AttemptOutcome, FailureClassification};

    #[derive(Clone, Default)]
    struct FakeRepo {
        intents: Arc<Mutex<HashMap<IntentId, PaymentIntent>>>,
        seen_dedup_hashes: Arc<Mutex<HashSet<String>>>,
    }

    #[async_trait]
    impl ProviderWebhookRepo for FakeRepo {
        async fn find_intent_for_provider_event(
            &self,
            provider_name: &str,
            provider_reference: Option<&str>,
            merchant_reference: Option<&str>,
        ) -> Result<Option<PaymentIntent>, PersistenceError> {
            let intents = self.intents.lock().unwrap();

            Ok(intents
                .values()
                .find(|intent| {
                    intent.provider.0 == provider_name
                        && (provider_reference.is_some()
                            && intent
                                .provider_reference
                                .as_ref()
                                .map(|reference| reference.0.as_str())
                                == provider_reference
                            || merchant_reference.is_some()
                                && Some(intent.merchant_reference.0.as_str()) == merchant_reference)
                })
                .cloned())
        }

        async fn persist_provider_webhook_effect(
            &self,
            input: SaveProviderEventInput,
            intent: Option<&PaymentIntent>,
            _previous_state: Option<IntentState>,
            _audit_event_type: &str,
            _audit_payload: Value,
        ) -> Result<bool, PersistenceError> {
            let mut seen = self.seen_dedup_hashes.lock().unwrap();
            if !seen.insert(input.dedup_hash) {
                return Ok(false);
            }

            if let Some(intent) = intent {
                self.intents
                    .lock()
                    .unwrap()
                    .insert(intent.id, intent.clone());
            }

            Ok(true)
        }
    }

    fn service(repo: FakeRepo) -> ProviderWebhookService<FakeRepo> {
        ProviderWebhookService::new(repo, StdDuration::from_secs(10))
            .with_supported_providers(vec!["mockpay".into()])
    }

    fn unknown_intent(now: DateTime<Utc>) -> PaymentIntent {
        let mut intent = PaymentIntent::new(
            "order_123|#scenario=timeout_after_acceptance",
            "idem_123",
            5000,
            "NGN",
            "mockpay",
            now,
        )
        .unwrap();

        intent.validate(now).unwrap();
        intent.queue(now).unwrap();
        intent.lease(now).unwrap();
        intent.begin_execution(now).unwrap();
        intent
            .finish_current_attempt(
                now,
                AttemptOutcome::UnknownOutcome {
                    classification: FailureClassification::UnknownOutcome,
                    reason: "timeout after provider submit".into(),
                },
                None,
                Some("ambiguous outcome".into()),
            )
            .unwrap();

        intent
    }

    #[tokio::test]
    async fn delayed_success_webhook_resolves_unknown_outcome() {
        let now = Utc::now();
        let intent = unknown_intent(now);
        let repo = FakeRepo {
            intents: Arc::new(Mutex::new(HashMap::from([(intent.id, intent.clone())]))),
            seen_dedup_hashes: Arc::new(Mutex::new(HashSet::new())),
        };

        let result = service(repo.clone())
            .ingest(IngestProviderWebhookCommand {
                provider_name: "mockpay".into(),
                provider_event_id: "evt_success_1".into(),
                provider_reference: Some("mock_ref_1".into()),
                merchant_reference: Some(intent.merchant_reference.0.clone()),
                event_type: "payment.updated".into(),
                status: ProviderWebhookStatus::Succeeded,
                raw_payload: json!({"status":"succeeded"}),
                received_at: now,
            })
            .await
            .unwrap();

        assert!(!result.duplicate);
        assert_eq!(result.state.as_deref(), Some("succeeded"));
        assert_eq!(
            repo.intents.lock().unwrap().get(&intent.id).unwrap().state,
            IntentState::Succeeded
        );
    }

    #[tokio::test]
    async fn duplicate_webhook_is_ignored_after_first_insert() {
        let now = Utc::now();
        let intent = unknown_intent(now);
        let repo = FakeRepo {
            intents: Arc::new(Mutex::new(HashMap::from([(intent.id, intent.clone())]))),
            seen_dedup_hashes: Arc::new(Mutex::new(HashSet::new())),
        };

        let svc = service(repo.clone());
        let first = IngestProviderWebhookCommand {
            provider_name: "mockpay".into(),
            provider_event_id: "evt_duplicate_1".into(),
            provider_reference: Some("mock_ref_2".into()),
            merchant_reference: Some(intent.merchant_reference.0.clone()),
            event_type: "payment.updated".into(),
            status: ProviderWebhookStatus::Succeeded,
            raw_payload: json!({"status":"succeeded"}),
            received_at: now,
        };

        let first_result = svc.ingest(first.clone()).await.unwrap();
        let second_result = svc.ingest(first).await.unwrap();

        assert!(!first_result.duplicate);
        assert!(second_result.duplicate);
        assert_eq!(second_result.state.as_deref(), Some("succeeded"));
        assert_eq!(repo.seen_dedup_hashes.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn out_of_order_terminal_webhook_does_not_regress_success() {
        let now = Utc::now();
        let mut intent = unknown_intent(now);
        intent
            .resolve_unknown_with_evidence(
                now,
                IntentState::Succeeded,
                EvidenceSource::ProviderWebhook {
                    event_id: "evt_success_first".into(),
                },
                Some("provider webhook confirmed success".into()),
            )
            .unwrap();

        let repo = FakeRepo {
            intents: Arc::new(Mutex::new(HashMap::from([(intent.id, intent.clone())]))),
            seen_dedup_hashes: Arc::new(Mutex::new(HashSet::new())),
        };

        let result = service(repo.clone())
            .ingest(IngestProviderWebhookCommand {
                provider_name: "mockpay".into(),
                provider_event_id: "evt_out_of_order_terminal".into(),
                provider_reference: Some("mock_ref_3".into()),
                merchant_reference: Some(intent.merchant_reference.0.clone()),
                event_type: "payment.updated".into(),
                status: ProviderWebhookStatus::FailedTerminal,
                raw_payload: json!({"status":"failed_terminal"}),
                received_at: now,
            })
            .await
            .unwrap();

        assert!(!result.duplicate);
        assert_eq!(result.state.as_deref(), Some("succeeded"));
        assert_eq!(
            repo.intents.lock().unwrap().get(&intent.id).unwrap().state,
            IntentState::Succeeded
        );
    }
}
