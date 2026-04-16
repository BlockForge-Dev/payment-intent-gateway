# Payment Intent Gateway

Reliability-First Execution Layer for Money-Critical Payment Actions

## Executive Summary

The Payment Intent Gateway is a narrow, infrastructure-grade system for executing payment actions safely.

It exists to durably capture intent, execute through a provider without losing truth under failure, preserve evidence, and reconcile uncertainty later when provider and internal state diverge.

This project is meant to demonstrate deep competence in:

- durable execution
- idempotency
- retry safety
- timeout ambiguity handling
- provider webhook processing
- callback reliability
- reconciliation
- operator-readable receipts and evidence

## Mission

Provide a safe, durable, auditable, and reconciliation-aware execution layer for payment intents so that money-critical actions can be tracked, explained, retried safely, and verified under failure.

## Vision

Become the trusted infrastructure boundary between business payment requests and real provider execution, where every accepted payment intent is:

- durably recorded
- safely executed
- clearly classified
- auditable end to end
- recoverable under failure
- explainable to operators and integrators

## Core Goal

The goal is not merely to process payments.

The goal is to execute payment intents safely while preserving truth under ambiguity, duplicates, retries, delayed confirmation, provider inconsistency, and callback failure.

## What This Project Is

This project is:

- a payment intent ingestion system
- a durable state machine
- a leased background execution engine
- a provider adapter boundary
- a callback delivery system
- a receipt and evidence system
- a reconciliation engine
- an operator-visible truth surface

## What This Project Is Not

This project is not:

- a checkout product
- a wallet
- a banking app
- a merchant management SaaS
- an accounting ledger
- an invoicing platform
- a subscription billing system
- a fraud engine
- a broad analytics product

The scope is intentionally focused on one hard problem: reliable payment execution under failure.

## Primary Users

1. An integrating backend or merchant service that submits payment intents.
2. An internal operator who needs to inspect what happened and why.
3. A reconciliation job that compares internal truth with provider truth.
4. A downstream receiver that consumes callbacks from the gateway.

## System Promise

For every accepted payment intent, the gateway will either provide a durable, queryable execution history and final outcome, or clearly preserve and expose ambiguity until reconciliation resolves it.

## Design Principles

1. Truth first.
2. Intent before execution.
3. No silent ambiguity.
4. No blind retries.
5. Evidence matters.
6. Internal truth and provider truth can diverge.
7. Callbacks are not the source of truth.
8. Provider adapters do not own lifecycle truth.
9. Idempotency is mandatory.
10. Every critical transition should be explainable.

## System Boundaries

### Ingress Boundary

Responsibilities:

- authenticate request
- validate payload
- persist intent durably
- enforce idempotency
- return a stable lineage identifier

Must never:

- treat request-thread completion as proof of provider outcome
- use inline provider execution as the primary truth path

### Execution Boundary

Responsibilities:

- lease eligible intents
- invoke provider safely
- classify outcomes
- persist attempt history
- schedule retry or reconciliation

Must never:

- hide ambiguity
- assume timeout means failure
- overwrite historical evidence

### Provider Boundary

Responsibilities:

- translate requests and responses
- expose provider reference and status lookup
- surface raw provider data
- verify inbound webhook authenticity when applicable

Must never:

- own internal state rules
- decide global retry policy
- replace internal truth with adapter-local assumptions

### Callback Boundary

Responsibilities:

- notify downstream systems
- retry callback delivery failures
- preserve delivery history

Must never:

- decide whether the payment itself succeeded

### Reconciliation Boundary

Responsibilities:

- fetch provider truth
- compare internal and provider state
- resolve eligible ambiguity
- flag mismatch or unresolved cases

Must never:

- mutate history silently
- create unsupported certainty

### Operator Query Boundary

Responsibilities:

- expose state, attempts, evidence, callbacks, and reconciliation runs

Must never:

- become a hidden side-effect path

## Core Lifecycle States

Suggested lifecycle for v1:

- `received`
- `validated`
- `rejected`
- `queued`
- `leased`
- `executing`
- `provider_pending`
- `retry_scheduled`
- `unknown_outcome`
- `succeeded`
- `failed_terminal`
- `reconciling`
- `reconciled`
- `manual_review`
- `dead_lettered`

## Failure Model

The gateway treats failure as categories, not a single boolean.

### Validation Failure

Bad request or unsupported input.

Result:

- reject
- do not enqueue
- do not retry

### Duplicate Request

Same idempotency key reused for the same business intent.

Result:

- return the existing lineage
- do not create duplicate execution

### Retryable Infrastructure Failure

Temporary network or provider outage.

Result:

- record attempt
- schedule safe retry

### Terminal Failure

Hard provider rejection or unsupported action.

Result:

- no automatic retry
- preserve final failure and evidence

### Unknown Outcome

The call may have reached the provider but the gateway cannot safely prove the result.

Result:

- preserve ambiguity explicitly
- do not blindly retry
- require later evidence or reconciliation

### Delayed Confirmation

Provider truth arrives later through webhook or status check.

Result:

- transition safely based on evidence
- preserve full timeline

### Duplicate Provider Event

Same webhook or provider-side event arrives more than once.

Result:

- deduplicate processing
- avoid duplicate side effects

### Callback Delivery Failure

Gateway knows the payment result but cannot notify the downstream consumer.

Result:

- retry callback delivery
- do not change execution truth

### Reconciliation Mismatch

Internal and provider truth do not agree.

Result:

- record the mismatch
- escalate to manual review or resolve with evidence

## v1 Functional Scope

Included:

- create payment intent
- query payment intent
- query receipt
- worker-based execution
- mock provider
- one real provider adapter later, after simulator stability
- webhook ingestion
- callback delivery
- reconciliation job
- minimal operator surface

Out of scope:

- consumer wallet or user product
- onboarding and back-office platform work
- billing and invoicing platform features
- payouts and settlement orchestration
- fraud platform
- full accounting
- mobile apps

## Success Criteria

The project succeeds if it shows:

- duplicate requests are handled safely
- retries happen only where safe
- ambiguous outcomes are preserved instead of guessed
- worker crashes do not lose intent truth
- callbacks fail independently from execution truth
- reconciliation can resolve uncertain states later
- the receipt explains what happened, when, and based on what evidence

## Milestone Ladder

1. Project foundation
2. Core domain model
3. Database schema and persistence
4. Intent ingestion API
5. Queueing, leasing, and worker foundation
6. Mock provider simulator
7. Execution attempt logic
8. Unknown outcome handling
9. Provider webhook ingestion
10. Callback delivery engine
11. Receipt and evidence model
12. Reconciliation engine
13. Minimal operator surface
14. Failure scenario demo suite
15. Documentation and presentation

## Target Repository Shape

```text
payment-intent-gateway/
|- apps/
|  |- api/
|  |- worker/
|  `- reconciler/
|- crates/
|  |- domain/
|  |- application/
|  |- persistence/
|  |- adapters/
|  |  |- mock_provider/
|  |  `- paystack/
|  |- callbacks/
|  |- receipts/
|  |- reconciliation/
|  |- shared/
|  `- config/
|- migrations/
|- docs/
`- README.md
```

## North Star Sentence

Everything in this project should serve this sentence:

Safely execute payment intents while preserving truth under duplicates, retries, ambiguity, asynchronous confirmation, and external inconsistency.
