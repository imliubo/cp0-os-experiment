import assert from "node:assert/strict";
import test from "node:test";

import { ReviewApi } from "../src/api.js";
import { ReviewWorkforceSessionClient } from "../src/workforce-session.js";

const csrf = "c".repeat(43);
const token = "review-workforce-token-" + "t".repeat(32);

function jsonResponse(body, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": status < 400 ? "application/json" : "application/problem+json" },
  });
}

function session(overrides = {}) {
  return {
    principal_id: `reviewer_${"1".repeat(32)}`,
    role: "reviewer",
    audience: "review",
    csrf_token: csrf,
    idle_expires_unix_seconds: 2_000,
    absolute_expires_unix_seconds: 3_000,
    mfa_authenticated_unix_seconds: 1_000,
    resource_version: 1,
    ...overrides,
  };
}

test("uses cookies only for the BFF and caches a short token in memory", async () => {
  const calls = [];
  const client = new ReviewWorkforceSessionClient({
    now: () => 1_000_000,
    fetchImpl: async (url, init) => {
      calls.push({ url, init });
      return url.endsWith("/session")
        ? jsonResponse(session())
        : jsonResponse({ access_token: token, token_type: "Bearer", expires_in: 300, scope: "store.review", audience: "review" });
    },
  });
  assert.equal(await client.controlToken(), token);
  assert.equal(await client.controlToken(), token);
  assert.equal(calls.length, 2);
  assert.equal(calls[0].init.credentials, "include");
  assert.equal(calls[1].init.credentials, "include");
  assert.equal(calls[1].init.headers["X-CSRF-Token"], csrf);
  assert.match(calls[1].init.headers["Idempotency-Key"], /^review-token-/);
  assert.equal(calls[1].init.headers.Authorization, undefined);
});

test("rejects cross-audience sessions and token scopes", async () => {
  const wrongSession = new ReviewWorkforceSessionClient({
    fetchImpl: async () => jsonResponse(session({ audience: "operations" })),
  });
  await assert.rejects(() => wrongSession.session(), /session response is invalid/);

  const wrongToken = new ReviewWorkforceSessionClient({
    fetchImpl: async (url) => url.endsWith("/session")
      ? jsonResponse(session())
      : jsonResponse({ access_token: token, token_type: "Bearer", expires_in: 300, scope: "store.editorial", audience: "review" }),
  });
  await assert.rejects(() => wrongToken.controlToken(), /control token response is invalid/);
});

test("default Review API provider keeps the control request cookieless", async () => {
  let captured;
  const api = new ReviewApi({
    sessionClient: { controlToken: async () => token },
    fetchImpl: async (url, init) => {
      captured = { url, init };
      return jsonResponse({ items: [], next_cursor: null });
    },
  });
  await api.listQueue();
  assert.equal(captured.init.credentials, "omit");
  assert.equal(captured.init.headers.Authorization, `Bearer ${token}`);
});

test("logout is coalesced and clears cached authorization", async () => {
  const calls = [];
  const client = new ReviewWorkforceSessionClient({
    fetchImpl: async (url, init) => {
      calls.push({ url, init });
      if (url.endsWith(":logout")) return new Response(null, { status: 204 });
      return jsonResponse(session());
    },
  });
  await Promise.all([client.logout(), client.logout()]);
  assert.equal(calls.length, 2);
  assert.equal(calls[1].init.credentials, "include");
  assert.match(calls[1].init.headers["Idempotency-Key"], /^review-logout-/);
});
