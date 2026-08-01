import assert from "node:assert/strict";
import test from "node:test";

import { OperationsApi } from "../src/api.js";
import { mapPublishedRelease } from "../src/model.js";

const token = "o".repeat(48);

function jsonResponse(body, { status = 200, headers = {} } = {}) {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json", ...headers } });
}

const releaseWire = (suffix, appId, name, sequence) => ({
  release_id: `rel_${suffix.repeat(32)}`,
  app_id: appId,
  name,
  version: "1.0.0",
  category: "utilities",
  catalog_sequence: sequence,
});
const releaseWires = [
  releaseWire("1", "dev.cardputerzero.notes", "Field Notes", 41),
  releaseWire("2", "dev.cardputerzero.signal", "Signal Lab", 42),
  releaseWire("3", "dev.cardputerzero.calc", "Pocket Calc", 43),
];
const releases = releaseWires.map(mapPublishedRelease);
const editorial = {
  layout_id: "today",
  headline: "Small tools for the field",
  featured: { release_id: releaseWires[0].release_id, app_id: releaseWires[0].app_id },
  collections: [{ title: "Offline", items: [
    { release_id: releaseWires[1].release_id, app_id: releaseWires[1].app_id },
    { release_id: releaseWires[2].release_id, app_id: releaseWires[2].app_id },
  ] }],
  resource_version: 8,
  updated_unix_seconds: 1_000,
};

function report() {
  return {
    report_id: `report_${"a".repeat(32)}`,
    release_id: releaseWires[0].release_id,
    app_id: releaseWires[0].app_id,
    version: "1.0.0",
    reason_code: "privacy",
    sla_class: "security",
    state: "submitted",
    disposition: null,
    decision_reason_codes: [],
    acknowledgement_due_unix_seconds: 1_100,
    resolution_due_unix_seconds: 1_200,
    acknowledged_unix_seconds: null,
    closed_unix_seconds: null,
    resource_version: 1,
    created_unix_seconds: 1_000,
    updated_unix_seconds: 1_000,
  };
}

test("client rejects unsafe origins and missing authorization", async () => {
  assert.throws(() => new OperationsApi({ origin: "http://operations.example" }), /bare HTTPS/);
  const api = new OperationsApi({ tokenProvider: async () => undefined, fetchImpl: async () => jsonResponse({}) });
  await assert.rejects(() => api.getToday(), /authorization/);
});

test("Today update omits cookies and binds ETag and idempotency", async () => {
  let captured;
  const api = new OperationsApi({
    tokenProvider: async () => token,
    fetchImpl: async (url, init) => {
      captured = { url, init };
      return jsonResponse(editorial, { headers: { etag: '"8"' } });
    },
  });
  const request = {
    headline: editorial.headline,
    featured_release_id: editorial.featured.release_id,
    collections: editorial.collections.map((collection) => ({ title: collection.title, release_ids: collection.items.map((item) => item.release_id) })),
  };
  const response = await api.saveToday(request, '"7"', releases);
  assert.equal(captured.url, "https://operations.cardputerzero.dev/v1/editorial/today");
  assert.equal(captured.init.method, "PUT");
  assert.equal(captured.init.credentials, "omit");
  assert.equal(captured.init.headers.Authorization, `Bearer ${token}`);
  assert.equal(captured.init.headers["If-Match"], '"7"');
  assert.match(captured.init.headers["Idempotency-Key"], /^operations-/);
  assert.equal(response.etag, '"8"');
});

