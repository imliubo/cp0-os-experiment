import { validateDecision } from "./model.js";
import { ReviewWorkforceSessionClient } from "./workforce-session.js";

const MAX_RESPONSE_BYTES = 64 * 1024;
const SUBMISSION_ID = /^sub_[0-9a-f]{32}$/;
const APP_ID = /^[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*){2,}$/;
const VERSION = /^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;
const SHA256 = /^[0-9a-f]{64}$/;
const REVIEW_CURSOR = /^[0-9a-f]{16}\.sub_[0-9a-f]{32}$/;
const ASSIGNMENT_ID = /^assignment_[0-9a-f]{32}$/;
const REVIEWER_ID = /^reviewer_[0-9a-f]{32}$/;
const MEMBER_ID = /^member_[0-9a-f]{32}$/;
const DECISION_ID = /^decision_[0-9a-f]{32}$/;
const MESSAGE_ID = /^msg_[0-9a-f]{32}$/;
const RISK_TIERS = new Set(["standard", "elevated", "high"]);
const REVIEW_STAGES = new Set(["primary", "secondary"]);
const SUBMISSION_STATES = new Set(["draft", "uploading", "processing", "ready-for-review", "in-review", "pending-secondary-review", "needs-changes", "approved", "rejected", "withdrawn"]);
const STORE_CATEGORIES = new Set(["developer-tools", "education", "entertainment", "games", "hardware", "media", "productivity", "utilities"]);
const REVIEWER_ROLES = new Set(["reviewer", "senior-reviewer", "admin"]);
const ASSIGNMENT_STATES = new Set(["active", "completed", "cancelled"]);
const REVIEW_DECISIONS = new Set(["needs-changes", "approved", "rejected"]);
const REASON_CODE = /^[a-z][a-z0-9-]{0,63}$/;
const AUDIT_ACTION = /^[a-z][a-z0-9.-]{0,127}$/;
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

function hasExactKeys(value, expected) {
  return value && typeof value === "object" && !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...expected].sort().join("\0");
}

function validText(value, maximum) {
  return typeof value === "string" && value === value.trim() && value.length > 0 && [...value].length <= maximum && !/[\u0000-\u001f\u007f]/.test(value);
}

function validReviewText(value, allowEmpty = false) {
  return typeof value === "string" && value === value.trim() && [...value].length <= 2000 &&
    (allowEmpty || value.length > 0) && !/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(value);
}

function validUnix(value) {
  return Number.isSafeInteger(value) && value >= 1;
}

function validEmail(value) {
  return validText(value, 254) && /^[^@\s]+@[^@\s]+$/.test(value);
}

function isSemanticVersion(value) {
  if (typeof value !== "string" || value.length > 64) return false;
  const match = VERSION.exec(value);
  return Boolean(match) && (!match[1] || match[1].split(".")
    .every((identifier) => !/^\d+$/.test(identifier) || identifier === "0" || !identifier.startsWith("0")));
}

function validateSubmission(value) {
  const keys = ["submission_id", "app_id", "version", "revision", "state", "package_sha256", "listing_sha256", "assets", "resource_version", "created_unix_seconds"];
  if (!hasExactKeys(value, keys) || !SUBMISSION_ID.test(value.submission_id) || !APP_ID.test(value.app_id) ||
      !isSemanticVersion(value.version) || !Number.isSafeInteger(value.revision) || value.revision < 1 ||
      !SUBMISSION_STATES.has(value.state) || !SHA256.test(value.package_sha256) || !SHA256.test(value.listing_sha256) ||
      !Array.isArray(value.assets) || value.assets.length < 2 || value.assets.length > 6 ||
      !Number.isSafeInteger(value.resource_version) || value.resource_version < 1 || !validUnix(value.created_unix_seconds)) {
    throw new Error("Review submission response is invalid");
  }
  for (const asset of value.assets) {
    if (!hasExactKeys(asset, ["path", "sha256", "bytes", "width", "height"]) || !validText(asset.path, 128) ||
        !SHA256.test(asset.sha256) || !Number.isSafeInteger(asset.bytes) || asset.bytes < 1 || asset.bytes > 524288 ||
        !Number.isSafeInteger(asset.width) || asset.width < 1 || asset.width > 320 ||
        !Number.isSafeInteger(asset.height) || asset.height < 1 || asset.height > 170) {
      throw new Error("Review submission asset response is invalid");
    }
  }
  return value;
}

function validateApp(value) {
  if (!hasExactKeys(value, ["name", "developer_name", "category"]) || !validText(value.name, 32) ||
      !validText(value.developer_name, 80) || !STORE_CATEGORIES.has(value.category)) {
    throw new Error("Review app response is invalid");
  }
  return value;
}

