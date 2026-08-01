const MAX_RESPONSE_BYTES = 64 * 1024;

async function boundedResponseText(response) {
  const declaredLength = Number(response.headers.get("content-length"));
  if (Number.isFinite(declaredLength) && declaredLength > MAX_RESPONSE_BYTES) {
    throw new Error("Store API response is too large");
  }
  if (!response.body?.getReader) {
    const encoded = await response.text();
    if (new TextEncoder().encode(encoded).length > MAX_RESPONSE_BYTES) throw new Error("Store API response is too large");
    return encoded;
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let total = 0;
  let encoded = "";
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > MAX_RESPONSE_BYTES) {
        await reader.cancel();
        throw new Error("Store API response is too large");
      }
      encoded += decoder.decode(value, { stream: true });
    }
    return encoded + decoder.decode();
  } finally {
    reader.releaseLock();
  }
}

function strictOrigin(value) {
  const parsed = new URL(value);
  if (
    parsed.protocol !== "https:" ||
    parsed.username ||
    parsed.password ||
    parsed.search ||
    parsed.hash ||
    (parsed.pathname !== "/" && parsed.pathname !== "")
  ) {
    throw new Error("Store API must be a bare HTTPS origin");
  }
  return parsed.origin;
}

function idempotencyKey() {
  return `portal-${crypto.randomUUID()}`;
}

export class StoreApi {
  constructor({ origin = "https://developer.cardputerzero.dev", tokenProvider, fetchImpl = fetch }) {
    this.origin = strictOrigin(origin);
    this.tokenProvider = tokenProvider;
    this.fetchImpl = fetchImpl;
  }

  async request(path, { method = "GET", body, etag, idempotent = false } = {}) {
    const target = new URL(path, this.origin);
    if (target.origin !== this.origin || !path.startsWith("/v1/") || target.pathname !== path || target.search || target.hash) {
      throw new Error("Store API path is outside v1");
    }
    const token = await this.tokenProvider?.();
    if (!token || token.length < 32 || /[\r\n]/.test(token)) throw new Error("Store authorization is unavailable");
    const headers = {
      Accept: "application/json",
      Authorization: `Bearer ${token}`,
    };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    if (idempotent) headers["Idempotency-Key"] = idempotencyKey();
    if (etag) headers["If-Match"] = etag;
    const response = await this.fetchImpl(target.href, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      credentials: "omit",
      redirect: "error",
      referrerPolicy: "no-referrer",
    });
    const encoded = await boundedResponseText(response);
    const decoded = encoded ? JSON.parse(encoded) : null;
    if (!response.ok) {
      const code = decoded?.code ?? "unknown-error";
      throw new Error(`Store API returned ${response.status} (${code})`);
    }
    return { data: decoded, etag: response.headers.get("etag") };
  }

  createApp(appId, defaultLocale) {
    return this.request("/v1/apps", {
      method: "POST",
      idempotent: true,
      body: { app_id: appId, default_locale: defaultLocale },
    });
  }

  getTeam(teamId) {
    return this.request(`/v1/teams/${encodeURIComponent(teamId)}`);
  }

  setTeamMemberRole(teamId, memberId, role, etag) {
    if (!etag) throw new Error("Team role changes require an ETag");
    if (!["owner", "developer", "release-manager", "viewer"].includes(role)) {
      throw new Error("Unknown team role");
    }
    return this.request(
      `/v1/teams/${encodeURIComponent(teamId)}/members/${encodeURIComponent(memberId)}:set-role`,
      {
        method: "POST",
        idempotent: true,
        etag,
        body: { role },
      },
    );
  }

  removeTeamMember(teamId, memberId, etag) {
    if (!etag) throw new Error("Team member removal requires an ETag");
    return this.request(
      `/v1/teams/${encodeURIComponent(teamId)}/members/${encodeURIComponent(memberId)}:remove`,
      {
        method: "POST",
        idempotent: true,
        etag,
      },
    );
  }

  suspendTeamMember(teamId, memberId, etag) {
    if (!etag) throw new Error("Team member suspension requires an ETag");
    return this.request(
      `/v1/teams/${encodeURIComponent(teamId)}/members/${encodeURIComponent(memberId)}:suspend`,
      { method: "POST", idempotent: true, etag },
    );
  }

  restoreTeamMember(teamId, memberId, etag) {
    if (!etag) throw new Error("Team member restoration requires an ETag");
    return this.request(
      `/v1/teams/${encodeURIComponent(teamId)}/members/${encodeURIComponent(memberId)}:restore`,
      { method: "POST", idempotent: true, etag },
    );
  }

  getApp(appId) {
    return this.request(`/v1/apps/${encodeURIComponent(appId)}`);
  }

  getSubmission(submissionId) {
    return this.request(`/v1/submissions/${encodeURIComponent(submissionId)}`);
  }

  withdrawSubmission(submissionId, etag) {
    if (!etag) throw new Error("Submission withdrawal requires an ETag");
    return this.request(`/v1/submissions/${encodeURIComponent(submissionId)}:withdraw`, {
      method: "POST",
      idempotent: true,
      etag,
    });
  }

  createRelease(submissionId, rolloutPercent) {
    return this.request("/v1/releases", {
      method: "POST",
      idempotent: true,
      body: { submission_id: submissionId, rollout_percent: rolloutPercent },
    });
  }

  mutateRelease(releaseId, action, etag, body) {
    if (!etag) throw new Error("Release mutation requires an ETag");
    if (!["schedule", "publish", "pause", "resume", "remove"].includes(action)) {
      throw new Error("Unknown release action");
    }
    return this.request(`/v1/releases/${encodeURIComponent(releaseId)}:${action}`, {
      method: "POST",
      idempotent: true,
      etag,
      body,
    });
  }
}

