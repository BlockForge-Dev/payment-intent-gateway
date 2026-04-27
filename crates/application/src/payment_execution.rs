use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use domain::{AttemptOutcome, FailureClassification, PaymentIntent};
use persistence::{LeasedPaymentIntent, PersistenceError, PostgresPersistence};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::ApplicationError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderObservedStatus {
    Succeeded,
    FailedTerminal,
    Pending,
}

#[derive(Debug, Clone)]
pub enum ProviderSubmitResult {
    Accepted {
        provider_reference: Option<String>,
        observed_status: ProviderObservedStatus,
        raw_summary: Value,
        note: String,
    },
    RetryableTransportError {
        reason: String,
        raw_summary: Option<Value>,
    },
    TimeoutUnknown {
        reason: String,
        raw_summary: Option<Value>,
    },
}

#[derive(Debug, Clone)]
pub enum ProviderStatusCheckResult {
    Observed {
        provider_reference: Option<String>,
        observed_status: ProviderObservedStatus,
        raw_summary: Value,
        note: String,
    },
    NotFound {
        raw_summary: Option<Value>,
        note: String,
    },
    RetryableTransportError {
        reason: String,
        raw_summary: Option<Value>,
    },
}

#[async_trait]
pub trait PaymentProviderAdapter: Clone + Send + Sync + 'static {
    async fn submit_payment(
        &self,
        intent: &PaymentIntent,
    ) -> Result<ProviderSubmitResult, ApplicationError>;

    async fn query_payment_status(
        &self,
        intent: &PaymentIntent,
    ) -> Result<ProviderStatusCheckResult, ApplicationError>;
}

#[async_trait]
pub trait ExecutionAttemptRepo: Clone + Send + Sync + 'static {
    async fn save_attempt_started_from_lease(
        &self,
        intent: &PaymentIntent,
        lease_token: Uuid,
        request_payload_snapshot: Value,
    ) -> Result<(), PersistenceError>;

    async fn save_attempt_finished(
        &self,
        intent: &PaymentIntent,
        raw_provider_response_summary: Option<Value>,
        retry_available_at: Option<DateTime<Utc>>,
    ) -> Result<(), PersistenceError>;
}

#[async_trait]
impl ExecutionAttemptRepo for PostgresPersistence {
    async fn save_attempt_started_from_lease(
        &self,
        intent: &PaymentIntent,
        lease_token: Uuid,
        request_payload_snapshot: Value,
    ) -> Result<(), PersistenceError> {
        PostgresPersistence::save_attempt_started_from_lease(
            self,
            intent,
            lease_token,
            request_payload_snapshot,
        )
        .await
    }

