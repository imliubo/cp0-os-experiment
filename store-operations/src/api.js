import { RELEASE_ID, validateDecision, validateEditorial } from "./model.js";

const MAX_RESPONSE_BYTES = 64 * 1024;

function strictOrigin(value) {
  const parsed = new URL(value);
  if (parsed.protocol !== "https:" || parsed.username || parsed.password || parsed.search || parsed.hash || !["", "/"].includes(parsed.pathname)) {
    throw new Error("Operations API must be a bare HTTPS origin");
  }
  return parsed.origin;
}

async function boundedJson(response) {
  const declared = Number(response.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > MAX_RESPONSE_BYTES) throw new Error("Operations API response is too large");
  if (!response.body?.getReader) {
    const body = await response.text();
    if (new TextEncoder().encode(body).length > MAX_RESPONSE_BYTES) throw new Error("Operations API response is too large");
    return body ? JSON.parse(body) : null;
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let body = "";
  let total = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > MAX_RESPONSE_BYTES) {
        await reader.cancel();
        throw new Error("Operations API response is too large");
      }
      body += decoder.decode(value, { stream: true });
    }
    body += decoder.decode();
    return body ? JSON.parse(body) : null;
  } finally {
    reader.releaseLock();
  }
}

function mutationKey() {
  return `operations-${crypto.randomUUID()}`;
}

function validEtag(value) {
  return typeof value === "string" && /^"[1-9][0-9]*"$/.test(value);
}

function validateQueue(value) {
  if (!value || typeof value !== "object" || !Array.isArray(value.items) || value.items.length > 50 ||
      (value.next_cursor !== null && value.next_cursor !== undefined && (typeof value.next_cursor !== "string" || value.next_cursor.length > 128))) {
    throw new Error("Moderation queue response is invalid");
  }
  return value;
}

export class OperationsApi {
  constructor({ origin = "https://operations.cardputerzero.dev", tokenProvider, fetchImpl = fetch }) {
    this.origin = strictOrigin(origin);
    this.tokenProvider = tokenProvider;
    this.fetchImpl = fetchImpl;
  }

  async request(path, { method = "GET", body, etag, query } = {}) {
    if (!path.startsWith("/v1/") || path.includes("?") || path.includes("#")) throw new Error("Operations API path is outside v1");
    const target = new URL(path, this.origin);
    if (target.origin !== this.origin || target.pathname !== path) throw new Error("Operations API path is outside v1");
    if (query) target.search = query.toString();
    const token = await this.tokenProvider?.();
    if (!token || token.length < 32 || /[\r\n]/.test(token)) throw new Error("Operator authorization is unavailable");
    const headers = { Accept: "application/json", Authorization: `Bearer ${token}` };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    if (method !== "GET") headers["Idempotency-Key"] = mutationKey();
    if (etag) {
      if (!validEtag(etag)) throw new Error("Operations mutation requires a strong ETag");
      headers["If-Match"] = etag;
    }
    const response = await this.fetchImpl(target.href, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      credentials: "omit",
      redirect: "error",
      referrerPolicy: "no-referrer",
    });
    const decoded = await boundedJson(response);
    if (!response.ok) throw new Error(`Operations API returned ${response.status} (${decoded?.code ?? "unknown-error"})`);
    return { data: decoded, etag: response.headers.get("etag") };
  }

  getToday() {
    return this.request("/v1/editorial/today");
  }

  saveToday(request, etag, releases = []) {
    if (Object.keys(validateEditorial(request, releases)).length) throw new Error("Today editorial request is invalid");
    return this.request("/v1/editorial/today", { method: etag ? "PUT" : "POST", body: request, etag });
  }

  listReports({ cursor, limit = 25 } = {}) {
    if (!Number.isInteger(limit) || limit < 1 || limit > 50) throw new Error("Moderation queue limit is outside 1-50");
    const query = new URLSearchParams({ limit: String(limit) });
    if (cursor) query.set("cursor", cursor);
    return this.request("/v1/moderation/reports", { query }).then((response) => ({ ...response, data: validateQueue(response.data) }));
  }

  decideReport(reportId, etag, request) {
    if (!/^report_[0-9a-f]{32}$/.test(reportId)) throw new Error("Report ID is invalid");
    if (!validEtag(etag)) throw new Error("Moderation decision requires a strong ETag");
    if (Object.keys(validateDecision(request)).length) throw new Error("Moderation decision is invalid");
    return this.request(`/v1/moderation/reports/${encodeURIComponent(reportId)}:decide`, { method: "POST", etag, body: request });
  }
}

export { RELEASE_ID };