function validateRisk(risk) {
  if (!hasExactKeys(risk, ["policy_version", "tier", "reasons"]) || !Number.isInteger(risk.policy_version) ||
      risk.policy_version < 1 || risk.policy_version > 32767 || !RISK_TIERS.has(risk.tier) ||
      !Array.isArray(risk.reasons) || risk.reasons.length > 7) {
    throw new Error("Review queue risk assessment is invalid");
  }
  const reasons = [...risk.reasons];
  if (reasons.some((reason) => !RISK_REASONS.has(reason)) || new Set(reasons).size !== reasons.length || reasons.join("\0") !== reasons.sort().join("\0")) {
    throw new Error("Review queue risk reasons are invalid");
  }
  return risk;
}

function validateQueuePage(value) {
  if (!hasExactKeys(value, ["items", "next_cursor"]) || !Array.isArray(value.items) || value.items.length > 50) {
    throw new Error("Review queue response is invalid");
  }
  if (value.next_cursor !== null && (typeof value.next_cursor !== "string" || !REVIEW_CURSOR.test(value.next_cursor))) {
    throw new Error("Review queue cursor is invalid");
  }
  for (const item of value.items) {
    if (!hasExactKeys(item, ["submission", "app", "review_stage", "assigned_to_caller", "risk"]) ||
        !REVIEW_STAGES.has(item.review_stage) || typeof item.assigned_to_caller !== "boolean") throw new Error("Review queue item response is invalid");
    validateSubmission(item.submission);
    validateApp(item.app);
    validateRisk(item.risk);
  }
  return value;
}

function validateDetail(value) {
  const keys = ["submission", "app", "review_stage", "assigned_to_caller", "risk", "scan", "assignments", "decisions", "messages", "messages_truncated", "audit", "audit_truncated"];
  if (!hasExactKeys(value, keys) || !REVIEW_STAGES.has(value.review_stage) || typeof value.assigned_to_caller !== "boolean" ||
      !Array.isArray(value.assignments) || value.assignments.length > 8 || !Array.isArray(value.decisions) || value.decisions.length > 8 ||
      !Array.isArray(value.messages) || value.messages.length > 6 || typeof value.messages_truncated !== "boolean" ||
      !Array.isArray(value.audit) || value.audit.length > 32 || typeof value.audit_truncated !== "boolean") {
    throw new Error("Review detail response is invalid");
  }
  validateSubmission(value.submission);
  validateApp(value.app);
  validateRisk(value.risk);
  const scan = value.scan;
  if (!hasExactKeys(scan, ["scan_id", "scanner_version", "report_sha256", "developer_key_sha256", "imports", "permissions", "findings"]) ||
      !/^scan_[0-9a-f]{32}$/.test(scan.scan_id) || !validText(scan.scanner_version, 64) || !SHA256.test(scan.report_sha256) ||
      (scan.developer_key_sha256 !== null && !SHA256.test(scan.developer_key_sha256)) || !Array.isArray(scan.imports) || scan.imports.length > 128 ||
      !Array.isArray(scan.permissions) || scan.permissions.length > 32 || !Array.isArray(scan.findings) || scan.findings.length > 64 ||
      scan.imports.some((item) => !validText(item, 128)) || scan.permissions.some((item) => !validText(item, 64)) ||
      scan.findings.some((finding) => !hasExactKeys(finding, ["code", "severity"]) || !validText(finding.code, 128) || !["error", "security"].includes(finding.severity))) {
    throw new Error("Review scan response is invalid");
  }
  if (value.assignments.some((item) => {
    if (!hasExactKeys(item, ["assignment_id", "reviewer_id", "reviewer_email", "reviewer_role", "assignment_kind", "state", "created_unix_seconds", "completed_unix_seconds"]) ||
        !ASSIGNMENT_ID.test(item.assignment_id) || !REVIEWER_ID.test(item.reviewer_id) || !validEmail(item.reviewer_email) ||
        !REVIEWER_ROLES.has(item.reviewer_role) || !REVIEW_STAGES.has(item.assignment_kind) || !ASSIGNMENT_STATES.has(item.state) ||
        !validUnix(item.created_unix_seconds)) return true;
    return item.state === "active"
      ? item.completed_unix_seconds !== null
      : !validUnix(item.completed_unix_seconds) || item.completed_unix_seconds < item.created_unix_seconds;
  })) {
    throw new Error("Review assignment response is invalid");
  }
  if (value.decisions.some((item) => !hasExactKeys(item, ["decision_id", "reviewer_id", "reviewer_email", "decision", "reason_codes", "note", "created_unix_seconds", "assignment_id"]) ||
      !DECISION_ID.test(item.decision_id) || !REVIEWER_ID.test(item.reviewer_id) || !validEmail(item.reviewer_email) ||
      !REVIEW_DECISIONS.has(item.decision) || !Array.isArray(item.reason_codes) || item.reason_codes.length > 16 ||
      new Set(item.reason_codes).size !== item.reason_codes.length || item.reason_codes.some((reason) => !REASON_CODE.test(reason)) ||
      (item.decision !== "approved" && item.reason_codes.length === 0) || !validReviewText(item.note, item.decision === "approved") ||
      !validUnix(item.created_unix_seconds) || !ASSIGNMENT_ID.test(item.assignment_id))) {
    throw new Error("Review decision response is invalid");
  }
  if (value.messages.some((item) => !hasExactKeys(item, ["message_id", "actor_id", "actor_kind", "actor_label", "body", "created_unix_seconds"]) ||
      !MESSAGE_ID.test(item.message_id) || !["developer", "reviewer"].includes(item.actor_kind) ||
      (item.actor_kind === "reviewer" ? !REVIEWER_ID.test(item.actor_id) : !MEMBER_ID.test(item.actor_id)) ||
      !validText(item.actor_label, 254) || !validReviewText(item.body) || !validUnix(item.created_unix_seconds))) {
    throw new Error("Review message response is invalid");
  }
  if (value.audit.some((item) => !hasExactKeys(item, ["sequence", "occurred_unix_seconds", "actor_id", "action", "before_state", "after_state", "resource_version"]) ||
      !validUnix(item.sequence) || !validUnix(item.occurred_unix_seconds) || !validText(item.actor_id, 128) ||
      typeof item.action !== "string" || !AUDIT_ACTION.test(item.action) ||
      (item.before_state !== null && !SUBMISSION_STATES.has(item.before_state)) ||
      (item.after_state !== null && !SUBMISSION_STATES.has(item.after_state)) ||
      !validUnix(item.resource_version))) {
    throw new Error("Review audit response is invalid");
  }
  return value;
}

