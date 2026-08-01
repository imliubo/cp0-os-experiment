import assert from "node:assert/strict";
import test from "node:test";

import { OperationsApi } from "../src/api.js";
import { createOperationsData } from "../src/model.js";

const token = "o".repeat(48);

function jsonResponse(body, { status = 200, headers = {} } = {}) {
  return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json", ...headers } });
}

test("client rejects unsafe origins and missing authorization", async () => {
  assert.throws(() => new OperationsApi({ origin: "http://operations.example" }), /bare HTTPS/);
  const api = new OperationsApi({ fetchImpl: async () => jsonResponse({}) });
  await assert.rejects(() => api.getToday(), /authorization/);
});

test("Today update omits cookies and binds ETag and idempotency", async () => {
  const data = createOperationsData();
  let captured;
  const api = new OperationsApi({
    tokenProvider: async () => token,
    fetchImpl: async (url, init) => {
      captured = { url, init };
      return jsonResponse(data.editorial, { headers: { etag: '"8"' } });
    },
  });
  const request = {
    headline: data.editorial.headline,
    featured_release_id: data.editorial.featured_release_id,
    collections: data.editorial.collections,
  };
  const response = await api.saveToday(request, '"7"', data.releases);
  assert.equal(captured.url, "https://operations.cardputerzero.dev/v1/editorial/today");
  assert.equal(captured.init.method, "PUT");
  assert.equal(captured.init.credentials, "omit");
  assert.equal(captured.init.headers.Authorization, `Bearer ${token}`);
  assert.equal(captured.init.headers["If-Match"], '"7"');
  assert.match(captured.init.headers["Idempotency-Key"], /^operations-/);
  assert.equal(response.etag, '"8"');
});

test("published Release discovery validates canonical current projections", async () => {
  const data = createOperationsData();
  const items = data.releases.slice(0, 2).map((release, index) => ({
    release_id: release.releaseId,
    app_id: release.appId,
    name: release.name,
    version: release.version,
    category: release.category.toLowerCase().replaceAll(" ", "-"),
    catalog_sequence: index + 41,
  }));
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
        ? jsonResponse({ report: {}, notice: null }, { headers: { etag: '"2"' } })
        : jsonResponse({ items: [], next_cursor: null });
    },
  });
  await api.listReports({ cursor: "next", limit: 50 });
  const reportId = `report_${"a".repeat(32)}`;
  await api.decideReport(reportId, '"1"', { disposition: "no-action", reason_codes: ["duplicate"] });
  assert.equal(calls[0].url, "https://operations.cardputerzero.dev/v1/moderation/reports?limit=50&cursor=next");
  assert.equal(calls[1].url, `https://operations.cardputerzero.dev/v1/moderation/reports/${reportId}:decide`);
  assert.equal(calls[1].init.headers["If-Match"], '"1"');
});

test("client rejects oversized responses before parsing", async () => {
  const api = new OperationsApi({
    tokenProvider: async () => token,
    fetchImpl: async () => new Response("{}", { headers: { "content-length": String(65537) } }),
  });
  await assert.rejects(() => api.getToday(), /too large/);
});
