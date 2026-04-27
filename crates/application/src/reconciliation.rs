use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::{
    EvidenceSource, IntentId, IntentState, PaymentIntent, ProviderReference, ReconComparison,
    ReconDecision, ReconResult,
};
use persistence::{PersistenceError, PostgresPersistence, SaveReconciliationRunInput};
use serde::Serialize;
use serde_json::Value;

use crate::{
    ApplicationError, PaymentProviderAdapter, ProviderObservedStatus, ProviderStatusCheckResult,
};

#[async_trait]
pub trait ReconciliationRepo: Clone + Send + Sync + 'static {
    async fn get_intent_by_id(
        &self,
        intent_id: IntentId,
    ) -> Result<PaymentIntent, PersistenceError>;

    async fn save_reconciliation_run(
        &self,
        intent: &PaymentIntent,
        input: SaveReconciliationRunInput,
    ) -> Result<(), PersistenceError>;
}

#[async_trait]
impl ReconciliationRepo for PostgresPersistence {
    async fn get_intent_by_id(
        &self,
        intent_id: IntentId,
    ) -> Result<PaymentIntent, PersistenceError> {
        PostgresPersistence::get_intent_by_id(self, intent_id).await
    }

    async fn save_reconciliation_run(
        &self,
        intent: &PaymentIntent,
        input: SaveReconciliationRunInput,
    ) -> Result<(), PersistenceError> {
        PostgresPersistence::save_reconciliation_run(self, intent, input).await
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconciliationSummary {
    pub intent_id: uuid::Uuid,
    pub previous_state: String,
    pub state: String,
    pub provider_reference: Option<String>,
    pub provider_state_seen: String,
    pub comparison: String,
    pub decision: String,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct ReconciliationService<R, P>
where
    R: ReconciliationRepo,
    P: PaymentProviderAdapter,
{
    repo: R,
    provider: P,
}

impl<R, P> ReconciliationService<R, P>
where
    R: ReconciliationRepo,
    P: PaymentProviderAdapter,
{
    pub fn new(repo: R, provider: P) -> Self {
        Self { repo, provider }
    }

    pub async fn reconcile_selected_intents(
        &self,
        intent_ids: Vec<IntentId>,
    ) -> Result<Vec<ReconciliationSummary>, ApplicationError> {
        let mut results = Vec::with_capacity(intent_ids.len());

        for intent_id in intent_ids {
            results.push(self.reconcile_intent(intent_id).await?);
        }

        Ok(results)
    }

    pub async fn reconcile_intent(
        &self,
        intent_id: IntentId,
    ) -> Result<ReconciliationSummary, ApplicationError> {
        let mut intent = self.repo.get_intent_by_id(intent_id).await?;
        let previous_state = intent.state;
        let started_at = Utc::now();

        intent.begin_reconciliation(started_at)?;

        let observation = self.observe_provider_truth(&intent).await?;
        let ended_at = Utc::now();

        if let Some(provider_reference) = observation.provider_reference() {
            intent.provider_reference = Some(ProviderReference(provider_reference.to_string()));
        }

        let result = build_reconciliation_result(previous_state, &intent, &observation, ended_at);
        let note = result
            .note
            .clone()
            .unwrap_or_else(|| "reconciliation completed".to_string());

        intent.apply_reconciliation(result.clone(), ended_at)?;

        self.repo
            .save_reconciliation_run(
                &intent,
                SaveReconciliationRunInput {
                    intent_id,
                    started_at,
                    ended_at,
                    provider_status_seen: result.provider_state.clone(),
                    raw_provider_summary: observation.raw_summary().cloned(),
                    internal_status_seen: previous_state,
                    comparison: result.comparison,
                    decision: result.decision,
                    evidence: result.evidence.clone(),
                    note: result.note.clone(),
                },
            )
            .await?;

        Ok(ReconciliationSummary {
            intent_id,
            previous_state: state_to_api(previous_state).to_string(),
            state: state_to_api(intent.state).to_string(),
            provider_reference: intent
                .provider_reference
                .as_ref()
                .map(|value| value.0.clone()),
            provider_state_seen: result.provider_state,
            comparison: recon_comparison_to_api(result.comparison).to_string(),
            decision: recon_decision_to_api(result.decision).to_string(),
            note,
        })
    }

    async fn observe_provider_truth(
        &self,
        intent: &PaymentIntent,
    ) -> Result<ProviderTruthObservation, ApplicationError> {
        match self.provider.query_payment_status(intent).await? {
            ProviderStatusCheckResult::Observed {
                provider_reference,
                observed_status,
                raw_summary,
                note,
            } => Ok(ProviderTruthObservation::Observed {
                provider_reference,
                observed_status,
                raw_summary,
                note,
            }),
            ProviderStatusCheckResult::NotFound { raw_summary, note } => {
                Ok(ProviderTruthObservation::Missing { raw_summary, note })
            }
            ProviderStatusCheckResult::RetryableTransportError {
                reason,
                raw_summary,
            } => Ok(ProviderTruthObservation::Unavailable {
                raw_summary,
                note: reason,
            }),
        }
    }
}

#[derive(Debug, Clone)]
enum ProviderTruthObservation {
    Observed {
        provider_reference: Option<String>,
        observed_status: ProviderObservedStatus,
        raw_summary: Value,
        note: String,
    },
    Missing {
        raw_summary: Option<Value>,
        note: String,
    },
    Unavailable {
        raw_summary: Option<Value>,
        note: String,
    },
}

impl ProviderTruthObservation {
    fn provider_reference(&self) -> Option<&str> {
        match self {
            Self::Observed {
                provider_reference: Some(provider_reference),
                ..
            } => Some(provider_reference.as_str()),
            _ => None,
        }
    }

    fn raw_summary(&self) -> Option<&Value> {
        match self {
            Self::Observed { raw_summary, .. } => Some(raw_summary),
            Self::Missing { raw_summary, .. } | Self::Unavailable { raw_summary, .. } => {
                raw_summary.as_ref()
            }
        }
    }
}

fn build_reconciliation_result(
    previous_state: IntentState,
    intent: &PaymentIntent,
    observation: &ProviderTruthObservation,
    compared_at: DateTime<Utc>,
) -> ReconResult {
    let evidence = EvidenceSource::ProviderStatusCheck {
        checked_at: compared_at,
    };

    match previous_state {
        IntentState::UnknownOutcome | IntentState::ProviderPending | IntentState::ManualReview => {
            match observation {
                ProviderTruthObservation::Observed {
                    observed_status: ProviderObservedStatus::Succeeded,
                    note,
                    ..
                } => ReconResult {
                    compared_at,
                    internal_state: previous_state,
                    provider_state: "succeeded".to_string(),
                    comparison: ReconComparison::Match,
                    decision: ReconDecision::ConfirmSucceeded,
                    evidence,
                    note: Some(format!(
                        "reconciliation confirmed provider success: {note}"
                    )),
                },
                ProviderTruthObservation::Observed {
                    observed_status: ProviderObservedStatus::FailedTerminal,
                    note,
                    ..
                } => ReconResult {
                    compared_at,
                    internal_state: previous_state,
                    provider_state: "failed_terminal".to_string(),
                    comparison: ReconComparison::Match,
                    decision: ReconDecision::ConfirmFailedTerminal,
                    evidence,
                    note: Some(format!(
                        "reconciliation confirmed provider terminal failure: {note}"
                    )),
                },
                ProviderTruthObservation::Observed {
                    observed_status: ProviderObservedStatus::Pending,
                    note,
                    ..
                } => ReconResult {
                    compared_at,
                    internal_state: previous_state,
                    provider_state: "pending".to_string(),
                    comparison: ReconComparison::Unresolved,
                    decision: ReconDecision::KeepUnknown,
                    evidence,
                    note: Some(format!(
                        "reconciliation observed provider still pending: {note}"
                    )),
                },
                ProviderTruthObservation::Missing { note, .. } if intent.provider_reference.is_some() => {
                    ReconResult {
                        compared_at,
                        internal_state: previous_state,
                        provider_state: "missing".to_string(),
                        comparison: ReconComparison::Mismatch,
                        decision: ReconDecision::EscalateManualReview,
                        evidence,
                        note: Some(format!(
                            "reconciliation could not find the provider record for a known provider reference: {note}"
                        )),
                    }
                }
                ProviderTruthObservation::Missing { note, .. } => ReconResult {
                    compared_at,
                    internal_state: previous_state,
                    provider_state: "missing".to_string(),
                    comparison: ReconComparison::Unresolved,
                    decision: ReconDecision::KeepUnknown,
                    evidence,
                    note: Some(format!(
                        "reconciliation could not find the provider-side record; intent remains unresolved: {note}"
                    )),
                },
                ProviderTruthObservation::Unavailable { note, .. } => ReconResult {
                    compared_at,
                    internal_state: previous_state,
                    provider_state: "unavailable".to_string(),
                    comparison: ReconComparison::Unresolved,
                    decision: if previous_state == IntentState::ManualReview {
                        ReconDecision::EscalateManualReview
                    } else {
                        ReconDecision::KeepUnknown
                    },
                    evidence,
                    note: Some(format!(
                        "provider truth was unavailable during reconciliation: {note}"
                    )),
                },
            }
        }
        IntentState::Succeeded => match observation {
            ProviderTruthObservation::Observed {
                observed_status: ProviderObservedStatus::Succeeded,
                note,
                ..
            } => ReconResult {
                compared_at,
                internal_state: previous_state,
                provider_state: "succeeded".to_string(),
                comparison: ReconComparison::Match,
                decision: ReconDecision::ConfirmSucceeded,
                evidence,
                note: Some(format!(
                    "reconciliation confirmed the already-succeeded intent: {note}"
                )),
            },
            ProviderTruthObservation::Observed {
                observed_status: ProviderObservedStatus::FailedTerminal,
                note,
                ..
            } => mismatch_manual_review(compared_at, previous_state, "failed_terminal", evidence, format!(
                "provider reported terminal failure for an internally succeeded intent: {note}"
            )),
            ProviderTruthObservation::Observed {
                observed_status: ProviderObservedStatus::Pending,
                note,
                ..
            } => mismatch_manual_review(compared_at, previous_state, "pending", evidence, format!(
                "provider still reports pending for an internally succeeded intent: {note}"
            )),
            ProviderTruthObservation::Missing { note, .. } => mismatch_manual_review(
                compared_at,
                previous_state,
                "missing",
                evidence,
                format!("provider record is missing for an internally succeeded intent: {note}"),
            ),
            ProviderTruthObservation::Unavailable { note, .. } => ReconResult {
                compared_at,
                internal_state: previous_state,
                provider_state: "unavailable".to_string(),
                comparison: ReconComparison::Unresolved,
                decision: ReconDecision::EscalateManualReview,
                evidence,
                note: Some(format!(
                    "provider truth was unavailable while reconciling an internally succeeded intent: {note}"
                )),
            },
        },
        IntentState::FailedTerminal => match observation {
            ProviderTruthObservation::Observed {
                observed_status: ProviderObservedStatus::FailedTerminal,
                note,
                ..
            } => ReconResult {
                compared_at,
                internal_state: previous_state,
                provider_state: "failed_terminal".to_string(),
                comparison: ReconComparison::Match,
                decision: ReconDecision::ConfirmFailedTerminal,
                evidence,
                note: Some(format!(
                    "reconciliation confirmed the already-failed intent: {note}"
                )),
            },
            ProviderTruthObservation::Observed {
                observed_status: ProviderObservedStatus::Succeeded,
                note,
                ..
            } => mismatch_manual_review(compared_at, previous_state, "succeeded", evidence, format!(
                "provider reported success for an internally failed intent: {note}"
            )),
            ProviderTruthObservation::Observed {
                observed_status: ProviderObservedStatus::Pending,
                note,
                ..
            } => mismatch_manual_review(compared_at, previous_state, "pending", evidence, format!(
                "provider still reports pending for an internally failed intent: {note}"
            )),
            ProviderTruthObservation::Missing { note, .. } => mismatch_manual_review(
                compared_at,
                previous_state,
                "missing",
                evidence,
                format!("provider record is missing for an internally failed intent: {note}"),
            ),
            ProviderTruthObservation::Unavailable { note, .. } => ReconResult {
                compared_at,
                internal_state: previous_state,
                provider_state: "unavailable".to_string(),
                comparison: ReconComparison::Unresolved,
                decision: ReconDecision::EscalateManualReview,
                evidence,
                note: Some(format!(
                    "provider truth was unavailable while reconciling an internally failed intent: {note}"
                )),
            },
        },
        other => mismatch_manual_review(
            compared_at,
            other,
            "unsupported".to_string(),
            evidence,
            format!(
                "reconciliation was requested for unsupported state {}",
                state_to_api(other)
            ),
        ),
    }
}

fn mismatch_manual_review(
    compared_at: DateTime<Utc>,
    internal_state: IntentState,
    provider_state: impl Into<String>,
    evidence: EvidenceSource,
    note: String,
) -> ReconResult {
    ReconResult {
        compared_at,
        internal_state,
        provider_state: provider_state.into(),
        comparison: ReconComparison::Mismatch,
        decision: ReconDecision::EscalateManualReview,
        evidence,
        note: Some(note),
    }
}

fn state_to_api(state: IntentState) -> &'static str {
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

fn recon_comparison_to_api(comparison: ReconComparison) -> &'static str {
    match comparison {
        ReconComparison::Match => "match",
        ReconComparison::Mismatch => "mismatch",
        ReconComparison::Unresolved => "unresolved",
    }
}

fn recon_decision_to_api(decision: ReconDecision) -> &'static str {
    match decision {
        ReconDecision::ConfirmSucceeded => "confirm_succeeded",
        ReconDecision::ConfirmFailedTerminal => "confirm_failed_terminal",
        ReconDecision::KeepUnknown => "keep_unknown",
        ReconDecision::EscalateManualReview => "escalate_manual_review",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use domain::{AttemptOutcome, FailureClassification};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeRepo {
        intents: Arc<Mutex<HashMap<uuid::Uuid, PaymentIntent>>>,
        saves: Arc<Mutex<Vec<(uuid::Uuid, PaymentIntent, SaveReconciliationRunInput)>>>,
    }

    #[async_trait]
    impl ReconciliationRepo for FakeRepo {
        async fn get_intent_by_id(
            &self,
            intent_id: IntentId,
        ) -> Result<PaymentIntent, PersistenceError> {
            self.intents
                .lock()
                .unwrap()
                .get(&intent_id)
                .cloned()
                .ok_or(PersistenceError::IntentNotFound(intent_id))
        }

        async fn save_reconciliation_run(
            &self,
            intent: &PaymentIntent,
            input: SaveReconciliationRunInput,
        ) -> Result<(), PersistenceError> {
            self.saves
                .lock()
                .unwrap()
                .push((intent.id, intent.clone(), input));
            self.intents
                .lock()
                .unwrap()
                .insert(intent.id, intent.clone());
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FakeProvider {
        result: ProviderStatusCheckResult,
    }

    #[async_trait]
    impl PaymentProviderAdapter for FakeProvider {
        async fn submit_payment(
            &self,
            _intent: &PaymentIntent,
        ) -> Result<crate::ProviderSubmitResult, ApplicationError> {
            unreachable!()
        }

        async fn query_payment_status(
            &self,
            _intent: &PaymentIntent,
        ) -> Result<ProviderStatusCheckResult, ApplicationError> {
            Ok(self.result.clone())
        }
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
                    reason: "timeout after submission".into(),
                },
                None,
                Some("ambiguous".into()),
            )
            .unwrap();
        intent
    }

    fn pending_intent(now: DateTime<Utc>) -> PaymentIntent {
        let mut intent = PaymentIntent::new(
            "order_124|#scenario=pending_then_resolves",
            "idem_124",
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
                AttemptOutcome::ProviderPending,
                Some("mock_ref_pending".into()),
                Some("pending".into()),
            )
            .unwrap();
        intent
    }

    fn succeeded_intent(now: DateTime<Utc>) -> PaymentIntent {
        let mut intent = PaymentIntent::new(
            "order_125|#scenario=immediate_success",
            "idem_125",
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
                AttemptOutcome::Succeeded,
                Some("mock_ref_success".into()),
                Some("succeeded".into()),
            )
            .unwrap();
        intent
    }

    fn service(
        repo: FakeRepo,
        provider: FakeProvider,
    ) -> ReconciliationService<FakeRepo, FakeProvider> {
        ReconciliationService::new(repo, provider)
    }

    #[tokio::test]
    async fn unknown_outcome_plus_provider_success_confirms_success() {
        let now = Utc::now();
        let intent = unknown_intent(now);
        let intent_id = intent.id;

        let repo = FakeRepo::default();
        repo.intents.lock().unwrap().insert(intent_id, intent);

        let result = service(
            repo.clone(),
            FakeProvider {
                result: ProviderStatusCheckResult::Observed {
                    provider_reference: Some("mock_ref_success".into()),
                    observed_status: ProviderObservedStatus::Succeeded,
                    raw_summary: serde_json::json!({"status":"succeeded"}),
                    note: "provider says succeeded".into(),
                },
            },
        )
        .reconcile_intent(intent_id)
        .await
        .unwrap();

        assert_eq!(result.comparison, "match");
        assert_eq!(result.decision, "confirm_succeeded");
        assert_eq!(result.state, "reconciled");
    }

    #[tokio::test]
    async fn succeeded_plus_provider_missing_escalates_manual_review() {
        let now = Utc::now();
        let intent = succeeded_intent(now);
        let intent_id = intent.id;

        let repo = FakeRepo::default();
        repo.intents.lock().unwrap().insert(intent_id, intent);

        let result = service(
            repo.clone(),
            FakeProvider {
                result: ProviderStatusCheckResult::NotFound {
                    raw_summary: Some(serde_json::json!({"http_status":404})),
                    note: "provider returned not found".into(),
                },
            },
        )
        .reconcile_intent(intent_id)
        .await
        .unwrap();

        assert_eq!(result.comparison, "mismatch");
        assert_eq!(result.decision, "escalate_manual_review");
        assert_eq!(result.state, "manual_review");
    }

    #[tokio::test]
    async fn provider_pending_plus_provider_failure_confirms_failure() {
        let now = Utc::now();
        let intent = pending_intent(now);
        let intent_id = intent.id;

        let repo = FakeRepo::default();
        repo.intents.lock().unwrap().insert(intent_id, intent);

        let result = service(
            repo.clone(),
            FakeProvider {
                result: ProviderStatusCheckResult::Observed {
                    provider_reference: Some("mock_ref_pending".into()),
                    observed_status: ProviderObservedStatus::FailedTerminal,
                    raw_summary: serde_json::json!({"status":"failed_terminal"}),
                    note: "provider says failed".into(),
                },
            },
        )
        .reconcile_intent(intent_id)
        .await
        .unwrap();

        assert_eq!(result.decision, "confirm_failed_terminal");
        assert_eq!(result.state, "reconciled");
    }

    #[tokio::test]
    async fn provider_pending_plus_provider_pending_stays_pending() {
        let now = Utc::now();
        let intent = pending_intent(now);
        let intent_id = intent.id;

        let repo = FakeRepo::default();
        repo.intents.lock().unwrap().insert(intent_id, intent);

        let result = service(
            repo.clone(),
            FakeProvider {
                result: ProviderStatusCheckResult::Observed {
                    provider_reference: Some("mock_ref_pending".into()),
                    observed_status: ProviderObservedStatus::Pending,
                    raw_summary: serde_json::json!({"status":"pending"}),
                    note: "provider still pending".into(),
                },
            },
        )
        .reconcile_intent(intent_id)
        .await
        .unwrap();

        assert_eq!(result.comparison, "unresolved");
        assert_eq!(result.decision, "keep_unknown");
        assert_eq!(result.state, "provider_pending");
    }
}
