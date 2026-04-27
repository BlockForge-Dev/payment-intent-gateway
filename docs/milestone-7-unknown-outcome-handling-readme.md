# Milestone 7 — Unknown Outcome Handling

## Goal

Treat ambiguity as a first-class state.

This milestone is one of the strongest signals of maturity in the entire project.

The system is no longer allowed to collapse timeout or post-submission uncertainty into a shallow “failure” bucket.

Instead, ambiguity becomes durable, visible, and actionable.

That is the point of this milestone.

---

## Why this milestone matters

In money-critical systems, timeout is not proof that nothing happened.

The waiting side may have stopped waiting while the provider still processed the request.

That means a timeout after submission can leave the system in a dangerous place:

- retrying immediately may duplicate a real side effect
- declaring failure may be false
- declaring success may also be false

The correct behavior is to preserve ambiguity honestly.

That is what this milestone adds.

---

## What this milestone does

This milestone adds four major capabilities:

### 1. Durable ambiguity follow-up metadata
The database now stores:

- `next_resolution_at`
- `last_resolution_at`
- `resolution_attempt_count`

These fields make ambiguity operational.

An `UnknownOutcome` or `ProviderPending` intent is not just stuck in a state.
It is marked for future status-check work.

### 2. Receipt visibility for ambiguity
The receipt now exposes:

- `next_resolution_at`
- `last_resolution_at`
- `resolution_attempt_count`

That means ambiguity is visible to operators and downstream readers.

### 3. Status-check scheduling from execution
When execution ends in:
- `UnknownOutcome`
or
- `ProviderPending`

the system schedules a later status check instead of retrying blindly.

### 4. A dedicated resolver loop
A new resolver process queries due ambiguous intents and asks the provider for current status.

That later evidence can:

- resolve to `Succeeded`
- resolve to `FailedTerminal`
- downgrade ambiguity into `ProviderPending`
- keep ambiguity alive with a future status check
- escalate to `ManualReview` after too many inconclusive checks

---

## Core design idea

This milestone treats ambiguity as a first-class workflow, not just a label.

The important distinction is:

### Weak design
- mark timeout as unknown
- do nothing else

### Strong design
- mark timeout as unknown
- schedule follow-up
- record follow-up attempts
- expose the ambiguity in the receipt
- resolve later when evidence arrives
- avoid unsafe retry while unresolved

This milestone implements the strong design.

---

## What happens on timeout now

When the provider call times out after submission may already have happened:

1. the attempt outcome is recorded as `UnknownOutcome`
2. the payment intent state becomes `UnknownOutcome`
3. no retry is scheduled
4. `next_resolution_at` is set for a later status check
5. the receipt shows the ambiguity and next follow-up time

This is the correct behavior for a reliability-first execution system.

---

## What happens on pending now

When the provider responds with a known pending state:

1. the attempt outcome is recorded as `ProviderPending`
2. the payment intent state becomes `ProviderPending`
3. no retry is scheduled
4. `next_resolution_at` is set for a later status check
5. the receipt shows the follow-up metadata

This matters because pending is not failure and not final success.

---

## Resolver behavior

The resolver process looks for intents where:

- state is `unknown_outcome` or `provider_pending`
- `next_resolution_at <= now`

For each due candidate, it:

1. records a status-check attempt
2. queries provider status
3. applies later evidence safely

### If provider says `succeeded`
The intent is resolved to `Succeeded`.

### If provider says `failed_terminal`
The intent is resolved to `FailedTerminal`.

### If provider says `pending`
The intent stays `ProviderPending` and a future status check is scheduled.

### If the status check itself is inconclusive or fails transiently
The ambiguity stays active and another status check is scheduled.

### If too many status checks remain inconclusive
The intent is escalated to `ManualReview`.

That last step is important because ambiguity should not loop forever without operator visibility.

---

## Why this milestone blocks unsafe retry

This milestone explicitly prevents the most dangerous mistake:

retrying an ambiguous payment submission as if it were definitely not processed.

Retry is only scheduled for true retryable infrastructure failures.

Unknown outcome and provider pending now follow a separate path:
- status check
- later evidence
- reconciliation or manual review if needed

That separation is a strong reliability boundary.

---

## Why the receipt matters here

Unknown outcome handling is not complete if only the database “knows” about it.

Operators must be able to see:

- that the intent is ambiguous
- when the next follow-up is due
- how many status checks have already been attempted
- when the last status check happened

That is why receipt visibility is part of this milestone.

---

## Database additions in this milestone

The `payment_intents` table now includes:

- `next_resolution_at`
- `last_resolution_at`
- `resolution_attempt_count`

These fields give ambiguous states operational meaning.

They support:

- due work selection
- stale ambiguity inspection
- receipt visibility
- escalation decisions

---

## New app in this milestone

A new `resolver` app processes ambiguous intents.

Its role is intentionally narrow:

- find due unknown/pending intents
- perform provider status checks
- persist later evidence
- reschedule or resolve

This keeps ambiguity handling separate from the main execution worker.

That is a cleaner architectural boundary.

---

## Demo scenarios this milestone proves well

### Timeout after acceptance
A provider timeout becomes `UnknownOutcome`, appears in the receipt, and later resolves through status check.

### Pending then resolves
A pending result becomes `ProviderPending`, is not retried blindly, and later resolves through status check.

### Inconclusive checks
An intent can remain unresolved temporarily and be re-scheduled instead of lied about.

### Exhausted status checks
An intent that stays unresolved too long escalates to `ManualReview`.

---

## What done means for this milestone

Milestone 7 is done when:

- timeout ambiguity is represented durably in code and DB
- ambiguous cases appear in the receipt
- unsafe auto-retry is blocked
- later evidence can resolve the state
- a demo scenario proves safe behavior

That is what “done” means here.

Not:
“we store unknown outcome”

Done means:
**ambiguity is now a managed lifecycle.**

---

## Summary

Milestone 7 upgrades the Payment Intent Gateway from simply noticing ambiguity to actually managing it.

It adds:

- durable ambiguity follow-up metadata
- receipt visibility
- later status-check resolution
- safe handling of pending and timeout states
- escalation when ambiguity stays unresolved too long

This is a major maturity signal for the system.