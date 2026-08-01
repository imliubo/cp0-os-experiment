import { RELEASE_ID, validateDecision, validateEditorial } from "./model.js";
import { OperationsWorkforceSessionClient } from "./workforce-session.js";

const MAX_RESPONSE_BYTES = 64 * 1024;
const APP_ID = /^[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*){2,}$/;
const VERSION = /^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;
const EDITORIAL_RELEASE_CURSOR = /^[0-9a-f]{16}\.rel_[0-9a-f]{32}$/;
const STORE_CATEGORIES = new Set(["developer-tools", "education", "entertainment", "games", "hardware", "media", "productivity", "utilities"]);
const REPORT_ID = /^report_[0-9a-f]{32}$/;
const MODERATION_CURSOR = /^[1-9][0-9]*:report_[0-9a-f]{32}$/;
const NOTICE_ID = /^notice_[0-9a-f]{32}$/;
const APPEAL_ID = /^appeal_[0-9a-f]{32}$/;
const REPORT_REASONS = new Set(["malware", "privacy", "fraud", "harmful-content", "age-rating", "other"]);
const REPORT_STATES = new Set(["submitted", "closed-no-action", "notice-issued", "security-escalated", "closed-after-appeal"]);
const DECISION_REASONS = new Set(["duplicate", "insufficient-evidence", "policy-violation", "security-review", "identity-confirmed", "remediation-accepted"]);
const NOTICE_STATES = new Set(["open", "appealed", "resolved-accepted", "resolved-upheld"]);
const APPEAL_STATES = new Set(["pending", "accepted", "upheld"]);

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

function hasExactKeys(value, expected) {
  return value && typeof value === "object" && !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...expected].sort().join("\0");
}

function isSafeText(value, maximum) {
  return typeof value === "string" && value === value.trim() && value.length > 0 &&
    [...value].length <= maximum && !/[\u0000-\u001f\u007f]/.test(value);
}

function isSemanticVersion(value) {
  if (typeof value !== "string" || value.length > 64) return false;
  const match = VERSION.exec(value);
  return Boolean(match) && (!match[1] || match[1].split(".")
    .every((identifier) => !/^\d+$/.test(identifier) || identifier === "0" || !identifier.startsWith("0")));
}

function validateEditorialReleases(value) {
  if (!hasExactKeys(value, ["items", "next_cursor"]) || !Array.isArray(value.items) || value.items.length > 50 ||
      (value.next_cursor !== null && (typeof value.next_cursor !== "string" || !EDITORIAL_RELEASE_CURSOR.test(value.next_cursor)))) {
    throw new Error("Editorial Release response is invalid");
  }
  const releaseIds = new Set();
  const appIds = new Set();
  let previous = null;
  for (const release of value.items) {
    if (!hasExactKeys(release, ["release_id", "app_id", "name", "version", "category", "catalog_sequence"]) ||
        !RELEASE_ID.test(release.release_id) || typeof release.app_id !== "string" ||
        release.app_id.length > 128 || !APP_ID.test(release.app_id) || !isSafeText(release.name, 32) ||
        !isSemanticVersion(release.version) ||
        (release.category !== null && !STORE_CATEGORIES.has(release.category)) ||
        !Number.isSafeInteger(release.catalog_sequence) || release.catalog_sequence < 1 ||
        releaseIds.has(release.release_id) || appIds.has(release.app_id)) {
      throw new Error("Editorial Release response is invalid");
    }
    const order = [release.catalog_sequence, release.release_id];
    if (previous && (order[0] < previous[0] || (order[0] === previous[0] && order[1] <= previous[1]))) {
      throw new Error("Editorial Release response is invalid");
    }
    releaseIds.add(release.release_id);
    appIds.add(release.app_id);
    previous = order;
  }
  if (value.next_cursor !== null) {
    const last = value.items.at(-1);
    const expected = last && `${last.catalog_sequence.toString(16).padStart(16, "0")}.${last.release_id}`;
    if (!expected || value.next_cursor !== expected) throw new Error("Editorial Release response is invalid");
  }
  return value;
}

