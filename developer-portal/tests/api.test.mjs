import test from "node:test";
import assert from "node:assert/strict";

import { PortalApi, StoreApi } from "../src/api.js";

const token = "t".repeat(64);

test("rejects non-HTTPS and credential-bearing API origins", () => {
  assert.throws(() => new StoreApi({ origin: "http://developer.example", tokenProvider: () => token }));
  assert.throws(() => new StoreApi({ origin: "https://user@example.dev", tokenProvider: () => token }));
  assert.throws(() => new StoreApi({ origin: "https://example.dev/api", tokenProvider: () => token }));
});

test("sends bounded idempotent requests without browser credentials", async () => {
  let observed;
  const api = new StoreApi({
    origin: "https://developer.example.dev",
    tokenProvider: () => token,
    fetchImpl: async (url, options) => {
      observed = { url, options };
      return new Response(JSON.stringify({ app_id: "dev.cardputerzero.notes" }), {
        status: 201,
        headers: { etag: '"4"', "content-type": "application/json" },
      });
    },
  });
  const response = await api.createApp("dev.cardputerzero.notes", "en-US");
  assert.equal(response.etag, '"4"');
  assert.equal(observed.url, "https://developer.example.dev/v1/apps");
  assert.equal(observed.options.credentials, "omit");
  assert.equal(observed.options.redirect, "error");
  assert.match(observed.options.headers["Idempotency-Key"], /^portal-/);
  assert.equal(observed.options.headers.Authorization, `Bearer ${token}`);
});

test("requires ETags for existing-resource mutations", () => {
  const api = new StoreApi({ tokenProvider: () => token, fetchImpl: async () => new Response() });
  assert.throws(() => api.setTeamMemberRole("team_0123456789abcdef0123456789abcdef", "member_0123456789abcdef0123456789abcdef", "developer", ""), /ETag/);
  assert.throws(() => api.removeTeamMember("team_0123456789abcdef0123456789abcdef", "member_0123456789abcdef0123456789abcdef", ""), /ETag/);
  assert.throws(() => api.suspendTeamMember("team_0123456789abcdef0123456789abcdef", "member_0123456789abcdef0123456789abcdef", ""), /ETag/);
  assert.throws(() => api.restoreTeamMember("team_0123456789abcdef0123456789abcdef", "member_0123456789abcdef0123456789abcdef", ""), /ETag/);
  assert.throws(() => api.withdrawSubmission("sub_0123456789abcdef0123456789abcdef", ""), /ETag/);
  assert.throws(() => api.mutateRelease("rel_0123456789abcdef0123456789abcdef", "pause", ""), /ETag/);
});

test("sends team role changes with a strong precondition", async () => {
  let observed;
  const api = new StoreApi({
    tokenProvider: () => token,
    fetchImpl: async (url, options) => {
      observed = { url, options };
      return new Response(JSON.stringify({ team_id: "team_0123456789abcdef0123456789abcdef" }), {
        status: 200,
        headers: { etag: '"2"', "content-type": "application/json" },
      });
    },
  });
  const response = await api.setTeamMemberRole(
    "team_0123456789abcdef0123456789abcdef",
    "member_0123456789abcdef0123456789abcdef",
    "release-manager",
    '"1"',
  );
  assert.equal(response.etag, '"2"');
  assert.equal(observed.options.method, "POST");
  assert.equal(observed.options.headers["If-Match"], '"1"');
  assert.match(observed.options.headers["Idempotency-Key"], /^portal-/);
  assert.deepEqual(JSON.parse(observed.options.body), { role: "release-manager" });
});

test("sends member removal with no request body", async () => {
  let observed;
  const api = new StoreApi({
    tokenProvider: () => token,
    fetchImpl: async (url, options) => {
      observed = { url, options };
      return new Response(JSON.stringify({ team_id: "team_0123456789abcdef0123456789abcdef", members: [] }), {
        status: 200,
        headers: { etag: '"8"', "content-type": "application/json" },
      });
    },
  });
  const response = await api.removeTeamMember(
    "team_0123456789abcdef0123456789abcdef",
    "member_0123456789abcdef0123456789abcdef",
    '"7"',
  );
  assert.equal(response.etag, '"8"');
  assert.equal(observed.url, "https://developer.cardputerzero.dev/v1/teams/team_0123456789abcdef0123456789abcdef/members/member_0123456789abcdef0123456789abcdef:remove");
  assert.equal(observed.options.method, "POST");
  assert.equal(observed.options.headers["If-Match"], '"7"');
  assert.match(observed.options.headers["Idempotency-Key"], /^portal-/);
  assert.equal(observed.options.headers["Content-Type"], undefined);
  assert.equal(observed.options.body, undefined);
});

