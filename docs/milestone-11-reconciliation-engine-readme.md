# Milestone 11

Reconciliation Engine

## Goal

Resolve internal/provider mismatch and uncertainty later using explicit reconciliation rules.

## What This Milestone Adds

This milestone adds a real reconciliation use case on top of the provider status lookup path.

The system can now:

- run reconciliation for selected payment intents
- fetch provider truth through provider reference or merchant-reference lookup
- distinguish observed provider states from provider `not found` and transport unavailability
- compare internal truth against provider truth
- persist every reconciliation run, even when the outcome stays unresolved
- resolve eligible ambiguous intents with reconciliation evidence
- escalate contradictory or missing provider truth to manual review

## New Reconciliation Service

The new orchestration lives in:

- `crates/application/src/reconciliation.rs`

It loads the selected intent, transitions it into `reconciling`, queries provider truth,
applies the reconciliation rule matrix, persists the reconciliation run, and returns a summary.

## New Reconciler App

Milestone 11 adds a dedicated one-shot job:

- `apps/reconciler`

It is intentionally explicit:

- provide a comma-separated list of intent ids through `RECONCILE_INTENT_IDS`
- the job reconciles those selected intents once
- it logs the comparison, decision, provider truth seen, and resulting state

This keeps reconciliation separate from:

- the execution worker
- the unknown-outcome resolver
- the callback worker

## Rule Matrix Implemented

### Ambiguous and pending states

For internal `unknown_outcome`, `provider_pending`, or `manual_review`:

- provider success -> `confirm_succeeded`
- provider terminal failure -> `confirm_failed_terminal`
- provider pending -> `keep_unknown`
- provider missing without a known provider reference -> unresolved and preserved
- provider missing with a known provider reference -> `escalate_manual_review`

### Strong internal outcomes

For internal `succeeded`:

- provider success -> `confirm_succeeded`
- provider failure / pending / missing -> `escalate_manual_review`

For internal `failed_terminal`:

- provider terminal failure -> `confirm_failed_terminal`
- provider success / pending / missing -> `escalate_manual_review`

### Provider truth unavailable

If the provider lookup fails transiently:

- ambiguous states stay unresolved
- strong outcome states escalate to manual review

## Domain Changes

The state machine now allows reconciliation to start from:

- `unknown_outcome`
- `provider_pending`
- `manual_review`
- `succeeded`
- `failed_terminal`

That matters because mismatch detection is not only for ambiguous states.

It is also for cases like:

- internal success, provider missing
- internal terminal failure, provider success

The reconciliation flow can now surface those contradictions safely.

## Provider Lookup Improvement

Provider status lookup now distinguishes:

- observed status
- `not found`
- retryable transport error

That distinction is important because:

- `not found` may indicate a genuine mismatch
- a transport failure only means truth is temporarily unavailable

## Persistence and Evidence

Every reconciliation run is written into:

- `reconciliation_runs`

And the audit payload now also carries raw provider lookup summary data for the run.

The receipt surface from Milestone 10 already shows reconciliation history, so this milestone
plugs directly into that operator view.

## What This Means in Practice

Examples now handled by code:

- internal `unknown_outcome` + provider success -> resolved with recon evidence
- internal `provider_pending` + provider terminal failure -> resolved with recon evidence
- internal `succeeded` + provider missing -> visible mismatch and `manual_review`
- provider still pending -> unresolved but recorded

That is the difference between “we queried the provider” and “we reconciled the system truth.”

## Tests Added

Reconciliation-focused tests now cover:

- unknown outcome confirmed to success
- pending confirmed to terminal failure
- pending remaining pending
- internal success plus provider missing escalating to manual review

## Definition of Done Check

This milestone is done because:

- a recon job can run for selected intents
- reconciliation results are stored durably
- receipt history already shows reconciliation runs
- ambiguity can now be resolved through reconciliation evidence
- mismatch cases are visible instead of silently patched

## Why It Matters

Reconciliation is one of the clearest signals that the system is designed for money-critical truth.

It shows the gateway is not just executing actions.

It is also willing to question its own internal state later, compare it against external reality,
and preserve the result as evidence instead of guessing.
