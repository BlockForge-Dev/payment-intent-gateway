# Milestone 9

Callback Delivery Engine

## Goal

Notify downstream consumers of relevant payment state changes reliably.

## What This Milestone Adds

This milestone turns outbound callback delivery into its own durable workflow.

The gateway can now:

- accept an optional downstream `callback_url` when creating a payment intent
- treat `callback_url` as part of idempotent request identity
- schedule callback notifications durably when callback-worthy state transitions happen
- lease and deliver callback work through a dedicated `callback-worker`
- retry failed callback deliveries without re-executing the payment intent
- store delivery attempt history in `callback_deliveries`
- expose both callback queue state and callback delivery history through the receipt

## New Persistence Model

### `payment_intents.callback_url`

Each intent can now carry an optional downstream callback destination.

This keeps callback routing attached to the business lineage instead of hiding it in worker memory.

### `callback_notifications`

This new table is the durable callback queue.

Each row stores:

- the triggering event key
- target intent id
- destination URL
- target state
- frozen callback payload
- queue status
- next attempt time
- attempt counters
- lease metadata
- last error / last status code

This is important because `callback_deliveries` is history, not a queue.

## Callback Scheduling Rules

Callbacks are scheduled only for callback-worthy state changes, not for internal coordination states.

The current implementation schedules notifications for:

- `provider_pending`
- `unknown_outcome`
- `succeeded`
- `failed_terminal`
- `reconciled`
- `manual_review`
- `dead_lettered`

Scheduling happens in the same database transaction as the state change in these paths:

- execution attempt completion
- provider webhook application
- unknown-outcome status check updates
- reconciliation result persistence

That means the system does not rely on “remembering later” to notify downstream consumers.

## Delivery Worker

Milestone 9 adds a new app:

- `apps/callback-worker`

The callback worker:

- leases due callback notifications with `FOR UPDATE SKIP LOCKED`
- sends the stored payload to the downstream destination
- records every delivery attempt durably
- marks the notification as delivered, retry-scheduled, or dead-lettered

This keeps callback delivery separate from payment execution workers.

## Retry Policy

The current retry policy is intentionally simple and safe:

- HTTP `2xx` marks the callback as delivered
- any other failure is retried until `CALLBACK_MAX_ATTEMPTS`
- once attempts are exhausted, the notification is marked `dead_lettered`

The important boundary is preserved:

callback failure does not mutate payment execution truth.

## Optional Signing

If `CALLBACK_SIGNING_SECRET` is configured, outbound callbacks include:

- `X-Gateway-Signature`

The current implementation computes a deterministic SHA-256 signature over:

- `secret`
- `:`
- serialized JSON payload

This is optional and useful for local or internal verification flows.

## API Changes

### `POST /payment-intents`

The request body now accepts:

- `callback_url` (optional)

If present:

- it must be a valid `http` or `https` URL
- it becomes part of the idempotency fingerprint

So:

- same idempotency key + same payload + same callback URL returns the same lineage
- same idempotency key + different callback URL is rejected as a conflict

## Receipt Changes

`GET /payment-intents/{id}/receipt` now exposes:

- callback notifications
- callback deliveries

That lets operators see:

- what was scheduled
- what was attempted
- what failed
- whether the payment itself already succeeded independently

## Tests Added

Callback-focused tests now cover:

- successful callback delivery
- failed delivery scheduling a retry
- dead-lettering after maximum attempts
- callback URL validation
- idempotency conflict when the callback URL changes under the same key

## Definition of Done Check

This milestone is done because:

- downstream callback delivery is triggered after relevant state changes
- failures are retried durably
- callback delivery attempts are stored durably
- callback failure is inspectable separately from execution truth
- callback retries do not re-execute the payment intent

## Why It Matters

Money systems need a clean boundary between:

- what is true
- and who has been notified

This milestone makes that boundary explicit.

The payment outcome can already be final while callback delivery is still failing, retrying, or dead-lettered.

That separation is one of the clearest signs that the system is modeling operational truth correctly.
