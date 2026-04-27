use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use domain::{EvidenceSource, IntentState, PaymentIntent, ProviderReference};
use persistence::{PersistenceError, PostgresPersistence};
use serde::Serialize;
use serde_json::Value;

use crate::{
    ApplicationError, PaymentProviderAdapter, ProviderObservedStatus, ProviderStatusCheckResult,
};

#[async_trait]
pub trait AmbiguityResolutionRepo: Clone + Send + Sync + 'static {
    async fn list_due_resolution_candidates(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<PaymentIntent>, PersistenceError>;

    async fn save_status_check_update(
        &self,
        intent: &PaymentIntent,
        observed_status: Option<&str>,
        raw_summary: Option<Value>,
        note: &str,
    ) -> Result<(), PersistenceError>;
}

#[async_trait]
impl AmbiguityResolutionRepo for PostgresPersistence {
    async fn list_due_resolution_candidates(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<PaymentIntent>, PersistenceError> {
        PostgresPersistence::list_due_resolution_candidates(self, now, limit).await
    }

    async fn save_status_check_update(
        &self,
        intent: &PaymentIntent,
        observed_status: Option<&str>,
        raw_summary: Option<Value>,
        note: &str,
    ) -> Result<(), PersistenceError> {
        PostgresPersistence::save_status_check_update(
            self,
            intent,
            observed_status,
            raw_summary,
            note,
        )
        .await
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusCheckSummary {
    pub intent_id: uuid::Uuid,
    pub state: String,
    pub provider_reference: Option<String>,
    pub next_resolution_at: Option<DateTime<Utc>>,
    pub resolution_attempt_count: u32,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct UnknownOutcomeResolutionService<R, P>
where
    R: AmbiguityResolutionRepo,
    P: PaymentProviderAdapter,
{
    repo: R,
    provider: P,
    check_delay: StdDuration,
    max_attempts_before_manual_review: u32,
}

impl<R, P> UnknownOutcomeResolutionService<R, P>
where
    R: AmbiguityResolutionRepo,
    P: PaymentProviderAdapter,
{
    pub fn new(
        repo: R,
        provider: P,
        check_delay: StdDuration,
        max_attempts_before_manual_review: u32,
    ) -> Self {
        Self {
            repo,
            provider,
            check_delay,
            max_attempts_before_manual_review,
        }
    }

    pub async fn process_due_candidates(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<StatusCheckSummary>, ApplicationError> {
        let candidates = self.repo.list_due_resolution_candidates(now, limit).await?;
        let mut results = Vec::with_capacity(candidates.len());

        for candidate in candidates {
            results.push(self.process_one(candidate, now).await?);
        }

        Ok(results)
    }

    async fn process_one(
        &self,
        mut intent: PaymentIntent,
        now: DateTime<Utc>,
    ) -> Result<StatusCheckSummary, ApplicationError> {
        intent.record_status_check_attempt(now)?;

        let status_result = self.provider.query_payment_status(&intent).await?;

        let (observed_status, raw_summary, note) = match status_result {
            ProviderStatusCheckResult::Observed {
                provider_reference,
                observed_status,
                raw_summary,
                note,
            } => {
                if let Some(provider_reference) = provider_reference {
                    intent.provider_reference = Some(ProviderReference(provider_reference));
                }

                match observed_status {
                    ProviderObservedStatus::Succeeded => {
                        intent.resolve_unknown_with_evidence(
                            now,
                            IntentState::Succeeded,
                            EvidenceSource::ProviderStatusCheck { checked_at: now },
                            Some("status check confirmed provider success".to_string()),
                        )?;
                    }
                    ProviderObservedStatus::FailedTerminal => {
                        intent.resolve_unknown_with_evidence(
                            now,
                            IntentState::FailedTerminal,
                            EvidenceSource::ProviderStatusCheck { checked_at: now },
                            Some("status check confirmed terminal provider failure".to_string()),
                        )?;
                    }
                    ProviderObservedStatus::Pending => {
                        if intent.state == IntentState::UnknownOutcome {
                            intent.resolve_unknown_with_evidence(
                                now,
                                IntentState::ProviderPending,
                                EvidenceSource::ProviderStatusCheck { checked_at: now },
                                Some(
                                    "status check confirmed provider still has the intent pending"
                                        .to_string(),
                                ),
                            )?;
                        }

                        if intent.resolution_attempt_count >= self.max_attempts_before_manual_review
                        {
                            intent.resolve_unknown_with_evidence(
                                now,
                                IntentState::ManualReview,
                                EvidenceSource::ProviderStatusCheck { checked_at: now },
                                Some(
                                    "pending status persisted beyond maximum status check attempts; hand off to manual review or reconciliation".to_string()
                                )
                            )?;
                        } else {
                            let next_at = now
                                + Duration::from_std(self.check_delay).map_err(|_| {
                                    ApplicationError::Validation(
                                        "invalid status check delay".to_string(),
                                    )
                                })?;
                            intent.schedule_status_check(now, next_at)?;
                        }
                    }
                }

                (Some(observed_status), Some(raw_summary), note)
            }
            ProviderStatusCheckResult::RetryableTransportError {
                reason,
                raw_summary,
            } => {
                if intent.resolution_attempt_count >= self.max_attempts_before_manual_review {
                    intent.resolve_unknown_with_evidence(
                        now,
                        IntentState::ManualReview,
                        EvidenceSource::ProviderStatusCheck { checked_at: now },
                        Some(
                            "status checks remained inconclusive beyond maximum attempts; hand off to manual review or reconciliation".to_string()
                        )
                    )?;
                } else {
                    let next_at = now
                        + Duration::from_std(self.check_delay).map_err(|_| {
                            ApplicationError::Validation("invalid status check delay".to_string())
                        })?;
                    intent.schedule_status_check(now, next_at)?;
                }

                (None, raw_summary, reason)
            }
            ProviderStatusCheckResult::NotFound { raw_summary, note } => {
                if intent.resolution_attempt_count >= self.max_attempts_before_manual_review {
                    intent.resolve_unknown_with_evidence(
                        now,
                        IntentState::ManualReview,
                        EvidenceSource::ProviderStatusCheck { checked_at: now },
                        Some(
                            "provider lookup remained missing beyond maximum attempts; hand off to manual review or reconciliation".to_string(),
                        ),
                    )?;
                } else {
                    let next_at = now
                        + Duration::from_std(self.check_delay).map_err(|_| {
                            ApplicationError::Validation("invalid status check delay".to_string())
                        })?;
                    intent.schedule_status_check(now, next_at)?;
                }

                (None, raw_summary, note)
            }
        };

        self.repo
            .save_status_check_update(
                &intent,
                observed_status.as_ref().map(observed_status_to_str),
                raw_summary,
                &note,
            )
            .await?;

        Ok(StatusCheckSummary {
            intent_id: intent.id,
            state: format!("{:?}", intent.state),
            provider_reference: intent.provider_reference.as_ref().map(|p| p.0.clone()),
            next_resolution_at: intent.next_resolution_at,
            resolution_attempt_count: intent.resolution_attempt_count,
            note,
        })
    }
}

fn observed_status_to_str(status: &ProviderObservedStatus) -> &'static str {
    match status {
        ProviderObservedStatus::Succeeded => "succeeded",
        ProviderObservedStatus::FailedTerminal => "failed_terminal",
        ProviderObservedStatus::Pending => "pending",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeRepo {
        due: Arc<Mutex<Vec<PaymentIntent>>>,
        saved: Arc<Mutex<Vec<(uuid::Uuid, Option<String>, String)>>>,
    }

    #[async_trait]
    impl AmbiguityResolutionRepo for FakeRepo {
        async fn list_due_resolution_candidates(
            &self,
            _now: DateTime<Utc>,
            _limit: i64,
        ) -> Result<Vec<PaymentIntent>, PersistenceError> {
            Ok(self.due.lock().unwrap().clone())
        }

        async fn save_status_check_update(
            &self,
            intent: &PaymentIntent,
            observed_status: Option<&str>,
            _raw_summary: Option<Value>,
            note: &str,
        ) -> Result<(), PersistenceError> {
            self.saved.lock().unwrap().push((
                intent.id,
                observed_status.map(|s| s.to_string()),
                note.to_string(),
            ));
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
                domain::AttemptOutcome::UnknownOutcome {
                    classification: domain::FailureClassification::UnknownOutcome,
                    reason: "timeout after submission".into(),
                },
                None,
                Some("ambiguous".into()),
            )
            .unwrap();
        intent.schedule_status_check(now, now).unwrap();
        intent
    }

    #[tokio::test]
    async fn status_check_can_resolve_unknown_to_success() {
        let now = Utc::now();
        let repo = FakeRepo {
            due: Arc::new(Mutex::new(vec![unknown_intent(now)])),
            saved: Arc::new(Mutex::new(vec![])),
        };

        let provider = FakeProvider {
            result: ProviderStatusCheckResult::Observed {
                provider_reference: Some("mock_ref_1".into()),
                observed_status: ProviderObservedStatus::Succeeded,
                raw_summary: serde_json::json!({"status":"succeeded"}),
                note: "provider says succeeded".into(),
            },
        };

        let svc = UnknownOutcomeResolutionService::new(
            repo.clone(),
            provider,
            StdDuration::from_secs(10),
            3,
        );

        let results = svc.process_due_candidates(now, 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].state, "Succeeded");
    }

    #[tokio::test]
    async fn pending_status_keeps_follow_up_scheduled() {
        let now = Utc::now();
        let repo = FakeRepo {
            due: Arc::new(Mutex::new(vec![unknown_intent(now)])),
            saved: Arc::new(Mutex::new(vec![])),
        };

        let provider = FakeProvider {
            result: ProviderStatusCheckResult::Observed {
                provider_reference: Some("mock_ref_1".into()),
                observed_status: ProviderObservedStatus::Pending,
                raw_summary: serde_json::json!({"status":"pending"}),
                note: "provider says pending".into(),
            },
        };

        let svc = UnknownOutcomeResolutionService::new(
            repo.clone(),
            provider,
            StdDuration::from_secs(10),
            3,
        );

        let results = svc.process_due_candidates(now, 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].state, "ProviderPending");
        assert!(results[0].next_resolution_at.is_some());
    }

    #[tokio::test]
    async fn repeated_inconclusive_checks_escalate_to_manual_review() {
        let now = Utc::now();
        let mut intent = unknown_intent(now);
        intent.resolution_attempt_count = 2;

        let repo = FakeRepo {
            due: Arc::new(Mutex::new(vec![intent])),
            saved: Arc::new(Mutex::new(vec![])),
        };

        let provider = FakeProvider {
            result: ProviderStatusCheckResult::RetryableTransportError {
                reason: "status check timed out".into(),
                raw_summary: None,
            },
        };

        let svc = UnknownOutcomeResolutionService::new(
            repo.clone(),
            provider,
            StdDuration::from_secs(10),
            3,
        );

        let results = svc.process_due_candidates(now, 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].state, "ManualReview");
    }
}
