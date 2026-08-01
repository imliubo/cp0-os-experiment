import assert from "node:assert/strict";
import test from "node:test";

import { OperationsApi } from "../src/api.js";
import { OperationsWorkforceSessionClient } from "../src/workforce-session.js";

const csrf = "c".repeat(43);
const editorialToken = "operations-editorial-token-" + "e".repeat(32);
const moderationToken = "operations-moderation-token-" + "m".repeat(32);

function jsonResponse(body, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": status < 400 ? "application/json" : "application/problem+json" },
  });
}

function session(overrides = {}) {
  return {
    principal_id: `operator_${"1".repeat(32)}`,
    role: "admin",
    audience: "operations",
    allowed_scopes: ["store.editorial", "store.moderation"],
    csrf_token: csrf,
    idle_expires_unix_seconds: 2_000,
    absolute_expires_unix_seconds: 3_000,
    mfa_authenticated_unix_seconds: 1_000,
    resource_version: 1,
    ...overrides,
  };
}

test("uses cookies only for audience-specific BFF tokens and caches each scope", async () => {
  const calls = [];
  const client = new OperationsWorkforceSessionClient({
    now: () => 1_000_000,
    fetchImpl: async (url, init) => {
      calls.push({ url, init });
      if (url.endsWith("/session")) return jsonResponse(session());
      const scope = JSON.parse(init.body).scope;
      return jsonResponse({
        access_token: scope === "store.editorial" ? editorialToken : moderationToken,
        token_type: "Bearer",
        expires_in: 300,
        scope,
        audience: "operations",
      });
    },
  });
  assert.equal(await client.controlToken("store.editorial"), editorialToken);
  assert.equal(await client.controlToken("store.moderation"), moderationToken);
  assert.equal(await client.controlToken("store.editorial"), editorialToken);
  assert.equal(calls.length, 3);
  assert.ok(calls.every((call) => call.init.credentials === "include"));
  assert.equal(calls[1].init.headers["X-CSRF-Token"], csrf);
  assert.equal(calls[1].init.headers.Authorization, undefined);
  assert.deepEqual(JSON.parse(calls[2].init.body), { scope: "store.moderation" });
});

test("rejects audience, role-scope, and token-scope mismatches", async () => {
  const wrongSession = new OperationsWorkforceSessionClient({
    fetchImpl: async () => jsonResponse(session({ audience: "review" })),
  });
  await assert.rejects(() => wrongSession.session(), /session response is invalid/);

  const editor = new OperationsWorkforceSessionClient({
    fetchImpl: async () => jsonResponse(session({ role: "editor", allowed_scopes: ["store.editorial"] })),
  });
  await assert.rejects(() => editor.controlToken("store.moderation"), /not authorized/);

  const wrongToken = new OperationsWorkforceSessionClient({
    fetchImpl: async (url) => url.endsWith("/session")
      ? jsonResponse(session())
      : jsonResponse({ access_token: editorialToken, token_type: "Bearer", expires_in: 300, scope: "store.moderation", audience: "operations" }),
  });
  await assert.rejects(() => wrongToken.controlToken("store.editorial"), /control token response is invalid/);
});

test("default Operations API requests exact scopes without cookies", async () => {
  const scopes = [];
  const requests = [];
  const api = new OperationsApi({
    sessionClient: {
      controlToken: async (scope) => {
        scopes.push(scope);
        return scope === "store.editorial" ? editorialToken : moderationToken;
      },
    },
    fetchImpl: async (url, init) => {
      requests.push({ url, init });
      return jsonResponse({ items: [], next_cursor: null });
    },
  });
  await api.listPublishedReleases();
  await api.listReports();
  assert.deepEqual(scopes, ["store.editorial", "store.moderation"]);
  assert.ok(requests.every((request) => request.init.credentials === "omit"));
  assert.equal(requests[0].init.headers.Authorization, `Bearer ${editorialToken}`);
  assert.equal(requests[1].init.headers.Authorization, `Bearer ${moderationToken}`);
});

test("login and logout remain on the Operations origin", async () => {
  const calls = [];
  const client = new OperationsWorkforceSessionClient({
    fetchImpl: async (url, init) => {
      calls.push({ url, init });
      if (url.endsWith(":logout")) return new Response(null, { status: 204 });
      return jsonResponse(session());
    },
  });
  assert.equal(
    client.loginUrl("primary"),
    "https://operations.cardputerzero.dev/operations/auth/login?provider=primary",
  );
  await client.logout();
  assert.equal(calls[1].init.credentials, "include");
  assert.match(calls[1].init.headers["Idempotency-Key"], /^operations-logout-/);
});
