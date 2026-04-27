use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use persistence::{
    CallbackDeliveryDisposition, FinalizeCallbackDeliveryAttemptInput, LeasedCallbackNotification,
    PersistenceError, PostgresPersistence,
};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::ApplicationError;

pub const CALLBACK_SIGNATURE_HEADER: &str = "X-Gateway-Signature";

#[derive(Debug, Clone)]
pub enum CallbackDispatchResult {
    Delivered {
        http_status_code: u16,
        response_body: Option<String>,
    },
    Failed {
        http_status_code: Option<u16>,
        response_body: Option<String>,
        error_message: String,
    },
}

#[async_trait]
pub trait CallbackDeliveryRepo: Clone + Send + Sync + 'static {
    async fn lease_next_due_callback_notification(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_for: StdDuration,
    ) -> Result<Option<LeasedCallbackNotification>, PersistenceError>;

    async fn finalize_callback_delivery_attempt(
        &self,
        input: FinalizeCallbackDeliveryAttemptInput,
    ) -> Result<(), PersistenceError>;
}

#[async_trait]
impl CallbackDeliveryRepo for PostgresPersistence {
    async fn lease_next_due_callback_notification(
        &self,
        worker_id: &str,
        now: DateTime<Utc>,
        lease_for: StdDuration,
    ) -> Result<Option<LeasedCallbackNotification>, PersistenceError> {
        PostgresPersistence::lease_next_due_callback_notification(self, worker_id, now, lease_for)
            .await
    }

    async fn finalize_callback_delivery_attempt(
        &self,
        input: FinalizeCallbackDeliveryAttemptInput,
    ) -> Result<(), PersistenceError> {
        PostgresPersistence::finalize_callback_delivery_attempt(self, input).await
    }
}

#[async_trait]
pub trait CallbackDispatcher: Clone + Send + Sync + 'static {
    async fn dispatch(
        &self,
        destination_url: &str,
        payload: &Value,
        signature: Option<String>,
    ) -> Result<CallbackDispatchResult, ApplicationError>;
}

#[derive(Clone)]
pub struct HttpCallbackDispatcher {
    client: reqwest::Client,
}

impl HttpCallbackDispatcher {
    pub fn new(timeout: StdDuration) -> Result<Self, ApplicationError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| {
                ApplicationError::Validation(format!(
                    "invalid callback client configuration: {err}"
                ))
            })?;
        Ok(Self { client })
    }
}

