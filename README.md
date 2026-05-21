# Payment Intent Gateway

Reliability-first payment execution infrastructure for money-critical actions.

This project is a Rust-based gateway that accepts payment intents, preserves them durably, executes them safely through a provider boundary, handles ambiguity without guessing, and exposes operator-readable truth through receipts, timelines, callbacks, and reconciliation history.

It is not a checkout app, wallet, or fintech super app.

It is a focused system for one hard problem:

**how to execute payment actions safely when reality gets messy.**

## Why This Project Exists

Most systems can submit a payment request.

Far fewer systems handle these questions well:

- Did the provider actually receive the request before the timeout?
- Is it safe to retry, or could that duplicate money movement?
- What happens if two workers race for the same intent?
- What if the provider webhook arrives twice or out of order?
- What if the payment succeeded but the downstream callback failed?
- What if internal state and provider state no longer agree?
- Can an operator inspect the full execution story later?

This project is built around those questions.

## What It Demonstrates

This repo is designed to make reliability engineering in payments visible.

Core signals:

- durable intent capture before execution
- idempotent ingestion and replay safety
- leased worker execution with recovery after failure
- explicit classification of success, retryable failure, terminal failure, pending, and unknown outcome
- provider webhook ingestion with deduplication and conservative state updates
- downstream callback delivery with retry history, separate from execution truth
- reconciliation that can resolve ambiguity or surface mismatch
- operator-readable receipts that stitch attempts, evidence, callbacks, and reconciliation into one timeline

## Current Status

This is a serious working build, not just a foundation skeleton.

Implemented through the current repo:

- intent ingestion API
- Postgres-backed durable persistence
- worker leasing and execution coordination
- controllable mock provider for failure-heavy scenarios
- unknown-outcome follow-up and status checking
- provider webhook ingestion
- callback delivery engine
- receipt and evidence model
- reconciliation engine
- minimal operator UI in Next.js + TypeScript
- end-to-end demo scenario runner

This is still a portfolio-grade infrastructure project, not a production rollout. The focus is correctness, explainability, and failure handling rather than full product polish or multi-provider commercial readiness.

## Architecture At A Glance

Main runtime surfaces:

- `apps/api`: intent creation, query, receipt, and webhook endpoints
- `apps/worker`: leased execution worker
- `apps/resolver`: follow-up worker for unknown outcomes and pending states
- `apps/callback-worker`: downstream callback delivery worker
- `apps/reconciler`: selected-intent reconciliation runner
- `apps/mock-provider`: controllable provider simulator
- `apps/demo-receiver`: controllable callback target for delivery-failure demos
- `apps/operator-ui`: operator inspection surface

Core crates:

- `crates/domain`: payment intent aggregate, states, attempts, invariants, reconciliation types
- `crates/application`: orchestration for ingestion, execution, webhooks, callbacks, receipts, and reconciliation
- `crates/persistence`: Postgres repositories, leasing, callback queueing, evidence history, receipt assembly

## What You Can Demo

The repo includes a live scenario suite that makes the reliability story concrete.

Supported scenarios:

- duplicate request with the same idempotency key
- retryable provider outage
- terminal provider rejection
- timeout leading to unknown outcome
- delayed webhook resolving unknown outcome
- duplicate webhook event
- callback delivery failure and retry
- reconciliation mismatch
- worker crash and recovery
- stale pending intent requiring reconciliation

Artifacts from each run are written to `demo-output/<scenario>-<timestamp>/`.

Full walkthrough: [docs/demo-scenarios.md](docs/demo-scenarios.md)

## Quick Start

### 1. Apply migrations

```powershell
$env:PGPASSWORD = 'your-postgres-password'
psql -h localhost -U postgres -d payment_gateway -v ON_ERROR_STOP=1 -f migerations/0001_init_payment_gateway.sql
psql -h localhost -U postgres -d payment_gateway -v ON_ERROR_STOP=1 -f migerations/0002_add_worker_leasing.sql
psql -h localhost -U postgres -d payment_gateway -v ON_ERROR_STOP=1 -f migerations/0003_add_unknown_outcome_follow_up.sql
psql -h localhost -U postgres -d payment_gateway -v ON_ERROR_STOP=1 -f migerations/0004_add_callback_delivery_engine.sql
```

### 2. Start the stack

Run each in a separate terminal:

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

Optional but useful for demos:

```powershell
cargo run -p demo-receiver
```

### 3. Run tests

```powershell
cargo test --workspace
```

### 4. Run a demo scenario

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\run-demo-scenario.ps1 -Scenario timeout_unknown_outcome -ApiBearerToken <your-api-token>
```

Or try the callback retry path:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\run-demo-scenario.ps1 -Scenario callback_delivery_failure_and_retry -ApiBearerToken <your-api-token>
```

## Operator UI

The repo includes a minimal operator surface built with Next.js and TypeScript.

Run it with:

```powershell
cd apps/operator-ui
$env:OPERATOR_API_BASE_URL = 'http://127.0.0.1:3000'
$env:OPERATOR_API_BEARER_TOKEN = 'your-api-token'
npm install
npm run dev
```

What the operator surface shows:

- intent list with current state and high-signal flags
- full receipt view for a single intent
- attempt history
- webhook history
- callback notification and delivery history
- reconciliation runs
- stitched timeline of operational evidence

## Key Docs

- [docs/foundation-spec.md](docs/foundation-spec.md): product identity, trust model, scope, and design principles
- [docs/phase-1-implementation-blueprint.md](docs/phase-1-implementation-blueprint.md): build order and implementation blueprint
- [docs/milestone-1-invariants.md](docs/milestone-1-invariants.md): domain invariants
- [docs/milestone-8-provider-webhook-ingestion-readme.md](docs/milestone-8-provider-webhook-ingestion-readme.md): webhook ingestion
- [docs/milestone-9-callback-delivery-engine-readme.md](docs/milestone-9-callback-delivery-engine-readme.md): callback delivery engine
- [docs/milestone-10-receipt-and-evidence-model-readme.md](docs/milestone-10-receipt-and-evidence-model-readme.md): receipt model
- [docs/milestone-11-reconciliation-engine-readme.md](docs/milestone-11-reconciliation-engine-readme.md): reconciliation engine
- [docs/milestone-12-minimal-operator-surface-readme.md](docs/milestone-12-minimal-operator-surface-readme.md): operator UI
- [docs/milestone-13-failure-scenario-demo-suite-readme.md](docs/milestone-13-failure-scenario-demo-suite-readme.md): demo suite

## Honest Notes

- The migration folder is currently named `migerations/`, not `migrations/`.
- The root `cargo run` placeholder binary is not the real app entrypoint; run the individual apps listed above.
- The current implementation is strongest around safety, failure handling, and observability. It is intentionally not trying to be a polished end-user product.

## Why This Matters

The real value of a payment system is not just that it works when everything is healthy.

It is whether it preserves truth when the outcome is uncertain.

That is the design center of this project.
