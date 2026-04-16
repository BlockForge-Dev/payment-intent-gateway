# Phase 1 Implementation Blueprint

This document turns the project foundation into an implementation plan for the first serious build phase.

## Phase 1 Goal

Deliver a trustworthy vertical slice where a payment intent can be created, persisted durably, executed asynchronously through a mock provider, inspected through a receipt, and left in a truthful state when outcomes are ambiguous.

## Phase 1 Deliverables

- create and query payment intents
- enforce idempotency in storage
- lease queued intents to a worker safely
- record immutable execution attempts
- classify success, terminal failure, retryable failure, pending, and unknown outcome
- expose a receipt endpoint that tells the lifecycle story
- support provider event persistence and callback delivery persistence
- define reconciliation inputs and rules, even if the job itself lands slightly later

## Exact Database Tables

Phase 1 should use these durable tables.

### `payment_intents`

Purpose:

- canonical internal lineage for one business payment intent

Columns:

- `id UUID PRIMARY KEY`
- `merchant_reference TEXT NOT NULL`
- `amount_minor BIGINT NOT NULL CHECK (amount_minor > 0)`
- `currency TEXT NOT NULL`
- `provider TEXT NOT NULL`
- `state TEXT NOT NULL`
- `latest_failure_classification TEXT NULL`
- `provider_reference TEXT NULL`
- `created_at TIMESTAMPTZ NOT NULL`
- `updated_at TIMESTAMPTZ NOT NULL`

Indexes:

- state
- provider reference
- merchant reference

### `idempotency_keys`

Purpose:

- map one request identity to one business lineage

Columns:

- `id BIGSERIAL PRIMARY KEY`
- `scope TEXT NOT NULL`
- `idempotency_key TEXT NOT NULL`
- `intent_id UUID NOT NULL REFERENCES payment_intents(id)`
- `request_fingerprint TEXT NOT NULL`
- `created_at TIMESTAMPTZ NOT NULL`

Constraints:

- `UNIQUE (scope, idempotency_key)`

Behavior:

- same key plus same fingerprint returns existing lineage
- same key plus different fingerprint is an idempotency conflict

### `execution_attempts`

Purpose:

- preserve every execution try durably

Columns:

- `id BIGSERIAL PRIMARY KEY`
- `intent_id UUID NOT NULL REFERENCES payment_intents(id)`
- `attempt_no INTEGER NOT NULL CHECK (attempt_no > 0)`
- `started_at TIMESTAMPTZ NOT NULL`
- `ended_at TIMESTAMPTZ NULL`
- `request_payload_snapshot JSONB NOT NULL`
- `outcome_kind TEXT NULL`
- `raw_provider_response_summary JSONB NULL`
- `error_category TEXT NULL`
- `result_reason TEXT NULL`
- `provider_reference TEXT NULL`
- `note TEXT NULL`

Constraints:

- `UNIQUE (intent_id, attempt_no)`

### `provider_events`

Purpose:

- preserve inbound provider-side evidence and deduplicate processing

Columns:

- `id BIGSERIAL PRIMARY KEY`
- `provider_name TEXT NOT NULL`
- `provider_event_id TEXT NOT NULL`
- `intent_id UUID NULL REFERENCES payment_intents(id)`
- `provider_reference TEXT NULL`
- `event_type TEXT NOT NULL`
- `raw_payload JSONB NOT NULL`
- `dedup_hash TEXT NOT NULL`
- `received_at TIMESTAMPTZ NOT NULL`
- `processed_at TIMESTAMPTZ NULL`

Constraints:

- `UNIQUE (provider_name, dedup_hash)`

### `callback_deliveries`

Purpose:

- preserve outbound notification attempts independently from execution truth

Columns:

- `id BIGSERIAL PRIMARY KEY`
- `intent_id UUID NOT NULL REFERENCES payment_intents(id)`
- `destination_url TEXT NOT NULL`
- `attempt_no INTEGER NOT NULL CHECK (attempt_no > 0)`
- `payload JSONB NOT NULL`
- `http_status_code INTEGER NULL`
- `delivery_result TEXT NOT NULL`
- `started_at TIMESTAMPTZ NOT NULL`
- `ended_at TIMESTAMPTZ NULL`
- `retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0)`
- `response_body TEXT NULL`

Constraints:

- `UNIQUE (intent_id, destination_url, attempt_no)`

### `reconciliation_runs`

Purpose:

- preserve truth comparison between internal and provider state

Columns:

- `id BIGSERIAL PRIMARY KEY`
- `intent_id UUID NOT NULL REFERENCES payment_intents(id)`
- `started_at TIMESTAMPTZ NOT NULL`
- `ended_at TIMESTAMPTZ NOT NULL`
- `provider_status_seen TEXT NOT NULL`
- `internal_status_seen TEXT NOT NULL`
- `comparison_result TEXT NOT NULL`
- `decision TEXT NOT NULL`
- `evidence JSONB NOT NULL`
- `notes TEXT NULL`

