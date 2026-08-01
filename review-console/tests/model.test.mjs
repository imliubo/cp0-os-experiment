import test from "node:test";
import assert from "node:assert/strict";

import { applyDecision, canClaimReview, filterQueue, mapQueueItem, mapReviewDetail, validateDecision } from "../src/model.js";

const reviewerId = `reviewer_${"a".repeat(32)}`;
const submissionId = `sub_${"1".repeat(32)}`;
const queueValue = {
  submission: {
    submission_id: submissionId,
    app_id: "dev.cardputerzero.notes",
    version: "2.4.0",
    revision: 1,
    state: "ready-for-review",
    package_sha256: "1".repeat(64),
    listing_sha256: "2".repeat(64),
    assets: [
      { path: "icon.png", sha256: "3".repeat(64), bytes: 100, width: 48, height: 48 },
      { path: "screens/main.png", sha256: "4".repeat(64), bytes: 200, width: 320, height: 170 },
    ],
    resource_version: 3,
    created_unix_seconds: 1_000,
  },
  app: { name: "Field Notes", developer_name: "Northstar Devices", category: "productivity" },
  review_stage: "primary",
  assigned_to_caller: false,
  risk: { policy_version: 1, tier: "standard", reasons: [] },
};

test("maps and filters authoritative queue records", () => {
  const primary = mapQueueItem(queueValue, { reviewerId, nowSeconds: 1_600 });
  const secondary = { ...primary, id: `sub_${"2".repeat(32)}`, stage: "secondary", state: "pending-secondary-review" };
  assert.equal(primary.name, "Field Notes");
  assert.equal(primary.submitted, "10 min ago");
  assert.equal(primary.category, "Productivity");
  assert.equal(filterQueue([primary, secondary]).length, 2);
  assert.equal(filterQueue([primary, secondary], { stage: "secondary" }).length, 1);
  assert.equal(filterQueue([primary], { query: "northstar" })[0].appId, "dev.cardputerzero.notes");
});

test("maps bounded detail records and current assignment", () => {
  const detail = mapReviewDetail({
    ...queueValue,
    submission: { ...queueValue.submission, state: "in-review", resource_version: 4 },
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
      created_unix_seconds: 1_100,
      completed_unix_seconds: null,
    }],
    decisions: [],
    messages: [{ message_id: `msg_${"1".repeat(32)}`, actor_id: reviewerId, actor_kind: "reviewer", actor_label: "reviewer@example.test", body: "Checking metadata.", created_unix_seconds: 1_200 }],
    messages_truncated: false,
    audit: [{ sequence: 7, occurred_unix_seconds: 1_100, actor_id: reviewerId, action: "submission.review-begun", before_state: "ready-for-review", after_state: "in-review", resource_version: 4 }],
    audit_truncated: false,
  }, { etag: '"4"', reviewerId, nowSeconds: 1_300 });
  assert.equal(detail.assignee, reviewerId);
  assert.equal(detail.primaryReviewer, "reviewer@example.test");
  assert.deepEqual(detail.imports, ["cp0_storage_get"]);
  assert.equal(detail.messages[0].time, "1 min ago");
  assert.equal(detail.audit[0].resourceVersion, 4);
});

test("primary approval cannot become final approval", () => {
  const primary = { ...mapQueueItem(queueValue, { reviewerId }), state: "in-review", assignee: reviewerId };
  const next = applyDecision(primary, "approved");
  assert.equal(next.state, "pending-secondary-review");
  assert.equal(next.stage, "secondary");
  assert.equal(next.assignee, null);
  assert.equal(canClaimReview(next, reviewerId), false);
});

test("non-approval requires structured reasons", () => {
  assert.ok(validateDecision({ decision: "rejected", reasonCodes: [], note: "" }).reasonCodes);
  assert.deepEqual(validateDecision({ decision: "needs-changes", reasonCodes: ["privacy-disclosure"], note: "Clarify retention." }), {});
});
