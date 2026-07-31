import test from "node:test";
import assert from "node:assert/strict";

import {
  RELEASE_TRANSITIONS,
  SUBMISSION_TRANSITIONS,
  canTransition,
  createDemoState,
  listingProgress,
  releaseAction,
  validateAppDraft,
  validateListing,
} from "../src/model.js";

test("matches the frozen submission and release state machines", () => {
  assert.equal(canTransition(SUBMISSION_TRANSITIONS, "uploading", "processing"), true);
  assert.equal(canTransition(SUBMISSION_TRANSITIONS, "approved", "in-review"), false);
  assert.equal(canTransition(RELEASE_TRANSITIONS, "published", "paused"), true);
  assert.equal(canTransition(RELEASE_TRANSITIONS, "removed", "published"), false);
  assert.equal(releaseAction("published"), "pause");
  assert.equal(releaseAction("paused"), "resume");
});

test("validates permanent App IDs and complete listings", () => {
  assert.deepEqual(validateAppDraft({ appId: "dev.cardputerzero.notes", defaultLocale: "en-US" }), {});
  assert.ok(validateAppDraft({ appId: "Notes", defaultLocale: "english" }).appId);
  const state = createDemoState();
  assert.deepEqual(validateListing(state.apps[0].listing), {});
  assert.equal(listingProgress(state.apps[0].listing), 100);
  assert.ok(validateListing({ ...state.apps[0].listing, privacyUrl: "http://localhost" }).privacyUrl);
  assert.ok(validateListing({ ...state.apps[0].listing, privacyUrl: "https://user@example.dev/privacy" }).privacyUrl);
  assert.ok(validateListing({ ...state.apps[0].listing, privacyUrl: "https://example.dev/privacy#secret" }).privacyUrl);
});

test("demo developer keys contain public metadata only", () => {
  const encoded = JSON.stringify(createDemoState().team.developerKeys);
  assert.doesNotMatch(encoded, /private[_-]?key|secret/i);
  assert.match(encoded, /fingerprint/);
});
