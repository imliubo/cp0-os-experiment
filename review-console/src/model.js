export const REVIEW_STAGES = Object.freeze(["all", "primary", "secondary"]);
export const DECISIONS = Object.freeze(["approved", "needs-changes", "rejected"]);

export function formatState(value) {
  return value.replaceAll("-", " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

export function canClaimReview(item, reviewer) {
  if (!reviewer || item.assignee || !["ready-for-review", "pending-secondary-review"].includes(item.state)) return false;
  return item.stage !== "secondary" || item.primaryReviewer !== reviewer;
}

export function filterQueue(items, { stage = "all", query = "", reviewer = null } = {}) {
  const normalized = query.trim().toLowerCase();
  return items.filter((item) => {
    const stageMatch = stage === "all" || item.stage === stage;
    const queryMatch = !normalized || [item.name, item.appId, item.version, item.developer]
      .some((value) => value.toLowerCase().includes(normalized));
    const independentlyClaimable = !reviewer || item.assignee === reviewer || canClaimReview(item, reviewer);
    return stageMatch && queryMatch && independentlyClaimable && !["approved", "needs-changes", "rejected"].includes(item.state);
  });
}

export function validateDecision({ decision, reasonCodes, note }) {
  const errors = {};
  if (!DECISIONS.includes(decision)) errors.decision = "Select a valid decision.";
  const codes = Array.isArray(reasonCodes) ? reasonCodes.map((code) => typeof code === "string" ? code.trim() : "").filter(Boolean) : [];
  if (new Set(codes).size !== codes.length || codes.some((code) => !/^[a-z][a-z0-9-]{0,63}$/.test(code))) {
    errors.reasonCodes = "Reason codes must be unique lowercase identifiers.";
  }
  if (decision !== "approved" && codes.length === 0) errors.reasonCodes = "A reason code is required.";
  if (typeof note !== "string" || note.trim() !== note || note.length > 2000 || (decision !== "approved" && note.length === 0)) {
    errors.note = "Add a trimmed actionable note of at most 2,000 characters.";
  }
  return errors;
}

export function applyDecision(item, decision) {
  if (item.state !== "in-review" || !item.assignee) throw new Error("Review must be claimed first");
  if (decision === "approved" && item.stage === "primary") {
    return { ...item, state: "pending-secondary-review", stage: "secondary", primaryReviewer: item.assignee, assignee: null };
  }
  if (item.stage === "secondary" && item.primaryReviewer === item.assignee) throw new Error("Independent reviewer must differ from primary reviewer");
  return { ...item, state: decision, assignee: null };
}

function relativeTime(unixSeconds, nowSeconds) {
  const elapsed = Math.max(0, nowSeconds - unixSeconds);
  if (elapsed < 60) return "just now";
  if (elapsed < 3600) return `${Math.floor(elapsed / 60)} min ago`;
  if (elapsed < 86400) return `${Math.floor(elapsed / 3600)} hr ago`;
  return `${Math.floor(elapsed / 86400)} d ago`;
}

function mapRisk(risk) {
  return { policyVersion: risk.policy_version, tier: risk.tier, reasons: [...risk.reasons] };
}

export function mapQueueItem(value, { reviewerId, nowSeconds = Math.floor(Date.now() / 1000) } = {}) {
  const submission = value.submission;
  return {
    id: submission.submission_id,
    etag: `"${submission.resource_version}"`,
    appId: submission.app_id,
    name: value.app.name,
    version: submission.version,
    developer: value.app.developer_name,
    submitted: relativeTime(submission.created_unix_seconds, nowSeconds),
    stage: value.review_stage,
    state: submission.state,
    risk: mapRisk(value.risk),
    category: formatState(value.app.category),
    assignee: value.assigned_to_caller ? reviewerId : null,
    primaryReviewer: null,
    packageSha: submission.package_sha256,
    listingSha: submission.listing_sha256,
    assets: [...submission.assets],
    permissions: [],
    imports: [],
    findings: [],
    messages: [],
    audit: [],
    decisions: [],
    scannerVersion: null,
    scanSha: null,
    developerKeySha: null,
    messagesTruncated: false,
    auditTruncated: false,
    detailLoaded: false,
  };
}

export function mapReviewDetail(value, { etag, reviewerId, nowSeconds = Math.floor(Date.now() / 1000) } = {}) {
  const item = mapQueueItem({
    submission: value.submission,
    app: value.app,
    review_stage: value.review_stage,
    assigned_to_caller: value.assigned_to_caller,
    risk: value.risk,
  }, { reviewerId, nowSeconds });
  const active = value.assignments.find((assignment) => assignment.state === "active" && assignment.reviewer_id === reviewerId);
  const primary = [...value.assignments].reverse().find((assignment) => assignment.assignment_kind === "primary");
  return {
    ...item,
    etag: etag || item.etag,
    assignee: active?.reviewer_id ?? null,
    primaryReviewer: primary?.reviewer_email ?? null,
    permissions: [...value.scan.permissions],
    imports: [...value.scan.imports],
    findings: value.scan.findings.map((finding) => ({ ...finding })),
    scannerVersion: value.scan.scanner_version,
    scanSha: value.scan.report_sha256,
    developerKeySha: value.scan.developer_key_sha256,
    messages: value.messages.map((message) => ({
      id: message.message_id,
      actor: message.actor_label,
      role: message.actor_kind === "reviewer" ? "Reviewer" : "Developer",
      time: relativeTime(message.created_unix_seconds, nowSeconds),
      body: message.body,
    })),
    messagesTruncated: value.messages_truncated,
    audit: value.audit.map((event) => ({
      sequence: event.sequence,
      action: event.action,
      actorId: event.actor_id,
      beforeState: event.before_state,
      afterState: event.after_state,
      resourceVersion: event.resource_version,
      time: relativeTime(event.occurred_unix_seconds, nowSeconds),
    })),
    auditTruncated: value.audit_truncated,
    decisions: value.decisions.map((decision) => ({
      id: decision.decision_id,
      reviewer: decision.reviewer_email,
      decision: decision.decision,
      reasonCodes: [...decision.reason_codes],
      note: decision.note,
      time: relativeTime(decision.created_unix_seconds, nowSeconds),
    })),
    detailLoaded: true,
  };
}