    async fn save_attempt_finished(
        &self,
        intent: &PaymentIntent,
        raw_provider_response_summary: Option<Value>,
        retry_available_at: Option<DateTime<Utc>>,
    ) -> Result<(), PersistenceError> {
        PostgresPersistence::save_attempt_finished(
            self,
            intent,
            raw_provider_response_summary,
            retry_available_at,
        )
        .await
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionAttemptSummary {
    pub intent_id: uuid::Uuid,
    pub state: String,
    pub provider_reference: Option<String>,
    pub retry_available_at: Option<DateTime<Utc>>,
    pub next_resolution_at: Option<DateTime<Utc>>,
    pub outcome_note: String,
}

#[derive(Debug, Clone)]
pub struct ExecutionAttemptService<R, P>
where
    R: ExecutionAttemptRepo,
    P: PaymentProviderAdapter,
{
    repo: R,
    provider: P,
    retry_delay: StdDuration,
    status_check_delay: StdDuration,
}

impl<R, P> ExecutionAttemptService<R, P>
where
    R: ExecutionAttemptRepo,
    P: PaymentProviderAdapter,
{
    pub fn new(
        repo: R,
        provider: P,
        retry_delay: StdDuration,
        status_check_delay: StdDuration,
    ) -> Self {
        Self {
            repo,
            provider,
            retry_delay,
            status_check_delay,
        }
    }

    pub async fn execute_leased_intent(
        &self,
        leased: LeasedPaymentIntent,
        now: DateTime<Utc>,
    ) -> Result<ExecutionAttemptSummary, ApplicationError> {
        let mut intent = leased.intent.clone();
        intent.begin_execution(now)?;

        let request_payload_snapshot = build_request_payload_snapshot(&intent);
        self.repo
            .save_attempt_started_from_lease(&intent, leased.lease_token, request_payload_snapshot)
            .await?;

        let provider_result = self.provider.submit_payment(&intent).await?;

        let (
            outcome,
            provider_reference,
            raw_summary,
            retry_available_at,
            next_resolution_at,
            outcome_note,
        ) = classify_provider_result(
            provider_result,
            now,
            self.retry_delay,
            self.status_check_delay,
        )?;

        intent.finish_current_attempt(
            now,
            outcome,
            provider_reference.clone(),
            Some(outcome_note.clone()),
        )?;

        if let Some(next_resolution_at) = next_resolution_at {
            intent.schedule_status_check(now, next_resolution_at)?;
        }

        self.repo
            .save_attempt_finished(&intent, raw_summary, retry_available_at)
            .await?;

        Ok(ExecutionAttemptSummary {
            intent_id: intent.id,
            state: format!("{:?}", intent.state),
            provider_reference,
            retry_available_at,
            next_resolution_at: intent.next_resolution_at,
            outcome_note,
        })
    }
}

fn classify_provider_result(
    result: ProviderSubmitResult,
    now: DateTime<Utc>,
    retry_delay: StdDuration,
    status_check_delay: StdDuration,
) -> Result<
    (
        AttemptOutcome,
        Option<String>,
        Option<Value>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        String,
    ),
    ApplicationError,
> {
    match result {
        ProviderSubmitResult::Accepted {
            provider_reference,
            observed_status,
            raw_summary,
            note,
        } => match observed_status {
            ProviderObservedStatus::Succeeded => Ok((
                AttemptOutcome::Succeeded,
                provider_reference,
                Some(raw_summary),
                None,
                None,
                note,
            )),
            ProviderObservedStatus::FailedTerminal => Ok((
                AttemptOutcome::TerminalFailure {
                    classification: FailureClassification::TerminalProvider,
                    reason: note.clone(),
                },
                provider_reference,
                Some(raw_summary),
                None,
                None,
                note,
            )),
            ProviderObservedStatus::Pending => {
                let next_resolution_at = now
                    + Duration::from_std(status_check_delay).map_err(|_| {
                        ApplicationError::Validation("invalid status check delay".to_string())
                    })?;

                Ok((
                    AttemptOutcome::ProviderPending,
                    provider_reference,
                    Some(raw_summary),
                    None,
                    Some(next_resolution_at),
                    note,
                ))
            }
        },
        ProviderSubmitResult::RetryableTransportError {
            reason,
            raw_summary,
        } => {
            let retry_available_at = now
                + Duration::from_std(retry_delay)
                    .map_err(|_| ApplicationError::Validation("invalid retry delay".to_string()))?;

            Ok((
                AttemptOutcome::RetryableFailure {
                    classification: FailureClassification::RetryableInfrastructure,
                    reason: reason.clone(),
                },
                None,
                raw_summary,
                Some(retry_available_at),
                None,
                reason,
            ))
        }
        ProviderSubmitResult::TimeoutUnknown {
            reason,
            raw_summary,
        } => {
            let next_resolution_at = now
                + Duration::from_std(status_check_delay).map_err(|_| {
                    ApplicationError::Validation("invalid status check delay".to_string())
                })?;

            Ok((
                AttemptOutcome::UnknownOutcome {
                    classification: FailureClassification::UnknownOutcome,
                    reason: reason.clone(),
                },
                None,
                raw_summary,
                None,
                Some(next_resolution_at),
                reason,
            ))
        }
    }
}

fn build_request_payload_snapshot(intent: &PaymentIntent) -> Value {
    let directives = infer_mock_intent_directives(&intent.merchant_reference.0);
    json!({
        "provider": intent.provider.0,
        "merchant_reference": intent.merchant_reference.0,
        "amount_minor": intent.money.amount_minor,
        "currency": intent.money.currency,
        "mock_scenario": directives.scenario,
        "provider_webhook_enabled": directives.provider_webhook_enabled,
        "resolution_delay_ms": directives.resolution_delay_ms,
        "timeout_response_delay_ms": directives.timeout_response_delay_ms,
    })
}

pub fn infer_mock_scenario(merchant_reference: &str) -> String {
    infer_mock_intent_directives(merchant_reference).scenario
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MockIntentDirectives {
    scenario: String,
    resolution_delay_ms: Option<u64>,
    timeout_response_delay_ms: Option<u64>,
    provider_webhook_enabled: bool,
}

fn infer_mock_intent_directives(merchant_reference: &str) -> MockIntentDirectives {
    let scenario = merchant_reference_directive(merchant_reference, "scenario")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "immediate_success".to_string());

    let resolution_delay_ms =
        merchant_reference_directive(merchant_reference, "resolution_delay_ms")
            .and_then(|value| value.parse::<u64>().ok());

    let timeout_response_delay_ms =
        merchant_reference_directive(merchant_reference, "timeout_response_delay_ms")
            .and_then(|value| value.parse::<u64>().ok());

    let provider_webhook_enabled = !matches!(
        merchant_reference_directive(merchant_reference, "provider_webhook").as_deref(),
        Some("off" | "false" | "disabled" | "none")
    );

    MockIntentDirectives {
        scenario,
        resolution_delay_ms,
        timeout_response_delay_ms,
        provider_webhook_enabled,
    }
}

fn merchant_reference_directive(merchant_reference: &str, key: &str) -> Option<String> {
    let prefix = format!("#{key}=");

    merchant_reference
        .split('|')
        .find_map(|segment| {
            segment
                .trim()
                .strip_prefix(&prefix)
                .map(|s| s.trim().to_string())
        })
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone)]
pub struct MockProviderAdapter {
    client: reqwest::Client,
    base_url: String,
    default_resolution_delay_ms: u64,
    callback_url: Option<String>,
}

impl MockProviderAdapter {
    pub fn new(
        base_url: impl Into<String>,
        request_timeout: StdDuration,
        default_resolution_delay_ms: u64,
    ) -> Result<Self, ApplicationError> {
        let client = reqwest::Client::builder()
            .timeout(request_timeout)
            .build()
            .map_err(|e| {
                ApplicationError::Validation(format!("failed to build provider client: {e}"))
            })?;

        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            default_resolution_delay_ms,
            callback_url: None,
        })
    }

    pub fn with_webhook_callback_url(mut self, callback_url: Option<String>) -> Self {
        self.callback_url = callback_url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self
    }
}

