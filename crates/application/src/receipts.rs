use chrono::{DateTime, Utc};
use domain::{
    AttemptOutcome, EvidenceSource, FailureClassification, IntentState, PaymentReceipt,
    ReconComparison, ReconDecision,
};
use persistence::{ComputedReceipt, StoredAuditEvent, StoredReconciliationRun};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorReceipt {
    pub summary: OperatorReceiptSummary,
    pub ambiguity: OperatorReceiptAmbiguity,
    pub attempts: Vec<OperatorExecutionAttempt>,
    pub provider_webhooks: OperatorWebhookHistory,
    pub callbacks: OperatorCallbackHistory,
    pub reconciliation: OperatorReconciliationHistory,
    pub timeline: Vec<OperatorTimelineEntry>,
    pub evidence_notes: Vec<OperatorEvidenceNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorReceiptSummary {
    pub intent_id: uuid::Uuid,
    pub merchant_reference: String,
    pub idempotency_key: String,
    pub amount_minor: i64,
    pub currency: String,
    pub provider: String,
    pub callback_url: Option<String>,
    pub current_state: String,
    pub final_classification: Option<String>,
    pub latest_failure_classification: Option<String>,
    pub provider_reference: Option<String>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorReceiptAmbiguity {
    pub visible: bool,
    pub next_resolution_at: Option<DateTime<Utc>>,
    pub last_resolution_at: Option<DateTime<Utc>>,
    pub resolution_attempt_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorExecutionAttempt {
    pub attempt_no: u32,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub outcome: Option<String>,
    pub classification: Option<String>,
    pub reason: Option<String>,
    pub provider_reference: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorWebhookHistory {
    pub total_events: usize,
    pub latest_event_at: Option<DateTime<Utc>>,
    pub events: Vec<OperatorWebhookEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorWebhookEvent {
    pub provider_name: String,
    pub provider_event_id: String,
    pub provider_reference: Option<String>,
    pub event_type: String,
    pub status_hint: Option<String>,
    pub received_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorCallbackHistory {
    pub configured: bool,
    pub destination_url: Option<String>,
    pub notification_count: usize,
    pub delivered_count: usize,
    pub pending_count: usize,
    pub dead_lettered_count: usize,
    pub delivery_attempt_count: usize,
    pub notifications: Vec<OperatorCallbackNotification>,
    pub deliveries: Vec<OperatorCallbackDelivery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorCallbackNotification {
    pub event_key: String,
    pub destination_url: String,
    pub target_state: String,
    pub status: String,
    pub next_attempt_at: DateTime<Utc>,
    pub attempt_count: i32,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub last_http_status_code: Option<i32>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorCallbackDelivery {
    pub destination_url: String,
    pub attempt_no: i32,
    pub delivery_result: String,
    pub http_status_code: Option<i32>,
    pub retry_count: i32,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub response_body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorReconciliationHistory {
    pub latest: Option<OperatorReconciliationRun>,
    pub runs: Vec<OperatorReconciliationRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorReconciliationRun {
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    pub provider_status_seen: String,
    pub internal_status_seen: String,
    pub comparison: String,
    pub decision: String,
    pub evidence: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorTimelineEntry {
    pub at: DateTime<Utc>,
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub state: Option<String>,
    pub evidence_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorEvidenceNote {
    pub at: DateTime<Utc>,
    pub source: String,
    pub note: String,
}

pub fn build_operator_receipt(receipt: ComputedReceipt) -> OperatorReceipt {
    let summary = build_summary(&receipt.core);
    let ambiguity = OperatorReceiptAmbiguity {
        visible: matches!(
            receipt.core.current_state,
            IntentState::UnknownOutcome | IntentState::ProviderPending | IntentState::ManualReview
        ),
        next_resolution_at: receipt.core.next_resolution_at,
        last_resolution_at: receipt.core.last_resolution_at,
        resolution_attempt_count: receipt.core.resolution_attempt_count,
    };

    let attempts = receipt
        .core
        .attempts
        .iter()
        .map(|attempt| OperatorExecutionAttempt {
            attempt_no: attempt.attempt_no,
            started_at: attempt.started_at,
            ended_at: attempt.ended_at,
            outcome: attempt.outcome.as_ref().map(attempt_outcome_label),
            classification: attempt
                .outcome
                .as_ref()
                .and_then(attempt_outcome_classification),
            reason: attempt.outcome.as_ref().and_then(attempt_outcome_reason),
            provider_reference: attempt
                .provider_reference
                .as_ref()
                .map(|value| value.0.clone()),
            note: attempt.note.clone(),
        })
        .collect::<Vec<_>>();

    let webhook_events = receipt
        .provider_events
        .iter()
        .map(|event| OperatorWebhookEvent {
            provider_name: event.provider_name.clone(),
            provider_event_id: event.provider_event_id.clone(),
            provider_reference: event.provider_reference.clone(),
            event_type: event.event_type.clone(),
            status_hint: event
                .raw_payload
                .get("status")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
            received_at: event.received_at,
            processed_at: event.processed_at,
        })
        .collect::<Vec<_>>();

    let callback_notifications = receipt
        .callback_notifications
        .iter()
        .map(|notification| OperatorCallbackNotification {
            event_key: notification.event_key.clone(),
            destination_url: notification.destination_url.clone(),
            target_state: notification.target_state.clone(),
            status: notification.status.clone(),
            next_attempt_at: notification.next_attempt_at,
            attempt_count: notification.attempt_count,
            last_attempt_at: notification.last_attempt_at,
            delivered_at: notification.delivered_at,
            last_http_status_code: notification.last_http_status_code,
            last_error: notification.last_error.clone(),
            created_at: notification.created_at,
            updated_at: notification.updated_at,
        })
        .collect::<Vec<_>>();

    let callback_deliveries = receipt
        .callback_deliveries
        .iter()
        .map(|delivery| OperatorCallbackDelivery {
            destination_url: delivery.destination_url.clone(),
            attempt_no: delivery.attempt_no,
            delivery_result: delivery.delivery_result.clone(),
            http_status_code: delivery.http_status_code,
            retry_count: delivery.retry_count,
            started_at: delivery.started_at,
            ended_at: delivery.ended_at,
            response_body: delivery.response_body.clone(),
        })
        .collect::<Vec<_>>();

    let reconciliation_runs = receipt
        .reconciliation_runs
        .iter()
        .map(map_reconciliation_run)
        .collect::<Vec<_>>();

    let provider_webhooks = OperatorWebhookHistory {
        total_events: webhook_events.len(),
        latest_event_at: webhook_events.last().map(|event| event.received_at),
        events: webhook_events,
    };

    let callbacks = OperatorCallbackHistory {
        configured: receipt.core.callback_url.is_some(),
        destination_url: receipt.core.callback_url.clone(),
        notification_count: callback_notifications.len(),
        delivered_count: callback_notifications
            .iter()
            .filter(|notification| notification.status == "delivered")
            .count(),
        pending_count: callback_notifications
            .iter()
            .filter(|notification| {
                notification.status == "scheduled"
                    || notification.status == "retry_scheduled"
                    || notification.status == "delivering"
            })
            .count(),
        dead_lettered_count: callback_notifications
            .iter()
            .filter(|notification| notification.status == "dead_lettered")
            .count(),
        delivery_attempt_count: callback_deliveries.len(),
        notifications: callback_notifications,
        deliveries: callback_deliveries,
    };

    let reconciliation = OperatorReconciliationHistory {
        latest: reconciliation_runs.last().cloned(),
        runs: reconciliation_runs,
    };

    let timeline = build_timeline(&receipt);
    let evidence_notes = build_evidence_notes(&receipt);

    OperatorReceipt {
        summary,
        ambiguity,
        attempts,
        provider_webhooks,
        callbacks,
        reconciliation,
        timeline,
        evidence_notes,
    }
}

fn build_summary(core: &PaymentReceipt) -> OperatorReceiptSummary {
    OperatorReceiptSummary {
        intent_id: core.intent_id,
        merchant_reference: core.merchant_reference.0.clone(),
        idempotency_key: core.idempotency_key.0.clone(),
        amount_minor: core.money.amount_minor,
        currency: core.money.currency.clone(),
        provider: core.provider.0.clone(),
        callback_url: core.callback_url.clone(),
        current_state: state_to_api(core.current_state).to_string(),
        final_classification: final_classification(core),
        latest_failure_classification: core
            .latest_failure
            .as_ref()
            .map(failure_to_api)
            .map(str::to_string),
        provider_reference: core
            .provider_reference
            .as_ref()
            .map(|reference| reference.0.clone()),
        generated_at: Utc::now(),
    }
}

fn build_timeline(receipt: &ComputedReceipt) -> Vec<OperatorTimelineEntry> {
    let mut entries = Vec::new();

    for transition in &receipt.core.timeline {
        entries.push(OperatorTimelineEntry {
            at: transition.at,
            kind: "state_transition".to_string(),
            title: format!("State changed to {}", state_to_api(transition.state)),
            detail: transition
                .note
                .clone()
                .unwrap_or_else(|| format!("intent entered {}", state_to_api(transition.state))),
            state: Some(state_to_api(transition.state).to_string()),
            evidence_source: None,
        });
    }

    for attempt in &receipt.core.attempts {
        entries.push(OperatorTimelineEntry {
            at: attempt.started_at,
            kind: "execution_attempt_started".to_string(),
            title: format!("Execution attempt {} started", attempt.attempt_no),
            detail: attempt
                .note
                .clone()
                .unwrap_or_else(|| "worker started an execution attempt".to_string()),
            state: None,
            evidence_source: None,
        });

        if let Some(ended_at) = attempt.ended_at {
            let outcome = attempt
                .outcome
                .as_ref()
                .map(attempt_outcome_label)
                .unwrap_or_else(|| "unknown".to_string());
            let reason = attempt
                .outcome
                .as_ref()
                .and_then(attempt_outcome_reason)
                .or_else(|| attempt.note.clone())
                .unwrap_or_else(|| "attempt finished".to_string());

            entries.push(OperatorTimelineEntry {
                at: ended_at,
                kind: "execution_attempt_finished".to_string(),
                title: format!("Execution attempt {} finished", attempt.attempt_no),
                detail: format!("outcome={outcome}; {reason}"),
                state: None,
                evidence_source: None,
            });
        }
    }

    for event in &receipt.provider_events {
        let status_hint = event
            .raw_payload
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        entries.push(OperatorTimelineEntry {
            at: event.received_at,
            kind: "provider_webhook".to_string(),
            title: "Provider webhook received".to_string(),
            detail: format!(
                "provider={} event_type={} status={} provider_reference={}",
                event.provider_name,
                event.event_type,
                status_hint,
                event
                    .provider_reference
                    .clone()
                    .unwrap_or_else(|| "n/a".to_string())
            ),
            state: None,
            evidence_source: Some("provider_webhook".to_string()),
        });
    }

    for notification in &receipt.callback_notifications {
        entries.push(OperatorTimelineEntry {
            at: notification.created_at,
            kind: "callback_notification_scheduled".to_string(),
            title: "Callback notification scheduled".to_string(),
            detail: format!(
                "target_state={} destination={} status={}",
                notification.target_state, notification.destination_url, notification.status
            ),
            state: Some(notification.target_state.clone()),
            evidence_source: None,
        });
    }

    for delivery in &receipt.callback_deliveries {
        entries.push(OperatorTimelineEntry {
            at: delivery.ended_at.unwrap_or(delivery.started_at),
            kind: "callback_delivery".to_string(),
            title: format!(
                "Callback attempt {} {}",
                delivery.attempt_no, delivery.delivery_result
            ),
            detail: format!(
                "destination={} http_status={} retry_count={}",
                delivery.destination_url,
                delivery
                    .http_status_code
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "n/a".to_string()),
                delivery.retry_count
            ),
            state: None,
            evidence_source: None,
        });
    }

    for run in &receipt.reconciliation_runs {
        entries.push(OperatorTimelineEntry {
            at: run.ended_at,
            kind: "reconciliation".to_string(),
            title: "Reconciliation run completed".to_string(),
            detail: format!(
                "provider_status={} comparison={} decision={}",
                run.provider_status_seen,
                recon_comparison_to_api(run.comparison),
                recon_decision_to_api(run.decision)
            ),
            state: Some(state_to_api(run.internal_status_seen).to_string()),
            evidence_source: Some(evidence_to_api(&run.evidence)),
        });
    }

    for event in &receipt.audit_events {
        if let Some(mapped) = map_audit_event_to_timeline(event) {
            entries.push(mapped);
        }
    }

    entries.sort_by_key(|entry| entry.at);
    entries
}

fn build_evidence_notes(receipt: &ComputedReceipt) -> Vec<OperatorEvidenceNote> {
    let mut notes = Vec::new();

    for transition in &receipt.core.timeline {
        if let Some(note) = &transition.note {
            notes.push(OperatorEvidenceNote {
                at: transition.at,
                source: "state_transition".to_string(),
                note: note.clone(),
            });
        }
    }

    for attempt in &receipt.core.attempts {
        if let Some(note) = &attempt.note {
            notes.push(OperatorEvidenceNote {
                at: attempt.ended_at.unwrap_or(attempt.started_at),
                source: format!("execution_attempt_{}", attempt.attempt_no),
                note: note.clone(),
            });
        }
    }

    for run in &receipt.reconciliation_runs {
        if let Some(note) = &run.note {
            notes.push(OperatorEvidenceNote {
                at: run.ended_at,
                source: "reconciliation".to_string(),
                note: note.clone(),
            });
        }
    }

    for notification in &receipt.callback_notifications {
        if let Some(last_error) = &notification.last_error {
            notes.push(OperatorEvidenceNote {
                at: notification.updated_at,
                source: "callback_notification".to_string(),
                note: last_error.clone(),
            });
        }
    }

    for event in &receipt.audit_events {
        if let Some(note) = event.payload.get("note").and_then(|value| value.as_str()) {
            notes.push(OperatorEvidenceNote {
                at: event.created_at,
                source: event.event_type.clone(),
                note: note.to_string(),
            });
        }

        if let Some(error_message) = event
            .payload
            .get("error_message")
            .and_then(|value| value.as_str())
        {
            notes.push(OperatorEvidenceNote {
                at: event.created_at,
                source: event.event_type.clone(),
                note: error_message.to_string(),
            });
        }
    }

    notes.sort_by_key(|note| note.at);
    notes
}

fn map_audit_event_to_timeline(event: &StoredAuditEvent) -> Option<OperatorTimelineEntry> {
    match event.event_type.as_str() {
        "lease_acquired" => Some(OperatorTimelineEntry {
            at: event.created_at,
            kind: "lease_acquired".to_string(),
            title: "Worker lease acquired".to_string(),
            detail: format!(
                "worker={} lease_token={} expires_at={}",
                event
                    .payload
                    .get("worker_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown"),
                event
                    .payload
                    .get("lease_token")
                    .and_then(|value| value.as_str())
                    .unwrap_or("n/a"),
                event
                    .payload
                    .get("lease_expires_at")
                    .and_then(|value| value.as_str())
                    .unwrap_or("n/a")
            ),
            state: Some("leased".to_string()),
            evidence_source: Some("worker_lease".to_string()),
        }),
        "execution_claimed_from_lease" => Some(OperatorTimelineEntry {
            at: event.created_at,
            kind: "execution_claimed_from_lease".to_string(),
            title: "Leased work moved into execution".to_string(),
            detail: event
                .payload
                .get("reason")
                .and_then(|value| value.as_str())
                .unwrap_or("worker consumed the lease for execution")
                .to_string(),
            state: Some("executing".to_string()),
            evidence_source: Some("worker_lease".to_string()),
        }),
        "lease_released" => Some(OperatorTimelineEntry {
            at: event.created_at,
            kind: "lease_released".to_string(),
            title: "Lease returned to queue".to_string(),
            detail: format!(
                "available_at={} reason={}",
                event
                    .payload
                    .get("available_at")
                    .and_then(|value| value.as_str())
                    .unwrap_or("n/a"),
                event
                    .payload
                    .get("reason")
                    .and_then(|value| value.as_str())
                    .unwrap_or("lease released")
            ),
            state: Some("queued".to_string()),
            evidence_source: Some("worker_lease".to_string()),
        }),
        "retry_scheduled" => Some(OperatorTimelineEntry {
            at: event.created_at,
            kind: "retry_scheduled".to_string(),
            title: "Retry scheduled".to_string(),
            detail: format!(
                "available_at={} reason={}",
                event
                    .payload
                    .get("available_at")
                    .and_then(|value| value.as_str())
                    .unwrap_or("n/a"),
                event
                    .payload
                    .get("reason")
                    .and_then(|value| value.as_str())
                    .unwrap_or("retry scheduled")
            ),
            state: Some("retry_scheduled".to_string()),
            evidence_source: Some("execution_worker".to_string()),
        }),
        "status_check_observed" => Some(OperatorTimelineEntry {
            at: event.created_at,
            kind: "status_check".to_string(),
            title: "Provider status check observed".to_string(),
            detail: format!(
                "observed_status={} attempts={} note={}",
                event
                    .payload
                    .get("observed_status")
                    .and_then(|value| value.as_str())
                    .unwrap_or("inconclusive"),
                event
                    .payload
                    .get("resolution_attempt_count")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0),
                event
                    .payload
                    .get("note")
                    .and_then(|value| value.as_str())
                    .unwrap_or("status check recorded")
            ),
            state: None,
            evidence_source: Some("provider_status_check".to_string()),
        }),
        "provider_webhook_applied" | "provider_webhook_recorded" | "provider_webhook_unmatched" => {
            Some(OperatorTimelineEntry {
                at: event.created_at,
                kind: event.event_type.clone(),
                title: "Webhook evidence evaluated".to_string(),
                detail: event
                    .payload
                    .get("note")
                    .and_then(|value| value.as_str())
                    .unwrap_or("provider webhook recorded")
                    .to_string(),
                state: event
                    .payload
                    .get("state_after")
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string()),
                evidence_source: Some("provider_webhook".to_string()),
            })
        }
        _ => None,
    }
}

fn map_reconciliation_run(run: &StoredReconciliationRun) -> OperatorReconciliationRun {
    OperatorReconciliationRun {
        started_at: run.started_at,
        ended_at: run.ended_at,
        provider_status_seen: run.provider_status_seen.clone(),
        internal_status_seen: state_to_api(run.internal_status_seen).to_string(),
        comparison: recon_comparison_to_api(run.comparison).to_string(),
        decision: recon_decision_to_api(run.decision).to_string(),
        evidence: evidence_to_api(&run.evidence),
        note: run.note.clone(),
    }
}

fn attempt_outcome_label(outcome: &AttemptOutcome) -> String {
    match outcome {
        AttemptOutcome::Succeeded => "succeeded".to_string(),
        AttemptOutcome::RetryableFailure { .. } => "retryable_failure".to_string(),
        AttemptOutcome::TerminalFailure { .. } => "terminal_failure".to_string(),
        AttemptOutcome::ProviderPending => "provider_pending".to_string(),
        AttemptOutcome::UnknownOutcome { .. } => "unknown_outcome".to_string(),
    }
}

fn attempt_outcome_classification(outcome: &AttemptOutcome) -> Option<String> {
    match outcome {
        AttemptOutcome::Succeeded | AttemptOutcome::ProviderPending => None,
        AttemptOutcome::RetryableFailure { classification, .. }
        | AttemptOutcome::TerminalFailure { classification, .. }
        | AttemptOutcome::UnknownOutcome { classification, .. } => {
            Some(failure_to_api(classification).to_string())
        }
    }
}

fn attempt_outcome_reason(outcome: &AttemptOutcome) -> Option<String> {
    match outcome {
        AttemptOutcome::RetryableFailure { reason, .. }
        | AttemptOutcome::TerminalFailure { reason, .. }
        | AttemptOutcome::UnknownOutcome { reason, .. } => Some(reason.clone()),
        AttemptOutcome::Succeeded | AttemptOutcome::ProviderPending => None,
    }
}

fn final_classification(core: &PaymentReceipt) -> Option<String> {
    match core.current_state {
        IntentState::Succeeded => Some("succeeded".to_string()),
        IntentState::FailedTerminal => Some("failed_terminal".to_string()),
        IntentState::UnknownOutcome => Some("unknown_outcome".to_string()),
        IntentState::ProviderPending => Some("provider_pending".to_string()),
        IntentState::ManualReview => Some("manual_review".to_string()),
        IntentState::DeadLettered => Some("dead_lettered".to_string()),
        IntentState::Rejected => core
            .latest_failure
            .as_ref()
            .map(failure_to_api)
            .or(Some("rejected"))
            .map(str::to_string),
        IntentState::RetryScheduled => core
            .latest_failure
            .as_ref()
            .map(failure_to_api)
            .map(str::to_string),
        IntentState::Reconciled => core.reconciliation.as_ref().map(|reconciliation| {
            match reconciliation.decision {
                ReconDecision::ConfirmSucceeded => "succeeded",
                ReconDecision::ConfirmFailedTerminal => "failed_terminal",
                ReconDecision::KeepUnknown => "unknown_outcome",
                ReconDecision::EscalateManualReview => "manual_review",
            }
            .to_string()
        }),
        IntentState::Received
        | IntentState::Validated
        | IntentState::Queued
        | IntentState::Leased
        | IntentState::Executing
        | IntentState::Reconciling => None,
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

fn evidence_to_api(evidence: &EvidenceSource) -> String {
    match evidence {
        EvidenceSource::ProviderWebhook { event_id } => {
            format!("provider_webhook:{event_id}")
        }
        EvidenceSource::ProviderStatusCheck { checked_at } => {
            format!("provider_status_check:{}", checked_at.to_rfc3339())
        }
        EvidenceSource::ManualOperatorDecision { operator_id, .. } => {
            format!("manual_operator_decision:{operator_id}")
        }
        EvidenceSource::InternalValidation => "internal_validation".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use domain::{
        ExecutionAttempt, FailureClassification, IdempotencyKey, MerchantReference, Money,
        ProviderName, ProviderReference, ReceiptTimelineEntry, ReconResult,
    };
    use persistence::{
        StoredAuditEvent, StoredCallbackDelivery, StoredCallbackNotification, StoredProviderEvent,
        StoredReconciliationRun,
    };
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn operator_receipt_makes_ambiguity_visible() {
        let at = Utc.with_ymd_and_hms(2026, 4, 27, 12, 0, 0).unwrap();
        let receipt = ComputedReceipt {
            core: PaymentReceipt {
                intent_id: Uuid::new_v4(),
                merchant_reference: MerchantReference("order_123".to_string()),
                idempotency_key: IdempotencyKey("idem_123".to_string()),
                money: Money::new(5000, "NGN"),
                provider: ProviderName("mockpay".to_string()),
                callback_url: Some("https://merchant.example/callbacks".to_string()),
                provider_reference: Some(ProviderReference("prov_123".to_string())),
                current_state: IntentState::UnknownOutcome,
                latest_failure: Some(FailureClassification::UnknownOutcome),
                timeline: vec![ReceiptTimelineEntry {
                    state: IntentState::UnknownOutcome,
                    at,
                    note: Some("timeout left the outcome ambiguous".to_string()),
                }],
                attempts: vec![ExecutionAttempt {
                    attempt_no: 1,
                    started_at: at,
                    ended_at: Some(at),
                    outcome: Some(AttemptOutcome::UnknownOutcome {
                        classification: FailureClassification::UnknownOutcome,
                        reason: "timeout after submission".to_string(),
                    }),
                    provider_reference: Some(ProviderReference("prov_123".to_string())),
                    note: Some("ambiguous provider timeout".to_string()),
                }],
                reconciliation: None,
                next_resolution_at: Some(at),
                last_resolution_at: Some(at),
                resolution_attempt_count: 2,
            },
            provider_events: vec![StoredProviderEvent {
                provider_name: "mockpay".to_string(),
                provider_event_id: "evt_1".to_string(),
                intent_id: None,
                provider_reference: Some("prov_123".to_string()),
                event_type: "payment.updated".to_string(),
                raw_payload: json!({"status":"pending"}),
                dedup_hash: "hash".to_string(),
                received_at: at,
                processed_at: Some(at),
            }],
            callback_notifications: vec![StoredCallbackNotification {
                event_key: "event".to_string(),
                intent_id: Uuid::new_v4(),
                destination_url: "https://merchant.example/callbacks".to_string(),
                target_state: "unknown_outcome".to_string(),
                payload: json!({"state":"unknown_outcome"}),
                status: "retry_scheduled".to_string(),
                next_attempt_at: at,
                attempt_count: 1,
                last_attempt_at: Some(at),
                delivered_at: None,
                last_http_status_code: Some(500),
                last_error: Some("downstream timeout".to_string()),
                created_at: at,
                updated_at: at,
            }],
            callback_deliveries: vec![StoredCallbackDelivery {
                intent_id: Uuid::new_v4(),
                destination_url: "https://merchant.example/callbacks".to_string(),
                attempt_no: 1,
                payload: json!({"state":"unknown_outcome"}),
                http_status_code: Some(500),
                delivery_result: "retry_scheduled".to_string(),
                started_at: at,
                ended_at: Some(at),
                retry_count: 0,
                response_body: Some("temporary failure".to_string()),
            }],
            reconciliation_runs: vec![],
            audit_events: vec![StoredAuditEvent {
                intent_id: None,
                event_type: "status_check_observed".to_string(),
                payload: json!({
                    "observed_status": "pending",
                    "resolution_attempt_count": 2,
                    "note": "provider still reports pending"
                }),
                created_at: at,
            }],
        };

        let operator = build_operator_receipt(receipt);

        assert!(operator.ambiguity.visible);
        assert_eq!(
            operator.summary.final_classification.as_deref(),
            Some("unknown_outcome")
        );
        assert!(operator
            .timeline
            .iter()
            .any(|entry| entry.kind == "status_check"));
        assert!(operator
            .evidence_notes
            .iter()
            .any(|note| note.note.contains("pending")));
    }

    #[test]
    fn operator_receipt_includes_reconciliation_history_and_final_classification() {
        let at = Utc.with_ymd_and_hms(2026, 4, 27, 13, 0, 0).unwrap();
        let recon = ReconResult {
            compared_at: at,
            internal_state: IntentState::UnknownOutcome,
            provider_state: "succeeded".to_string(),
            comparison: ReconComparison::Match,
            decision: ReconDecision::ConfirmSucceeded,
            evidence: EvidenceSource::ProviderStatusCheck { checked_at: at },
            note: Some("reconciliation confirmed provider success".to_string()),
        };

        let receipt = ComputedReceipt {
            core: PaymentReceipt {
                intent_id: Uuid::new_v4(),
                merchant_reference: MerchantReference("order_456".to_string()),
                idempotency_key: IdempotencyKey("idem_456".to_string()),
                money: Money::new(7500, "NGN"),
                provider: ProviderName("mockpay".to_string()),
                callback_url: None,
                provider_reference: Some(ProviderReference("prov_456".to_string())),
                current_state: IntentState::Reconciled,
                latest_failure: None,
                timeline: vec![ReceiptTimelineEntry {
                    state: IntentState::Reconciled,
                    at,
                    note: Some("reconciliation confirmed success".to_string()),
                }],
                attempts: vec![],
                reconciliation: Some(recon.clone()),
                next_resolution_at: None,
                last_resolution_at: Some(at),
                resolution_attempt_count: 1,
            },
            provider_events: vec![],
            callback_notifications: vec![],
            callback_deliveries: vec![],
            reconciliation_runs: vec![StoredReconciliationRun {
                intent_id: Uuid::new_v4(),
                started_at: at,
                ended_at: at,
                provider_status_seen: "succeeded".to_string(),
                internal_status_seen: IntentState::UnknownOutcome,
                comparison: ReconComparison::Match,
                decision: ReconDecision::ConfirmSucceeded,
                evidence: EvidenceSource::ProviderStatusCheck { checked_at: at },
                note: Some("reconciliation confirmed provider success".to_string()),
            }],
            audit_events: vec![],
        };

        let operator = build_operator_receipt(receipt);

        assert_eq!(
            operator.summary.final_classification.as_deref(),
            Some("succeeded")
        );
        assert_eq!(operator.reconciliation.runs.len(), 1);
        assert!(operator
            .timeline
            .iter()
            .any(|entry| entry.kind == "reconciliation"));
    }
}
