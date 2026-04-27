export function humanize(value: string): string {
  return value
    .split("_")
    .filter(Boolean)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join(" ");
}

export function formatDateTime(value: string): string {
  return new Intl.DateTimeFormat("en-US", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

export function formatMoney(amountMinor: number, currency: string): string {
  const amount = amountMinor / 100;

  try {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency,
      minimumFractionDigits: 2,
    }).format(amount);
  } catch {
    return `${currency} ${amount.toFixed(2)}`;
  }
}

export function toneForCallbackStatus(status: string): string {
  switch (status) {
    case "delivered":
      return "delivered";
    case "retry_scheduled":
      return "retry_scheduled";
    case "scheduled":
      return "scheduled";
    case "delivering":
      return "delivering";
    case "dead_lettered":
      return "dead_lettered";
    case "failed":
      return "failed";
    default:
      return "neutral";
  }
}
