# Milestone 8

Provider Webhook Ingestion

## Goal

Handle provider-side asynchronous status updates safely.

## What This Milestone Adds

This milestone turns provider webhook arrival into a first-class evidence path.

The gateway can now:

- accept provider webhook calls through `POST /provider/webhooks/{provider}`
- store raw provider events durably in `provider_events`
- deduplicate repeated provider events before side effects
- map provider events back to an internal intent through provider reference or merchant reference
- update internal truth conservatively based on webhook evidence
- expose stored provider events through `GET /payment-intents/{id}/receipt`

## Current Endpoint Shape

### `POST /provider/webhooks/mockpay`

The current implementation supports the mock provider path used for failure-heavy scenarios.

The mock provider sends:

- `provider_event_id`
- `provider_reference`
- `event_type`
- `status`
- `merchant_reference`
- `amount_minor`
- `currency`
- `occurred_at`

If `MOCK_PROVIDER_WEBHOOK_SECRET` is configured, the gateway verifies the incoming
`X-Mockpay-Signature` header before accepting the event.

If no secret is configured, signature verification is skipped for local development.

### `GET /payment-intents/{id}/receipt`

This receipt endpoint now exposes:

- core intent summary
- execution attempts
- provider events
- callback deliveries
- audit events

That makes webhook evidence queryable instead of hidden inside logs.

## Safety Rules Implemented

### 1. Store raw event before business meaning is trusted

Webhook payloads are written into `provider_events` with:

- provider name
- provider event id
- internal intent id when known
- provider reference
- raw payload
- dedup hash
- received/processed timestamps

### 2. Deduplicate duplicate delivery

The gateway computes a durable dedup hash from:

- normalized provider name
- provider event id

If the same provider event arrives again, the second delivery is ignored safely.

### 3. Map webhook to internal lineage

The gateway tries to resolve the target lineage in this order:

1. provider reference
2. merchant reference fallback

This matters for timeout scenarios where the provider may know the payment but the gateway may not yet have a provider reference.

### 4. Do not regress truth on noisy or out-of-order events

Webhook evidence updates state conservatively.

Examples:

- `unknown_outcome -> succeeded` is allowed when webhook evidence confirms success
- `unknown_outcome -> provider_pending` is allowed and follow-up stays scheduled
- `provider_pending -> succeeded` is allowed
- already-terminal success is not regressed by a later contradictory webhook

The rule is simple:

record the event, but do not let a late or duplicate webhook corrupt a safer known truth.

## Mock Provider Integration

The mock provider now:

- sends stable `provider_event_id` values
- posts real webhook callbacks back to the gateway
- can emit duplicate webhook deliveries for the `duplicate_webhook` scenario

The worker now sends a callback URL to the mock provider through:

- `GATEWAY_PROVIDER_WEBHOOK_URL`

Default:

- `http://127.0.0.1:3000/provider/webhooks/mockpay`

## Tests Added

Webhook-focused tests now cover:

- delayed webhook success resolving an `unknown_outcome`
- duplicate webhook delivery being ignored after the first durable record
- out-of-order terminal webhook not regressing a known success

## Live Verification

This milestone was also smoke-tested end to end:

- `timeout_after_acceptance` finished as `succeeded` through webhook evidence
- `duplicate_webhook` finished as `succeeded`
- each receipt showed exactly one stored provider event after deduplication

## Definition of Done Check

This milestone is done because:

- webhooks can update internal truth safely
- duplicate provider events are harmless
- raw provider events are queryable through the receipt endpoint
- webhook arrival is reflected in stored receipt evidence
- tests exist for duplicate and delayed webhook behavior

## Why It Matters

Real providers do not behave like clean request/response APIs.

They are asynchronous, noisy, and repetitive.

A money-critical gateway has to treat webhook evidence as durable truth input, not as a best-effort side channel.
