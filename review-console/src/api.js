import { validateDecision } from "./model.js";

const MAX_RESPONSE_BYTES = 64 * 1024;
const RISK_TIERS = new Set(["standard", "elevated", "high"]);
const RISK_REASONS = new Set([
  "camera-capture",
  "hardware-control",
  "microphone-capture",
  "multiple-sensitive-capabilities",
  "network-access",
  "radio-transmit",
  "user-documents",
]);

function strictOrigin(value) {
  const parsed = new URL(value);
  if (parsed.protocol !== "https:" || parsed.username || parsed.password || parsed.search || parsed.hash || !["", "/"].includes(parsed.pathname)) {
    throw new Error("Review API must be a bare HTTPS origin");
  }
  return parsed.origin;
}

async function boundedJson(response) {
  const declared = Number(response.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > MAX_RESPONSE_BYTES) throw new Error("Review API response is too large");
  if (!response.body?.getReader) {
    const encoded = await response.text();
    if (new TextEncoder().encode(encoded).length > MAX_RESPONSE_BYTES) throw new Error("Review API response is too large");
    return encoded ? JSON.parse(encoded) : null;
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let total = 0;
  let encoded = "";
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > MAX_RESPONSE_BYTES) {
        await reader.cancel();
        throw new Error("Review API response is too large");
      }
      encoded += decoder.decode(value, { stream: true });
    }
    encoded += decoder.decode();
    return encoded ? JSON.parse(encoded) : null;
  } finally {
    reader.releaseLock();
  }
}

function mutationKey() {
  return `review-${crypto.randomUUID()}`;
}

function validateQueuePage(value) {
  if (!value || typeof value !== "object" || !Array.isArray(value.items) || value.items.length > 50) {
    throw new Error("Review queue response is invalid");
  }
  if (value.next_cursor !== null && value.next_cursor !== undefined && (typeof value.next_cursor !== "string" || value.next_cursor.length > 256)) {
    throw new Error("Review queue cursor is invalid");
  }
  for (const item of value.items) {
    const risk = item?.risk;
    if (!risk || !Number.isInteger(risk.policy_version) || risk.policy_version < 1 || risk.policy_version > 32767 || !RISK_TIERS.has(risk.tier) || !Array.isArray(risk.reasons) || risk.reasons.length > 7) {
      throw new Error("Review queue risk assessment is invalid");
    }
    const reasons = [...risk.reasons];
    if (reasons.some((reason) => !RISK_REASONS.has(reason)) || new Set(reasons).size !== reasons.length || reasons.join("\0") !== reasons.sort().join("\0")) {
      throw new Error("Review queue risk reasons are invalid");
    }
  }
  return value;
}

export class ReviewApi {
  constructor({ origin = "https://review.cardputerzero.dev", tokenProvider, fetchImpl = fetch }) {
    this.origin = strictOrigin(origin);
    this.tokenProvider = tokenProvider;
    this.fetchImpl = fetchImpl;
  }

  async request(path, { method = "GET", body, etag, query } = {}) {
    if (!path.startsWith("/v1/") || path.includes("?") || path.includes("#")) throw new Error("Review API path is outside v1");
    const target = new URL(path, this.origin);
    if (target.origin !== this.origin || target.pathname !== path) throw new Error("Review API path is outside v1");
    if (query) target.search = query.toString();
    const token = await this.tokenProvider?.();
    if (!token || token.length < 32 || /[\r\n]/.test(token)) throw new Error("Reviewer authorization is unavailable");
    const headers = { Accept: "application/json", Authorization: `Bearer ${token}` };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    if (method !== "GET") headers["Idempotency-Key"] = mutationKey();
    if (etag) headers["If-Match"] = etag;
    const response = await this.fetchImpl(target.href, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      credentials: "omit",
      redirect: "error",
      referrerPolicy: "no-referrer",
    });
    const decoded = await boundedJson(response);
    if (!response.ok) throw new Error(`Review API returned ${response.status} (${decoded?.code ?? "unknown-error"})`);
    return { data: decoded, etag: response.headers.get("etag") };
  }

  listQueue({ cursor, limit = 25 } = {}) {
    if (!Number.isInteger(limit) || limit < 1 || limit > 50) throw new Error("Review queue limit is outside 1-50");
    const query = new URLSearchParams({ limit: String(limit) });
    if (cursor) query.set("cursor", cursor);
    return this.request("/v1/review/submissions", { query }).then((response) => ({
      ...response,
      data: validateQueuePage(response.data),
    }));
  }

  beginReview(submissionId, etag) {
    if (!etag) throw new Error("Review claim requires an ETag");
    return this.request(`/v1/review/submissions/${encodeURIComponent(submissionId)}:begin`, { method: "POST", etag });
  }

  decideReview(submissionId, etag, request) {
    if (!etag) throw new Error("Review decision requires an ETag");
    if (Object.keys(validateDecision(request)).length) throw new Error("Review decision is invalid");
    return this.request(`/v1/review/submissions/${encodeURIComponent(submissionId)}/decisions`, { method: "POST", etag, body: request });
  }

  postMessage(submissionId, body) {
    if (!body || body.trim() !== body || body.length > 2000) throw new Error("Review message is invalid");
    return this.request(`/v1/submissions/${encodeURIComponent(submissionId)}/messages`, { method: "POST", body: { body } });
  }
}