### `audit_events`

Purpose:

- preserve state transitions and other operator-visible timeline evidence

Columns:

- `id BIGSERIAL PRIMARY KEY`
- `intent_id UUID NULL REFERENCES payment_intents(id)`
- `event_type TEXT NOT NULL`
- `payload JSONB NOT NULL`
- `created_at TIMESTAMPTZ NOT NULL`

### Receipt Read Model Strategy

For Phase 1, use a computed read model rather than a separate `receipt_snapshots` table.

That keeps writes simpler while the domain is still stabilizing.

The receipt should be assembled from:

- `payment_intents`
- `execution_attempts`
- `provider_events`
- `callback_deliveries`
- `reconciliation_runs`
- `audit_events`

## Exact Lifecycle Transitions

Phase 1 should keep the state model explicit and conservative.

Allowed transitions:

- `received -> validated`
- `received -> rejected`
- `validated -> queued`
- `validated -> rejected`
- `queued -> leased`
- `queued -> executing`
- `leased -> executing`
- `executing -> succeeded`
- `executing -> failed_terminal`
- `executing -> retry_scheduled`
- `executing -> provider_pending`
- `executing -> unknown_outcome`
- `retry_scheduled -> queued`
- `provider_pending -> reconciling`
- `provider_pending -> succeeded`
- `provider_pending -> failed_terminal`
- `provider_pending -> manual_review`
- `unknown_outcome -> reconciling`
- `unknown_outcome -> succeeded`
- `unknown_outcome -> failed_terminal`
- `unknown_outcome -> manual_review`
- `reconciling -> reconciled`
- `reconciling -> unknown_outcome`
- `reconciling -> manual_review`
- `reconciling -> succeeded`
- `reconciling -> failed_terminal`
- `* -> dead_lettered` only when the current state is not terminal

Interpretation rule:

- `reconciled` means a reconciliation process completed and preserved a recon decision as evidence
- `succeeded` and `failed_terminal` remain the business outcome states when the gateway has strong enough evidence before or during reconciliation

## API Contract

Phase 1 should expose these endpoints.

### `POST /payment-intents`

Purpose:

- ingest and durably create or reuse a payment lineage

Request body:

```json
{
  "merchant_reference": "order_123",
  "idempotency_key": "idem_123",
  "amount_minor": 5000,
  "currency": "NGN",
  "provider": "mock",
  "callback_url": "https://merchant.example/callbacks/payments"
}
```

Response on create:

```json
{
  "intent_id": "uuid",
  "state": "queued",
  "created": true
}
```

Response on duplicate idempotent replay:

```json
{
  "intent_id": "uuid",
  "state": "queued",
  "created": false
}
```

Errors:

- `400` invalid payload
- `409` idempotency key reused with conflicting payload
- `401` or `403` authentication failure

### `GET /payment-intents/{id}`

Purpose:

- lightweight current status query

Response shape:

```json
{
  "intent_id": "uuid",
  "merchant_reference": "order_123",
  "provider": "mock",
  "state": "unknown_outcome",
  "provider_reference": "prov_123",
  "latest_failure_classification": "unknown_outcome",
  "updated_at": "2026-04-15T12:00:00Z"
}
```

### `GET /payment-intents/{id}/receipt`

Purpose:

- operator-readable truth surface

Response should include:

- intent identity and summary
- timeline of transitions
- execution attempts
- provider event summary
- callback delivery history
- reconciliation history

### `POST /provider/webhooks/{provider}`

Purpose:

- ingest provider-side asynchronous evidence

Rules:

- verify signature when real provider integration exists
- store raw event first
- deduplicate before side effects
- update internal truth idempotently

## Worker Flow

Phase 1 worker flow should be:

1. Select eligible intents in `queued` or expired `leased` state.
2. Acquire lease using a transaction and row-level locking.
3. Transition the intent to `leased`.
4. Transition to `executing` and append attempt `N`.
5. Build provider request and snapshot it into the attempt record.
6. Call the provider adapter.
7. Classify the outcome.
8. Persist attempt completion and update intent state in one transaction.
9. If the state changes to a callback-worthy state, schedule callback delivery.
10. If the outcome is ambiguous or still pending, mark the intent for later reconciliation.

Lease rules:

- one intent may be leased by only one worker at a time
- lease expiry must allow recovery after crash
- lease recovery must not fabricate duplicate attempts

## Outcome Classification Rules

Provider responses must be translated into these internal outcomes:

### `Succeeded`

Use when the provider response is explicit, durable enough, and tied to a provider reference.

State result:

- `succeeded`

### `TerminalFailure`

