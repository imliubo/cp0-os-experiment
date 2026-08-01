export const RELEASE_ID = /^rel_[0-9a-f]{32}$/;

export const DECISION_REASONS = [
  "duplicate",
  "insufficient-evidence",
  "policy-violation",
  "security-review",
  "identity-confirmed",
  "remediation-accepted",
];

const DISPOSITIONS = new Set(["no-action", "developer-notice", "security-escalation"]);
const REASONS = new Set(DECISION_REASONS);

function boundedText(value, maximum) {
  return typeof value === "string" && value === value.trim() && value.length > 0 && [...value].length <= maximum;
}

export function formatCode(value) {
  return value.split("-").map((part) => part.charAt(0).toUpperCase() + part.slice(1)).join(" ");
}

export function validateEditorial(value, releases = []) {
  const errors = {};
  const releaseById = new Map(releases.map((release) => [release.releaseId, release]));
  if (!boundedText(value?.headline, 48)) errors.headline = "Headline must be 1-48 trimmed characters";
  if (!RELEASE_ID.test(value?.featured_release_id ?? "")) errors.featured_release_id = "Select one published Release";
  if (releases.length && releaseById.get(value?.featured_release_id)?.state !== "published") errors.featured_release_id = "Select one published Release";
  if (!Array.isArray(value?.collections) || value.collections.length < 1 || value.collections.length > 2) {
    errors.collections = "Today requires one or two collections";
    return errors;
  }
  const usedReleases = new Set([value.featured_release_id]);
  const usedApps = new Set();
  const featured = releaseById.get(value.featured_release_id);
  if (featured) usedApps.add(featured.appId);
  value.collections.forEach((collection, index) => {
    if (!boundedText(collection?.title, 32)) errors[`collection.${index}.title`] = "Title must be 1-32 trimmed characters";
    if (!Array.isArray(collection?.release_ids) || collection.release_ids.length < 1 || collection.release_ids.length > 4) {
      errors[`collection.${index}.release_ids`] = "Select one to four Releases";
      return;
    }
    for (const releaseId of collection.release_ids) {
      if (!RELEASE_ID.test(releaseId) || usedReleases.has(releaseId)) {
        errors[`collection.${index}.release_ids`] = "Releases cannot repeat";
        continue;
      }
      usedReleases.add(releaseId);
      const release = releaseById.get(releaseId);
      if (releases.length && release?.state !== "published") errors[`collection.${index}.release_ids`] = "Select published Releases only";
      if (release && usedApps.has(release.appId)) errors[`collection.${index}.release_ids`] = "Apps cannot repeat";
      if (release) usedApps.add(release.appId);
    }
  });
  return errors;
}

export function validateDecision(value) {
  const errors = {};
  if (!DISPOSITIONS.has(value?.disposition)) errors.disposition = "Select a disposition";
  if (!Array.isArray(value?.reason_codes) || value.reason_codes.length < 1 || value.reason_codes.length > 4 ||
      new Set(value.reason_codes).size !== value.reason_codes.length || value.reason_codes.some((reason) => !REASONS.has(reason))) {
    errors.reason_codes = "Select one to four distinct reason codes";
  }
  return errors;
}

export function filterReports(reports, { query = "", sla = "all" } = {}) {
  const normalized = query.trim().toLowerCase();
  return reports.filter((report) => report.state === "submitted")
    .filter((report) => sla === "all" || report.slaClass === sla)
    .filter((report) => !normalized || [report.appName, report.appId, report.version, report.reasonCode]
      .some((value) => value.toLowerCase().includes(normalized)))
    .sort((left, right) => left.acknowledgementDue - right.acknowledgementDue || left.reportId.localeCompare(right.reportId));
}

export function applyDecision(reports, reportId, request) {
  return reports.map((report) => report.reportId === reportId ? {
    ...report,
    state: request.disposition === "no-action" ? "closed-no-action" : request.disposition === "developer-notice" ? "notice-issued" : "security-escalated",
    disposition: request.disposition,
    decisionReasonCodes: [...request.reason_codes],
    resourceVersion: report.resourceVersion + 1,
  } : report);
}

const ACCENTS = ["green", "blue", "amber", "red", "cyan", "violet"];

function accentFor(appId) {
  let hash = 0;
  for (const character of appId) hash = (hash * 31 + character.codePointAt(0)) >>> 0;
  return ACCENTS[hash % ACCENTS.length];
}

function relativeTime(unixSeconds, nowSeconds) {
  const elapsed = Math.max(0, nowSeconds - unixSeconds);
  if (elapsed < 60) return "just now";
  if (elapsed < 3600) return `${Math.floor(elapsed / 60)} min ago`;
  if (elapsed < 86400) return `${Math.floor(elapsed / 3600)} hr ago`;
  return `${Math.floor(elapsed / 86400)} d ago`;
}

export function mapPublishedRelease(value) {
  return {
    releaseId: value.release_id,
    appId: value.app_id,
    name: value.name,
    version: value.version,
    category: value.category ? formatCode(value.category) : "Uncategorized",
    catalogSequence: value.catalog_sequence,
    accent: accentFor(value.app_id),
    state: "published",
  };
}

export function mapEditorial(value) {
  return {
    layout_id: value.layout_id,
    headline: value.headline,
    featured_release_id: value.featured.release_id,
    collections: value.collections.map((collection) => ({
      title: collection.title,
      release_ids: collection.items.map((item) => item.release_id),
    })),
    resource_version: value.resource_version,
    updated_unix_seconds: value.updated_unix_seconds,
  };
}

export function emptyEditorial() {
  return {
    layout_id: "today",
    headline: "",
    featured_release_id: "",
    collections: [{ title: "", release_ids: [] }],
    resource_version: null,
    updated_unix_seconds: null,
  };
}

export function mapReport(value, releases = [], nowSeconds = Math.floor(Date.now() / 1000)) {
  const release = releases.find((candidate) => candidate.releaseId === value.release_id || candidate.appId === value.app_id);
  return {
    reportId: value.report_id,
    releaseId: value.release_id,
    appId: value.app_id,
    appName: release?.name ?? value.app_id,
    version: value.version,
    reasonCode: value.reason_code,
    slaClass: value.sla_class,
    state: value.state,
    disposition: value.disposition,
    decisionReasonCodes: [...value.decision_reason_codes],
    acknowledgementDue: value.acknowledgement_due_unix_seconds,
    resolutionDue: value.resolution_due_unix_seconds,
    resourceVersion: value.resource_version,
    received: relativeTime(value.created_unix_seconds, nowSeconds),
  };
}
