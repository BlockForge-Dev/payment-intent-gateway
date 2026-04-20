# Milestone 3 — Intent Ingestion API

## Goal

Allow external systems to create payment intents safely.

This milestone is the first real trust boundary of the Payment Intent Gateway.

The purpose of this milestone is not just to expose an HTTP endpoint.

The purpose is to ensure that external systems can submit payment intents into the gateway in a way that is:

- authenticated
- validated
- idempotent
- durable
- queryable
- safe under HTTP retries

This milestone matters because payment intent ingestion is where accidental duplicates often begin.

If this boundary is weak, the rest of the system can be correct and still produce dangerous behavior.

---

## What this milestone is responsible for

This milestone owns:

- `POST /payment-intents`
- `GET /payment-intents/:id`

It is responsible for:

- authenticating the caller
- validating the incoming request
- requiring an idempotency key
- fingerprinting the business request payload
- creating a durable payment intent lineage
- returning the same lineage on safe retries
- rejecting conflicting idempotency reuse
- exposing current state through a query endpoint

It is explicitly **not** responsible for provider execution.

No provider call happens inline in this milestone.

That is intentional.

---

## Why this milestone exists

In money-critical systems, a retried HTTP request must not become a retried money movement.

A client may retry because:

- the network was unstable
- the client timed out waiting for a response
- the caller crashed and retried later
- an upstream service retried automatically
- a human operator retried because the first request looked stuck

If the API layer is not idempotent, those retries can become duplicate execution.

That is exactly what this milestone is designed to prevent.

---

## Design decision in this milestone

### We require `Idempotency-Key`
For this version, the API requires the `Idempotency-Key` header for `POST /payment-intents`.

We are choosing **require**, not optional, because this is a money-critical trust boundary.

That is the stronger default.

### We fingerprint the normalized payload
The API computes a request fingerprint from:

- merchant reference
- amount
- currency
- provider

This allows the system to distinguish:

- same idempotency key + same business payload → safe replay, return existing lineage
- same idempotency key + different payload → reject as conflict

### We do not execute providers inline
The endpoint only:
- validates
- creates the intent
- validates and queues the intent in domain state
- persists the durable lineage

Execution comes later through the worker path.

This keeps the ingestion boundary clean and replay-safe.

---

## Endpoint behavior

## `POST /payment-intents`

Creates a payment intent lineage safely.

### Request body
- `merchant_reference`
- `amount_minor`
- `currency`
- `provider`

### Required headers
- `Authorization: Bearer <token>`
- `Idempotency-Key: <key>`

### Successful responses
- `201 Created` when a new payment intent is created
- `200 OK` when the same idempotency key and same payload are replayed and the existing lineage is returned

### Conflict response
- `409 Conflict` when the same idempotency key is reused with a different payload

### Important behavior
This endpoint **never** performs provider execution inline.

It only creates durable intent truth.

---

## `GET /payment-intents/:id`

Returns the current durable header state of the payment intent.

This gives external systems and operators a safe way to inspect the current lifecycle state without inferring truth from logs.

---

## Validation behavior

This milestone validates:

- `merchant_reference` must be present
- `amount_minor` must be greater than zero
- `currency` must be present
- `provider` must be supported
- `Idempotency-Key` must be present
- caller must be authenticated

Unsupported provider is rejected before persistence.

At this stage, supported providers are:
- `paystack`
- `mockpay`

That list is intentionally small.

---

## What happens on create

The `POST /payment-intents` flow is:

1. authenticate caller
2. read and require `Idempotency-Key`
3. parse JSON body
4. normalize request fields
5. validate provider and basic request correctness
6. compute request fingerprint
7. build domain `PaymentIntent`
8. move domain state:
   - `Received`
   - `Validated`
   - `Queued`
9. persist the intent + idempotency record transactionally
10. return:
   - `201 Created` for new lineage
   - `200 OK` for same-lineage replay

That is the safe ingestion flow.

---

## Duplicate request behavior

This milestone must handle two critical cases:

### Case 1 — same idempotency key, same payload
Example:
- first request creates the intent
- second request is a network retry of the same business command

Result:
- do not create a second lineage
- do not create duplicate execution opportunity
- return the existing lineage

### Case 2 — same idempotency key, different payload
Example:
- first request used amount `5000`
- second request reuses the same key with amount `7000`

Result:
- reject with conflict
- do not silently merge or overwrite the original business lineage

This is one of the most important trust guarantees in the system.

---

## Why we do not constrain merchant reference yet

In this milestone we do **not** globally constrain `merchant_reference`.

Why?

Because in real systems, merchant reference uniqueness usually depends on tenant or merchant scope.

Since tenant identity is not fully modeled yet, imposing a global uniqueness rule here could create the wrong long-term boundary.

So for now:
- `merchant_reference` is validated
- `idempotency_key` is the real duplicate-protection boundary

That is the intentional choice for this version.

---

## Project goals this milestone supports

This milestone directly supports our broader project goals:

### Trust
The caller gets a stable lineage for the same business command.

### Reliability
Retries at the HTTP boundary do not become duplicate execution.

### Execution safety
No inline provider execution is triggered here.

### Operational truth
The created intent is durably queryable.

### Coordination
The API prepares the lineage for later worker execution without conflating ingestion with execution.

This is exactly aligned with what this project is supposed to prove.

---

## What done means for this milestone

Milestone 3 is done when:

- valid requests persist a new payment intent
- duplicate idempotent requests return the same lineage safely
- conflicting idempotency reuse is rejected
- current state can be queried by id
- no provider execution happens inline
- tests exist for duplicate submission scenarios
- the API boundary feels deliberate and safety-aware

That is what “done” means here.

Not just “the endpoint works.”

Done means:
**the first trust boundary behaves safely under retries.**

---

## Files introduced in this milestone

### `crates/application`
This contains the application use case for intent ingestion.

It owns:
- supported provider validation
- request normalization
- fingerprint creation
- use-case orchestration
- mapping domain + persistence into ingestion behavior

### `apps/api`
This contains the HTTP boundary.

It owns:
- auth check
- header extraction
- JSON request/response handling
- HTTP status mapping

This separation is important because it keeps HTTP concerns from leaking into domain logic.

---

## Testing focus in this milestone

This milestone includes tests for:

- same idempotency key + same payload → returns existing lineage
- same idempotency key + different payload → rejected as conflict
- unsupported provider → rejected

That test focus is intentional.

This milestone is about correctness at the ingestion boundary.

---

## What this milestone is not doing

This milestone is not yet doing:

- worker leasing
- provider execution
- webhook ingestion
- callback delivery
- reconciliation runs
- operator UI
- receipt endpoint

Those come later.

This milestone is intentionally narrow.

It is about making the first API boundary safe.

---

## Summary

Milestone 3 gives the Payment Intent Gateway its first safe external interface.

It allows callers to:

- submit payment intents
- rely on idempotent replay safety
- query current intent state

And it ensures that:
- duplicate HTTP retries do not create duplicate execution lineage
- conflicting reuse of idempotency keys is rejected
- provider execution is not mixed into request-thread truth

That is exactly the kind of boundary a reliability-first payment system should have.