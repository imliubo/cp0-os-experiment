import assert from "node:assert/strict";
import test from "node:test";

import {
  applyDecision,
  createOperationsData,
  filterReports,
  validateDecision,
  validateEditorial,
} from "../src/model.js";

test("editorial fixture satisfies the bounded Today contract", () => {
  const data = createOperationsData();
  assert.deepEqual(validateEditorial(data.editorial, data.releases), {});
});

test("editorial validation rejects duplicate Releases and Apps", () => {
  const data = createOperationsData();
  const duplicateRelease = structuredClone(data.editorial);
  duplicateRelease.collections[0].release_ids[0] = duplicateRelease.featured_release_id;
  assert.match(validateEditorial(duplicateRelease, data.releases)["collection.0.release_ids"], /repeat/);

  const duplicateApp = structuredClone(data.editorial);
  duplicateApp.collections[0].release_ids[1] = duplicateApp.collections[0].release_ids[0];
  assert.match(validateEditorial(duplicateApp, data.releases)["collection.0.release_ids"], /repeat/);

  const unpublished = structuredClone(data.editorial);
  unpublished.featured_release_id = `rel_${"f".repeat(32)}`;
  assert.match(validateEditorial(unpublished, data.releases).featured_release_id, /published/);
});

test("moderation queue stays SLA ordered and searchable", () => {
  const data = createOperationsData();
  assert.deepEqual(filterReports(data.reports).map((item) => item.appName), ["Signal Lab", "Neon Snake", "Weather Deck"]);
  assert.deepEqual(filterReports(data.reports, { sla: "security" }).map((item) => item.appName), ["Signal Lab"]);
  assert.deepEqual(filterReports(data.reports, { query: "fraud" }).map((item) => item.appName), ["Weather Deck"]);
});

test("structured decisions are bounded and close one report", () => {
  const data = createOperationsData();
  const request = { disposition: "developer-notice", reason_codes: ["policy-violation"] };
  assert.deepEqual(validateDecision(request), {});
  const updated = applyDecision(data.reports, data.reports[0].reportId, request);
  assert.equal(updated[0].state, "notice-issued");
  assert.equal(updated[0].resourceVersion, 2);
  assert.equal(filterReports(updated).length, 2);
  assert.ok(validateDecision({ disposition: "invalid", reason_codes: [] }).disposition);
});