export class ReviewApi {
  constructor({ origin = "https://review.cardputerzero.dev", workforceOrigin = origin, sessionClient, tokenProvider, fetchImpl = fetch } = {}) {
    this.origin = strictOrigin(origin);
    this.sessionClient = sessionClient ?? new ReviewWorkforceSessionClient({ origin: workforceOrigin, fetchImpl });
    this.tokenProvider = tokenProvider ?? (() => this.sessionClient.controlToken());
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
    if (!response.ok) {
      const error = new Error(`Review API returned ${response.status} (${decoded?.code ?? "unknown-error"})`);
      error.status = response.status;
      error.code = decoded?.code;
      throw error;
    }
    return { data: decoded, etag: response.headers.get("etag") };
  }

  listQueue({ cursor, limit = 25 } = {}) {
    if (!Number.isInteger(limit) || limit < 1 || limit > 50) throw new Error("Review queue limit is outside 1-50");
    if (cursor !== undefined && (typeof cursor !== "string" || !REVIEW_CURSOR.test(cursor))) throw new Error("Review queue cursor is invalid");
    const query = new URLSearchParams({ limit: String(limit) });
    if (cursor) query.set("cursor", cursor);
    return this.request("/v1/review/submissions", { query }).then((response) => ({
      ...response,
      data: validateQueuePage(response.data),
    }));
  }

  getSubmissionDetail(submissionId) {
    if (!SUBMISSION_ID.test(submissionId)) throw new Error("Submission ID is invalid");
    return this.request(`/v1/review/submissions/${encodeURIComponent(submissionId)}`).then((response) => ({
      ...response,
      data: validateDetail(response.data),
    }));
  }

  beginReview(submissionId, etag) {
    if (!etag) throw new Error("Review claim requires an ETag");
    return this.request(`/v1/review/submissions/${encodeURIComponent(submissionId)}:begin`, { method: "POST", etag });
  }

  decideReview(submissionId, etag, request) {
    if (!etag) throw new Error("Review decision requires an ETag");
    if (Object.keys(validateDecision(request)).length) throw new Error("Review decision is invalid");
    return this.request(`/v1/review/submissions/${encodeURIComponent(submissionId)}/decisions`, {
      method: "POST",
      etag,
      body: { decision: request.decision, reason_codes: request.reasonCodes, note: request.note },
    });
  }

  postMessage(submissionId, body) {
    if (!validReviewText(body)) throw new Error("Review message is invalid");
    return this.request(`/v1/submissions/${encodeURIComponent(submissionId)}/messages`, { method: "POST", body: { body } });
  }
}
