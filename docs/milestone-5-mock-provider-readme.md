# Milestone 5 — Mock Provider Simulator

## Goal

Build a simulated provider so failure scenarios can be exercised deliberately.

This milestone is not about real payment rails.

It is about building a controllable failure lab for the Payment Intent Gateway.

The mock provider exists so later milestones can force difficult situations on demand instead of waiting for real providers to misbehave.

That matters because our project is about:

- trust
- reliability
- execution safety
- reconciliation
- coordination
- operational truth

A system like that needs a way to create known bad conditions on purpose.

---

## Why this milestone matters

Without a mock provider, the rest of the gateway is forced to develop against:

- happy-path assumptions
- uncontrolled failures
- real provider limitations
- inconsistent reproducibility

That is weak.

A reliability-first system needs a simulator that can deliberately produce:

- success
- failure
- ambiguity
- delay
- duplicate evidence
- contradictory evidence
- pending resolution over time

This milestone gives us that.

---

## What this milestone provides

The mock provider is a standalone app that exposes:

- `POST /mock-provider/payments`
- `GET /mock-provider/payments/:provider_reference`
- `POST /mock-provider/payments/:provider_reference/webhooks/replay`
- `GET /mock-provider/scenarios`
- `GET /health`

The simulator stores provider-side payment records in memory.

That is acceptable here because this app is not the source of truth for the gateway.

The gateway’s truth still lives in Postgres.

The mock provider is only a controllable external dependency.

---

## Supported scenarios

### 1. `immediate_success`
The provider accepts immediately and reports `succeeded`.

Use this to validate:
- normal successful execution
- immediate success receipts
- straightforward callback flow

### 2. `terminal_failure`
The provider processes the request but rejects it terminally.

Use this to validate:
- non-retryable failures
- terminal classification
- no unsafe auto-retry

### 3. `retryable_infra_error`
The provider returns `503 Service Unavailable` and does not create a provider-side payment record.

Use this to validate:
- retryable infrastructure failure classification
- retry scheduling
- safe distinction from “request may already have succeeded”

### 4. `timeout_after_acceptance`
The provider stores a successful result immediately but delays the HTTP response long enough that the caller may time out first.

Use this to validate:
- unknown outcome handling
- “timeout does not prove failure”
- later evidence confirmation through status check or webhook

### 5. `delayed_confirmation`
The provider returns `pending` first, then resolves to `succeeded` after a configured delay and sends one webhook.

Use this to validate:
- pending state handling
- delayed confirmation
- webhook-driven resolution

### 6. `duplicate_webhook`
The provider returns `pending`, later resolves to `succeeded`, and sends the same logical webhook twice.

Use this to validate:
- webhook deduplication
- replay-safe evidence ingestion

### 7. `inconsistent_status_check_response`
The status endpoint returns a controlled sequence like:
- pending
- succeeded
- pending
- succeeded

before stabilizing.

Use this to validate:
- reconciliation resilience
- distrust of single reads
- evidence comparison logic

### 8. `pending_then_resolves`
The provider returns `pending`, later resolves to `succeeded`, but does not send a webhook.

Use this to validate:
- polling/status-check resolution
- reconciliation without webhook dependence

---

## Design decisions

### Scenario is request-driven
The scenario is passed in the create request body.

That means tests and demos can force a specific provider behavior every time.

### Delayed scenarios are background-scheduled
Delayed confirmation and duplicate webhook behavior are scheduled asynchronously after the initial request returns.

### Timeout-after-acceptance is truly ambiguous to the caller
The simulator stores success first, then intentionally delays its HTTP response.

If the gateway client uses a shorter timeout, it will experience the exact ambiguity we want to test:
the request may have succeeded even though the client stopped waiting.

### Inconsistent status checks are scripted
The status endpoint can return a fixed response sequence before stabilizing.

That makes reconciliation tests reproducible.

---

## What done means for this milestone

Milestone 5 is done when:

- provider behavior can be selected by scenario
- tests and demos can force known failure modes
- the system can be exercised without any real provider
- delayed, duplicate, inconsistent, and ambiguous behaviors are reproducible
- later gateway milestones can depend on this simulator as a failure-heavy external rail

That is what “done” means here.

Not just:
“fake endpoint exists”

Done means:
**we now have a reliable failure lab for the rest of the system.**

---

## What this milestone is not doing

This milestone is not yet:

- a real provider adapter
- a secure external service
- a durable source of truth
- a real webhook signature system
- a production-grade payment provider

That is intentional.

This milestone is about controllable behavior, not production provider hardening.

---

## Example create request

```json
{
  "merchant_reference": "order_123",
  "amount_minor": 5000,
  "currency": "NGN",
  "scenario": "delayed_confirmation",
  "callback_url": "http://localhost:3000/webhooks/provider",
  "resolution_delay_ms": 3000
}