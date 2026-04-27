BEGIN;

ALTER TABLE payment_intents
    ADD COLUMN IF NOT EXISTS callback_url TEXT NULL;

CREATE TABLE IF NOT EXISTS callback_notifications (
    id BIGSERIAL PRIMARY KEY,
    event_key TEXT NOT NULL UNIQUE,
    intent_id UUID NOT NULL REFERENCES payment_intents(id) ON DELETE CASCADE,
    destination_url TEXT NOT NULL,
    target_state TEXT NOT NULL,
    payload JSONB NOT NULL,
    status TEXT NOT NULL,
    next_attempt_at TIMESTAMPTZ NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    last_attempt_at TIMESTAMPTZ NULL,
    delivered_at TIMESTAMPTZ NULL,
    last_http_status_code INTEGER NULL,
    last_error TEXT NULL,
    lease_owner TEXT NULL,
    lease_token UUID NULL,
    lease_expires_at TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_callback_notifications_due
    ON callback_notifications (next_attempt_at, created_at)
    WHERE status IN ('scheduled', 'retry_scheduled');

CREATE INDEX IF NOT EXISTS idx_callback_notifications_expired_lease
    ON callback_notifications (lease_expires_at, created_at)
    WHERE status = 'delivering';

CREATE INDEX IF NOT EXISTS idx_callback_notifications_intent_id
    ON callback_notifications (intent_id);

COMMIT;