#[derive(Debug, Serialize)]
struct MockProviderCreatePaymentRequest {
    merchant_reference: String,
    amount_minor: i64,
    currency: String,
    scenario: String,
    callback_url: Option<String>,
    resolution_delay_ms: Option<u64>,
    timeout_response_delay_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
struct MockProviderCreatePaymentResponse {
    provider_reference: String,
    scenario: String,
    provider_status: ProviderObservedStatus,
    accepted: bool,
    note: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize)]
struct MockProviderGetPaymentStatusResponse {
    provider_reference: String,
    scenario: String,
    provider_status: ProviderObservedStatus,
    status_probe_count: u32,
    callback_url: Option<String>,
    updated_at: DateTime<Utc>,
    note: String,
}

#[async_trait]
impl PaymentProviderAdapter for MockProviderAdapter {
    async fn submit_payment(
        &self,
        intent: &PaymentIntent,
    ) -> Result<ProviderSubmitResult, ApplicationError> {
        let directives = infer_mock_intent_directives(&intent.merchant_reference.0);
        let scenario = directives.scenario.clone();

        let request_body = MockProviderCreatePaymentRequest {
            merchant_reference: intent.merchant_reference.0.clone(),
            amount_minor: intent.money.amount_minor,
            currency: intent.money.currency.clone(),
            scenario: scenario.clone(),
            callback_url: if directives.provider_webhook_enabled {
                self.callback_url.clone()
            } else {
                None
            },
            resolution_delay_ms: Some(
                directives
                    .resolution_delay_ms
                    .unwrap_or(self.default_resolution_delay_ms),
            ),
            timeout_response_delay_ms: Some(directives.timeout_response_delay_ms.unwrap_or(15_000)),
        };

        let url = format!("{}/mock-provider/payments", self.base_url);

        let response = self.client.post(url).json(&request_body).send().await;

        match response {
            Ok(resp) => {
                let status = resp.status();

                if status == StatusCode::SERVICE_UNAVAILABLE || status.is_server_error() {
                    return Ok(ProviderSubmitResult::RetryableTransportError {
                        reason: format!("provider transport returned retryable status {}", status),
                        raw_summary: Some(json!({
                            "http_status": status.as_u16(),
                            "scenario": scenario,
                        })),
                    });
                }

                if status.is_client_error() {
                    return Ok(ProviderSubmitResult::Accepted {
                        provider_reference: None,
                        observed_status: ProviderObservedStatus::FailedTerminal,
                        raw_summary: json!({
                            "http_status": status.as_u16(),
                            "scenario": scenario,
                        }),
                        note: format!("provider returned client error {}", status),
                    });
                }

                let body: MockProviderCreatePaymentResponse = resp.json().await.map_err(|e| {
                    ApplicationError::Validation(format!("failed to parse provider response: {e}"))
                })?;

                Ok(ProviderSubmitResult::Accepted {
                    provider_reference: Some(body.provider_reference.clone()),
                    observed_status: body.provider_status,
                    raw_summary: serde_json::to_value(&body).map_err(|e| {
                        ApplicationError::Validation(format!(
                            "failed to serialize provider response: {e}"
                        ))
                    })?,
                    note: body.note,
                })
            }
            Err(err) => {
                if err.is_timeout() {
                    return Ok(ProviderSubmitResult::TimeoutUnknown {
                        reason: "provider request timed out after submission attempt; outcome may be ambiguous".to_string(),
                        raw_summary: Some(
                            json!({
                            "error_kind": "timeout",
                            "scenario": scenario,
                        })
                        ),
                    });
                }

                Ok(ProviderSubmitResult::RetryableTransportError {
                    reason: format!("provider transport error: {err}"),
                    raw_summary: Some(json!({
                        "error_kind": "transport",
                        "scenario": scenario,
                    })),
                })
            }
        }
    }

