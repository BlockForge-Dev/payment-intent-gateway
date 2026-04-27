use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MockScenario {
    ImmediateSuccess,
    TerminalFailure,
    RetryableInfraError,
    TimeoutAfterAcceptance,
    DelayedConfirmation,
    DuplicateWebhook,
    InconsistentStatusCheckResponse,
    PendingThenResolves,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MockProviderStatus {
    Pending,
    Succeeded,
    FailedTerminal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMockPaymentRequest {
    pub merchant_reference: String,
    pub amount_minor: i64,
    pub currency: String,
    pub scenario: MockScenario,
    pub callback_url: Option<String>,
    pub resolution_delay_ms: Option<u64>,
    pub timeout_response_delay_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateMockPaymentResponse {
    pub provider_reference: String,
    pub scenario: MockScenario,
    pub provider_status: MockProviderStatus,
    pub accepted: bool,
    pub note: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GetMockPaymentStatusResponse {
    pub provider_reference: String,
    pub scenario: MockScenario,
    pub provider_status: MockProviderStatus,
    pub status_probe_count: u32,
    pub callback_url: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookReplayResponse {
    pub provider_reference: String,
    pub replayed: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MockPaymentAdminRecord {
    pub provider_reference: String,
    pub merchant_reference: String,
    pub scenario: MockScenario,
    pub provider_status: MockProviderStatus,
    pub callback_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteMockPaymentResponse {
    pub provider_reference: String,
    pub deleted: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResetMockProviderResponse {
    pub removed_payments: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct SimulatedPayment {
    pub provider_reference: String,
    pub merchant_reference: String,
    pub amount_minor: i64,
    pub currency: String,
    pub scenario: MockScenario,
    pub internal_status: MockProviderStatus,
    pub callback_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resolution_due_at: Option<DateTime<Utc>>,
    pub resolution_target: Option<MockProviderStatus>,
    pub webhook_duplicate_count: usize,
    pub webhook_on_resolution: bool,
    pub status_script: Vec<MockProviderStatus>,
    pub status_probe_count: u32,
}

impl SimulatedPayment {
    pub fn new(request: &CreateMockPaymentRequest, now: DateTime<Utc>) -> Self {
        let provider_reference = format!("mock_{}", Uuid::new_v4().simple());

        match request.scenario {
            MockScenario::ImmediateSuccess => Self {
                provider_reference,
                merchant_reference: request.merchant_reference.clone(),
                amount_minor: request.amount_minor,
                currency: request.currency.trim().to_uppercase(),
                scenario: request.scenario.clone(),
                internal_status: MockProviderStatus::Succeeded,
                callback_url: request.callback_url.clone(),
                created_at: now,
                updated_at: now,
                resolution_due_at: None,
                resolution_target: None,
                webhook_duplicate_count: 1,
                webhook_on_resolution: true,
                status_script: vec![],
                status_probe_count: 0,
            },
            MockScenario::TerminalFailure => Self {
                provider_reference,
                merchant_reference: request.merchant_reference.clone(),
                amount_minor: request.amount_minor,
                currency: request.currency.trim().to_uppercase(),
                scenario: request.scenario.clone(),
                internal_status: MockProviderStatus::FailedTerminal,
                callback_url: request.callback_url.clone(),
                created_at: now,
                updated_at: now,
                resolution_due_at: None,
                resolution_target: None,
                webhook_duplicate_count: 0,
                webhook_on_resolution: false,
                status_script: vec![],
                status_probe_count: 0,
            },
            MockScenario::TimeoutAfterAcceptance => Self {
                provider_reference,
                merchant_reference: request.merchant_reference.clone(),
                amount_minor: request.amount_minor,
                currency: request.currency.trim().to_uppercase(),
                scenario: request.scenario.clone(),
                internal_status: MockProviderStatus::Succeeded,
                callback_url: request.callback_url.clone(),
                created_at: now,
                updated_at: now,
                resolution_due_at: None,
                resolution_target: None,
                webhook_duplicate_count: 1,
                webhook_on_resolution: true,
                status_script: vec![],
                status_probe_count: 0,
            },
            MockScenario::DelayedConfirmation => Self {
                provider_reference,
                merchant_reference: request.merchant_reference.clone(),
                amount_minor: request.amount_minor,
                currency: request.currency.trim().to_uppercase(),
                scenario: request.scenario.clone(),
                internal_status: MockProviderStatus::Pending,
                callback_url: request.callback_url.clone(),
                created_at: now,
                updated_at: now,
                resolution_due_at: Some(
                    now + chrono::Duration::milliseconds(
                        request.resolution_delay_ms.unwrap_or(5_000) as i64,
                    ),
                ),
                resolution_target: Some(MockProviderStatus::Succeeded),
                webhook_duplicate_count: 1,
                webhook_on_resolution: true,
                status_script: vec![],
                status_probe_count: 0,
            },
            MockScenario::DuplicateWebhook => Self {
                provider_reference,
                merchant_reference: request.merchant_reference.clone(),
                amount_minor: request.amount_minor,
                currency: request.currency.trim().to_uppercase(),
                scenario: request.scenario.clone(),
                internal_status: MockProviderStatus::Pending,
                callback_url: request.callback_url.clone(),
                created_at: now,
                updated_at: now,
                resolution_due_at: Some(
                    now + chrono::Duration::milliseconds(
                        request.resolution_delay_ms.unwrap_or(5_000) as i64,
                    ),
                ),
                resolution_target: Some(MockProviderStatus::Succeeded),
                webhook_duplicate_count: 2,
                webhook_on_resolution: true,
                status_script: vec![],
                status_probe_count: 0,
            },
            MockScenario::InconsistentStatusCheckResponse => Self {
                provider_reference,
                merchant_reference: request.merchant_reference.clone(),
                amount_minor: request.amount_minor,
                currency: request.currency.trim().to_uppercase(),
                scenario: request.scenario.clone(),
                internal_status: MockProviderStatus::Pending,
                callback_url: request.callback_url.clone(),
                created_at: now,
                updated_at: now,
                resolution_due_at: None,
                resolution_target: Some(MockProviderStatus::Succeeded),
                webhook_duplicate_count: 0,
                webhook_on_resolution: false,
                status_script: vec![
                    MockProviderStatus::Pending,
                    MockProviderStatus::Succeeded,
                    MockProviderStatus::Pending,
                    MockProviderStatus::Succeeded,
                ],
                status_probe_count: 0,
            },
            MockScenario::PendingThenResolves => Self {
                provider_reference,
                merchant_reference: request.merchant_reference.clone(),
                amount_minor: request.amount_minor,
                currency: request.currency.trim().to_uppercase(),
                scenario: request.scenario.clone(),
                internal_status: MockProviderStatus::Pending,
                callback_url: request.callback_url.clone(),
                created_at: now,
                updated_at: now,
                resolution_due_at: Some(
                    now + chrono::Duration::milliseconds(
                        request.resolution_delay_ms.unwrap_or(5_000) as i64,
                    ),
                ),
                resolution_target: Some(MockProviderStatus::Succeeded),
                webhook_duplicate_count: 0,
                webhook_on_resolution: false,
                status_script: vec![],
                status_probe_count: 0,
            },
            MockScenario::RetryableInfraError => unreachable!("handled before payment creation"),
        }
    }

    pub fn apply_time_progress(&mut self, now: DateTime<Utc>) {
        if self.internal_status == MockProviderStatus::Pending {
            if let Some(due_at) = self.resolution_due_at {
                if now >= due_at {
                    if let Some(target) = &self.resolution_target {
                        self.internal_status = target.clone();
                        self.updated_at = now;
                    }
                }
            }
        }
    }

    pub fn visible_status_for_read(&mut self, now: DateTime<Utc>) -> MockProviderStatus {
        self.apply_time_progress(now);

        if self.scenario == MockScenario::InconsistentStatusCheckResponse {
            let idx = std::cmp::min(
                self.status_probe_count as usize,
                self.status_script.len().saturating_sub(1),
            );
            let visible = self
                .status_script
                .get(idx)
                .cloned()
                .unwrap_or_else(|| self.internal_status.clone());

            self.status_probe_count += 1;

            if idx == self.status_script.len().saturating_sub(1) {
                self.internal_status = visible.clone();
                self.updated_at = now;
            }

            return visible;
        }

        self.internal_status.clone()
    }

    pub fn note(&self) -> &'static str {
        match self.scenario {
            MockScenario::ImmediateSuccess => "simulated immediate success",
            MockScenario::TerminalFailure => "simulated terminal provider rejection",
            MockScenario::RetryableInfraError => "simulated retryable infrastructure error",
            MockScenario::TimeoutAfterAcceptance => {
                "simulated timeout after request may have been accepted"
            }
            MockScenario::DelayedConfirmation => {
                "simulated delayed confirmation that later succeeds"
            }
            MockScenario::DuplicateWebhook => {
                "simulated delayed confirmation followed by duplicate webhook delivery"
            }
            MockScenario::InconsistentStatusCheckResponse => {
                "simulated inconsistent status checks before stabilizing"
            }
            MockScenario::PendingThenResolves => {
                "simulated pending state that later resolves without webhook"
            }
        }
    }
}

impl CreateMockPaymentResponse {
    pub fn from_payment(payment: &SimulatedPayment, accepted: bool) -> Self {
        Self {
            provider_reference: payment.provider_reference.clone(),
            scenario: payment.scenario.clone(),
            provider_status: payment.internal_status.clone(),
            accepted,
            note: payment.note().to_string(),
            created_at: payment.created_at,
        }
    }
}

impl GetMockPaymentStatusResponse {
    pub fn from_payment(payment: &SimulatedPayment, visible_status: MockProviderStatus) -> Self {
        Self {
            provider_reference: payment.provider_reference.clone(),
            scenario: payment.scenario.clone(),
            provider_status: visible_status,
            status_probe_count: payment.status_probe_count,
            callback_url: payment.callback_url.clone(),
            updated_at: payment.updated_at,
            note: payment.note().to_string(),
        }
    }
}

impl MockPaymentAdminRecord {
    pub fn from_payment(payment: &SimulatedPayment) -> Self {
        Self {
            provider_reference: payment.provider_reference.clone(),
            merchant_reference: payment.merchant_reference.clone(),
            scenario: payment.scenario.clone(),
            provider_status: payment.internal_status.clone(),
            callback_url: payment.callback_url.clone(),
            created_at: payment.created_at,
            updated_at: payment.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_then_resolves_after_deadline() {
        let now = Utc::now();
        let req = CreateMockPaymentRequest {
            merchant_reference: "order_1".into(),
            amount_minor: 5000,
            currency: "NGN".into(),
            scenario: MockScenario::PendingThenResolves,
            callback_url: None,
            resolution_delay_ms: Some(1000),
            timeout_response_delay_ms: None,
        };

        let mut payment = SimulatedPayment::new(&req, now);
        assert_eq!(payment.internal_status, MockProviderStatus::Pending);

        payment.apply_time_progress(now + chrono::Duration::milliseconds(1200));
        assert_eq!(payment.internal_status, MockProviderStatus::Succeeded);
    }

    #[test]
    fn inconsistent_status_script_advances() {
        let now = Utc::now();
        let req = CreateMockPaymentRequest {
            merchant_reference: "order_1".into(),
            amount_minor: 5000,
            currency: "NGN".into(),
            scenario: MockScenario::InconsistentStatusCheckResponse,
            callback_url: None,
            resolution_delay_ms: None,
            timeout_response_delay_ms: None,
        };

        let mut payment = SimulatedPayment::new(&req, now);

        assert_eq!(
            payment.visible_status_for_read(now),
            MockProviderStatus::Pending
        );
        assert_eq!(
            payment.visible_status_for_read(now),
            MockProviderStatus::Succeeded
        );
        assert_eq!(
            payment.visible_status_for_read(now),
            MockProviderStatus::Pending
        );
        assert_eq!(
            payment.visible_status_for_read(now),
            MockProviderStatus::Succeeded
        );
    }

    #[test]
    fn duplicate_webhook_scenario_sets_two_webhooks() {
        let now = Utc::now();
        let req = CreateMockPaymentRequest {
            merchant_reference: "order_1".into(),
            amount_minor: 5000,
            currency: "NGN".into(),
            scenario: MockScenario::DuplicateWebhook,
            callback_url: Some("http://localhost:3000/test".into()),
            resolution_delay_ms: Some(1000),
            timeout_response_delay_ms: None,
        };

        let payment = SimulatedPayment::new(&req, now);
        assert_eq!(payment.webhook_duplicate_count, 2);
        assert!(payment.webhook_on_resolution);
    }
}
