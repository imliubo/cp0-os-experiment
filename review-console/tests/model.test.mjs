import test from "node:test";
import assert from "node:assert/strict";

import { applyDecision, canClaimReview, createReviewData, filterQueue, validateDecision } from "../src/model.js";

test("filters bounded review stages and search text", () => {
  const items = createReviewData();
  assert.equal(filterQueue(items).length, 4);
  assert.equal(filterQueue(items, { stage: "secondary" }).length, 2);
  assert.equal(filterQueue(items, { query: "signal" })[0].appId, "dev.cardputerzero.signallab");
});

test("primary approval cannot become final approval", () => {
  const primary = { ...createReviewData()[1], state: "in-review", assignee: "Liang Bo" };
  const next = applyDecision(primary, "approved");
  assert.equal(next.state, "pending-secondary-review");
  assert.equal(next.stage, "secondary");
  assert.equal(next.assignee, null);
  assert.equal(next.primaryReviewer, "Liang Bo");
  assert.equal(canClaimReview(next, "Liang Bo"), false);
  assert.equal(canClaimReview(next, "Maya Chen"), true);
  assert.equal(filterQueue([next], { reviewer: "Liang Bo" }).length, 0);
});

test("secondary approval is final and non-approval requires reasons", () => {
  const secondary = { ...createReviewData()[0], state: "in-review", assignee: "Liang Bo" };
  assert.equal(applyDecision(secondary, "approved").state, "approved");
  assert.throws(
    () => applyDecision({ ...secondary, assignee: secondary.primaryReviewer }, "approved"),
    /Independent reviewer must differ/,
  );
  assert.ok(validateDecision({ decision: "rejected", reasonCodes: [], note: "" }).reasonCodes);
  assert.deepEqual(validateDecision({ decision: "needs-changes", reasonCodes: ["privacy-disclosure"], note: "Clarify retention." }), {});
});