    async fn query_payment_status(
        &self,
        intent: &PaymentIntent,
    ) -> Result<ProviderStatusCheckResult, ApplicationError> {
        let response = if let Some(provider_reference) = &intent.provider_reference {
            let url = format!(
                "{}/mock-provider/payments/{}",
                self.base_url, provider_reference.0
            );
            self.client.get(url).send().await
        } else {
            let url = format!(
                "{}/mock-provider/payments/by-merchant-reference",
                self.base_url
            );
            self.client
                .get(url)
                .query(&[("merchant_reference", intent.merchant_reference.0.as_str())])
                .send()
                .await
        };

        match response {
            Ok(resp) => {
                let status = resp.status();

                if status == StatusCode::SERVICE_UNAVAILABLE || status.is_server_error() {
                    return Ok(ProviderStatusCheckResult::RetryableTransportError {
                        reason: format!("status check returned retryable status {}", status),
                        raw_summary: Some(json!({
                            "http_status": status.as_u16(),
                            "phase": "status_check",
                        })),
                    });
                }

                if status == StatusCode::NOT_FOUND {
                    return Ok(ProviderStatusCheckResult::NotFound {
                        raw_summary: Some(json!({
                            "http_status": status.as_u16(),
                            "phase": "status_check",
                        })),
                        note: "provider status check returned not found".to_string(),
                    });
                }

                let body: MockProviderGetPaymentStatusResponse =
                    resp.json().await.map_err(|e| {
                        ApplicationError::Validation(format!(
                            "failed to parse provider status response: {e}"
                        ))
                    })?;

                Ok(ProviderStatusCheckResult::Observed {
                    provider_reference: Some(body.provider_reference.clone()),
                    observed_status: body.provider_status,
                    raw_summary: serde_json::to_value(&body).map_err(|e| {
                        ApplicationError::Validation(format!(
                            "failed to serialize provider status response: {e}"
                        ))
                    })?,
                    note: body.note,
                })
            }
            Err(err) => {
                if err.is_timeout() {
                    return Ok(ProviderStatusCheckResult::RetryableTransportError {
                        reason: "provider status check timed out; keeping ambiguity".to_string(),
                        raw_summary: Some(json!({
                            "error_kind": "timeout",
                            "phase": "status_check",
                        })),
                    });
                }

                Ok(ProviderStatusCheckResult::RetryableTransportError {
                    reason: format!("provider status check transport error: {err}"),
                    raw_summary: Some(json!({
                        "error_kind": "transport",
                        "phase": "status_check",
                    })),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use domain::PaymentIntent;
    use persistence::LeasedPaymentIntent;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeRepo {
        started: Arc<Mutex<Vec<(uuid::Uuid, Uuid, Value)>>>,
        finished: Arc<Mutex<Vec<(uuid::Uuid, Option<Value>, Option<DateTime<Utc>>, String)>>>,
    }

    #[async_trait]
    impl ExecutionAttemptRepo for FakeRepo {
        async fn save_attempt_started_from_lease(
            &self,
            intent: &PaymentIntent,
            lease_token: Uuid,
            request_payload_snapshot: Value,
        ) -> Result<(), PersistenceError> {
            self.started
                .lock()
                .unwrap()
                .push((intent.id, lease_token, request_payload_snapshot));
            Ok(())
        }

        async fn save_attempt_finished(
            &self,
            intent: &PaymentIntent,
            raw_provider_response_summary: Option<Value>,
            retry_available_at: Option<DateTime<Utc>>,
        ) -> Result<(), PersistenceError> {
            self.finished.lock().unwrap().push((
                intent.id,
                raw_provider_response_summary,
                retry_available_at,
                format!("{:?}", intent.state),
            ));
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FakeProvider {
        result: ProviderSubmitResult,
    }

    #[async_trait]
    impl PaymentProviderAdapter for FakeProvider {
        async fn submit_payment(
            &self,
            _intent: &PaymentIntent,
        ) -> Result<ProviderSubmitResult, ApplicationError> {
            Ok(self.result.clone())
        }

        async fn query_payment_status(
            &self,
            _intent: &PaymentIntent,
        ) -> Result<ProviderStatusCheckResult, ApplicationError> {
            Ok(ProviderStatusCheckResult::RetryableTransportError {
                reason: "not used in these tests".into(),
                raw_summary: None,
            })
        }
    }

    fn leased_intent(now: DateTime<Utc>) -> LeasedPaymentIntent {
        let mut intent = PaymentIntent::new(
            "order_123|#scenario=immediate_success",
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

        LeasedPaymentIntent {
            intent,
            lease_token: Uuid::new_v4(),
            worker_id: "worker-1".to_string(),
            leased_at: now,
            lease_expires_at: now + chrono::Duration::seconds(30),
        }
    }

    #[tokio::test]
    async fn success_becomes_succeeded() {
        let now = Utc::now();
        let repo = FakeRepo::default();
        let provider = FakeProvider {
            result: ProviderSubmitResult::Accepted {
                provider_reference: Some("mock_ref_1".into()),
                observed_status: ProviderObservedStatus::Succeeded,
                raw_summary: json!({"provider_reference":"mock_ref_1"}),
                note: "simulated immediate success".into(),
            },
        };

        let svc = ExecutionAttemptService::new(
            repo.clone(),
            provider,
            StdDuration::from_secs(10),
            StdDuration::from_secs(20),
        );
        let result = svc
            .execute_leased_intent(leased_intent(now), now)
            .await
            .unwrap();

        assert_eq!(result.state, "Succeeded");
        assert!(result.retry_available_at.is_none());
        assert!(result.next_resolution_at.is_none());
        assert_eq!(repo.started.lock().unwrap().len(), 1);
        assert_eq!(repo.finished.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn terminal_failure_does_not_retry() {
        let now = Utc::now();
        let repo = FakeRepo::default();
        let provider = FakeProvider {
            result: ProviderSubmitResult::Accepted {
                provider_reference: Some("mock_ref_2".into()),
                observed_status: ProviderObservedStatus::FailedTerminal,
                raw_summary: json!({"provider_reference":"mock_ref_2"}),
                note: "simulated terminal provider rejection".into(),
            },
        };

        let svc = ExecutionAttemptService::new(
            repo.clone(),
            provider,
            StdDuration::from_secs(10),
            StdDuration::from_secs(20),
        );
        let result = svc
            .execute_leased_intent(leased_intent(now), now)
            .await
            .unwrap();

        assert_eq!(result.state, "FailedTerminal");
        assert!(result.retry_available_at.is_none());
        assert!(result.next_resolution_at.is_none());
    }

    #[tokio::test]
    async fn retryable_failure_schedules_retry() {
        let now = Utc::now();
        let repo = FakeRepo::default();
        let provider = FakeProvider {
            result: ProviderSubmitResult::RetryableTransportError {
                reason: "provider transport returned retryable status 503 Service Unavailable"
                    .into(),
                raw_summary: Some(json!({"http_status":503})),
            },
        };

        let svc = ExecutionAttemptService::new(
            repo.clone(),
            provider,
            StdDuration::from_secs(15),
            StdDuration::from_secs(20),
        );
        let result = svc
            .execute_leased_intent(leased_intent(now), now)
            .await
            .unwrap();

        assert_eq!(result.state, "RetryScheduled");
        assert!(result.retry_available_at.is_some());
        assert!(result.next_resolution_at.is_none());
    }

    #[tokio::test]
    async fn timeout_becomes_unknown_outcome_and_schedules_status_check() {
        let now = Utc::now();
        let repo = FakeRepo::default();
        let provider = FakeProvider {
            result: ProviderSubmitResult::TimeoutUnknown {
                reason:
                    "provider request timed out after submission attempt; outcome may be ambiguous"
                        .into(),
                raw_summary: Some(json!({"error_kind":"timeout"})),
            },
        };

        let svc = ExecutionAttemptService::new(
            repo.clone(),
            provider,
            StdDuration::from_secs(15),
            StdDuration::from_secs(20),
        );
        let result = svc
            .execute_leased_intent(leased_intent(now), now)
            .await
            .unwrap();

        assert_eq!(result.state, "UnknownOutcome");
        assert!(result.retry_available_at.is_none());
        assert!(result.next_resolution_at.is_some());
    }

    #[tokio::test]
    async fn pending_stays_pending_and_schedules_status_check() {
        let now = Utc::now();
        let repo = FakeRepo::default();
        let provider = FakeProvider {
            result: ProviderSubmitResult::Accepted {
                provider_reference: Some("mock_ref_3".into()),
                observed_status: ProviderObservedStatus::Pending,
                raw_summary: json!({"provider_reference":"mock_ref_3"}),
                note: "simulated pending provider state".into(),
            },
        };

        let svc = ExecutionAttemptService::new(
            repo.clone(),
            provider,
            StdDuration::from_secs(15),
            StdDuration::from_secs(20),
        );
        let result = svc
            .execute_leased_intent(leased_intent(now), now)
            .await
            .unwrap();

        assert_eq!(result.state, "ProviderPending");
        assert!(result.retry_available_at.is_none());
        assert!(result.next_resolution_at.is_some());
    }

    #[test]
    fn merchant_reference_directives_override_timing_and_webhook_behavior() {
        let directives = infer_mock_intent_directives(
            "order_123|#scenario=timeout_after_acceptance|#provider_webhook=off|#resolution_delay_ms=1500|#timeout_response_delay_ms=2200",
        );

        assert_eq!(directives.scenario, "timeout_after_acceptance");
        assert_eq!(directives.resolution_delay_ms, Some(1500));
        assert_eq!(directives.timeout_response_delay_ms, Some(2200));
        assert!(!directives.provider_webhook_enabled);
    }
}
