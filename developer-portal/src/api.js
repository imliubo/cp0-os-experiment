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
