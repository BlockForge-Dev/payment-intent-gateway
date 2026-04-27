# Milestone 10

Receipt and Evidence Model

## Goal

Expose a clear operator-readable execution history through one coherent receipt view.

## What This Milestone Adds

This milestone upgrades the receipt endpoint from a raw persistence bundle into an
operator-focused evidence surface.

`GET /payment-intents/{id}/receipt` now returns:

- a summary section with identity, provider, callback target, current state, latest failure, and final classification
- explicit ambiguity metadata so unknown or pending states are visible instead of hidden
- execution attempt history with outcome classification and reasons
- webhook history with status hints
- callback notification and callback delivery history
- reconciliation history across all recorded runs
- extracted evidence notes
- one stitched chronological timeline

## Why This Matters

The receipt is where the system proves that it preserves truth instead of merely storing rows.

An operator should be able to answer:

- what state is this intent in right now
- why is it in that state
- what execution attempts happened
- whether the provider sent later evidence
- whether callbacks were scheduled, delivered, retried, or dead-lettered
- whether reconciliation changed the understanding of the outcome
- whether ambiguity is still open or already resolved

## Current Receipt Shape

The endpoint is now built as an operator receipt in the application layer.

That view is assembled from the persistence read model and includes:

- `summary`
- `ambiguity`
- `attempts`
- `provider_webhooks`
- `callbacks`
- `reconciliation`
- `timeline`
- `evidence_notes`

## Key Design Choice

The raw evidence is still loaded durably from:

- `payment_intents`
- `execution_attempts`
- `provider_events`
- `callback_notifications`
- `callback_deliveries`
- `reconciliation_runs`
- `audit_events`

But the API no longer exposes that raw structure directly as the main receipt contract.

Instead, the application layer reshapes it into an operator-readable narrative.

That keeps the storage model flexible while making the truth surface understandable.

## Ambiguity Visibility

The receipt now makes ambiguity explicit through:

- `ambiguity.visible`
- `next_resolution_at`
- `last_resolution_at`
- `resolution_attempt_count`

So an `unknown_outcome`, `provider_pending`, or `manual_review` state is visible as an
operational condition, not just a string.

## Reconciliation History

The receipt now includes all reconciliation runs, not only the latest reconciliation snapshot.

That means operators can inspect:

- when reconciliation started and ended
- what provider status was observed
- what internal state was compared
- whether the result was a match, mismatch, or unresolved
- what decision was taken
- what evidence supported it

## Timeline Strategy

The new timeline is stitched from:

- state transitions
- execution attempts
- provider webhooks
- callback notification scheduling
- callback delivery attempts
- reconciliation runs
- important audit evidence such as status-check observations

The goal is not to mirror every database row.

The goal is to tell the story in chronological order.

## Evidence Notes

The receipt now extracts notes from:

- transition notes
- attempt notes
- reconciliation notes
- callback errors
- audit event notes

This helps operators understand why something happened without reading raw JSON payloads first.

## Definition of Done Check

This milestone is done because:

- any intent can now be inspected through one coherent receipt view
- ambiguity is visible instead of hidden
- attempts and evidence are stitched into a readable timeline
- webhook, callback, and reconciliation histories are included explicitly
- the receipt explains the lifecycle well enough for an operator to reason about it

## Verification

This milestone adds focused receipt tests covering:

- ambiguity visibility
- final classification derivation after reconciliation
- stitched timeline entries
- reconciliation history inclusion

That makes the receipt an intentional system surface, not an afterthought.
