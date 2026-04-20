# Milestone 4 — Queueing / Leasing / Worker Foundation

## Goal

Move from accepted intent to safe background execution.

This milestone is where the Payment Intent Gateway starts acting like an execution system instead of just an ingestion API.

The purpose of this milestone is to create the worker-side coordination layer that ensures:

- eligible work can be picked safely
- one worker claims one intent at a time
- concurrent workers do not pick the same intent together
- claims expire if a worker disappears
- retries can be scheduled safely for later availability

This milestone is not yet about provider execution.

It is about **safe ownership of work**.

---

## Why this milestone matters

In money-critical systems, accepting work is not enough.

The system must also answer:

- which worker currently owns this intent?
- how do we stop two workers from executing it?
- what happens if a worker dies after claiming?
- how do we return work to the queue safely?
- how do we schedule retry availability instead of hot-looping?

That is what this milestone builds.

This is the bridge between:
- “intent exists”
and
- “intent can be executed safely later”

---

## What we add in this milestone

### Schema additions on `payment_intents`
We add lease and availability metadata:

- `available_at`
- `lease_owner`
- `lease_token`
- `lease_expires_at`
- `last_leased_at`

This allows us to treat `payment_intents` as the queue source of truth without creating a separate queue table yet.

### Worker claim behavior
A worker can atomically claim one eligible intent.

Eligible means either:

- `state IN ('queued', 'retry_scheduled')` and `available_at <= now`
or
- `state = 'leased'` but the lease has expired

That second case is important.

It is what allows the system to recover from worker crash or abandonment.

### Safe release behavior
A worker can:

- return a claimed intent back to `queued`
- schedule retry with `retry_scheduled`
- move a claimed intent into `executing`

All of those transitions require the correct `lease_token`.

That token is the proof of ownership for the claim.

---

## Core design choice

We are using Postgres row locking for claiming work.

The claim query uses `FOR UPDATE SKIP LOCKED` inside a transaction so that multiple workers can compete for work without waiting on the same row and without both claiming it. That is the core concurrency primitive for this milestone.

We also rely on transaction-wrapped updates so partial lease acquisition does not commit accidentally.

---

## Queue semantics in this milestone

This project still stores business truth in `payment_intents`.

We are not creating a separate queue abstraction yet.

Instead:

- `queued` means ready for lease when `available_at <= now`
- `retry_scheduled` means not yet runnable until `available_at`
- `leased` means a worker currently owns it, unless the lease expires
- `executing` comes after a worker consumes a valid lease for real execution

This keeps the system focused and simple while preserving strong coordination semantics.

---

## What the lease fields mean

### `available_at`
The earliest time this intent is eligible to be claimed from queue states.

This supports:
- delayed retry
- backoff
- intentional deferral

### `lease_owner`
The worker identity that currently holds the claim.

### `lease_token`
A unique ownership token for the current lease.

This is what later write operations must prove they own.

### `lease_expires_at`
The time after which the lease is considered stale.

If the worker dies and the lease expires, another worker can reclaim the intent.

### `last_leased_at`
Useful for observability and debugging.

---

## What the repository now supports

### `lease_next_available_intent`
Claims one eligible intent atomically.

Behavior:
- find one ready row
- skip rows another transaction already locked
- set:
  - `state = leased`
  - `lease_owner`
  - `lease_token`
  - `lease_expires_at`
  - `last_leased_at`
- record audit events
- return the leased intent with lease metadata

### `renew_lease`
Extends an active lease when the worker still owns it.

### `return_lease_to_queue`
Releases a claimed intent back to `queued` and sets a future `available_at`.

### `schedule_retry_from_lease`
Moves a claimed intent into `retry_scheduled` with a future `available_at`.

### `mark_leased_as_executing`
Consumes the lease and moves the intent into `executing`.

This will be used by the execution milestone.

---

## Why the lease token matters

The lease token is how we prevent one worker from mutating an intent claimed by another worker.

Without it, a stale worker process or delayed message could still update work it no longer owns.

So every “post-claim” mutation in this milestone checks:

- intent id
- state is `leased`
- matching `lease_token`

If that check fails, the repository rejects the mutation.

That is a real execution-safety boundary.

---

## Why we allow reclaiming expired `leased` rows

A worker can crash after claiming work.

If the system only looked at `queued` rows, that claimed row would be stuck forever.

So the claim query also considers:

- `state = leased`
- `lease_expires_at <= now`

That makes stale work recoverable.

This is one of the most important reliability properties in the milestone.

---

## Worker app behavior in this milestone

The `apps/worker` binary is intentionally minimal.

It proves:

- polling
- leasing
- visibility of claims
- safe release back to queue

For Milestone 4 only, the worker immediately returns leased work back to queue after a short delay.

That is deliberate.

We are proving safe claim/release mechanics first.

Actual provider execution begins in the next milestone.

---

## What done means for this milestone

Milestone 4 is done when:

- queued intents can be claimed by a worker
- concurrent workers cannot safely claim the same row at the same time
- stale leases can be reclaimed after expiry
- lease ownership is protected by `lease_token`
- work can be released back to queue
- work can be scheduled for retry at a future time
- work can be transitioned from `leased` to `executing`
- audit history records lease lifecycle events

That is what “done” means here.

Not just:
“worker runs”

Done means:
**ownership of execution opportunity is durable, explicit, and safe.**

---

## What this milestone is not doing yet

This milestone is not yet doing:

- provider API calls
- execution attempt recording
- timeout ambiguity classification
- provider webhook handling
- callback delivery
- reconciliation logic

Those come next.

This milestone is specifically about:
**claiming, ownership, expiry, release, and retry scheduling.**

---

## Why this fits our project goal

Our project is about:

- trust
- reliability
- execution safety
- reconciliation
- coordination
- operational truth

This milestone directly strengthens the coordination and execution-safety part.

It proves that we are not just storing payment intents.

We are building a system that can safely control who is allowed to act on them.

That is a serious systems boundary.

---

## Summary

Milestone 4 creates the worker foundation of the Payment Intent Gateway.

It gives us:

- durable queue readiness through `available_at`
- safe one-worker-at-a-time claims
- expiring leases
- reclaimable stale work
- ownership tokens for post-claim writes
- retry scheduling
- clean transition toward real execution

This is the first real coordination engine in the project.