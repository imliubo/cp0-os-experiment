import assert from "node:assert/strict";
import test from "node:test";

import { applyDecision, filterReports, mapEditorial, mapPublishedRelease, mapReport, validateDecision, validateEditorial } from "../src/model.js";

const releaseWire = (suffix, appId, name, category = "utilities") => ({
  release_id: `rel_${suffix.repeat(32)}`,
  app_id: appId,
  name,
  version: "1.0.0",
  category,
  catalog_sequence: Number(suffix),
});

const releases = [
  mapPublishedRelease(releaseWire("1", "dev.cardputerzero.notes", "Field Notes", "productivity")),
  mapPublishedRelease(releaseWire("2", "dev.cardputerzero.signal", "Signal Lab")),
  mapPublishedRelease(releaseWire("3", "dev.cardputerzero.calc", "Pocket Calc")),
];

test("maps the authoritative Today contract into the editor model", () => {
  const editorial = mapEditorial({
    layout_id: "today",
    headline: "Small tools for the field",
    featured: { release_id: releases[0].releaseId, app_id: releases[0].appId },
    collections: [{ title: "Offline", items: [
      { release_id: releases[1].releaseId, app_id: releases[1].appId },
      { release_id: releases[2].releaseId, app_id: releases[2].appId },
    ] }],
    resource_version: 7,
    updated_unix_seconds: 1_000,
  });
  assert.deepEqual(validateEditorial(editorial, releases), {});
  assert.equal(editorial.featured_release_id, releases[0].releaseId);
  assert.deepEqual(editorial.collections[0].release_ids, [releases[1].releaseId, releases[2].releaseId]);
});

test("editorial validation rejects duplicate Releases and Apps", () => {
  const editorial = {
    headline: "Small tools for the field",
    featured_release_id: releases[0].releaseId,
    collections: [{ title: "Offline", release_ids: [releases[1].releaseId, releases[2].releaseId] }],
  };
  const duplicateRelease = structuredClone(editorial);
  duplicateRelease.collections[0].release_ids[0] = duplicateRelease.featured_release_id;
  assert.match(validateEditorial(duplicateRelease, releases)["collection.0.release_ids"], /repeat/);

  const duplicateAppReleases = [...releases, { ...releases[2], releaseId: `rel_${"4".repeat(32)}`, appId: releases[1].appId }];
  const duplicateApp = structuredClone(editorial);
  duplicateApp.collections[0].release_ids[1] = duplicateAppReleases[3].releaseId;
  assert.match(validateEditorial(duplicateApp, duplicateAppReleases)["collection.0.release_ids"], /Apps/);
});

test("maps, orders and searches moderation reports", () => {
  const report = (suffix, release, reason, sla, due) => mapReport({
    report_id: `report_${suffix.repeat(32)}`,
    release_id: release.releaseId,
    app_id: release.appId,
    version: release.version,
    reason_code: reason,
    sla_class: sla,
    state: "submitted",
    disposition: null,
    decision_reason_codes: [],
    acknowledgement_due_unix_seconds: due,
    resolution_due_unix_seconds: due + 100,
    acknowledged_unix_seconds: null,
    closed_unix_seconds: null,
    resource_version: 1,
    created_unix_seconds: 900,
    updated_unix_seconds: 900,
  }, releases, 1_000);
  const reports = [
    report("a", releases[1], "privacy", "security", 1_200),
    report("b", releases[2], "fraud", "standard", 1_100),
  ];
  assert.deepEqual(filterReports(reports).map((item) => item.appName), ["Pocket Calc", "Signal Lab"]);
  assert.deepEqual(filterReports(reports, { sla: "security" }).map((item) => item.appName), ["Signal Lab"]);
  assert.deepEqual(filterReports(reports, { query: "fraud" }).map((item) => item.appName), ["Pocket Calc"]);
});

test("structured decisions are bounded and close one report", () => {
  const reports = [{ reportId: `report_${"a".repeat(32)}`, state: "submitted", resourceVersion: 1 }];
  const request = { disposition: "developer-notice", reason_codes: ["policy-violation"] };
  assert.deepEqual(validateDecision(request), {});
  const updated = applyDecision(reports, reports[0].reportId, request);
  assert.equal(updated[0].state, "notice-issued");
  assert.equal(updated[0].resourceVersion, 2);
  assert.ok(validateDecision({ disposition: "invalid", reason_codes: [] }).disposition);
});
