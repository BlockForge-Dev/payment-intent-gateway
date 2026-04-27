import Link from "next/link";
import { notFound } from "next/navigation";

import {
  CallbackStatusBadge,
  FlagCluster,
  Panel,
  SectionHeading,
  StatusBadge,
} from "@/components/ui";
import { formatDateTime, formatMoney, humanize } from "@/lib/format";
import { GatewayHttpError, getOperatorReceipt } from "@/lib/gateway";

export const dynamic = "force-dynamic";

type IntentDetailPageProps = {
  params: {
    id: string;
  };
};

export default async function IntentDetailPage({ params }: IntentDetailPageProps) {
  try {
    const receipt = await getOperatorReceipt(params.id);
    const summaryFlags = {
      has_unknown_outcome: receipt.ambiguity.visible,
      has_reconciliation_mismatch:
        receipt.summary.latest_failure_classification === "reconciliation_mismatch",
      needs_manual_review: receipt.summary.current_state === "manual_review",
    };

    return (
      <main className="shell">
        <section className="hero detail-hero">
          <div>
            <Link className="back-link" href="/">
              Back to intent inbox
            </Link>
            <p className="eyebrow">Intent detail</p>
            <h1>{receipt.summary.merchant_reference}</h1>
            <p className="lede">
              {receipt.summary.intent_id} |{" "}
              {formatMoney(receipt.summary.amount_minor, receipt.summary.currency)}
            </p>
          </div>
          <div className="hero-meta">
            <StatusBadge state={receipt.summary.current_state} large />
            <FlagCluster flags={summaryFlags} />
          </div>
        </section>

        <section className="grid two-up">
          <Panel title="Summary" subtitle={`Generated ${formatDateTime(receipt.summary.generated_at)}`}>
            <dl className="summary-grid">
              <div>
                <dt>Provider</dt>
                <dd>{humanize(receipt.summary.provider)}</dd>
              </div>
              <div>
                <dt>Final classification</dt>
                <dd>{receipt.summary.final_classification ? humanize(receipt.summary.final_classification) : "Open"}</dd>
              </div>
              <div>
                <dt>Latest failure</dt>
                <dd>{receipt.summary.latest_failure_classification ? humanize(receipt.summary.latest_failure_classification) : "None"}</dd>
              </div>
              <div>
                <dt>Provider reference</dt>
                <dd>{receipt.summary.provider_reference ?? "Pending"}</dd>
              </div>
              <div>
                <dt>Idempotency key</dt>
                <dd className="mono">{receipt.summary.idempotency_key}</dd>
              </div>
              <div>
                <dt>Callback target</dt>
                <dd>{receipt.summary.callback_url ?? "Not configured"}</dd>
              </div>
            </dl>
          </Panel>

          <Panel title="Ambiguity and follow-up">
            <dl className="summary-grid">
              <div>
                <dt>Ambiguity visible</dt>
                <dd>{receipt.ambiguity.visible ? "Yes" : "No"}</dd>
              </div>
              <div>
                <dt>Next resolution check</dt>
                <dd>{receipt.ambiguity.next_resolution_at ? formatDateTime(receipt.ambiguity.next_resolution_at) : "None scheduled"}</dd>
              </div>
              <div>
                <dt>Last resolution check</dt>
                <dd>{receipt.ambiguity.last_resolution_at ? formatDateTime(receipt.ambiguity.last_resolution_at) : "No checks yet"}</dd>
              </div>
              <div>
                <dt>Resolution attempts</dt>
                <dd>{receipt.ambiguity.resolution_attempt_count}</dd>
              </div>
            </dl>
          </Panel>
        </section>

        <section className="content-grid">
          <div className="content-main">
            <Panel title="Timeline" subtitle="Chronological execution and evidence story">
              <ol className="timeline">
                {receipt.timeline.map((entry, index) => (
                  <li className="timeline-row" key={`${entry.kind}-${entry.at}-${index}`}>
                    <div className="timeline-dot" />
                    <div className="timeline-body">
                      <div className="timeline-topline">
                        <strong>{entry.title}</strong>
                        <span>{formatDateTime(entry.at)}</span>
                      </div>
                      <p>{entry.detail}</p>
                      <div className="timeline-meta">
                        {entry.state ? <StatusBadge state={entry.state} /> : null}
                        {entry.evidence_source ? (
                          <span className="meta-pill">{humanize(entry.evidence_source)}</span>
                        ) : null}
                      </div>
                    </div>
                  </li>
                ))}
              </ol>
            </Panel>

            <Panel title="Execution attempts">
              {receipt.attempts.length === 0 ? (
                <p className="subtle">No execution attempts have been recorded yet.</p>
              ) : (
                <div className="stack">
                  {receipt.attempts.map((attempt) => (
                    <article className="detail-card" key={attempt.attempt_no}>
                      <SectionHeading
                        title={`Attempt ${attempt.attempt_no}`}
                        accessory={
                          attempt.outcome ? (
                            <span className="meta-pill">{humanize(attempt.outcome)}</span>
                          ) : (
                            <span className="meta-pill">Open</span>
                          )
                        }
                      />
                      <dl className="detail-grid">
                        <div>
                          <dt>Started</dt>
                          <dd>{formatDateTime(attempt.started_at)}</dd>
                        </div>
                        <div>
                          <dt>Ended</dt>
                          <dd>{attempt.ended_at ? formatDateTime(attempt.ended_at) : "Still running"}</dd>
                        </div>
                        <div>
                          <dt>Classification</dt>
                          <dd>{attempt.classification ? humanize(attempt.classification) : "None"}</dd>
                        </div>
                        <div>
                          <dt>Provider reference</dt>
                          <dd>{attempt.provider_reference ?? "Unavailable"}</dd>
                        </div>
                      </dl>
                      {attempt.reason ? <p>{attempt.reason}</p> : null}
                      {attempt.note ? <p className="subtle">{attempt.note}</p> : null}
                    </article>
                  ))}
                </div>
              )}
            </Panel>

            <Panel title="Callbacks">
              <div className="stack">
                <article className="detail-card">
                  <SectionHeading title="Notification summary" />
                  <dl className="detail-grid">
                    <div>
                      <dt>Configured</dt>
                      <dd>{receipt.callbacks.configured ? "Yes" : "No"}</dd>
                    </div>
                    <div>
                      <dt>Destination</dt>
                      <dd>{receipt.callbacks.destination_url ?? "Not configured"}</dd>
                    </div>
                    <div>
                      <dt>Delivered</dt>
                      <dd>{receipt.callbacks.delivered_count}</dd>
                    </div>
                    <div>
                      <dt>Pending</dt>
                      <dd>{receipt.callbacks.pending_count}</dd>
                    </div>
                    <div>
                      <dt>Dead-lettered</dt>
                      <dd>{receipt.callbacks.dead_lettered_count}</dd>
                    </div>
                    <div>
                      <dt>Delivery attempts</dt>
                      <dd>{receipt.callbacks.delivery_attempt_count}</dd>
                    </div>
                  </dl>
                </article>

                {receipt.callbacks.notifications.map((notification) => (
                  <article className="detail-card" key={notification.event_key}>
                    <SectionHeading
                      title={`Notification ${notification.event_key}`}
                      accessory={<CallbackStatusBadge status={notification.status} />}
                    />
                    <dl className="detail-grid">
                      <div>
                        <dt>Target state</dt>
                        <dd>{humanize(notification.target_state)}</dd>
                      </div>
                      <div>
                        <dt>Next attempt</dt>
                        <dd>{formatDateTime(notification.next_attempt_at)}</dd>
                      </div>
                      <div>
                        <dt>Attempts</dt>
                        <dd>{notification.attempt_count}</dd>
                      </div>
                      <div>
                        <dt>Last HTTP status</dt>
                        <dd>{notification.last_http_status_code ?? "n/a"}</dd>
                      </div>
                    </dl>
                    {notification.last_error ? <p>{notification.last_error}</p> : null}
                  </article>
                ))}

                {receipt.callbacks.deliveries.map((delivery, index) => (
                  <article className="detail-card" key={`${delivery.attempt_no}-${delivery.started_at}-${index}`}>
                    <SectionHeading
                      title={`Delivery attempt ${delivery.attempt_no}`}
                      accessory={<CallbackStatusBadge status={delivery.delivery_result} />}
                    />
                    <dl className="detail-grid">
                      <div>
                        <dt>Started</dt>
                        <dd>{formatDateTime(delivery.started_at)}</dd>
                      </div>
                      <div>
                        <dt>Ended</dt>
                        <dd>{delivery.ended_at ? formatDateTime(delivery.ended_at) : "In progress"}</dd>
                      </div>
                      <div>
                        <dt>HTTP status</dt>
                        <dd>{delivery.http_status_code ?? "n/a"}</dd>
                      </div>
                      <div>
                        <dt>Retry count</dt>
                        <dd>{delivery.retry_count}</dd>
                      </div>
                    </dl>
                    {delivery.response_body ? <p className="subtle">{delivery.response_body}</p> : null}
                  </article>
                ))}
              </div>
            </Panel>
          </div>

          <aside className="content-side">
            <Panel title="Provider webhooks" subtitle={`${receipt.provider_webhooks.total_events} event(s)`}>
              {receipt.provider_webhooks.events.length === 0 ? (
                <p className="subtle">No provider webhooks have been recorded for this intent.</p>
              ) : (
                <div className="stack">
                  {receipt.provider_webhooks.events.map((event) => (
                    <article className="detail-card compact" key={event.provider_event_id}>
                      <SectionHeading
                        title={event.event_type}
                        accessory={
                          event.status_hint ? (
                            <span className="meta-pill">{humanize(event.status_hint)}</span>
                          ) : null
                        }
                      />
                      <p className="mono compact-text">{event.provider_event_id}</p>
                      <p className="subtle">{formatDateTime(event.received_at)}</p>
                    </article>
                  ))}
                </div>
              )}
            </Panel>

            <Panel title="Reconciliation" subtitle={`${receipt.reconciliation.runs.length} run(s)`}>
              {receipt.reconciliation.runs.length === 0 ? (
                <p className="subtle">No reconciliation runs recorded yet.</p>
              ) : (
                <div className="stack">
                  {receipt.reconciliation.runs.map((run, index) => (
                    <article className="detail-card compact" key={`${run.ended_at}-${index}`}>
                      <SectionHeading
                        title={humanize(run.decision)}
                        accessory={<span className="meta-pill">{humanize(run.comparison)}</span>}
                      />
                      <p>Provider saw: {humanize(run.provider_status_seen)}</p>
                      <p>Internal state: {humanize(run.internal_status_seen)}</p>
                      <p className="subtle">{formatDateTime(run.ended_at)}</p>
                      {run.note ? <p className="subtle">{run.note}</p> : null}
                    </article>
                  ))}
                </div>
              )}
            </Panel>

            <Panel title="Evidence notes">
              {receipt.evidence_notes.length === 0 ? (
                <p className="subtle">No extra evidence notes were extracted for this intent.</p>
              ) : (
                <ul className="notes-list">
                  {receipt.evidence_notes.map((note, index) => (
                    <li key={`${note.at}-${index}`}>
                      <strong>{humanize(note.source)}</strong>
                      <p>{note.note}</p>
                      <span>{formatDateTime(note.at)}</span>
                    </li>
                  ))}
                </ul>
              )}
            </Panel>
          </aside>
        </section>
      </main>
    );
  } catch (error) {
    if (error instanceof GatewayHttpError && error.status === 404) {
      notFound();
    }

    const message = error instanceof Error ? error.message : "Unknown operator surface failure";
    return (
      <main className="shell">
        <section className="hero">
          <div>
            <Link className="back-link" href="/">
              Back to intent inbox
            </Link>
            <p className="eyebrow">Intent detail</p>
            <h1>Unable to load receipt</h1>
            <p className="lede">{message}</p>
          </div>
        </section>
      </main>
    );
  }
}
