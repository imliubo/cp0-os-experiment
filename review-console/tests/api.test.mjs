import test from "node:test";
import assert from "node:assert/strict";

import { ReviewApi } from "../src/api.js";

const token = "review-console-token-0000000000000001";

test("rejects unsafe origins and queue bounds", () => {
  assert.throws(() => new ReviewApi({ origin: "http://review.example" }), /HTTPS/);
  assert.throws(() => new ReviewApi({ origin: "https://user@review.example" }), /HTTPS/);
  const api = new ReviewApi({ tokenProvider: () => token, fetchImpl: async () => new Response() });
  assert.throws(() => api.listQueue({ limit: 51 }), /1-50/);
  assert.throws(() => api.beginReview("sub_0123456789abcdef0123456789abcdef", ""), /ETag/);
});

test("sends bounded queue queries and strong claim preconditions", async () => {
  const requests = [];
  const api = new ReviewApi({
    tokenProvider: () => token,
    fetchImpl: async (url, options) => {
      requests.push({ url, options });
      return new Response(JSON.stringify({ items: [], next_cursor: null }), { status: 200, headers: { "content-type": "application/json", etag: '"4"' } });
    },
  });
  await api.listQueue({ cursor: "cursor-value", limit: 20 });
  await api.beginReview("sub_0123456789abcdef0123456789abcdef", '"3"');
  assert.equal(new URL(requests[0].url).search, "?limit=20&cursor=cursor-value");
  assert.equal(requests[1].options.headers["If-Match"], '"3"');
  assert.match(requests[1].options.headers["Idempotency-Key"], /^review-/);
  assert.equal(requests[1].options.credentials, "omit");
});

test("rejects unversioned or noncanonical queue risk assessments", async () => {
  const api = new ReviewApi({
    tokenProvider: () => token,
    fetchImpl: async () => new Response(JSON.stringify({
      items: [{ risk: { policy_version: 1, tier: "high", reasons: ["network-access", "camera-capture"] } }],
      next_cursor: null,
    }), { status: 200 }),
  });
  await assert.rejects(api.listQueue(), /risk reasons/);
});

test("validates structured decisions before sending", () => {
  const api = new ReviewApi({ tokenProvider: () => token, fetchImpl: async () => new Response("{}", { status: 201 }) });
  assert.throws(() => api.decideReview("sub_0123456789abcdef0123456789abcdef", '"4"', { decision: "rejected", reasonCodes: [], note: "" }), /invalid/);
});
