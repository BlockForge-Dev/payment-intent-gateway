export type OperatorIntentList = {
  generated_at: string;
  items: OperatorIntentListItem[];
};

export type OperatorIntentListItem = {
  intent_id: string;
  merchant_reference: string;
  amount_minor: number;
  currency: string;
  provider: string;
  state: string;
  latest_failure_classification: string | null;
  provider_reference: string | null;
  updated_at: string;
  flags: OperatorIntentListFlags;
};

export type OperatorIntentListFlags = {
  has_unknown_outcome: boolean;
  has_reconciliation_mismatch: boolean;
  needs_manual_review: boolean;
};

export type OperatorReceipt = {
  summary: OperatorReceiptSummary;
  ambiguity: OperatorReceiptAmbiguity;
  attempts: OperatorExecutionAttempt[];
  provider_webhooks: OperatorWebhookHistory;
  callbacks: OperatorCallbackHistory;
  reconciliation: OperatorReconciliationHistory;
  timeline: OperatorTimelineEntry[];
  evidence_notes: OperatorEvidenceNote[];
};

export type OperatorReceiptSummary = {
  intent_id: string;
  merchant_reference: string;
  idempotency_key: string;
  amount_minor: number;
  currency: string;
  provider: string;
  callback_url: string | null;
  current_state: string;
  final_classification: string | null;
  latest_failure_classification: string | null;
  provider_reference: string | null;
  generated_at: string;
};

export type OperatorReceiptAmbiguity = {
  visible: boolean;
  next_resolution_at: string | null;
  last_resolution_at: string | null;
  resolution_attempt_count: number;
};

export type OperatorExecutionAttempt = {
  attempt_no: number;
  started_at: string;
  ended_at: string | null;
  outcome: string | null;
  classification: string | null;
  reason: string | null;
  provider_reference: string | null;
  note: string | null;
};

export type OperatorWebhookHistory = {
  total_events: number;
  latest_event_at: string | null;
  events: OperatorWebhookEvent[];
};

export type OperatorWebhookEvent = {
  provider_name: string;
  provider_event_id: string;
  provider_reference: string | null;
  event_type: string;
  status_hint: string | null;
  received_at: string;
  processed_at: string | null;
};

export type OperatorCallbackHistory = {
  configured: boolean;
  destination_url: string | null;
  notification_count: number;
  delivered_count: number;
  pending_count: number;
  dead_lettered_count: number;
  delivery_attempt_count: number;
  notifications: OperatorCallbackNotification[];
  deliveries: OperatorCallbackDelivery[];
};

export type OperatorCallbackNotification = {
  event_key: string;
  destination_url: string;
  target_state: string;
  status: string;
  next_attempt_at: string;
  attempt_count: number;
  last_attempt_at: string | null;
  delivered_at: string | null;
  last_http_status_code: number | null;
  last_error: string | null;
  created_at: string;
  updated_at: string;
};

export type OperatorCallbackDelivery = {
  destination_url: string;
  attempt_no: number;
  delivery_result: string;
  http_status_code: number | null;
  retry_count: number;
  started_at: string;
  ended_at: string | null;
  response_body: string | null;
};

export type OperatorReconciliationHistory = {
  latest: OperatorReconciliationRun | null;
  runs: OperatorReconciliationRun[];
};

export type OperatorReconciliationRun = {
  started_at: string;
  ended_at: string;
  provider_status_seen: string;
  internal_status_seen: string;
  comparison: string;
  decision: string;
  evidence: string;
  note: string | null;
};

export type OperatorTimelineEntry = {
  at: string;
  kind: string;
  title: string;
  detail: string;
  state: string | null;
  evidence_source: string | null;
};

export type OperatorEvidenceNote = {
  at: string;
  source: string;
  note: string;
};
