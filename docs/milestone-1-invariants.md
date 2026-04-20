Invariant 1

One idempotency key maps to one business lineage.

Meaning:

the same idempotency key must not create multiple payment intent lineages
persistence later must enforce this

Important note:

Milestone 1 defines the rule
Milestone 2 enforces it durably in storage
Invariant 2

Terminal states should not auto-retry.

Terminal states:

Succeeded
FailedTerminal
DeadLettered

Meaning:

once terminal, the system must not silently schedule retry execution
Invariant 3

Unknown outcome cannot be silently converted without evidence.

Meaning:

UnknownOutcome and ProviderPending require external evidence before being finalized
acceptable evidence:
provider webhook
provider status check
manual operator decision
Invariant 4

Callback failure must not alter execution truth.

Meaning:

callback delivery is downstream notification
it does not change whether payment execution happened
Invariant 5

Every attempt must be durably recorded.

Meaning:

each execution try must become an attempt record later in persistence
no execution should happen “off the books”
Invariant 6

State transitions must be explicit and guarded.

Meaning:

no random intent.state = ... across the codebase
lifecycle changes should happen only through domain methods
Invariant 7

Provider adapters do not own internal truth.

Meaning:

adapters report outcomes
the aggregate decides how that affects lifecycle
What “done” means for Milestone 1

Milestone 1 is done when all of these are true:

PaymentIntent exists as the aggregate root
lifecycle states are modeled explicitly
attempt outcome and failure classification are separate
transitions are guarded
invalid transitions return domain errors
unknown outcome requires evidence
terminal states do not retry
receipt model exists
reconciliation result model exists
tests cover happy path and invalid path
invariants are documented in prose
A few design choices here that matter
Why PaymentIntent is the aggregate root

Because this is the central business lineage.

Everything attaches to it:

attempts
reconciliation
timeline
provider reference
current truth
Why failure classification is separate from state

Because state answers:

“where are we now?”

Failure classification answers:

“what kind of failure happened?”

Those are different questions.

Why UnknownOutcome is its own state

Because in money systems, timeout is not equal to failure.

That single distinction is one of the biggest signals of maturity in this project.

Why receipt exists this early

Because this system is not just about doing work.
It is about preserving operational truth.