function validateEditorialLayout(value) {
  const keys = ["layout_id", "headline", "featured", "collections", "resource_version", "updated_unix_seconds"];
  if (!hasExactKeys(value, keys) || value.layout_id !== "today" || !isSafeText(value.headline, 48) ||
      !hasExactKeys(value.featured, ["release_id", "app_id"]) || !RELEASE_ID.test(value.featured.release_id) || !APP_ID.test(value.featured.app_id) ||
      !Array.isArray(value.collections) || value.collections.length < 1 || value.collections.length > 2 ||
      !Number.isSafeInteger(value.resource_version) || value.resource_version < 1 ||
      !Number.isSafeInteger(value.updated_unix_seconds) || value.updated_unix_seconds < 1) {
    throw new Error("Today editorial response is invalid");
  }
  const releases = new Set([value.featured.release_id]);
  const apps = new Set([value.featured.app_id]);
  for (const collection of value.collections) {
    if (!hasExactKeys(collection, ["title", "items"]) || !isSafeText(collection.title, 32) || !Array.isArray(collection.items) || collection.items.length < 1 || collection.items.length > 4) {
      throw new Error("Today editorial response is invalid");
    }
    for (const item of collection.items) {
      if (!hasExactKeys(item, ["release_id", "app_id"]) || !RELEASE_ID.test(item.release_id) || !APP_ID.test(item.app_id) || releases.has(item.release_id) || apps.has(item.app_id)) {
        throw new Error("Today editorial response is invalid");
      }
      releases.add(item.release_id);
      apps.add(item.app_id);
    }
  }
  return value;
}

function validOptionalUnix(value) {
  return value === null || (Number.isSafeInteger(value) && value >= 1);
}

function validateContentReport(value) {
  const keys = ["report_id", "release_id", "app_id", "version", "reason_code", "sla_class", "state", "disposition", "decision_reason_codes", "acknowledgement_due_unix_seconds", "resolution_due_unix_seconds", "acknowledged_unix_seconds", "closed_unix_seconds", "resource_version", "created_unix_seconds", "updated_unix_seconds"];
  if (!hasExactKeys(value, keys) || !REPORT_ID.test(value.report_id) || !RELEASE_ID.test(value.release_id) || !APP_ID.test(value.app_id) ||
      !isSemanticVersion(value.version) || !REPORT_REASONS.has(value.reason_code) || !["security", "standard"].includes(value.sla_class) ||
      !REPORT_STATES.has(value.state) || ![null, "no-action", "developer-notice", "security-escalation"].includes(value.disposition) ||
      !Array.isArray(value.decision_reason_codes) || value.decision_reason_codes.length > 4 || new Set(value.decision_reason_codes).size !== value.decision_reason_codes.length ||
      value.decision_reason_codes.some((reason) => !DECISION_REASONS.has(reason)) || !Number.isSafeInteger(value.acknowledgement_due_unix_seconds) ||
      !Number.isSafeInteger(value.resolution_due_unix_seconds) || !validOptionalUnix(value.acknowledged_unix_seconds) || !validOptionalUnix(value.closed_unix_seconds) ||
      !Number.isSafeInteger(value.resource_version) || value.resource_version < 1 || !Number.isSafeInteger(value.created_unix_seconds) || value.created_unix_seconds < 1 ||
      !Number.isSafeInteger(value.updated_unix_seconds) || value.updated_unix_seconds < 1) {
    throw new Error("Moderation report response is invalid");
  }
  return value;
}

function validateQueue(value) {
  if (!hasExactKeys(value, ["items", "next_cursor"]) || !Array.isArray(value.items) || value.items.length > 50 ||
      (value.next_cursor !== null && (typeof value.next_cursor !== "string" || !MODERATION_CURSOR.test(value.next_cursor)))) {
    throw new Error("Moderation queue response is invalid");
  }
  value.items.forEach(validateContentReport);
  return value;
}

function validateDeveloperNotice(value) {
  const keys = ["notice_id", "report_id", "release_id", "app_id", "version", "state", "reason_codes", "appeal_deadline_unix_seconds", "appeal_id", "appeal_state", "resource_version", "created_unix_seconds", "updated_unix_seconds"];
  if (!hasExactKeys(value, keys) || !NOTICE_ID.test(value.notice_id) || !REPORT_ID.test(value.report_id) ||
      !RELEASE_ID.test(value.release_id) || !APP_ID.test(value.app_id) || !isSemanticVersion(value.version) ||
      !NOTICE_STATES.has(value.state) || !Array.isArray(value.reason_codes) || value.reason_codes.length < 1 || value.reason_codes.length > 4 ||
      new Set(value.reason_codes).size !== value.reason_codes.length || value.reason_codes.some((reason) => !DECISION_REASONS.has(reason)) ||
      !Number.isSafeInteger(value.appeal_deadline_unix_seconds) || value.appeal_deadline_unix_seconds < 1 ||
      (value.appeal_id !== null && !APPEAL_ID.test(value.appeal_id)) ||
      (value.appeal_state !== null && !APPEAL_STATES.has(value.appeal_state)) ||
      (value.appeal_id === null) !== (value.appeal_state === null) ||
      !Number.isSafeInteger(value.resource_version) || value.resource_version < 1 ||
      !Number.isSafeInteger(value.created_unix_seconds) || value.created_unix_seconds < 1 ||
      !Number.isSafeInteger(value.updated_unix_seconds) || value.updated_unix_seconds < value.created_unix_seconds) {
    throw new Error("Developer notice response is invalid");
  }
  return value;
}

