#[cfg(test)]
mod tests {
    use chrono::Utc;

    use crate::{
        AttemptOutcome,
        EvidenceSource,
        FailureClassification,
        IntentState,
        PaymentIntent,
    };

    #[test]
    fn new_intent_starts_in_received_state() {
        let now = Utc::now();
        let intent = PaymentIntent::new(
            "order_123",
            "idem_123",
            5000,
            "NGN",
            "paystack",
            now
        ).unwrap();

        assert_eq!(intent.state, IntentState::Received);
        assert_eq!(intent.timeline.len(), 1);
    }

    #[test]
    fn invalid_transition_is_rejected() {
        let now = Utc::now();
        let mut intent = PaymentIntent::new(
            "order_123",
            "idem_123",
            5000,
            "NGN",
            "paystack",
            now
        ).unwrap();

        let result = intent.queue(now);
        assert!(result.is_err());
    }

    #[test]
    fn normal_happy_path_works() {
        let now = Utc::now();
        let mut intent = PaymentIntent::new(
            "order_123",
            "idem_123",
            5000,
            "NGN",
            "paystack",
            now
        ).unwrap();

        intent.validate(now).unwrap();
        intent.queue(now).unwrap();
        intent.lease(now).unwrap();
        intent.begin_execution(now).unwrap();

        intent
            .finish_current_attempt(
                now,
                AttemptOutcome::Succeeded,
                Some("prov_123".into()),
                Some("provider confirmed success".into())
            )
            .unwrap();

        assert_eq!(intent.state, IntentState::Succeeded);
        assert_eq!(intent.attempts.len(), 1);
    }

    #[test]
    fn terminal_state_cannot_be_retried() {
        let now = Utc::now();
        let mut intent = PaymentIntent::new(
            "order_456",
            "idem_456",
            7000,
            "NGN",
            "paystack",
            now
        ).unwrap();

        intent.validate(now).unwrap();
        intent.queue(now).unwrap();
        intent.lease(now).unwrap();
        intent.begin_execution(now).unwrap();

        intent
            .finish_current_attempt(
                now,
                AttemptOutcome::TerminalFailure {
                    classification: FailureClassification::TerminalProvider,
                    reason: "insufficient funds".into(),
                },
                Some("prov_456".into()),
                Some("provider rejected request".into())
            )
            .unwrap();

        let retry = intent.requeue_retry(now);
        assert!(retry.is_err());
    }

    #[test]
    fn unknown_outcome_requires_real_evidence_to_resolve() {
        let now = Utc::now();
        let mut intent = PaymentIntent::new(
            "order_789",
            "idem_789",
            9000,
            "NGN",
            "paystack",
            now
        ).unwrap();

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
                Some("prov_789".into()),
                Some("ambiguous outcome".into())
            )
            .unwrap();

        let result = intent.resolve_unknown_with_evidence(
            now,
            IntentState::Succeeded,
            EvidenceSource::InternalValidation,
            Some("should not work".into())
        );

        assert!(result.is_err());
    }

    #[test]
    fn webhook_evidence_can_resolve_unknown_outcome() {
        let now = Utc::now();
        let mut intent = PaymentIntent::new(
            "order_999",
            "idem_999",
            10000,
            "NGN",
            "paystack",
            now
        ).unwrap();

        intent.validate(now).unwrap();
        intent.queue(now).unwrap();
        intent.lease(now).unwrap();
        intent.begin_execution(now).unwrap();

        intent
            .finish_current_attempt(
                now,
                AttemptOutcome::UnknownOutcome {
                    classification: FailureClassification::UnknownOutcome,
                    reason: "timeout".into(),
                },
                Some("prov_999".into()),
                Some("unknown".into())
            )
            .unwrap();

        intent
            .resolve_unknown_with_evidence(
                now,
                IntentState::Succeeded,
                EvidenceSource::ProviderWebhook {
                    event_id: "evt_1".into(),
                },
                Some("provider webhook confirmed success".into())
            )
            .unwrap();

        assert_eq!(intent.state, IntentState::Succeeded);
    }
}
