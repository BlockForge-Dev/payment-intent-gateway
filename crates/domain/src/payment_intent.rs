use chrono::{ DateTime, Utc };
use serde::{ Deserialize, Serialize };
use uuid::Uuid;

use crate::{
    AttemptOutcome,
    DomainError,
    EvidenceSource,
    ExecutionAttempt,
    FailureClassification,
    IdempotencyKey,
    IntentId,
    IntentState,
    MerchantReference,
    Money,
    PaymentReceipt,
    ProviderName,
    ProviderReference,
    ReceiptTimelineEntry,
    ReconDecision,
    ReconResult,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentIntent {
    pub id: IntentId,
    pub merchant_reference: MerchantReference,
    pub idempotency_key: IdempotencyKey,
    pub money: Money,
    pub provider: ProviderName,
    pub provider_reference: Option<ProviderReference>,
    pub state: IntentState,
    pub latest_failure: Option<FailureClassification>,
    pub attempts: Vec<ExecutionAttempt>,
    pub reconciliation: Option<ReconResult>,
    pub timeline: Vec<ReceiptTimelineEntry>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PaymentIntent {
    pub fn new(
        merchant_reference: impl Into<String>,
        idempotency_key: impl Into<String>,
        amount_minor: i64,
        currency: impl Into<String>,
        provider: impl Into<String>,
        now: DateTime<Utc>
    ) -> Result<Self, DomainError> {
        let merchant_reference = merchant_reference.into();
        let idempotency_key = idempotency_key.into();
        let currency = currency.into();
        let provider = provider.into();

        if merchant_reference.trim().is_empty() {
            return Err(DomainError::EmptyMerchantReference);
        }
        if idempotency_key.trim().is_empty() {
            return Err(DomainError::EmptyIdempotencyKey);
        }
        if provider.trim().is_empty() {
            return Err(DomainError::EmptyProvider);
        }
        if currency.trim().is_empty() {
            return Err(DomainError::EmptyCurrency);
        }
        if amount_minor <= 0 {
            return Err(DomainError::InvalidAmount);
        }

        Ok(Self {
            id: Uuid::new_v4(),
            merchant_reference: MerchantReference(merchant_reference),
            idempotency_key: IdempotencyKey(idempotency_key),
            money: Money::new(amount_minor, currency),
            provider: ProviderName(provider),
            provider_reference: None,
            state: IntentState::Received,
            latest_failure: None,
            attempts: Vec::new(),
            reconciliation: None,
            timeline: vec![ReceiptTimelineEntry {
                state: IntentState::Received,
                at: now,
                note: Some("intent durably accepted into the system".to_string()),
            }],
            created_at: now,
            updated_at: now,
        })
    }

    pub fn validate(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        self.transition_to(IntentState::Validated, now, Some("intent validated".into()))
    }

    pub fn reject(&mut self, now: DateTime<Utc>, reason: String) -> Result<(), DomainError> {
        self.latest_failure = Some(FailureClassification::Validation);
        self.transition_to(IntentState::Rejected, now, Some(reason))
    }

    pub fn queue(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        self.transition_to(IntentState::Queued, now, Some("intent queued for execution".into()))
    }

    pub fn lease(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        self.transition_to(IntentState::Leased, now, Some("worker lease acquired".into()))
    }

    pub fn begin_execution(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        if !self.state.can_begin_execution() {
            return Err(DomainError::InvalidStateTransition {
                from: self.state,
                to: IntentState::Executing,
            });
        }

        self.transition_to(IntentState::Executing, now, Some("execution attempt started".into()))?;

        let next_attempt_no = (self.attempts.len() as u32) + 1;
        self.attempts.push(ExecutionAttempt::started(next_attempt_no, now));
        Ok(())
    }

    pub fn finish_current_attempt(
        &mut self,
        now: DateTime<Utc>,
        outcome: AttemptOutcome,
        provider_reference: Option<String>,
        note: Option<String>
    ) -> Result<(), DomainError> {
        let last = self.attempts.pop().ok_or(DomainError::InvalidAttemptNumber)?;

        let provider_reference = provider_reference.map(ProviderReference);

        let updated_attempt = last.finish(
            now,
            outcome.clone(),
            provider_reference.clone(),
            note.clone()
        );
        self.attempts.push(updated_attempt);

        if let Some(pref) = provider_reference {
            self.provider_reference = Some(pref);
        }

        match outcome {
            AttemptOutcome::Succeeded => {
                self.latest_failure = None;
                self.transition_to(IntentState::Succeeded, now, note)?;
            }
            AttemptOutcome::RetryableFailure { classification, .. } => {
                self.latest_failure = Some(classification);
                self.transition_to(IntentState::RetryScheduled, now, note)?;
            }
            AttemptOutcome::TerminalFailure { classification, .. } => {
                self.latest_failure = Some(classification);
                self.transition_to(IntentState::FailedTerminal, now, note)?;
            }
            AttemptOutcome::ProviderPending => {
                self.latest_failure = None;
                self.transition_to(IntentState::ProviderPending, now, note)?;
            }
            AttemptOutcome::UnknownOutcome { classification, .. } => {
                self.latest_failure = Some(classification);
                self.transition_to(IntentState::UnknownOutcome, now, note)?;
            }
        }

        Ok(())
    }

    pub fn requeue_retry(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        if self.state == IntentState::FailedTerminal || self.state.is_terminal() {
            return Err(DomainError::TerminalStateNotRetryable(self.state));
        }

        if self.state != IntentState::RetryScheduled {
            return Err(DomainError::InvalidStateTransition {
                from: self.state,
                to: IntentState::Queued,
            });
        }

        self.transition_to(IntentState::Queued, now, Some("retry re-queued".into()))
    }

    pub fn begin_reconciliation(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        if !self.state.needs_reconciliation() {
            return Err(DomainError::InvalidStateTransition {
                from: self.state,
                to: IntentState::Reconciling,
            });
        }

        self.transition_to(IntentState::Reconciling, now, Some("reconciliation started".into()))
    }

    pub fn apply_reconciliation(
        &mut self,
        result: ReconResult,
        now: DateTime<Utc>
    ) -> Result<(), DomainError> {
        self.reconciliation = Some(result.clone());

        match result.decision {
            ReconDecision::ConfirmSucceeded => {
                self.latest_failure = None;
                self.transition_to(
                    IntentState::Reconciled,
                    now,
                    Some("reconciliation confirmed success".into())
                )?;
            }
            ReconDecision::ConfirmFailedTerminal => {
                self.latest_failure = Some(FailureClassification::TerminalProvider);
                self.transition_to(
                    IntentState::Reconciled,
                    now,
                    Some("reconciliation confirmed terminal failure".into())
                )?;
            }
            ReconDecision::KeepUnknown => {
                self.transition_to(
                    IntentState::UnknownOutcome,
                    now,
                    Some("reconciliation kept state as unknown".into())
                )?;
            }
            ReconDecision::EscalateManualReview => {
                self.transition_to(
                    IntentState::ManualReview,
                    now,
                    Some("reconciliation escalated to manual review".into())
                )?;
            }
        }

        Ok(())
    }

    pub fn resolve_unknown_with_evidence(
        &mut self,
        now: DateTime<Utc>,
        to_state: IntentState,
        evidence: EvidenceSource,
        note: Option<String>
    ) -> Result<(), DomainError> {
        if
            self.state != IntentState::UnknownOutcome &&
            self.state != IntentState::ProviderPending &&
            self.state != IntentState::Reconciling
        {
            return Err(DomainError::EvidenceRequiredForUnknownResolution);
        }

        match evidence {
            | EvidenceSource::ProviderWebhook { .. }
            | EvidenceSource::ProviderStatusCheck { .. }
            | EvidenceSource::ManualOperatorDecision { .. } => {}
            EvidenceSource::InternalValidation => {
                return Err(DomainError::EvidenceRequiredForUnknownResolution);
            }
        }

        match to_state {
            IntentState::Succeeded | IntentState::FailedTerminal | IntentState::ManualReview => {
                self.transition_to(to_state, now, note)
            }
            _ =>
                Err(DomainError::InvalidStateTransition {
                    from: self.state,
                    to: to_state,
                }),
        }
    }

    pub fn record_callback_delivery_failure(&self) -> Result<(), DomainError> {
        Err(DomainError::CallbackFailureCannotMutateExecutionTruth)
    }

    pub fn to_receipt(&self) -> PaymentReceipt {
        PaymentReceipt {
            intent_id: self.id,
            merchant_reference: self.merchant_reference.clone(),
            idempotency_key: self.idempotency_key.clone(),
            money: self.money.clone(),
            provider: self.provider.clone(),
            provider_reference: self.provider_reference.clone(),
            current_state: self.state,
            latest_failure: self.latest_failure.clone(),
            timeline: self.timeline.clone(),
            attempts: self.attempts.clone(),
            reconciliation: self.reconciliation.clone(),
        }
    }

    fn transition_to(
        &mut self,
        to: IntentState,
        now: DateTime<Utc>,
        note: Option<String>
    ) -> Result<(), DomainError> {
        if !Self::is_valid_transition(self.state, to) {
            return Err(DomainError::InvalidStateTransition {
                from: self.state,
                to,
            });
        }

        self.state = to;
        self.updated_at = now;
        self.timeline.push(ReceiptTimelineEntry { state: to, at: now, note });
        Ok(())
    }

    fn is_valid_transition(from: IntentState, to: IntentState) -> bool {
        use IntentState::*;

        match (from, to) {
            (Received, Validated) => true,
            (Received, Rejected) => true,

            (Validated, Queued) => true,
            (Validated, Rejected) => true,

            (Queued, Leased) => true,
            (Queued, Executing) => true,

            (Leased, Executing) => true,

            (Executing, RetryScheduled) => true,
            (Executing, ProviderPending) => true,
            (Executing, UnknownOutcome) => true,
            (Executing, Succeeded) => true,
            (Executing, FailedTerminal) => true,

            (RetryScheduled, Queued) => true,

            (ProviderPending, Reconciling) => true,
            (ProviderPending, Succeeded) => true,
            (ProviderPending, FailedTerminal) => true,
            (ProviderPending, ManualReview) => true,

            (UnknownOutcome, Reconciling) => true,
            (UnknownOutcome, Succeeded) => true,
            (UnknownOutcome, FailedTerminal) => true,
            (UnknownOutcome, ManualReview) => true,

            (Reconciling, Reconciled) => true,
            (Reconciling, UnknownOutcome) => true,
            (Reconciling, ManualReview) => true,
            (Reconciling, Succeeded) => true,
            (Reconciling, FailedTerminal) => true,

            // optional escalation paths
            (_, DeadLettered) if !from.is_terminal() => true,

            _ => false,
        }
    }
}
