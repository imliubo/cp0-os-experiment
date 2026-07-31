export const SUBMISSION_TRANSITIONS = Object.freeze({
  draft: ["uploading", "withdrawn"],
  uploading: ["processing", "withdrawn"],
  processing: ["ready-for-review", "needs-changes", "rejected", "withdrawn"],
  "ready-for-review": ["in-review", "withdrawn"],
  "in-review": ["approved", "needs-changes", "rejected", "withdrawn"],
  "needs-changes": [],
  approved: [],
  rejected: [],
  withdrawn: [],
});

export const RELEASE_TRANSITIONS = Object.freeze({
  ready: ["scheduled", "publishing", "removed"],
  scheduled: ["ready", "publishing", "removed"],
  publishing: ["published", "publish-failed", "removed"],
  "publish-failed": ["ready", "removed"],
  published: ["paused", "removed"],
  paused: ["published", "removed"],
  removed: [],
});

export const CATEGORIES = Object.freeze([
  "developer-tools",
  "education",
  "entertainment",
  "games",
  "hardware",
  "media",
  "productivity",
  "utilities",
]);

export const AGE_RATINGS = Object.freeze(["4+", "9+", "12+", "17+"]);

const APP_ID_PATTERN = /^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*){2,}$/;
const LOCALE_PATTERN = /^[a-z]{2,3}(-[A-Z][a-z]{3})?(-([A-Z]{2}|[0-9]{3}))?$/;

function isPublicHttpsUrl(value) {
  if (typeof value !== "string" || value.length > 2048) return false;
  try {
    const parsed = new URL(value);
    return parsed.protocol === "https:"
      && !parsed.username
      && !parsed.password
      && !parsed.hash
      && parsed.hostname.includes(".")
      && !parsed.hostname.endsWith(".local");
  } catch {
    return false;
  }
}

export function validateAppDraft({ appId, defaultLocale }) {
  const errors = {};
  if (!APP_ID_PATTERN.test(appId) || appId.length > 128) {
    errors.appId = "Use at least three lowercase DNS-style segments.";
  }
  if (!LOCALE_PATTERN.test(defaultLocale)) {
    errors.defaultLocale = "Use a canonical locale such as en-US.";
  }
  return errors;
}

export function validateListing(listing) {
  const errors = {};
  if (!CATEGORIES.includes(listing.category)) errors.category = "Select a category.";
  if (!AGE_RATINGS.includes(listing.ageRating)) errors.ageRating = "Select an age rating.";
  if (!isPublicHttpsUrl(listing.privacyUrl)) errors.privacyUrl = "Use a public HTTPS URL.";
  if (!isPublicHttpsUrl(listing.supportUrl)) errors.supportUrl = "Use a public HTTPS URL.";
  if (!listing.name?.trim() || listing.name.length > 32) errors.name = "Name must be 1-32 characters.";
  if (!listing.subtitle?.trim() || listing.subtitle.length > 48) errors.subtitle = "Subtitle must be 1-48 characters.";
  if (!listing.description?.trim() || listing.description.length > 1024) errors.description = "Description is required.";
  if (!listing.releaseNotes?.trim() || listing.releaseNotes.length > 512) errors.releaseNotes = "Release notes are required.";
  if (!listing.iconReady) errors.iconReady = "Add one 48x48 PNG icon.";
  if ((listing.screenshotCount ?? 0) < 1 || listing.screenshotCount > 5) errors.screenshotCount = "Add 1-5 screenshots.";
  return errors;
}

export function listingProgress(listing) {
  const checks = [
    listing.name?.trim(),
    listing.subtitle?.trim(),
    listing.description?.trim(),
    listing.releaseNotes?.trim(),
    CATEGORIES.includes(listing.category),
    AGE_RATINGS.includes(listing.ageRating),
    isPublicHttpsUrl(listing.privacyUrl),
    isPublicHttpsUrl(listing.supportUrl),
    listing.iconReady,
    (listing.screenshotCount ?? 0) >= 1,
    listing.privacy?.dataCollection === false,
    listing.privacy?.tracking === false,
  ];
  return Math.round((checks.filter(Boolean).length / checks.length) * 100);
}

export function canTransition(graph, from, to) {
  return graph[from]?.includes(to) ?? false;
}

export function releaseAction(state) {
  if (state === "published") return "pause";
  if (state === "paused") return "resume";
  if (["ready", "scheduled", "publish-failed"].includes(state)) return "publish";
  return null;
}

export function formatState(state) {
  return state.replaceAll("-", " ").replace(/\b\w/g, (value) => value.toUpperCase());
}

