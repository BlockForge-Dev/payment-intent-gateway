# Milestone 13

Failure Scenario Demo Suite

## Goal

Make the project demonstrable as a reliability system instead of only a codebase with good ideas.

## What This Milestone Adds

This milestone adds a reproducible demo harness around the reliability features already built in
Milestones 1 to 12.

The main additions are:

- a PowerShell scenario runner in `scripts/run-demo-scenario.ps1`
- a new local callback test target in `apps/demo-receiver`
- mock-provider admin routes for reset, inspection, and deletion
- merchant-reference demo directives for webhook and timing control
- receipt timeline improvements for lease and recovery evidence
- a dedicated demo scenarios document in `docs/demo-scenarios.md`

## Why This Matters

The project is strongest when someone can watch failure happen on purpose and see the system stay
correct.

This milestone makes it possible to show:

- idempotent duplicate handling
- safe retry scheduling
- terminal rejection handling
- ambiguity preservation
- webhook-driven resolution
- duplicate webhook safety
- callback retry without re-execution
- mismatch escalation through reconciliation
- lease recovery after worker failure
- stale pending intent resolution through later evidence

## New Demo Receiver

The new `apps/demo-receiver` app exists to prove the distinction between:

- payment execution truth
- downstream callback delivery success

It can:

- always succeed
- fail once
- fail twice
- always fail

That gives the callback worker a controllable downstream target for retry demos.

## New Mock Provider Admin Surface

The mock provider now exposes:

- `GET /mock-provider/admin/payments`
- `POST /mock-provider/admin/reset`
- `DELETE /mock-provider/payments/{provider_reference}`

These routes exist for demo control, not for production use.

They make mismatch and cleanup scenarios reproducible.

## Demo Directives

The worker now parses merchant-reference directives like:

- `#scenario=...`
- `#provider_webhook=off`
- `#resolution_delay_ms=...`
- `#timeout_response_delay_ms=...`

This allows one running stack to demonstrate different timing and webhook paths without changing
the external API or recompiling services.

## Worker Crash Demo Support

The worker now supports:

- `WORKER_PRE_EXECUTION_DELAY_MS`
- `WORKER_EXIT_AFTER_FIRST_LEASE`

These controls exist only to make lease recovery demonstrable.

They let the demo suite simulate a crash after lease acquisition and then show safe recovery when
another worker later reclaims the intent.

## Receipt Improvements

The receipt timeline now surfaces:

- `lease_acquired`
- `execution_claimed_from_lease`
- `lease_released`
- `retry_scheduled`

That matters because the crash-and-recovery story should be visible in the operator truth surface,
not only in logs.

## Verification

Milestone 13 was verified with:

- `cargo test --workspace`

The demo runner itself depends on local runtime services, so its full end-to-end behavior is
documented in `docs/demo-scenarios.md` and exercised against the running stack.

## Done Means

This milestone is done because:

- each requested failure scenario has a reproducible path
- receipts can be saved as demo artifacts
- callback failure can be demonstrated separately from execution truth
- mismatch and recovery stories are visible in receipts
- the project now has a concrete demo narrative instead of only internal implementation quality