function validateModerationDecision(value) {
  if (!hasExactKeys(value, ["report", "notice"])) throw new Error("Moderation decision response is invalid");
  validateContentReport(value.report);
  if (value.notice !== null) validateDeveloperNotice(value.notice);
  const noticeExpected = value.report.disposition === "developer-notice";
  if (noticeExpected !== (value.notice !== null) || (value.notice && (
    value.notice.report_id !== value.report.report_id || value.notice.release_id !== value.report.release_id ||
    value.notice.app_id !== value.report.app_id || value.notice.version !== value.report.version ||
    value.notice.reason_codes.join("\0") !== value.report.decision_reason_codes.join("\0")
  ))) {
    throw new Error("Moderation decision response is invalid");
  }
  return value;
}

export class OperationsApi {
  constructor({ origin = "https://operations.cardputerzero.dev", workforceOrigin = origin, sessionClient, tokenProvider, fetchImpl = fetch } = {}) {
    this.origin = strictOrigin(origin);
    this.sessionClient = sessionClient ?? new OperationsWorkforceSessionClient({ origin: workforceOrigin, fetchImpl });
    this.tokenProvider = tokenProvider ?? ((scope) => this.sessionClient.controlToken(scope));
    this.fetchImpl = fetchImpl;
  }

  async request(path, { method = "GET", body, etag, query, scope = "store.editorial" } = {}) {
    if (!path.startsWith("/v1/") || path.includes("?") || path.includes("#")) throw new Error("Operations API path is outside v1");
    const target = new URL(path, this.origin);
    if (target.origin !== this.origin || target.pathname !== path) throw new Error("Operations API path is outside v1");
    if (query) target.search = query.toString();
    const token = await this.tokenProvider?.(scope);
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
    if (!response.ok) {
      const error = new Error(`Operations API returned ${response.status} (${decoded?.code ?? "unknown-error"})`);
      error.status = response.status;
      error.code = decoded?.code;
      throw error;
    }
    return { data: decoded, etag: response.headers.get("etag") };
  }

  getToday() {
    return this.request("/v1/editorial/today").then((response) => ({ ...response, data: validateEditorialLayout(response.data) }));
  }

  listPublishedReleases({ cursor, limit = 25 } = {}) {
    if (!Number.isInteger(limit) || limit < 1 || limit > 50) throw new Error("Editorial Release limit is outside 1-50");
    if (cursor !== undefined && (typeof cursor !== "string" || !EDITORIAL_RELEASE_CURSOR.test(cursor))) {
      throw new Error("Editorial Release cursor is invalid");
    }
    const query = new URLSearchParams({ limit: String(limit) });
    if (cursor) query.set("cursor", cursor);
    return this.request("/v1/editorial/releases", { query })
      .then((response) => ({ ...response, data: validateEditorialReleases(response.data) }));
  }

  saveToday(request, etag, releases = []) {
    if (Object.keys(validateEditorial(request, releases)).length) throw new Error("Today editorial request is invalid");
    return this.request("/v1/editorial/today", { method: etag ? "PUT" : "POST", body: request, etag })
      .then((response) => ({ ...response, data: validateEditorialLayout(response.data) }));
  }

  listReports({ cursor, limit = 25 } = {}) {
    if (!Number.isInteger(limit) || limit < 1 || limit > 50) throw new Error("Moderation queue limit is outside 1-50");
    if (cursor !== undefined && (typeof cursor !== "string" || !MODERATION_CURSOR.test(cursor))) throw new Error("Moderation queue cursor is invalid");
    const query = new URLSearchParams({ limit: String(limit) });
    if (cursor) query.set("cursor", cursor);
    return this.request("/v1/moderation/reports", { query, scope: "store.moderation" }).then((response) => ({ ...response, data: validateQueue(response.data) }));
  }

  decideReport(reportId, etag, request) {
    if (!/^report_[0-9a-f]{32}$/.test(reportId)) throw new Error("Report ID is invalid");
    if (!validEtag(etag)) throw new Error("Moderation decision requires a strong ETag");
    if (Object.keys(validateDecision(request)).length) throw new Error("Moderation decision is invalid");
    return this.request(`/v1/moderation/reports/${encodeURIComponent(reportId)}:decide`, { method: "POST", etag, body: request, scope: "store.moderation" })
      .then((response) => ({ ...response, data: validateModerationDecision(response.data) }));
  }
}

export { RELEASE_ID };