test("published Release discovery validates canonical current projections", async () => {
  const items = releaseWires.slice(0, 2);
  const next_cursor = `000000000000002a.${items[1].release_id}`;
  let captured;
  const api = new OperationsApi({
    tokenProvider: async () => token,
    fetchImpl: async (url, init) => {
      captured = { url, init };
      return jsonResponse({ items, next_cursor });
    },
  });
  const response = await api.listPublishedReleases({ limit: 2 });
  assert.equal(captured.url, "https://operations.cardputerzero.dev/v1/editorial/releases?limit=2");
  assert.equal(captured.init.credentials, "omit");
  assert.deepEqual(response.data, { items, next_cursor });

  const duplicate = structuredClone(items);
  duplicate[1].app_id = duplicate[0].app_id;
  const invalid = new OperationsApi({
    tokenProvider: async () => token,
    fetchImpl: async () => jsonResponse({ items: duplicate, next_cursor }),
  });
  await assert.rejects(() => invalid.listPublishedReleases({ limit: 2 }), /response is invalid/);
  const invalidVersion = structuredClone(items);
  invalidVersion[1].version = "1.0.0-01";
  const invalidVersionApi = new OperationsApi({
    tokenProvider: async () => token,
    fetchImpl: async () => jsonResponse({ items: invalidVersion, next_cursor }),
  });
  await assert.rejects(() => invalidVersionApi.listPublishedReleases({ limit: 2 }), /response is invalid/);
  assert.throws(
    () => api.listPublishedReleases({ cursor: `000000000000002a.release_${"f".repeat(32)}` }),
    /cursor is invalid/,
  );
});

test("moderation request uses a bounded query and exact mutation path", async () => {
  const calls = [];
  const api = new OperationsApi({
    tokenProvider: async () => token,
    fetchImpl: async (url, init) => {
      calls.push({ url, init });
      return url.includes(":decide")
        ? jsonResponse({ report: { ...report(), state: "closed-no-action", disposition: "no-action", decision_reason_codes: ["duplicate"], acknowledged_unix_seconds: 1_050, closed_unix_seconds: 1_050, resource_version: 2, updated_unix_seconds: 1_050 }, notice: null }, { headers: { etag: '"2"' } })
        : jsonResponse({ items: [], next_cursor: null });
    },
  });
  const moderationCursor = `1100:report_${"a".repeat(32)}`;
  await api.listReports({ cursor: moderationCursor, limit: 50 });
  const reportId = `report_${"a".repeat(32)}`;
  await api.decideReport(reportId, '"1"', { disposition: "no-action", reason_codes: ["duplicate"] });
  assert.equal(calls[0].url, `https://operations.cardputerzero.dev/v1/moderation/reports?limit=50&cursor=1100%3Areport_${"a".repeat(32)}`);
  assert.equal(calls[1].url, `https://operations.cardputerzero.dev/v1/moderation/reports/${reportId}:decide`);
  assert.equal(calls[1].init.headers["If-Match"], '"1"');
  assert.throws(() => api.listReports({ cursor: "next" }), /cursor is invalid/);
});

test("moderation decision strictly binds a developer notice to its report", async () => {
  const decided = {
    ...report(),
    state: "notice-issued",
    disposition: "developer-notice",
    decision_reason_codes: ["policy-violation"],
    acknowledged_unix_seconds: 1_050,
    resource_version: 2,
    updated_unix_seconds: 1_050,
  };
  const notice = {
    notice_id: `notice_${"b".repeat(32)}`,
    report_id: decided.report_id,
    release_id: decided.release_id,
    app_id: decided.app_id,
    version: decided.version,
    state: "open",
    reason_codes: ["policy-violation"],
    appeal_deadline_unix_seconds: 2_000,
    appeal_id: null,
    appeal_state: null,
    resource_version: 1,
    created_unix_seconds: 1_050,
    updated_unix_seconds: 1_050,
  };
  const api = new OperationsApi({
    tokenProvider: async () => token,
    fetchImpl: async () => jsonResponse({ report: decided, notice }),
  });
  assert.equal((await api.decideReport(decided.report_id, '"1"', { disposition: "developer-notice", reason_codes: ["policy-violation"] })).data.notice.notice_id, notice.notice_id);

  const invalid = new OperationsApi({
    tokenProvider: async () => token,
    fetchImpl: async () => jsonResponse({ report: decided, notice: { ...notice, app_id: "dev.cardputerzero.other" } }),
  });
  await assert.rejects(() => invalid.decideReport(decided.report_id, '"1"', { disposition: "developer-notice", reason_codes: ["policy-violation"] }), /decision response is invalid/);
});

test("client rejects oversized responses before parsing", async () => {
  const api = new OperationsApi({
    tokenProvider: async () => token,
    fetchImpl: async () => new Response("{}", { headers: { "content-length": String(65537) } }),
  });
  await assert.rejects(() => api.getToday(), /too large/);
});
