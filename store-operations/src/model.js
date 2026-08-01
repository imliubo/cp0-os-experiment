export const RELEASE_ID = /^release_[0-9a-f]{32}$/;

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

const release = (suffix, appId, name, version, category, accent) => ({
  releaseId: `release_${suffix.repeat(32)}`,
  appId,
  name,
  version,
  category,
  accent,
  state: "published",
});

export function createOperationsData() {
  const releases = [
    release("1", "dev.cardputerzero.field-notes", "Field Notes", "2.3.0", "Productivity", "green"),
    release("2", "dev.cardputerzero.signal-lab", "Signal Lab", "1.8.2", "Utilities", "blue"),
    release("3", "dev.cardputerzero.pocket-calc", "Pocket Calc", "1.4.1", "Utilities", "amber"),
    release("4", "dev.cardputerzero.neon-snake", "Neon Snake", "1.2.0", "Games", "red"),
    release("5", "dev.cardputerzero.weather-deck", "Weather Deck", "3.0.1", "Lifestyle", "cyan"),
    release("6", "dev.cardputerzero.hex-reader", "Hex Reader", "0.9.4", "Developer Tools", "violet"),
  ];
  return {
    releases,
    editorial: {
      layout_id: "today",
      headline: "Small tools, ready for the field",
      featured_release_id: releases[0].releaseId,
      collections: [
        { title: "Work offline", release_ids: [releases[1].releaseId, releases[2].releaseId] },
        { title: "New this week", release_ids: [releases[3].releaseId, releases[4].releaseId] },
      ],
      resource_version: 7,
      updated_unix_seconds: 1785600000,
    },
    reports: [
      {
        reportId: `report_${"a".repeat(32)}`,
        releaseId: releases[1].releaseId,
        appId: releases[1].appId,
        appName: releases[1].name,
        version: releases[1].version,
        reasonCode: "privacy",
        slaClass: "security",
        state: "submitted",
        disposition: null,
        decisionReasonCodes: [],
        acknowledgementDue: 1785607200,
        resolutionDue: 1785852000,
        resourceVersion: 1,
        received: "18 min ago",
      },
      {
        reportId: `report_${"b".repeat(32)}`,
        releaseId: releases[3].releaseId,
        appId: releases[3].appId,
        appName: releases[3].name,
        version: releases[3].version,
        reasonCode: "harmful-content",
        slaClass: "standard",
        state: "submitted",
        disposition: null,
        decisionReasonCodes: [],
        acknowledgementDue: 1785848400,
        resolutionDue: 1786813200,
        resourceVersion: 1,
        received: "2 hr ago",
      },
      {
        reportId: `report_${"c".repeat(32)}`,
        releaseId: releases[4].releaseId,
        appId: releases[4].appId,
        appName: releases[4].name,
        version: releases[4].version,
        reasonCode: "fraud",
        slaClass: "standard",
        state: "submitted",
        disposition: null,
        decisionReasonCodes: [],
        acknowledgementDue: 1785852000,
        resolutionDue: 1786816800,
        resourceVersion: 3,
        received: "4 hr ago",
      },
    ],
  };
}
