import { validateDecision } from "./model.js";

const MAX_RESPONSE_BYTES = 64 * 1024;

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
    return this.request("/v1/review/submissions", { query });
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
