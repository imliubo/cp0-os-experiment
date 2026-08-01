const MAX_RESPONSE_BYTES = 64 * 1024;
const REFRESH_SKEW_MILLISECONDS = 30_000;
const REVIEW_ROLES = new Set(["reviewer", "senior-reviewer", "admin"]);
const PRINCIPAL_ID = /^reviewer_[0-9a-f]{32}$/;
const SECRET = /^[A-Za-z0-9_-]{43}$/;

function strictOrigin(value) {
  const parsed = new URL(value);
  if (parsed.protocol !== "https:" || parsed.username || parsed.password || parsed.search || parsed.hash || !["", "/"].includes(parsed.pathname)) {
    throw new Error("Review Workforce origin must be a bare HTTPS origin");
  }
  return parsed.origin;
}

function hasExactKeys(value, expected) {
  return value && typeof value === "object" && !Array.isArray(value) &&
    Object.keys(value).sort().join("\0") === [...expected].sort().join("\0");
}

function positiveInteger(value) {
  return Number.isSafeInteger(value) && value >= 1;
}

async function boundedJson(response) {
  const contentType = response.headers.get("content-type")?.split(";", 1)[0];
  if (contentType !== (response.ok ? "application/json" : "application/problem+json")) {
    throw new Error("Review Workforce response content type is invalid");
  }
  const declared = Number(response.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > MAX_RESPONSE_BYTES) {
    throw new Error("Review Workforce response is too large");
  }
  const body = await response.text();
  if (new TextEncoder().encode(body).length > MAX_RESPONSE_BYTES) {
    throw new Error("Review Workforce response is too large");
  }
  try {
    return body ? JSON.parse(body) : null;
  } catch {
    throw new Error("Review Workforce response is invalid JSON");
  }
}

function validateSession(value) {
  const keys = [
    "principal_id",
    "role",
    "audience",
    "csrf_token",
    "idle_expires_unix_seconds",
    "absolute_expires_unix_seconds",
    "mfa_authenticated_unix_seconds",
    "resource_version",
  ];
  if (!hasExactKeys(value, keys) || !PRINCIPAL_ID.test(value.principal_id) ||
      !REVIEW_ROLES.has(value.role) || value.audience !== "review" ||
      !SECRET.test(value.csrf_token) || !positiveInteger(value.idle_expires_unix_seconds) ||
      !positiveInteger(value.absolute_expires_unix_seconds) ||
      !positiveInteger(value.mfa_authenticated_unix_seconds) ||
      !positiveInteger(value.resource_version) ||
      value.idle_expires_unix_seconds > value.absolute_expires_unix_seconds ||
      value.mfa_authenticated_unix_seconds > value.absolute_expires_unix_seconds) {
    throw new Error("Review Workforce session response is invalid");
  }
  return Object.freeze({ ...value });
}

function validateControlToken(value) {
  const keys = ["access_token", "token_type", "expires_in", "scope", "audience"];
  if (!hasExactKeys(value, keys) || typeof value.access_token !== "string" ||
      value.access_token.length < 32 || value.access_token.length > 4096 ||
      /[\u0000-\u0020\u007f]/.test(value.access_token) || value.token_type !== "Bearer" ||
      !Number.isSafeInteger(value.expires_in) || value.expires_in < 1 || value.expires_in > 300 ||
      value.scope !== "store.review" || value.audience !== "review") {
    throw new Error("Review Workforce control token response is invalid");
  }
  return value;
}

function requestKey(prefix) {
  return `${prefix}-${crypto.randomUUID()}`;
}

export class ReviewWorkforceSessionClient {
  constructor({ origin = "https://review.cardputerzero.dev", fetchImpl = fetch, now = () => Date.now() } = {}) {
    this.origin = strictOrigin(origin);
    this.fetchImpl = fetchImpl;
    this.now = now;
    this.cachedSession = null;
    this.cachedToken = null;
    this.tokenPromise = null;
    this.logoutPromise = null;
  }

  loginUrl(provider = "primary") {
    if (!/^[a-z][a-z0-9-]{0,31}$/.test(provider)) throw new Error("Review Workforce provider is invalid");
    const target = new URL("/review/auth/login", this.origin);
    target.searchParams.set("provider", provider);
    return target.href;
  }

  async session({ force = false } = {}) {
    if (this.cachedSession && !force) return this.cachedSession;
    const response = await this.fetchImpl(`${this.origin}/review/v1/session`, {
      method: "GET",
      headers: { Accept: "application/json" },
      credentials: "include",
      redirect: "error",
      referrerPolicy: "no-referrer",
    });
    const decoded = await boundedJson(response);
    if (!response.ok) {
      this.clear();
      throw new Error(`Review Workforce returned ${response.status} (${decoded?.code ?? "unknown-error"})`);
    }
    this.cachedSession = validateSession(decoded);
    return this.cachedSession;
  }

  async controlToken() {
    if (this.cachedToken && this.now() < this.cachedToken.expiresAt - REFRESH_SKEW_MILLISECONDS) {
      return this.cachedToken.value;
    }
    if (!this.tokenPromise) {
      this.tokenPromise = this.issueControlToken().finally(() => {
        this.tokenPromise = null;
      });
    }
    return this.tokenPromise;
  }

  async issueControlToken() {
    const session = await this.session();
    const response = await this.fetchImpl(`${this.origin}/review/v1/token`, {
      method: "POST",
      headers: {
        Accept: "application/json",
        "X-CSRF-Token": session.csrf_token,
        "Idempotency-Key": requestKey("review-token"),
      },
      credentials: "include",
      redirect: "error",
      referrerPolicy: "no-referrer",
    });
    const decoded = await boundedJson(response);
    if (!response.ok) {
      if (response.status === 401 || response.status === 403) this.clear();
      throw new Error(`Review Workforce returned ${response.status} (${decoded?.code ?? "unknown-error"})`);
    }
    const token = validateControlToken(decoded);
    this.cachedToken = {
      value: token.access_token,
      expiresAt: this.now() + token.expires_in * 1000,
    };
    return this.cachedToken.value;
  }

  logout() {
    if (!this.logoutPromise) this.logoutPromise = this.performLogout();
    return this.logoutPromise;
  }

  async performLogout() {
    const session = await this.session();
    const response = await this.fetchImpl(`${this.origin}/review/v1/session:logout`, {
      method: "POST",
      headers: {
        "X-CSRF-Token": session.csrf_token,
        "Idempotency-Key": requestKey("review-logout"),
      },
      credentials: "include",
      redirect: "error",
      referrerPolicy: "no-referrer",
    });
    if (response.status !== 204) {
      const decoded = await boundedJson(response);
      throw new Error(`Review Workforce returned ${response.status} (${decoded?.code ?? "unknown-error"})`);
    }
    this.clear();
  }

  clear() {
    this.cachedSession = null;
    this.cachedToken = null;
  }
}
