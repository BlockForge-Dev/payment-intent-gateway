# Milestone 6 — Execution Attempt Logic

## Goal

Execute payment intents through the provider adapter and classify outcomes.

This milestone is the heart of the Payment Intent Gateway.

Up to this point, the system could:

- accept payment intents safely
- store them durably
- lease them to workers safely
- simulate provider behavior deliberately

But now the system must do the real work of an execution engine:

- create an execution attempt
- call a provider adapter
- classify the outcome carefully
- persist the result durably
- move lifecycle state correctly

This is where the project starts proving that it understands money-critical ambiguity.

---

## What this milestone is responsible for

This milestone owns the execution-attempt flow:

1. worker leases a queued intent
2. domain transitions the intent into `Executing`
3. an attempt record is created durably
4. the provider adapter is called
5. the provider result is classified into:
   - success
   - terminal failure
   - retryable failure
   - unknown outcome
   - pending
6. the attempt is finished durably
7. the payment intent header is updated
8. retry timing is set when appropriate

This milestone does **not** yet resolve pending or unknown outcomes.

It records them correctly.

That distinction matters.

---

## Why this milestone matters

In a money-critical system, the provider call is not just an HTTP request.

It is a side-effect boundary.

That means the system must not reduce outcomes to a shallow binary like:

- success
- failure

That is too weak.

Instead this milestone models the real categories that matter operationally:

### Success
The provider clearly indicates the action succeeded.

### Terminal failure
The provider clearly indicates the action failed in a way that should not be retried automatically.

### Retryable failure
The attempt failed because of transport or infrastructure conditions that may be safe to retry later.

### Unknown outcome
The client timed out or lost certainty after the request may already have been accepted.

### Pending
The provider has acknowledged the action but final confirmation is still outstanding.

Those distinctions are the entire point of a serious execution system.

---

## Core design choices in this milestone

### Attempt start is persisted before provider classification completes
As soon as the worker consumes a lease and begins execution, the system writes the attempt start durably.

That means the attempt exists even if the process crashes mid-call.

This is important because invisible execution is unacceptable.

### The lease is consumed when execution starts
The persistence method for attempt start moves the intent from `leased` to `executing` and clears lease ownership in the same write path.

That means a lease is not left hanging around after execution begins.

### Retry scheduling is stored on finish
If the provider result is retryable, the system sets:

- state = `RetryScheduled`
- `available_at = now + retry_delay`

That means retry is not just a state label.
It is a real queue timing decision.

### Timeout is classified as unknown outcome
A provider timeout is not treated as terminal failure.

It becomes `UnknownOutcome`.

That is one of the most important behaviors in the project.

---

## Provider adapter in this milestone

This milestone adds a provider adapter abstraction and a `MockProviderAdapter`.

The mock adapter calls the simulator from Milestone 5 and maps provider behavior into execution-relevant categories.

For now, scenario selection is inferred from the merchant reference using this convention:

`order_123|#scenario=timeout_after_acceptance`

If no scenario is present, the adapter defaults to:

`immediate_success`

This keeps the core model focused while still allowing failure-heavy demos.

---

## Execution flow

The execution service performs this sequence:

### 1. Begin execution in the domain model
The leased `PaymentIntent` transitions from `Leased` to `Executing`.

An attempt is appended in memory.

### 2. Persist attempt start from lease
Persistence verifies the correct `lease_token`, moves the row to `executing`, clears lease fields, inserts the attempt row, and records audit events.

### 3. Call provider adapter
The provider adapter submits the payment request.

### 4. Classify the provider result
The service maps provider adapter output into one of the domain `AttemptOutcome` variants.

### 5. Finish the attempt in the domain model
The aggregate updates:
- attempt outcome
- provider reference if known
- lifecycle state

### 6. Persist finished result
Persistence updates:
- `payment_intents`
- `execution_attempts`
- `available_at` if retry is scheduled
- audit events

That is the full milestone flow.

---

## Classification rules in this milestone

### Success
Provider explicitly says the action succeeded.

Result:
- attempt outcome = `Succeeded`
- intent state = `Succeeded`

### Terminal failure
Provider explicitly says the action failed terminally.

Result:
- attempt outcome = `TerminalFailure`
- classification = `TerminalProvider`
- intent state = `FailedTerminal`

### Retryable infrastructure failure
Transport error or retryable `5xx` provider response.

Result:
- attempt outcome = `RetryableFailure`
- classification = `RetryableInfrastructure`
- intent state = `RetryScheduled`
- `available_at` is set for a later retry

### Unknown outcome
Request timed out after submission may have already happened.

Result:
- attempt outcome = `UnknownOutcome`
- classification = `UnknownOutcome`
- intent state = `UnknownOutcome`

### Pending
Provider accepted but final result is not yet known.

Result:
- attempt outcome = `ProviderPending`
- intent state = `ProviderPending`

---

## Important safety rules in this milestone

### Network timeout after provider call is not auto-failure
This milestone explicitly protects that case.

Timeout means:
we stopped waiting

It does **not** mean:
the provider definitely did nothing

So timeout becomes `UnknownOutcome`.

### Terminal failures do not retry
Terminal provider rejections move to `FailedTerminal`.

No retry availability is scheduled.

### Retryable infra failures do retry later
Retryable transport problems become `RetryScheduled` with a future `available_at`.

### Attempt history remains reconstructable
The system records:
- attempt start
- attempt finish
- provider summary
- final attempt outcome
- resulting lifecycle state

That is enough to reconstruct the story later.

---

## What done means for this milestone

Milestone 6 is done when:

- one leased intent becomes one recorded execution attempt
- provider results are classified into the correct domain outcomes
- state transitions reflect those outcomes correctly
- retry scheduling sets real future availability
- timeout ambiguity becomes `UnknownOutcome`
- tests exist for:
  - success
  - terminal failure
  - retryable failure
  - unknown outcome
  - pending

That is what “done” means here.

Not just:
“worker called the provider”

Done means:
**execution truth is durably classified.**

---

## What this milestone is not doing yet

This milestone is not yet doing:

- webhook ingestion
- callback delivery
- reconciliation resolution
- pending-to-success follow-up
- unknown-outcome resolution

Those come next.

This milestone is about:
- first attempt creation
- first provider classification
- first durable execution truth

---

## Why this milestone is the heart of the project

This is the milestone where the system stops being queue infrastructure and becomes execution infrastructure.

It now knows how to say:

- this succeeded
- this failed terminally
- this should retry later
- this is pending
- this is ambiguous and must not be lied about

That is the core intellectual value of the whole project.

---

## Summary

Milestone 6 adds the execution-attempt engine of the Payment Intent Gateway.

It gives us:

- durable attempt start
- provider adapter integration
- explicit classification rules
- retry scheduling
- unknown outcome handling
- immutable-enough attempt history for operator truth

This is the center of the system.