export class PortalApi {
  constructor({ origin = "https://developer.cardputerzero.dev", fetchImpl = fetch }) {
    this.origin = strictOrigin(origin);
    this.fetchImpl = fetchImpl;
    this.csrfToken = null;
  }

  async request(
    path,
    { method = "GET", body, etag, protectedMutation = false, idempotent = false } = {},
  ) {
    const target = new URL(path, this.origin);
    if (
      target.origin !== this.origin ||
      !path.startsWith("/portal/v1/") ||
      target.pathname !== path ||
      target.search ||
      target.hash
    ) {
      throw new Error("Portal API path is outside portal v1");
    }
    const headers = { Accept: "application/json" };
    if (body !== undefined) headers["Content-Type"] = "application/json";
    if (protectedMutation) {
      if (!this.csrfToken) throw new Error("Portal session is unavailable");
      headers["X-CSRF-Token"] = this.csrfToken;
    }
    if (idempotent) headers["Idempotency-Key"] = idempotencyKey();
    if (etag) headers["If-Match"] = etag;
    const response = await this.fetchImpl(target.href, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
      credentials: "include",
      redirect: "error",
      referrerPolicy: "no-referrer",
    });
    const encoded = await boundedResponseText(response);
    let decoded = null;
    try {
      decoded = encoded ? JSON.parse(encoded) : null;
    } catch {
      throw new Error("Portal API returned invalid JSON");
    }
    if (!response.ok) {
      const code = decoded?.code ?? "unknown-error";
      throw new Error(`Portal API returned ${response.status} (${code})`);
    }
    return { data: decoded, etag: response.headers.get("etag") };
  }

  async getSession() {
    const response = await this.request("/portal/v1/session");
    const csrf = response.data?.csrf_token;
    if (typeof csrf !== "string" || csrf.length < 43 || /[\r\n]/.test(csrf)) {
      throw new Error("Portal API returned an invalid CSRF token");
    }
    this.csrfToken = csrf;
    return response;
  }

  listIdentityLinks() {
    return this.request("/portal/v1/identity-links");
  }

  beginIdentityLink(provider, etag) {
    if (!etag) throw new Error("Identity linking requires an ETag");
    return this.request("/portal/v1/identity-links", {
      method: "POST",
      body: { provider },
      etag,
      protectedMutation: true,
      idempotent: true,
    });
  }

  removeIdentityLink(linkId, etag) {
    if (!etag) throw new Error("Identity removal requires an ETag");
    return this.request(
      `/portal/v1/identity-links/${encodeURIComponent(linkId)}:remove`,
      {
        method: "POST",
        etag,
        protectedMutation: true,
        idempotent: true,
      },
    );
  }

  beginStepUp(etag) {
    if (!etag) throw new Error("MFA step-up requires a session ETag");
    return this.request("/portal/v1/session:step-up", {
      method: "POST",
      etag,
      protectedMutation: true,
      idempotent: true,
    });
  }

  async logout() {
    const response = await this.request("/portal/v1/session:logout", {
      method: "POST",
      protectedMutation: true,
      idempotent: true,
    });
    this.csrfToken = null;
    return response;
  }

  listInvitations(teamId) {
    return this.request(`/portal/v1/teams/${encodeURIComponent(teamId)}/invitations`);
  }

  createInvitation(teamId, email, role, etag) {
    if (!etag) throw new Error("Invitation creation requires a Team ETag");
    return this.request(`/portal/v1/teams/${encodeURIComponent(teamId)}/invitations`, {
      method: "POST",
      body: { email, role },
      etag,
      protectedMutation: true,
      idempotent: true,
    });
  }

  cancelInvitation(invitationId, etag) {
    if (!etag) throw new Error("Invitation cancellation requires a Team ETag");
    return this.request(
      `/portal/v1/invitations/${encodeURIComponent(invitationId)}:cancel`,
      {
        method: "POST",
        etag,
        protectedMutation: true,
        idempotent: true,
      },
    );
  }

  inspectInvitation(invitationToken) {
    return this.request("/portal/v1/invitations:inspect", {
      method: "POST",
      body: { invitation_token: invitationToken },
    });
  }

  acceptInvitation(invitationToken) {
    return this.request("/portal/v1/invitations:accept", {
      method: "POST",
      body: { invitation_token: invitationToken },
      protectedMutation: true,
      idempotent: true,
    });
  }
}
