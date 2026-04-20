BEGIN;

ALTER TABLE payment_intents
    ADD COLUMN IF NOT EXISTS available_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS lease_owner TEXT NULL,
    ADD COLUMN IF NOT EXISTS lease_token UUID NULL,
    ADD COLUMN IF NOT EXISTS lease_expires_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS last_leased_at TIMESTAMPTZ NULL;

CREATE INDEX IF NOT EXISTS idx_payment_intents_queue_ready
    ON payment_intents (state, available_at, created_at)
    WHERE state IN ('queued', 'retry_scheduled');

CREATE INDEX IF NOT EXISTS idx_payment_intents_expired_lease
    ON payment_intents (lease_expires_at, created_at)
    WHERE state = 'leased' AND lease_expires_at IS NOT NULL;

COMMIT;