BEGIN;

CREATE TABLE IF NOT EXISTS payment_intents (
    id UUID PRIMARY KEY,
    merchant_reference TEXT NOT NULL,
    amount_minor BIGINT NOT NULL CHECK (amount_minor > 0),
    currency TEXT NOT NULL CHECK (length(trim(currency)) > 0),
    provider TEXT NOT NULL CHECK (length(trim(provider)) > 0),
    state TEXT NOT NULL,
    latest_failure_classification TEXT NULL,
    provider_reference TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_payment_intents_state
    ON payment_intents (state);

CREATE INDEX IF NOT EXISTS idx_payment_intents_provider_reference
    ON payment_intents (provider_reference);

CREATE INDEX IF NOT EXISTS idx_payment_intents_merchant_reference
    ON payment_intents (merchant_reference);

CREATE TABLE IF NOT EXISTS idempotency_keys (
    id BIGSERIAL PRIMARY KEY,
    scope TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    intent_id UUID NOT NULL REFERENCES payment_intents(id) ON DELETE RESTRICT,
    request_fingerprint TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    UNIQUE (scope, idempotency_key)
);

CREATE INDEX IF NOT EXISTS idx_idempotency_keys_intent_id
    ON idempotency_keys (intent_id);

CREATE TABLE IF NOT EXISTS execution_attempts (
    id BIGSERIAL PRIMARY KEY,
    intent_id UUID NOT NULL REFERENCES payment_intents(id) ON DELETE CASCADE,
    attempt_no INTEGER NOT NULL CHECK (attempt_no > 0),
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ NULL,
    request_payload_snapshot JSONB NOT NULL,
    outcome_kind TEXT NULL,
    raw_provider_response_summary JSONB NULL,
    error_category TEXT NULL,
    result_reason TEXT NULL,
    provider_reference TEXT NULL,
    note TEXT NULL,
    UNIQUE (intent_id, attempt_no)
);

CREATE INDEX IF NOT EXISTS idx_execution_attempts_intent_id
    ON execution_attempts (intent_id);

CREATE TABLE IF NOT EXISTS provider_events (
    id BIGSERIAL PRIMARY KEY,
    provider_name TEXT NOT NULL,
    provider_event_id TEXT NOT NULL,
    intent_id UUID NULL REFERENCES payment_intents(id) ON DELETE SET NULL,
    provider_reference TEXT NULL,
    event_type TEXT NOT NULL,
    raw_payload JSONB NOT NULL,
    dedup_hash TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL,
    processed_at TIMESTAMPTZ NULL,
    UNIQUE (provider_name, dedup_hash)
);

CREATE INDEX IF NOT EXISTS idx_provider_events_intent_id
    ON provider_events (intent_id);

CREATE INDEX IF NOT EXISTS idx_provider_events_provider_reference
    ON provider_events (provider_reference);

CREATE TABLE IF NOT EXISTS callback_deliveries (
    id BIGSERIAL PRIMARY KEY,
    intent_id UUID NOT NULL REFERENCES payment_intents(id) ON DELETE CASCADE,
    destination_url TEXT NOT NULL,
    attempt_no INTEGER NOT NULL CHECK (attempt_no > 0),
    payload JSONB NOT NULL,
    http_status_code INTEGER NULL,
    delivery_result TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ NULL,
    retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
    response_body TEXT NULL,
    UNIQUE (intent_id, destination_url, attempt_no)
);

CREATE INDEX IF NOT EXISTS idx_callback_deliveries_intent_id
    ON callback_deliveries (intent_id);

CREATE TABLE IF NOT EXISTS reconciliation_runs (
    id BIGSERIAL PRIMARY KEY,
    intent_id UUID NOT NULL REFERENCES payment_intents(id) ON DELETE CASCADE,
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ NOT NULL,
    provider_status_seen TEXT NOT NULL,
    internal_status_seen TEXT NOT NULL,
    comparison_result TEXT NOT NULL,
    decision TEXT NOT NULL,
    evidence JSONB NOT NULL,
    notes TEXT NULL
);

CREATE INDEX IF NOT EXISTS idx_reconciliation_runs_intent_id
    ON reconciliation_runs (intent_id);

CREATE TABLE IF NOT EXISTS audit_events (
    id BIGSERIAL PRIMARY KEY,
    intent_id UUID NULL REFERENCES payment_intents(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_events_intent_id
    ON audit_events (intent_id);

CREATE INDEX IF NOT EXISTS idx_audit_events_event_type
    ON audit_events (event_type);

COMMIT;