test("sends suspension and restoration as empty-body mutations", async () => {
  const observed = [];
  const api = new StoreApi({
    tokenProvider: () => token,
    fetchImpl: async (url, options) => {
      observed.push({ url, options });
      return new Response(JSON.stringify({ team_id: "team_0123456789abcdef0123456789abcdef", members: [] }), {
        status: 200,
        headers: { etag: '"9"', "content-type": "application/json" },
      });
    },
  });
  await api.suspendTeamMember(
    "team_0123456789abcdef0123456789abcdef",
    "member_0123456789abcdef0123456789abcdef",
    '"7"',
  );
  await api.restoreTeamMember(
    "team_0123456789abcdef0123456789abcdef",
    "member_0123456789abcdef0123456789abcdef",
    '"8"',
  );
  assert.match(observed[0].url, /:suspend$/);
  assert.match(observed[1].url, /:restore$/);
  for (const request of observed) {
    assert.equal(request.options.method, "POST");
    assert.match(request.options.headers["Idempotency-Key"], /^portal-/);
    assert.equal(request.options.headers["Content-Type"], undefined);
    assert.equal(request.options.body, undefined);
  }
});

test("rejects oversized and structured error responses", async () => {
  const oversized = new StoreApi({
    tokenProvider: () => token,
    fetchImpl: async () => new Response("x".repeat(70 * 1024), { status: 200 }),
  });
  await assert.rejects(oversized.getApp("dev.cardputerzero.notes"), /too large/);

  const failed = new StoreApi({
    tokenProvider: () => token,
    fetchImpl: async () => new Response(JSON.stringify({ code: "precondition-failed" }), { status: 412 }),
  });
  await assert.rejects(failed.getSubmission("sub_0123456789abcdef0123456789abcdef"), /precondition-failed/);
});

test("rejects normalized paths that escape the v1 control plane", async () => {
  const api = new StoreApi({ tokenProvider: () => token, fetchImpl: async () => new Response() });
  await assert.rejects(api.request("/v1/../admin"), /outside v1/);
  await assert.rejects(api.request("//attacker.example/v1/apps"), /outside v1/);
});

test("uses cookie, CSRF, idempotency, and ETag for Portal identity mutations", async () => {
  const observed = [];
  const api = new PortalApi({
    fetchImpl: async (url, options) => {
      observed.push({ url, options });
      if (url.endsWith("/portal/v1/session")) {
        return new Response(JSON.stringify({ csrf_token: "c".repeat(43) }), {
          status: 200,
          headers: { etag: '"4"' },
        });
      }
      return new Response(JSON.stringify({ authorization_uri: "https://identity.example/authorize" }), {
        status: 200,
      });
    },
  });
  await api.getSession();
  await api.beginIdentityLink("secondary", '"2"');
  assert.equal(observed[0].options.credentials, "include");
  assert.equal(observed[0].options.headers.Authorization, undefined);
  assert.equal(observed[1].url, "https://developer.cardputerzero.dev/portal/v1/identity-links");
  assert.equal(observed[1].options.credentials, "include");
  assert.equal(observed[1].options.headers["X-CSRF-Token"], "c".repeat(43));
  assert.equal(observed[1].options.headers["If-Match"], '"2"');
  assert.match(observed[1].options.headers["Idempotency-Key"], /^portal-/);
  assert.equal(observed[1].options.headers.Authorization, undefined);
  assert.deepEqual(JSON.parse(observed[1].options.body), { provider: "secondary" });
});

test("fails closed before Portal mutations and rejects escaped BFF paths", async () => {
  const api = new PortalApi({ fetchImpl: async () => new Response() });
  await assert.rejects(api.beginIdentityLink("secondary", '"1"'), /session is unavailable/);
  await assert.rejects(api.request("/portal/v1/../admin"), /outside portal v1/);
  await assert.rejects(api.request("//attacker.example/portal/v1/session"), /outside portal v1/);
  assert.throws(() => api.removeIdentityLink("link_0123456789abcdef0123456789abcdef", ""), /ETag/);
});
