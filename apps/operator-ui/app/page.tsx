import Link from "next/link";

import { FlagCluster, Panel, StatusBadge } from "@/components/ui";
import { listOperatorIntents } from "@/lib/gateway";
import { formatDateTime, formatMoney, humanize } from "@/lib/format";

export const dynamic = "force-dynamic";

export default async function HomePage() {
  try {
    const list = await listOperatorIntents();
    const ambiguousCount = list.items.filter((item) => item.flags.has_unknown_outcome).length;
    const mismatchCount = list.items.filter(
      (item) => item.flags.has_reconciliation_mismatch,
    ).length;
    const manualReviewCount = list.items.filter(
      (item) => item.flags.needs_manual_review,
    ).length;

    return (
      <main className="shell">
        <section className="hero">
          <div>
            <p className="eyebrow">Milestone 12</p>
            <h1>Operator Surface</h1>
            <p className="lede">
              One place to inspect payment intent truth, ambiguity, callback trouble, and
              reconciliation risk without guessing from logs.
            </p>
          </div>
          <div className="hero-stats">
            <Panel tone="neutral">
              <span className="stat-label">Visible intents</span>
              <strong className="stat-value">{list.items.length}</strong>
            </Panel>
            <Panel tone="warning">
              <span className="stat-label">Ambiguous or pending</span>
              <strong className="stat-value">{ambiguousCount}</strong>
            </Panel>
            <Panel tone="danger">
              <span className="stat-label">Manual review</span>
              <strong className="stat-value">{manualReviewCount}</strong>
            </Panel>
            <Panel tone="accent">
              <span className="stat-label">Mismatch signals</span>
              <strong className="stat-value">{mismatchCount}</strong>
            </Panel>
          </div>
        </section>

        <Panel title="Intent inbox" subtitle={`Generated ${formatDateTime(list.generated_at)}`}>
          {list.items.length === 0 ? (
            <div className="empty-state">
              <h2>No payment intents yet</h2>
              <p>The gateway has not recorded any intents for the operator surface to inspect.</p>
            </div>
          ) : (
            <div className="table-wrap">
              <table className="intent-table">
                <thead>
                  <tr>
                    <th>Intent</th>
                    <th>Amount</th>
                    <th>Provider</th>
                    <th>State</th>
                    <th>Flags</th>
                    <th>Updated</th>
                  </tr>
                </thead>
                <tbody>
                  {list.items.map((item) => (
                    <tr key={item.intent_id}>
                      <td>
                        <Link className="intent-link" href={`/intents/${item.intent_id}`}>
                          {item.merchant_reference}
                        </Link>
                        <div className="subtle">{item.intent_id}</div>
                      </td>
                      <td>{formatMoney(item.amount_minor, item.currency)}</td>
                      <td>{humanize(item.provider)}</td>
                      <td>
                        <StatusBadge state={item.state} />
                        {item.latest_failure_classification ? (
                          <div className="subtle">
                            {humanize(item.latest_failure_classification)}
                          </div>
                        ) : null}
                      </td>
                      <td>
                        <FlagCluster flags={item.flags} />
                      </td>
                      <td>{formatDateTime(item.updated_at)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Panel>
      </main>
    );
  } catch (error) {
    const message = error instanceof Error ? error.message : "Unknown operator surface failure";
    return (
      <main className="shell">
        <section className="hero">
          <div>
            <p className="eyebrow">Operator Surface</p>
            <h1>Gateway connection problem</h1>
            <p className="lede">
              The frontend is up, but it could not reach the Payment Intent Gateway API.
            </p>
          </div>
        </section>
        <Panel tone="danger" title="What failed">
          <p>{message}</p>
          <p className="subtle">
            Set <code>OPERATOR_API_BEARER_TOKEN</code> and make sure the Rust API is running.
          </p>
        </Panel>
      </main>
    );
  }
}
