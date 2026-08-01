import test from "node:test";
import assert from "node:assert/strict";

import { ReviewApi } from "../src/api.js";

const token = "review-console-token-0000000000000001";
const reviewerId = `reviewer_${"a".repeat(32)}`;
const queueCursor = `0000000000000001.sub_${"1".repeat(32)}`;

function queueItem() {
  return {
    submission: {
      submission_id: `sub_${"1".repeat(32)}`,
      app_id: "dev.cardputerzero.notes",
      version: "1.0.0",
      revision: 1,
      state: "ready-for-review",
      package_sha256: "1".repeat(64),
      listing_sha256: "2".repeat(64),
      assets: [
        { path: "icon.png", sha256: "3".repeat(64), bytes: 100, width: 48, height: 48 },
        { path: "screens/main.png", sha256: "4".repeat(64), bytes: 200, width: 320, height: 170 },
      ],
      resource_version: 1,
      created_unix_seconds: 1,
    },
    app: { name: "Field Notes", developer_name: "Team A", category: "productivity" },
    review_stage: "primary",
    assigned_to_caller: false,
    risk: { policy_version: 1, tier: "standard", reasons: [] },
  };
}

function detail() {
  return {
    ...queueItem(),
    submission: { ...queueItem().submission, state: "in-review", resource_version: 2 },
    assigned_to_caller: true,
    scan: {
      scan_id: `scan_${"1".repeat(32)}`,
      scanner_version: "cp0-store-scan/1",
      report_sha256: "5".repeat(64),
      developer_key_sha256: "6".repeat(64),
      imports: ["cp0_storage_get"],
      permissions: [],
      findings: [],
    },
    assignments: [{
      assignment_id: `assignment_${"1".repeat(32)}`,
      reviewer_id: reviewerId,
      reviewer_email: "reviewer@example.test",
      reviewer_role: "reviewer",
      assignment_kind: "primary",
      state: "active",
      created_unix_seconds: 2,
      completed_unix_seconds: null,
    }],
    decisions: [],
    messages: [{
      message_id: `msg_${"1".repeat(32)}`,
      actor_id: reviewerId,
      actor_kind: "reviewer",
      actor_label: "reviewer@example.test",
      body: "First line.\nSecond line.",
      created_unix_seconds: 3,
    }],
    messages_truncated: false,
    audit: [{
      sequence: 1,
      occurred_unix_seconds: 2,
      actor_id: reviewerId,
      action: "submission.review-begun",
      before_state: "ready-for-review",
      after_state: "in-review",
      resource_version: 2,
    }],
    audit_truncated: false,
  };
}

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
  await api.listQueue({ cursor: queueCursor, limit: 20 });
  await api.beginReview("sub_0123456789abcdef0123456789abcdef", '"3"');
  assert.equal(new URL(requests[0].url).search, `?limit=20&cursor=${queueCursor}`);
  assert.equal(requests[1].options.headers["If-Match"], '"3"');
  assert.match(requests[1].options.headers["Idempotency-Key"], /^review-/);
  assert.equal(requests[1].options.credentials, "omit");
});

test("rejects unversioned or noncanonical queue risk assessments", async () => {
  const api = new ReviewApi({
    tokenProvider: () => token,
    fetchImpl: async () => new Response(JSON.stringify({
      items: [{ ...queueItem(), risk: { policy_version: 1, tier: "high", reasons: ["network-access", "camera-capture"] } }],
      next_cursor: null,
    }), { status: 200 }),
  });
  await assert.rejects(api.listQueue(), /risk reasons/);
});

test("validates every bounded review detail record and allows canonical multiline text", async () => {
  const valid = detail();
  const api = new ReviewApi({
    tokenProvider: () => token,
    fetchImpl: async () => new Response(JSON.stringify(valid), { status: 200, headers: { etag: '"2"' } }),
  });
  assert.equal((await api.getSubmissionDetail(valid.submission.submission_id)).data.messages[0].body, "First line.\nSecond line.");

  const malformed = structuredClone(valid);
  malformed.assignments[0].reviewer_role = "owner";
  const invalid = new ReviewApi({
    tokenProvider: () => token,
    fetchImpl: async () => new Response(JSON.stringify(malformed), { status: 200 }),
  });
  await assert.rejects(() => invalid.getSubmissionDetail(valid.submission.submission_id), /assignment response is invalid/);
});

test("validates structured decisions and sends the backend wire shape", async () => {
  let captured;
  const api = new ReviewApi({ tokenProvider: () => token, fetchImpl: async (_url, init) => {
    captured = init;
    return new Response("{}", { status: 201 });
  } });
  assert.throws(() => api.decideReview("sub_0123456789abcdef0123456789abcdef", '"4"', { decision: "rejected", reasonCodes: [], note: "" }), /invalid/);
  await api.decideReview("sub_0123456789abcdef0123456789abcdef", '"4"', { decision: "needs-changes", reasonCodes: ["privacy-disclosure"], note: "Clarify retention." });
  assert.deepEqual(JSON.parse(captured.body), { decision: "needs-changes", reason_codes: ["privacy-disclosure"], note: "Clarify retention." });
  await api.postMessage("sub_0123456789abcdef0123456789abcdef", "First line.\nSecond line.");
  assert.deepEqual(JSON.parse(captured.body), { body: "First line.\nSecond line." });
});
