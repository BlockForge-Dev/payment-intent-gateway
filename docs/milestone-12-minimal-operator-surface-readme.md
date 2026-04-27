# Milestone 12

Minimal Operator Surface

## Goal

Expose system truth for inspection through a minimal web UI built with Next.js and TypeScript.

## What This Milestone Adds

This milestone turns the receipt and evidence model into a real operator-facing surface.

The repo now includes:

- a new operator list endpoint on `GET /payment-intents`
- a new Next.js application in `apps/operator-ui`
- a receipt-driven detail page backed by `GET /payment-intents/{id}/receipt`

The operator surface provides two core views:

- an intent inbox showing the latest payment intents and their operational flags
- an intent detail page showing the full execution story for a single intent

## Why This Matters

Milestones 8 to 11 established the truth model:

- provider webhook evidence
- callback notification truth
- receipt stitching
- reconciliation history

Milestone 12 makes that truth visible.

An operator can now inspect:

- which intents are ambiguous or still pending
- which intents need manual review
- which intents carry reconciliation mismatch signals
- whether callback trouble is separate from execution truth
- the timeline, attempts, webhook evidence, callback history, and recon history for one intent

This is where the system stops feeling like internal plumbing and starts feeling like an
operational product.

## Backend Changes

The API now supports an operator-oriented list view through:

- `GET /payment-intents?limit=100`

That response includes:

- intent id
- merchant reference
- amount and currency
- provider
- current state
- latest failure classification
- provider reference
- updated timestamp
- operator flags for:
  - unknown outcome / pending follow-up
  - reconciliation mismatch
  - manual review

The existing receipt endpoint remains the source for detail inspection:

- `GET /payment-intents/{id}/receipt`

## Frontend Surface

The new app lives in:

- `apps/operator-ui`

It is intentionally server-rendered and API-backed.

That means:

- the bearer token stays on the server side
- the UI always reads live gateway truth
- there is no client-side replay of sensitive operator credentials

### List view

The list page highlights:

- recent payment intents
- current state badges
- latest failure signal
- ambiguity, mismatch, and manual review flags

### Detail view

The detail page highlights:

- summary and identity
- ambiguity and follow-up metadata
- execution attempts
- callback notification and delivery history
- provider webhook history
- reconciliation history
- extracted evidence notes
- one stitched timeline

## Design Choice

This milestone uses the computed operator receipt from Milestone 10 rather than building a
second competing read model for detail pages.

That keeps the operator UI aligned with the same operational truth contract already exposed by
the API.

## Local Run Flow

Start the Rust services first:

```powershell
cargo run -p api
cargo run -p worker
cargo run -p resolver
cargo run -p callback-worker
cargo run -p reconciler
cargo run -p mock-provider
```

Then start the operator UI:

```powershell
cd apps/operator-ui
$env:OPERATOR_API_BASE_URL='http://127.0.0.1:3000'
$env:OPERATOR_API_BEARER_TOKEN='your-api-token'
npm install
npm run dev
```

The production build also verifies successfully with:

```powershell
npm run build
```

## Verification

The milestone was verified with:

- `cargo test --workspace`
- `npm run build` inside `apps/operator-ui`

## Done Means

Milestone 12 is done because:

- an operator can inspect payment intents end to end
- ambiguous states stand out clearly
- callback delivery trouble is visible separately from provider outcome truth
- manual review candidates are visible from the list
- receipt evidence is readable through a minimal web surface
