import type { ReactNode } from "react";

import { humanize, toneForCallbackStatus } from "@/lib/format";

type PanelProps = {
  title?: string;
  subtitle?: string;
  tone?: "neutral" | "warning" | "danger" | "accent";
  children: ReactNode;
};

export function Panel({ title, subtitle, tone = "neutral", children }: PanelProps) {
  return (
    <section className={`panel ${tone}`}>
      {title || subtitle ? (
        <header className="panel-header">
          <div>
            {title ? <h2>{title}</h2> : null}
            {subtitle ? <p>{subtitle}</p> : null}
          </div>
        </header>
      ) : null}
      <div className="panel-body">{children}</div>
    </section>
  );
}

export function StatusBadge({
  state,
  large = false,
}: {
  state: string;
  large?: boolean;
}) {
  return (
    <span className={`status-badge state-${state} ${large ? "large" : ""}`.trim()}>
      {humanize(state)}
    </span>
  );
}

export function CallbackStatusBadge({ status }: { status: string }) {
  const tone = toneForCallbackStatus(status);
  return <span className={`callback-pill callback-${tone}`}>{humanize(status)}</span>;
}

export function FlagCluster({
  flags,
}: {
  flags: {
    has_unknown_outcome: boolean;
    has_reconciliation_mismatch: boolean;
    needs_manual_review: boolean;
  };
}) {
  const entries = [
    flags.has_unknown_outcome ? { label: "Ambiguous", tone: "warning" } : null,
    flags.has_reconciliation_mismatch ? { label: "Mismatch", tone: "danger" } : null,
    flags.needs_manual_review ? { label: "Manual review", tone: "danger" } : null,
  ].filter(Boolean) as Array<{ label: string; tone: "warning" | "danger" }>;

  if (entries.length === 0) {
    return <span className="subtle">No risk flags</span>;
  }

  return (
    <div className="flag-cluster">
      {entries.map((entry) => (
        <span className={`flag-pill ${entry.tone}`} key={entry.label}>
          {entry.label}
        </span>
      ))}
    </div>
  );
}

export function SectionHeading({
  title,
  accessory,
}: {
  title: string;
  accessory?: ReactNode;
}) {
  return (
    <div className="section-heading">
      <h3>{title}</h3>
      {accessory}
    </div>
  );
}
