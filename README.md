# Payment Intent Gateway

Reliability-first execution infrastructure for money-critical payment actions.

## North Star

Safely execute payment intents while preserving truth under duplicates, retries, ambiguity, asynchronous confirmation, and external inconsistency.

## What This Project Is

The Payment Intent Gateway is not a checkout UI, wallet, banking app, or merchant SaaS platform.

It is a trust boundary between a business request and real provider execution.

Its job is to:

- accept and durably persist payment intents
- enforce idempotency
- execute intents asynchronously through a provider adapter
- classify success, terminal failure, retryable failure, pending, and unknown outcome correctly
- preserve attempt history and operational evidence
- ingest provider webhooks safely
- deliver downstream callbacks without confusing notification truth with execution truth
- reconcile internal truth against provider truth when outcomes are uncertain
- expose operator-readable receipts and timelines

## Current Implementation Status

The repo now contains a working vertical slice through the reliability story:

- `crates/domain`: payment intent aggregate, lifecycle states, attempts, reconciliation types, and invariants
- `crates/application`: ingestion, execution, webhooks, callbacks, receipts, reconciliation, and operator-facing query services
- `crates/persistence`: Postgres persistence, leasing, callback queueing, receipts, and evidence history
- `apps/api`: payment intent API, receipt endpoint, and provider webhook ingestion
- `apps/worker`: leased background execution worker
- `apps/resolver`: unknown-outcome follow-up and status-check worker
- `apps/reconciler`: selected-intent reconciliation runner
- `apps/callback-worker`: downstream callback delivery worker
- `apps/mock-provider`: controllable failure-heavy provider simulator
- `apps/demo-receiver`: controllable downstream callback target for demo scenarios
- `apps/operator-ui`: Next.js + TypeScript operator surface for inspecting intents end to end

## Implemented Milestones

The repo currently covers the following milestones:

- Milestone 1: core domain model and invariants
- Milestone 2: durable persistence layer
- Milestone 3: replay-safe intent ingestion API
- Milestone 4: queueing, leasing, and worker foundation
- Milestone 5: failure-heavy mock provider simulator
- Milestone 6: execution attempt logic and classification
- Milestone 7: unknown outcome handling and follow-up
- Milestone 8: provider webhook ingestion
- Milestone 9: callback delivery engine
- Milestone 10: receipt and evidence model
- Milestone 11: reconciliation engine
- Milestone 12: minimal operator surface
- Milestone 13: failure scenario demo suite

## Run The Stack

The repo root placeholder binary is not the real app entrypoint. Run the actual services instead.

Apply the SQL migrations first:

```powershell
$env:PGPASSWORD = 'your-postgres-password'
psql -h localhost -U postgres -d payment_gateway -v ON_ERROR_STOP=1 -f migerations/0001_init_payment_gateway.sql
psql -h localhost -U postgres -d payment_gateway -v ON_ERROR_STOP=1 -f migerations/0002_add_worker_leasing.sql
psql -h localhost -U postgres -d payment_gateway -v ON_ERROR_STOP=1 -f migerations/0003_add_unknown_outcome_follow_up.sql
psql -h localhost -U postgres -d payment_gateway -v ON_ERROR_STOP=1 -f migerations/0004_add_callback_delivery_engine.sql
```

Start the runtime services in separate terminals:

```powershell
cargo run -p api
```

```powershell
cargo run -p worker
```

```powershell
cargo run -p resolver
```

```powershell
cargo run -p callback-worker
```

```powershell
cargo run -p mock-provider
```

Optional:

```powershell
cargo run -p demo-receiver
```

```powershell
cargo run -p reconciler
```

## Tests

Run the full workspace test suite from the repo root:

```powershell
cargo test --workspace
```

The current workspace includes domain tests, ingestion tests, execution classification tests, webhook tests, callback delivery tests, reconciliation tests, receipt tests, and mock/demo app tests.

## Demo Suite

Milestone 13 includes a reproducible live demo suite with saved artifacts.

Main runner:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\run-demo-scenario.ps1 -Scenario <scenario_name> -ApiBearerToken <your-api-token>
```

Artifacts are written to:

```text
demo-output/<scenario>-<timestamp>/
```

Supported scenarios:

- `duplicate_request_same_idempotency`
- `retryable_provider_outage`
- `terminal_provider_rejection`
- `timeout_unknown_outcome`
- `delayed_webhook_resolves_unknown`
- `duplicate_webhook_event`
- `callback_delivery_failure_and_retry`
- `reconciliation_mismatch`
- `worker_crash_and_recovery`
- `stale_pending_requires_recon`

See [docs/demo-scenarios.md](docs/demo-scenarios.md) for the full flow and expected outcomes.

## Operator UI

The operator surface is a Next.js + TypeScript app in `apps/operator-ui`.

Run it with:

```powershell
cd apps/operator-ui
$env:OPERATOR_API_BASE_URL = 'http://127.0.0.1:3000'
$env:OPERATOR_API_BEARER_TOKEN = 'your-api-token'
npm install
npm run dev
```

## Key Docs

- [docs/foundation-spec.md](docs/foundation-spec.md): product identity, trust model, system boundaries, and v1 scope
- [docs/phase-1-implementation-blueprint.md](docs/phase-1-implementation-blueprint.md): implementation blueprint and build order
- [docs/milestone-1-invariants.md](docs/milestone-1-invariants.md): domain invariants
- [docs/milestone-8-provider-webhook-ingestion-readme.md](docs/milestone-8-provider-webhook-ingestion-readme.md): webhook ingestion
- [docs/milestone-9-callback-delivery-engine-readme.md](docs/milestone-9-callback-delivery-engine-readme.md): callback delivery engine
- [docs/milestone-10-receipt-and-evidence-model-readme.md](docs/milestone-10-receipt-and-evidence-model-readme.md): receipt model
- [docs/milestone-11-reconciliation-engine-readme.md](docs/milestone-11-reconciliation-engine-readme.md): reconciliation engine
- [docs/milestone-12-minimal-operator-surface-readme.md](docs/milestone-12-minimal-operator-surface-readme.md): operator UI
- [docs/milestone-13-failure-scenario-demo-suite-readme.md](docs/milestone-13-failure-scenario-demo-suite-readme.md): demo suite

## Why This Repo Exists

This project is meant to show strong engineering judgment in the parts that actually matter in money systems:

- no silent ambiguity
- no blind retries
- no duplicate money movement from duplicate requests
- no loss of execution truth during worker failure
- no confusion between provider truth, internal truth, and downstream notification truth
- no silent reconciliation patching without evidence

## Current Note

The migration folder is currently named `migerations/`. That path is preserved for repo stability right now, even though `migrations/` is the intended long-term convention.
