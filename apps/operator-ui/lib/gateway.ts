import { OperatorIntentList, OperatorReceipt } from "@/lib/types";

const DEFAULT_API_BASE_URL = "http://127.0.0.1:3000";

export class GatewayHttpError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "GatewayHttpError";
    this.status = status;
  }
}

function getGatewayConfig() {
  const baseUrl = process.env.OPERATOR_API_BASE_URL ?? DEFAULT_API_BASE_URL;
  const bearerToken =
    process.env.OPERATOR_API_BEARER_TOKEN ?? process.env.API_BEARER_TOKEN;

  if (!bearerToken) {
    throw new Error(
      "Missing OPERATOR_API_BEARER_TOKEN. The operator surface needs a bearer token to query the gateway API.",
    );
  }

  return { baseUrl, bearerToken };
}

async function gatewayFetch<T>(path: string): Promise<T> {
  const { baseUrl, bearerToken } = getGatewayConfig();
  const response = await fetch(new URL(path, baseUrl), {
    method: "GET",
    headers: {
      Authorization: `Bearer ${bearerToken}`,
    },
    cache: "no-store",
  });

  if (!response.ok) {
    const body = await response.text();
    throw new GatewayHttpError(
      response.status,
      `Gateway API request failed for ${path}: ${response.status} ${body}`.trim(),
    );
  }

  return (await response.json()) as T;
}

export function listOperatorIntents(limit = 100): Promise<OperatorIntentList> {
  return gatewayFetch<OperatorIntentList>(`/payment-intents?limit=${limit}`);
}

export function getOperatorReceipt(intentId: string): Promise<OperatorReceipt> {
  return gatewayFetch<OperatorReceipt>(`/payment-intents/${intentId}/receipt`);
}
