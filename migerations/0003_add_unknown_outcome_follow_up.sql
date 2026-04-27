BEGIN;

ALTER TABLE payment_intents
    ADD COLUMN IF NOT EXISTS next_resolution_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS last_resolution_at TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS resolution_attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (resolution_attempt_count >= 0);

CREATE INDEX IF NOT EXISTS idx_payment_intents_resolution_due
    ON payment_intents (next_resolution_at, created_at)
    WHERE state IN ('unknown_outcome', 'provider_pending') AND next_resolution_at IS NOT NULL;

COMMIT;