#[async_trait]
impl CallbackDispatcher for HttpCallbackDispatcher {
    async fn dispatch(
        &self,
        destination_url: &str,
        payload: &Value,
        signature: Option<String>,
    ) -> Result<CallbackDispatchResult, ApplicationError> {
        let mut request = self.client.post(destination_url).json(payload);

        if let Some(signature) = signature {
            request = request.header(CALLBACK_SIGNATURE_HEADER, signature);
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(err) => {
                return Ok(CallbackDispatchResult::Failed {
                    http_status_code: None,
                    response_body: None,
                    error_message: err.to_string(),
                });
            }
        };

        let status = response.status();
        let response_body = response.text().await.ok().filter(|body| !body.is_empty());

        if status.is_success() {
            return Ok(CallbackDispatchResult::Delivered {
                http_status_code: status.as_u16(),
                response_body,
            });
        }

        Ok(CallbackDispatchResult::Failed {
            http_status_code: Some(status.as_u16()),
            response_body: response_body.clone(),
            error_message: classify_http_failure(status, response_body.as_deref()),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CallbackDeliverySummary {
    pub notification_id: i64,
    pub intent_id: uuid::Uuid,
    pub destination_url: String,
    pub target_state: String,
    pub attempt_no: i32,
    pub outcome: String,
    pub http_status_code: Option<u16>,
    pub retry_at: Option<DateTime<Utc>>,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct CallbackDeliveryService<R, D>
where
    R: CallbackDeliveryRepo,
    D: CallbackDispatcher,
{
    repo: R,
    dispatcher: D,
    worker_id: String,
    lease_for: StdDuration,
    retry_delay: StdDuration,
    max_attempts: i32,
    signing_secret: Option<String>,
}

impl<R, D> CallbackDeliveryService<R, D>
where
    R: CallbackDeliveryRepo,
    D: CallbackDispatcher,
{
    pub fn new(
        repo: R,
        dispatcher: D,
        worker_id: impl Into<String>,
        lease_for: StdDuration,
        retry_delay: StdDuration,
        max_attempts: i32,
    ) -> Self {
        Self {
            repo,
            dispatcher,
            worker_id: worker_id.into(),
            lease_for,
            retry_delay,
            max_attempts,
            signing_secret: None,
        }
    }

    pub fn with_signing_secret(mut self, signing_secret: Option<String>) -> Self {
        self.signing_secret = signing_secret.filter(|secret| !secret.trim().is_empty());
        self
    }

    pub async fn poll_once(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<CallbackDeliverySummary>, ApplicationError> {
        let Some(notification) = self
            .repo
            .lease_next_due_callback_notification(&self.worker_id, now, self.lease_for)
            .await?
        else {
            return Ok(None);
        };

        let attempt_no = notification.attempt_count + 1;
        let signature = build_signature(self.signing_secret.as_deref(), &notification.payload)?;
        let dispatch_result = self
            .dispatcher
            .dispatch(
                &notification.destination_url,
                &notification.payload,
                signature,
            )
            .await?;

        let finished_at = Utc::now();

        let summary = match dispatch_result {
            CallbackDispatchResult::Delivered {
                http_status_code,
                response_body,
            } => {
                self.repo
                    .finalize_callback_delivery_attempt(FinalizeCallbackDeliveryAttemptInput {
                        notification_id: notification.notification_id,
                        lease_token: notification.lease_token,
                        finished_at,
                        disposition: CallbackDeliveryDisposition::Delivered,
                        http_status_code: Some(i32::from(http_status_code)),
                        response_body,
                        error_message: None,
                        retry_at: None,
                    })
                    .await?;

                CallbackDeliverySummary {
                    notification_id: notification.notification_id,
                    intent_id: notification.intent_id,
                    destination_url: notification.destination_url,
                    target_state: notification.target_state,
                    attempt_no,
                    outcome: "delivered".to_string(),
                    http_status_code: Some(http_status_code),
                    retry_at: None,
                    note: "callback delivered successfully".to_string(),
                }
            }
            CallbackDispatchResult::Failed {
                http_status_code,
                response_body,
                error_message,
            } => {
                let retry_at = if attempt_no >= self.max_attempts {
                    None
                } else {
                    Some(
                        now + Duration::from_std(self.retry_delay).map_err(|_| {
                            ApplicationError::Validation("invalid callback retry delay".to_string())
                        })?,
                    )
                };

                let disposition = if retry_at.is_some() {
                    CallbackDeliveryDisposition::RetryScheduled
                } else {
                    CallbackDeliveryDisposition::DeadLettered
                };

                self.repo
                    .finalize_callback_delivery_attempt(FinalizeCallbackDeliveryAttemptInput {
                        notification_id: notification.notification_id,
                        lease_token: notification.lease_token,
                        finished_at,
                        disposition,
                        http_status_code: http_status_code.map(i32::from),
                        response_body,
                        error_message: Some(error_message.clone()),
                        retry_at,
                    })
                    .await?;

                let outcome = match disposition {
                    CallbackDeliveryDisposition::RetryScheduled => "retry_scheduled",
                    CallbackDeliveryDisposition::DeadLettered => "dead_lettered",
                    CallbackDeliveryDisposition::Delivered => unreachable!(),
                };

                let note = if retry_at.is_some() {
                    format!("callback delivery failed and was scheduled for retry: {error_message}")
                } else {
                    format!("callback delivery exhausted retries and was dead-lettered: {error_message}")
                };

                CallbackDeliverySummary {
                    notification_id: notification.notification_id,
                    intent_id: notification.intent_id,
                    destination_url: notification.destination_url,
                    target_state: notification.target_state,
                    attempt_no,
                    outcome: outcome.to_string(),
                    http_status_code,
                    retry_at,
                    note,
                }
            }
        };

        Ok(Some(summary))
    }
}

fn build_signature(
    signing_secret: Option<&str>,
    payload: &Value,
) -> Result<Option<String>, ApplicationError> {
    let Some(signing_secret) = signing_secret else {
        return Ok(None);
    };

    let body = serde_json::to_vec(payload).map_err(|err| {
        ApplicationError::Validation(format!("failed to serialize callback payload: {err}"))
    })?;
    let digest = Sha256::digest([signing_secret.as_bytes(), b":", &body].concat());
    Ok(Some(hex::encode(digest)))
}

fn classify_http_failure(status: StatusCode, response_body: Option<&str>) -> String {
    match response_body {
        Some(response_body) if !response_body.is_empty() => format!(
            "downstream callback returned {}: {}",
            status.as_u16(),
            response_body
        ),
        _ => format!("downstream callback returned {}", status.as_u16()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    #[derive(Clone, Default)]
    struct FakeRepo {
        leased: Arc<Mutex<VecDeque<LeasedCallbackNotification>>>,
        finalized: Arc<Mutex<Vec<FinalizeCallbackDeliveryAttemptInput>>>,
    }

    #[async_trait]
    impl CallbackDeliveryRepo for FakeRepo {
        async fn lease_next_due_callback_notification(
            &self,
            _worker_id: &str,
            _now: DateTime<Utc>,
            _lease_for: StdDuration,
        ) -> Result<Option<LeasedCallbackNotification>, PersistenceError> {
            Ok(self.leased.lock().unwrap().pop_front())
        }

        async fn finalize_callback_delivery_attempt(
            &self,
            input: FinalizeCallbackDeliveryAttemptInput,
        ) -> Result<(), PersistenceError> {
            self.finalized.lock().unwrap().push(input);
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FakeDispatcher {
        result: CallbackDispatchResult,
    }

    #[async_trait]
    impl CallbackDispatcher for FakeDispatcher {
        async fn dispatch(
            &self,
            _destination_url: &str,
            _payload: &Value,
            _signature: Option<String>,
        ) -> Result<CallbackDispatchResult, ApplicationError> {
            Ok(self.result.clone())
        }
    }

    fn leased_notification(now: DateTime<Utc>, attempt_count: i32) -> LeasedCallbackNotification {
        LeasedCallbackNotification {
            notification_id: 1,
            event_key: "evt_1".to_string(),
            intent_id: Uuid::new_v4(),
            destination_url: "https://merchant.example/callbacks".to_string(),
            target_state: "succeeded".to_string(),
            payload: serde_json::json!({
                "intent_id": Uuid::new_v4(),
                "state": "succeeded",
            }),
            attempt_count,
            lease_token: Uuid::new_v4(),
            worker_id: "callback-worker-1".to_string(),
            leased_at: now,
            lease_expires_at: now,
        }
    }

    fn service(
        repo: FakeRepo,
        dispatcher: FakeDispatcher,
        max_attempts: i32,
    ) -> CallbackDeliveryService<FakeRepo, FakeDispatcher> {
        CallbackDeliveryService::new(
            repo,
            dispatcher,
            "callback-worker-1",
            StdDuration::from_secs(30),
            StdDuration::from_secs(10),
            max_attempts,
        )
    }

    #[tokio::test]
    async fn successful_delivery_marks_notification_delivered() {
        let now = Utc::now();
        let repo = FakeRepo {
            leased: Arc::new(Mutex::new(VecDeque::from([leased_notification(now, 0)]))),
            finalized: Arc::new(Mutex::new(vec![])),
        };

        let summary = service(
            repo.clone(),
            FakeDispatcher {
                result: CallbackDispatchResult::Delivered {
                    http_status_code: 200,
                    response_body: Some("ok".to_string()),
                },
            },
            3,
        )
        .poll_once(now)
        .await
        .unwrap()
        .unwrap();

        assert_eq!(summary.outcome, "delivered");
        let finalized = repo.finalized.lock().unwrap();
        assert_eq!(finalized.len(), 1);
        assert!(matches!(
            finalized[0].disposition,
            CallbackDeliveryDisposition::Delivered
        ));
    }

    #[tokio::test]
    async fn failed_delivery_schedules_retry_before_max_attempts() {
        let now = Utc::now();
        let repo = FakeRepo {
            leased: Arc::new(Mutex::new(VecDeque::from([leased_notification(now, 0)]))),
            finalized: Arc::new(Mutex::new(vec![])),
        };

        let summary = service(
            repo.clone(),
            FakeDispatcher {
                result: CallbackDispatchResult::Failed {
                    http_status_code: Some(500),
                    response_body: Some("nope".to_string()),
                    error_message: "downstream callback returned 500".to_string(),
                },
            },
            3,
        )
        .poll_once(now)
        .await
        .unwrap()
        .unwrap();

        assert_eq!(summary.outcome, "retry_scheduled");
        assert!(summary.retry_at.is_some());
        let finalized = repo.finalized.lock().unwrap();
        assert!(matches!(
            finalized[0].disposition,
            CallbackDeliveryDisposition::RetryScheduled
        ));
    }

    #[tokio::test]
    async fn failed_delivery_dead_letters_when_max_attempts_is_reached() {
        let now = Utc::now();
        let repo = FakeRepo {
            leased: Arc::new(Mutex::new(VecDeque::from([leased_notification(now, 1)]))),
            finalized: Arc::new(Mutex::new(vec![])),
        };

        let summary = service(
            repo.clone(),
            FakeDispatcher {
                result: CallbackDispatchResult::Failed {
                    http_status_code: Some(500),
                    response_body: None,
                    error_message: "downstream callback returned 500".to_string(),
                },
            },
            2,
        )
        .poll_once(now)
        .await
        .unwrap()
        .unwrap();

        assert_eq!(summary.outcome, "dead_lettered");
        assert!(summary.retry_at.is_none());
        let finalized = repo.finalized.lock().unwrap();
        assert!(matches!(
            finalized[0].disposition,
            CallbackDeliveryDisposition::DeadLettered
        ));
    }
}
