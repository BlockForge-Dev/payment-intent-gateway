# Payment Intent Gateway

Reliability-first execution infrastructure for money-critical payment actions.

## North Star

Safely execute payment intents while preserving truth under duplicates, retries, ambiguity, asynchronous confirmation, and external inconsistency.

## What This Project Is

The Payment Intent Gateway is not a payment processor, wallet, checkout UI, or merchant SaaS product.

It is a trust boundary between a business request and real provider execution.

Its job is to:

- accept and durably persist payment intents
- enforce idempotency
- execute intents asynchronously through a provider adapter
- classify success, terminal failure, retryable failure, pending, and unknown outcome correctly
- preserve attempt history and operational evidence
- deliver downstream callbacks without confusing notification truth with execution truth
- reconcile internal truth against provider truth when outcomes are uncertain

## Current Repo Status

The repository currently contains the foundation of the domain and persistence layers:

- `crates/domain`: payment intent aggregate, states, attempts, reconciliation types, and invariants
- `crates/persistence`: Postgres schema access, repository methods, and receipt-oriented read model assembly
- `src/main.rs`: placeholder root binary that currently prints `Hello, world!`

The repo compiles and tests pass:

```powershell
cargo check --workspace
cargo test --workspace
cargo run
```

## Docs

- `docs/foundation-spec.md`: product identity, trust model, system boundaries, and v1 scope
- `docs/phase-1-implementation-blueprint.md`: exact implementation blueprint for the first serious build phase
- `docs/milestone-1-invariants.md`: foundational domain invariants
- `docs/milestone-2-persistence-readme.md`: persistence layer goals, schema rules, and definition of done

## v1 Scope

Included in v1:

- payment intent ingestion
- idempotent request handling
- durable persistence
- leased background execution
- mock provider simulation
- provider webhook ingestion
- callback delivery with retry history
- receipt and evidence query surfaces
- reconciliation for ambiguous or mismatched outcomes

Explicitly out of scope for v1:

- customer auth and wallet flows
- merchant onboarding platform
- subscription billing
- payouts
- full accounting ledger
- fraud platform
- mobile apps
- polished dashboard-heavy product work

## Near-Term Build Order

1. Stabilize domain and persistence around the current schema and invariants.
2. Add an application crate that coordinates ingestion, execution, callbacks, and reconciliation use cases.
3. Add `apps/api` for create/query/receipt and webhook endpoints.
4. Add `adapters/mock_provider` to drive failure-heavy scenarios.
5. Add `apps/worker` for leasing and provider execution.
6. Add `apps/reconciler` for scheduled truth comparison against provider state.

## Why This Repo Exists

The point of this project is not generic CRUD and not "send request to provider and hope."

The point is to show strong engineering judgment in the parts that actually matter in money systems:

- no silent ambiguity
- no blind retries
- no duplicate movement of money from duplicate requests
- no loss of history during failure
- no confusion between provider truth, internal truth, and downstream notification truth

## Current Note

The migration folder in the current repo is named `migerations/`. The intended long-term convention is `migrations/`, but the current path is preserved for now so the repo remains stable while the next layers are built.
