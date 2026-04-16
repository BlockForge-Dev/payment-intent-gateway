# Milestone 2

Persistence Layer and Durable Truth Model

## Goal

Milestone 2 exists to make the payment intent lifecycle durable in Postgres.

This milestone is where the project moves from domain-only correctness to storage-backed truth.

## What This Milestone Must Prove

The persistence layer must prove that:

- accepted intent lineage is durable
- idempotency is enforced in storage, not just in memory
- execution attempts can be reconstructed later
- provider evidence can be stored safely
- callback delivery history is separated from execution truth
- reconciliation history can be preserved
- receipts can be computed from durable data

## Durable Tables

The current schema uses these core tables:

- `payment_intents`
- `idempotency_keys`
- `execution_attempts`
- `provider_events`
- `callback_deliveries`
- `reconciliation_runs`
- `audit_events`

Current migration path:

- `migerations/0001_init_payment_gateway.sql`

The folder name is misspelled today, but the schema itself matches the intended milestone shape.

## Table Responsibilities

### `payment_intents`

Stores the current internal truth for a payment lineage:

- identity
- merchant reference
- amount and currency
- provider name
- current state
- latest failure classification
- provider reference
- timestamps

### `idempotency_keys`

Stores the durable mapping between a caller's idempotency key and the internal intent lineage.

This table is what prevents duplicate business submissions from creating duplicate execution.

### `execution_attempts`

Stores each provider execution try as a durable historical record.

This table must make it possible to answer:

- how many times did we try
- what did we send
- what did the provider appear to say
- what classification did we give that outcome

### `provider_events`

Stores inbound provider-side evidence such as webhooks or event notifications.

This table exists so duplicate or delayed provider events do not become invisible or dangerous.

### `callback_deliveries`

Stores outbound callback attempts separately from payment execution truth.

This table proves the system understands that "notification failed" is not the same as "payment failed."

### `reconciliation_runs`

Stores comparisons between internal state and provider state.

This table is what allows unknown outcomes and mismatches to become explainable later.

### `audit_events`

Stores operator-visible timeline evidence such as state transitions and major lifecycle events.

This is the backbone of a receipt-oriented truth surface.

## Persistence Invariants

Milestone 2 must preserve these invariants:

1. One idempotency key in a given scope maps to one lineage.
2. Conflicting payload reuse of the same idempotency key is rejected.
3. Every execution attempt is stored durably.
4. Timeline evidence is append-oriented and queryable.
5. Callback delivery history never rewrites payment outcome truth.
6. Reconciliation history is preserved even when no final decision is reached.

## Required Repository Behaviors

The persistence layer should support these operations cleanly:

- create intent with idempotency protection
- load intent by id
- save attempt start
- save attempt finish
- save provider event idempotently
- save callback delivery
- save reconciliation run
- compute a receipt view from durable records

## Transaction Rules

These operations should be transactional:

- creating a new intent plus its idempotency mapping
- starting an execution attempt plus timeline audit write
- finishing an execution attempt plus state update plus audit write
- writing a reconciliation run plus intent update plus audit write

The goal is simple:

no visible lifecycle step should partially commit and leave truth fractured.

## Why This Milestone Matters

This milestone is where the system earns the right to call itself reliability-first.

Without durable persistence:

- idempotency is not trustworthy
- retries are not reconstructable
- ambiguity is not explainable
- receipts are not meaningful
- reconciliation has no evidence base

## Definition of Done

Milestone 2 is done when:

- migrations run cleanly on a local Postgres instance
- all core tables exist with the right constraints
- repository methods support the domain lifecycle cleanly
- idempotency conflict is enforced durably
- attempts, provider events, callbacks, and reconciliation runs are queryable
- a receipt can be assembled from stored records
- persistence behavior is covered by focused tests

## Current Repo Status

The repo is already partway through this milestone:

- migration SQL exists
- row types exist
- repository logic exists for core writes and receipt assembly
- the workspace compiles and tests pass

The main remaining work for Milestone 2 is not conceptual. It is polish and proof:

- add persistence-focused tests
- validate the schema against a real Postgres instance
- wire the persistence layer into the incoming API and worker paths

## Handoff to Milestone 3

Once this milestone is stable, the next step is Milestone 3:

build the ingress API so external callers can create and query payment intents safely without triggering inline payment execution as the source of truth.