export function createDemoState() {
  return {
    viewer: {
      name: "Liubo",
      email: "liubo@example.dev",
      twoFactorEnabled: true,
    },
    team: {
      id: "team_35d07f00000000000000000000000001",
      name: "M5 Labs",
      role: "Owner",
      members: [
        { id: "member_01", name: "Liubo", email: "liubo@example.dev", role: "Owner", twoFactor: true },
        { id: "member_02", name: "Chen Yu", email: "chen@example.dev", role: "Developer", twoFactor: true },
        { id: "member_03", name: "Mira Xu", email: "mira@example.dev", role: "Release Manager", twoFactor: false },
      ],
      developerKeys: [
        {
          id: "key_01",
          name: "Release workstation",
          algorithm: "Ed25519",
          fingerprint: "596938f51fcc020b...f5ef729",
          created: "2026-07-28",
          lastUsed: "2026-08-01 01:18",
          status: "active",
        },
      ],
    },
    apps: [
      {
        id: "dev.cardputerzero.notes",
        name: "Field Notes",
        locale: "en-US",
        latestVersion: "1.2.0",
        status: "in-review",
        updated: "8 min ago",
        color: "green",
        listing: {
          name: "Field Notes",
          subtitle: "Capture structured notes in the field",
          description: "A compact field notebook for text, camera captures, and offline checklists.",
          releaseNotes: "Adds reviewed camera attachments and faster local search.",
          category: "productivity",
          ageRating: "4+",
          privacyUrl: "https://example.dev/field-notes/privacy",
          supportUrl: "https://example.dev/field-notes/support",
          locale: "en-US",
          keywords: ["notes", "field", "offline"],
          iconReady: true,
          screenshotCount: 3,
          privacy: { dataCollection: false, tracking: false, encryption: true },
        },
      },
      {
        id: "dev.cardputerzero.monitor",
        name: "Device Monitor",
        locale: "en-US",
        latestVersion: "0.9.4",
        status: "needs-changes",
        updated: "Yesterday",
        color: "yellow",
        listing: {
          name: "Device Monitor",
          subtitle: "Inspect local device resources",
          description: "Shows bounded device telemetry and diagnostic snapshots.",
          releaseNotes: "Initial review candidate.",
          category: "developer-tools",
          ageRating: "4+",
          privacyUrl: "https://example.dev/monitor/privacy",
          supportUrl: "https://example.dev/monitor/support",
          locale: "en-US",
          keywords: ["device", "diagnostics"],
          iconReady: true,
          screenshotCount: 2,
          privacy: { dataCollection: false, tracking: false, encryption: true },
        },
      },
      {
        id: "dev.cardputerzero.pixel-quest",
        name: "Pixel Quest",
        locale: "en-US",
        latestVersion: "1.0.0",
        status: "published",
        updated: "Jul 30",
        color: "red",
        listing: {
          name: "Pixel Quest",
          subtitle: "A tiny adventure for CardputerZero",
          description: "A keyboard-first pixel adventure designed for the 320x170 display.",
          releaseNotes: "Store launch release.",
          category: "games",
          ageRating: "9+",
          privacyUrl: "https://example.dev/pixel-quest/privacy",
          supportUrl: "https://example.dev/pixel-quest/support",
          locale: "en-US",
          keywords: ["game", "pixel", "adventure"],
          iconReady: true,
          screenshotCount: 4,
          privacy: { dataCollection: false, tracking: false, encryption: false },
        },
      },
    ],
    submissions: [
      {
        id: "sub_a17e6400000000000000000000000001",
        appId: "dev.cardputerzero.notes",
        appName: "Field Notes",
        version: "1.2.0",
        revision: 2,
        state: "in-review",
        created: "2026-08-01 01:18",
        digest: "2f42d960...e8530c1a",
        messages: [
          { actor: "Automated scan", body: "Package, Listing, assets, and signatures verified.", time: "01:19" },
          { actor: "Review", body: "Camera permission purpose is clear. Functional review started.", time: "01:34" },
        ],
      },
      {
        id: "sub_f81c2200000000000000000000000002",
        appId: "dev.cardputerzero.monitor",
        appName: "Device Monitor",
        version: "0.9.4",
        revision: 1,
        state: "needs-changes",
        created: "2026-07-31 20:44",
        digest: "81ad404c...477c02e8",
        messages: [
          { actor: "Review", body: "Replace the unrestricted diagnostics description with declared capability scope.", time: "21:12" },
        ],
      },
      {
        id: "sub_23aa1900000000000000000000000003",
        appId: "dev.cardputerzero.pixel-quest",
        appName: "Pixel Quest",
        version: "1.0.0",
        revision: 1,
        state: "approved",
        created: "2026-07-29 14:06",
        digest: "f5bd2081...b318bd72",
        messages: [],
      },
    ],
    releases: [
      {
        id: "rel_09ae0100000000000000000000000001",
        appId: "dev.cardputerzero.pixel-quest",
        appName: "Pixel Quest",
        version: "1.0.0",
        state: "published",
        rollout: 100,
        sequence: 18000000002,
        scheduled: null,
      },
      {
        id: "rel_221c4400000000000000000000000002",
        appId: "dev.cardputerzero.notes",
        appName: "Field Notes",
        version: "1.1.0",
        state: "paused",
        rollout: 25,
        sequence: 18000000001,
        scheduled: null,
      },
    ],
  };
}
