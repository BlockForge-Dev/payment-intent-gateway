# Demo Scenarios

This project includes a reproducible failure scenario demo suite for the Payment Intent Gateway.

The goal is to make the reliability story observable through:

- repeatable API requests
- controllable provider behavior
- controllable downstream callback behavior
- receipts saved as demo artifacts

## Prerequisites

Apply the database migrations, then start the runtime services.

```powershell
cargo run -p api
cargo run -p worker
cargo run -p resolver
cargo run -p callback-worker
cargo run -p mock-provider
cargo run -p demo-receiver
```

For reconciliation scenarios, the demo runner starts the one-shot reconciler itself with:

```powershell
cargo run -p reconciler
```

## Operator UI

If you want to inspect each scenario visually while it runs:

```powershell
cd apps/operator-ui
$env:OPERATOR_API_BASE_URL='http://127.0.0.1:3000'
$env:OPERATOR_API_BEARER_TOKEN='your-api-token'
npm run dev
```

## Scenario Runner

The main helper is:

```powershell
.\scripts\run-demo-scenario.ps1 -Scenario <scenario_name>
```

Artifacts are written to:

```text
demo-output/<scenario>-<timestamp>/
```

Each run saves the final receipt and any extra evidence captured during the scenario.

## Supported Scenarios

### Duplicate request with same idempotency key

```powershell
.\scripts\run-demo-scenario.ps1 -Scenario duplicate_request_same_idempotency
```

Expected outcome:

- both create requests return the same `intent_id`
- one business lineage exists
- receipt shows one intent lifecycle

### Retryable provider outage

```powershell
.\scripts\run-demo-scenario.ps1 -Scenario retryable_provider_outage
```

Expected outcome:

- state becomes `retry_scheduled`
- receipt shows a retryable failure
- no terminal outcome is invented

### Terminal provider rejection

```powershell
.\scripts\run-demo-scenario.ps1 -Scenario terminal_provider_rejection
```

Expected outcome:

- state becomes `failed_terminal`
- no automatic retry is scheduled

### Timeout leading to unknown outcome

```powershell
.\scripts\run-demo-scenario.ps1 -Scenario timeout_unknown_outcome
```

Expected outcome:

- state becomes `unknown_outcome`
- receipt shows ambiguity explicitly
- provider webhook is disabled for this demo run so the ambiguity stays visible

### Delayed webhook resolving unknown outcome

```powershell
.\scripts\run-demo-scenario.ps1 -Scenario delayed_webhook_resolves_unknown
```

Expected outcome:

- request first enters an ambiguous timeout path
- provider webhook later resolves the intent to `succeeded`
- receipt shows webhook evidence in the timeline

### Duplicate webhook event

```powershell
.\scripts\run-demo-scenario.ps1 -Scenario duplicate_webhook_event
```

Expected outcome:

- provider sends duplicate webhook attempts
- stored provider event history remains deduplicated
- receipt still tells a clean success story

### Callback delivery failure and retry

```powershell
.\scripts\run-demo-scenario.ps1 -Scenario callback_delivery_failure_and_retry
```

Expected outcome:

- downstream callback fails once on purpose
- callback worker retries
- receipt shows callback retry and eventual delivery
- payment execution truth remains separate from callback trouble

### Reconciliation mismatch

```powershell
.\scripts\run-demo-scenario.ps1 -Scenario reconciliation_mismatch
```

Expected outcome:

- intent first succeeds internally
- provider-side record is deleted from the mock provider
- reconciliation escalates the intent to `manual_review`
- receipt shows mismatch instead of silently trusting stale internal truth

### Worker crash and recovery

```powershell
.\scripts\run-demo-scenario.ps1 -Scenario worker_crash_and_recovery
```

Expected outcome:

- the first worker exits immediately after lease acquisition
- the lease expires
- a replacement worker later reclaims and executes the intent
- receipt timeline shows multiple lease acquisitions before success

Note:

- run this scenario without another long-running worker already active

### Stale pending intent requiring recon

```powershell
.\scripts\run-demo-scenario.ps1 -Scenario stale_pending_requires_recon
```

Expected outcome:

- intent becomes `provider_pending`
- provider later resolves without webhook evidence
- reconciliation resolves the stale pending intent later
- receipt shows the pending phase and the later reconciliation decision

## Scenario Control Directives

The demo suite uses merchant-reference directives to control provider behavior without changing
the API contract.

Examples:

- `#scenario=timeout_after_acceptance`
- `#provider_webhook=off`
- `#resolution_delay_ms=3000`
- `#timeout_response_delay_ms=15000`

These are parsed by the mock provider adapter and forwarded to the mock provider only for demo
purposes.