Use when the provider rejects the request in a way that should not be retried automatically.

Examples:

- insufficient funds
- invalid account
- unsupported action

State result:

- `failed_terminal`

### `RetryableFailure`

Use when infrastructure or provider availability caused a safe-to-retry failure before durable provider acceptance is believed to have happened.

Examples:

- provider unavailable before request acceptance
- transient internal dependency failure before submit

State result:

- `retry_scheduled`

### `ProviderPending`

Use when the provider explicitly accepts the request into a pending or asynchronous state.

State result:

- `provider_pending`

### `UnknownOutcome`

Use when the request may have reached the provider but the gateway cannot safely determine the result.

Examples:

- timeout after request write
- connection lost after ambiguous submit

State result:

- `unknown_outcome`

Critical rule:

- do not auto-retry unknown outcome

## Retry Rules

Retry policy belongs to the gateway, not the provider adapter.

Automatic retry is allowed only for `RetryableFailure`.

Automatic retry is not allowed for:

- `Succeeded`
- `FailedTerminal`
- `UnknownOutcome`
- `ProviderPending`

Suggested Phase 1 retry schedule:

- attempt 1 retry after 30 seconds
- attempt 2 retry after 2 minutes
- attempt 3 retry after 10 minutes
- attempt 4 retry after 30 minutes
- after max attempts, move to `manual_review` or `dead_lettered` based on operator policy

Store retry intent via durable state and timestamps, not in-memory timers alone.

## Webhook Processing Rules

1. Persist the raw event before applying business effects.
2. Deduplicate using provider name plus dedup hash.
3. Resolve target lineage using `intent_id` or `provider_reference`.
4. Ignore duplicate events after the first durable record.
5. Do not regress state unsafely on out-of-order events.
6. Append audit evidence for operator visibility.

## Callback Delivery Rules

Callback delivery must remain operationally separate from execution truth.

Trigger callbacks for important state changes such as:

- `succeeded`
- `failed_terminal`
- `provider_pending`
- `unknown_outcome`
- `reconciled`
- `manual_review`

Rules:

- callback failure never changes the payment outcome
- delivery retries only affect callback history
- all delivery attempts are durably recorded

## Reconciliation Rules

Phase 1 should at minimum define these rules, even if the reconciler app lands after API and worker.

### Inputs

- internal current state
- provider reference
- latest provider status lookup
- stored provider events
- latest attempt outcome

### Comparison Results

- `match`
- `mismatch`
- `unresolved`

### Decisions

- `confirm_succeeded`
- `confirm_failed_terminal`
- `keep_unknown`
- `escalate_manual_review`

### Decision Rules

- internal `unknown_outcome` plus provider success becomes `confirm_succeeded`
- internal `unknown_outcome` plus provider terminal failure becomes `confirm_failed_terminal`
- internal `provider_pending` plus provider still pending becomes `keep_unknown` or remains pending with evidence
- internal success plus provider missing becomes `escalate_manual_review`
- internal failure plus provider success becomes `escalate_manual_review`
- missing provider reference and no trustworthy provider-side lookup path also becomes `escalate_manual_review`

Record every reconciliation run even when no state changes.

## Repository Build Order

Build the Rust repo in this order to keep boundaries clean.

1. `crates/domain`
2. `crates/persistence`
3. `crates/application`
4. `crates/adapters/mock_provider`
5. `crates/adapters/paystack`
6. `crates/callbacks`
7. `crates/receipts`
8. `apps/api`
9. `apps/worker`
10. `apps/reconciler`

Ownership by layer:

- `domain`: aggregate state machine, attempts, failure classes, receipt types, reconciliation types
- `persistence`: schema access, transactions, receipt assembly
- `application`: use cases and orchestration
- `adapters`: provider integrations
- `callbacks`: payload signing and delivery helpers
- `receipts`: response shaping
- `apps`: process entrypoints

## Definition of Done for Phase 1

Phase 1 is done when all of these are true:

- `POST /payment-intents` durably creates or reuses a lineage through idempotency
- `GET /payment-intents/{id}` returns current truth without side effects
- `GET /payment-intents/{id}/receipt` explains the full lifecycle
- worker leasing prevents accidental concurrent execution
- every attempt is stored durably
- retryable failures schedule retry safely
- unknown outcomes are preserved without blind retry
- provider events and callback deliveries are queryable evidence
- reconciliation rules are encoded clearly enough to implement next without redesign

## Current Repo Mapping

The current repository already contains strong groundwork for this blueprint:

- domain lifecycle and invariants exist in `crates/domain`
- durable schema and repository logic exist in `crates/persistence`
- the root binary currently only serves as a placeholder

That means the next code milestone should be:

Add `crates/application` and `apps/api`, then drive the first end-to-end path against a mock provider before building the worker and reconciler binaries.
