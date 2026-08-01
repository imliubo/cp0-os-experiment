export const REVIEW_STAGES = Object.freeze(["all", "primary", "secondary"]);
export const DECISIONS = Object.freeze(["approved", "needs-changes", "rejected"]);

export function formatState(value) {
  return value.replaceAll("-", " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

export function canClaimReview(item, reviewer) {
  if (!reviewer || item.assignee || !["ready-for-review", "pending-secondary-review"].includes(item.state)) return false;
  return item.stage !== "secondary" || item.primaryReviewer !== reviewer;
}

export function filterQueue(items, { stage = "all", query = "", reviewer = null } = {}) {
  const normalized = query.trim().toLowerCase();
  return items.filter((item) => {
    const stageMatch = stage === "all" || item.stage === stage;
    const queryMatch = !normalized || [item.name, item.appId, item.version, item.developer]
      .some((value) => value.toLowerCase().includes(normalized));
    const independentlyClaimable = !reviewer || item.assignee === reviewer || canClaimReview(item, reviewer);
    return stageMatch && queryMatch && independentlyClaimable && !["approved", "needs-changes", "rejected"].includes(item.state);
  });
}

export function validateDecision({ decision, reasonCodes, note }) {
  const errors = {};
  if (!DECISIONS.includes(decision)) errors.decision = "Select a valid decision.";
  const codes = Array.isArray(reasonCodes) ? reasonCodes.map((code) => typeof code === "string" ? code.trim() : "").filter(Boolean) : [];
  if (new Set(codes).size !== codes.length || codes.some((code) => !/^[a-z][a-z0-9-]{0,63}$/.test(code))) {
    errors.reasonCodes = "Reason codes must be unique lowercase identifiers.";
  }
  if (decision !== "approved" && codes.length === 0) errors.reasonCodes = "A reason code is required.";
  if (typeof note !== "string" || note.trim() !== note || note.length > 2000 || (decision !== "approved" && note.length === 0)) {
    errors.note = "Add a trimmed actionable note of at most 2,000 characters.";
  }
  return errors;
}

export function applyDecision(item, decision) {
  if (item.state !== "in-review" || !item.assignee) throw new Error("Review must be claimed first");
  if (decision === "approved" && item.stage === "primary") {
    return { ...item, state: "pending-secondary-review", stage: "secondary", primaryReviewer: item.assignee, assignee: null };
  }
  if (item.stage === "secondary" && item.primaryReviewer === item.assignee) throw new Error("Independent reviewer must differ from primary reviewer");
  return { ...item, state: decision, assignee: null };
}

export function createReviewData() {
  return [
    {
      id: "sub_8a14c077cf944d879842dfeee45f3670",
      etag: '"7"',
      appId: "dev.cardputerzero.fieldnotes",
      name: "Field Notes",
      version: "2.4.0",
      developer: "Northstar Devices",
      submitted: "18 min ago",
      stage: "secondary",
      state: "pending-secondary-review",
      risk: "elevated",
      category: "Productivity",
      assignee: null,
      primaryReviewer: "Maya Chen",
      packageSha: "f01c7d9a41dd...8d31",
      listingSha: "26ca5bd2408a...3b92",
      permissions: ["camera.capture", "storage.private", "system.notify"],
      imports: ["cp0:camera/capture", "cp0:storage/read", "cp0:storage/write", "cp0:system/notify"],
      checks: [
        { name: "Package integrity", status: "passed", detail: "Developer signature and 18 entries verified" },
        { name: "Capability match", status: "attention", detail: "Camera purpose needs independent confirmation" },
        { name: "Listing assets", status: "passed", detail: "Icon and 3 screenshots validated" },
        { name: "Static behavior", status: "passed", detail: "No undeclared imports or ambient network access" },
      ],
      messages: [
        { actor: "Maya Chen", role: "Primary reviewer", time: "09:42", body: "Camera capture is user initiated and the disclosure matches the runtime prompt." },
        { actor: "Northstar Devices", role: "Developer", time: "09:18", body: "The retention note now states that images remain in app-private storage until deleted." },
      ],
      audit: ["Automated scan passed", "Primary review approved", "Queued for independent review"],
      screen: "notes",
    },
    {
      id: "sub_b84c3a9951f04b24815e8939ac1bbaca",
      etag: '"3"',
      appId: "dev.cardputerzero.signallab",
      name: "Signal Lab",
      version: "1.1.0",
      developer: "Open Bench Works",
      submitted: "31 min ago",
      stage: "primary",
      state: "ready-for-review",
      risk: "elevated",
      category: "Developer Tools",
      assignee: null,
      packageSha: "c8b6e56f9570...8432",
      listingSha: "ab09aeff3f31...ac11",
      permissions: ["radio.lora", "gpio.access", "storage.private"],
      imports: ["cp0:radio/send", "cp0:radio/receive", "cp0:gpio/read", "cp0:storage/write"],
      checks: [
        { name: "Package integrity", status: "passed", detail: "Developer signature and 14 entries verified" },
        { name: "Capability match", status: "attention", detail: "LoRa region declaration requires review" },
        { name: "Listing assets", status: "passed", detail: "Icon and 2 screenshots validated" },
        { name: "Static behavior", status: "passed", detail: "All host imports are declared" },
      ],
      messages: [],
      audit: ["Automated scan passed", "Entered primary queue"],
      screen: "signal",
    },
    {
      id: "sub_31f13a098c04468a9ac0317970f27528",
      etag: '"5"',
      appId: "dev.cardputerzero.pocketcalc",
      name: "Pocket Calc",
      version: "3.0.1",
      developer: "Little Byte Studio",
      submitted: "1 hr ago",
      stage: "primary",
      state: "in-review",
      risk: "standard",
      category: "Utilities",
      assignee: "Liang Bo",
      packageSha: "3c3e74c451aa...0fa2",
      listingSha: "dd6703c49acc...7ed8",
      permissions: ["storage.private"],
      imports: ["cp0:storage/read", "cp0:storage/write"],
      checks: [
        { name: "Package integrity", status: "passed", detail: "Developer signature and 8 entries verified" },
        { name: "Capability match", status: "passed", detail: "One declared private-storage capability" },
        { name: "Listing assets", status: "passed", detail: "Icon and 2 screenshots validated" },
        { name: "Static behavior", status: "passed", detail: "Deterministic scan completed" },
      ],
      messages: [{ actor: "Liang Bo", role: "Primary reviewer", time: "08:57", body: "Functional pass is complete; checking localized release notes." }],
      audit: ["Automated scan passed", "Primary review claimed by Liang Bo"],
      screen: "calc",
    },
    {
      id: "sub_2f8e4e2b97764ce19ad3e5fe06347895",
      etag: '"4"',
      appId: "dev.cardputerzero.snake",
      name: "Neon Snake",
      version: "1.3.0",
      developer: "Pixel Current",
      submitted: "2 hr ago",
      stage: "secondary",
      state: "pending-secondary-review",
      risk: "standard",
      category: "Games",
      assignee: null,
      primaryReviewer: "Rina Park",
      packageSha: "21b010a032e4...9fc0",
      listingSha: "673e7dc80e5b...ddc1",
      permissions: ["system.notify"],
      imports: ["cp0:system/notify"],
      checks: [
        { name: "Package integrity", status: "passed", detail: "Developer signature and 11 entries verified" },
        { name: "Capability match", status: "passed", detail: "Notification only after score milestones" },
        { name: "Listing assets", status: "passed", detail: "Icon and 4 screenshots validated" },
        { name: "Static behavior", status: "passed", detail: "No network, radio, camera or GPIO imports" },
      ],
      messages: [{ actor: "Rina Park", role: "Primary reviewer", time: "07:45", body: "Gameplay and age rating are consistent. Primary approval recorded." }],
      audit: ["Automated scan passed", "Primary review approved", "Queued for independent review"],
      screen: "snake",
    },
  ];
}
