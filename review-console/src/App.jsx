import { useEffect, useMemo, useState } from "react";
import {
  ArrowLeft,
  BadgeCheck,
  Check,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  ClipboardCheck,
  FileCheck2,
  FileCode2,
  History,
  Image as ImageIcon,
  Inbox,
  LayoutList,
  LoaderCircle,
  LogIn,
  LogOut,
  Menu,
  MessageSquare,
  RefreshCw,
  Search,
  Send,
  ShieldCheck,
  UserCheck,
  X,
} from "lucide-react";

import { ReviewApi } from "./api.js";
import { canClaimReview, filterQueue, formatState, mapQueueItem, mapReviewDetail, validateDecision } from "./model.js";

function IconButton({ label, children, ...props }) {
  return <button className="icon-button" type="button" aria-label={label} title={label} {...props}>{children}</button>;
}

function Status({ value }) {
  const tone = value === "passed" || value === "approved"
    ? "success"
    : value === "attention" || value === "pending-secondary-review"
      ? "warning"
      : value === "rejected" || value === "needs-changes"
        ? "danger"
        : "neutral";
  return <span className={`status ${tone}`}>{formatState(value)}</span>;
}

function RiskStatus({ risk }) {
  const tone = risk.tier === "standard" ? "success" : risk.tier === "elevated" ? "warning" : "danger";
  return <span className={`status ${tone}`}>{formatState(risk.tier)} risk</span>;
}

function AssetPreview({ assets, name }) {
  const screenshot = assets.find((asset) => asset.width === 320 && asset.height === 170) ?? assets.find((asset) => asset.width > asset.height);
  return <div className="device-screen asset-screen" role="img" aria-label={`${name} submitted asset metadata`}>
    <ImageIcon />
    <strong>{screenshot?.path ?? "No landscape screenshot"}</strong>
    {screenshot && <><span>{screenshot.width} x {screenshot.height} · {screenshot.bytes} bytes</span><code>{screenshot.sha256.slice(0, 16)}...</code></>}
  </div>;
}

function QueueList({ items, selectedId, onSelect }) {
  if (!items.length) return <div className="empty"><Inbox /><strong>Queue is clear</strong><span>No submissions match this view.</span></div>;
  return <div className="queue-items">{items.map((item) => (
    <button className={`queue-item ${selectedId === item.id ? "selected" : ""}`} key={item.id} type="button" onClick={() => onSelect(item.id)}>
      <span className={`risk-dot ${item.risk.tier}`} aria-label={`${item.risk.tier} risk`} />
      <span className="queue-copy"><strong>{item.name}</strong><small>{item.version} · {item.developer}</small><em>{item.stage === "secondary" ? "Independent review" : "Primary review"} · {item.submitted}</em></span>
      {item.assignee && <span className="assigned">You</span>}
      <ChevronRight />
    </button>
  ))}</div>;
}

function SummaryTab({ item }) {
  const checks = [
    { name: "Scan report", status: item.findings.length ? "attention" : "passed", detail: `${item.scannerVersion} · ${item.scanSha}` },
    { name: "Developer key", status: item.developerKeySha ? "passed" : "attention", detail: item.developerKeySha ?? "No trusted developer key recorded" },
    { name: "Submitted assets", status: "passed", detail: `${item.assets.length} immutable descriptors bound to this revision` },
  ];
  return <div className="detail-stack">
    <section className="summary-grid">
      <AssetPreview assets={item.assets} name={item.name} />
      <dl>
        <div><dt>Developer</dt><dd>{item.developer}</dd></div>
        <div><dt>Category</dt><dd>{item.category}</dd></div>
        <div><dt>Package SHA-256</dt><dd>{item.packageSha}</dd></div>
        <div><dt>Listing SHA-256</dt><dd>{item.listingSha}</dd></div>
      </dl>
    </section>
    <section className="section-band"><header><div><h3>Automated scan</h3><p>Bound to this immutable revision.</p></div><FileCheck2 /></header><div className="check-grid">{checks.map((check) => <div key={check.name}><span className={`check-icon ${check.status}`}>{check.status === "passed" ? <Check /> : <CircleAlert />}</span><span><strong>{check.name}</strong><small>{check.detail}</small></span></div>)}</div>{item.findings.length > 0 && <div className="risk-summary"><div>{item.findings.map((finding) => <code key={finding.code}>{finding.severity}: {finding.code}</code>)}</div></div>}</section>
    <section className="section-band"><header><div><h3>Risk assessment</h3><p>Policy version {item.risk.policyVersion} · bound to the scan result.</p></div><ShieldCheck /></header><div className="risk-summary"><RiskStatus risk={item.risk} /><div>{item.risk.reasons.length ? item.risk.reasons.map((reason) => <code key={reason}>{reason}</code>) : <span>No elevated capability signals</span>}</div></div></section>
    <section className="section-band"><header><div><h3>Capabilities</h3><p>Declared permissions and exact WASM host imports.</p></div><ShieldCheck /></header><div className="capability-grid"><div><h4>Permissions</h4>{item.permissions.length ? item.permissions.map((value) => <code key={value}>{value}</code>) : <span>None</span>}</div><div><h4>Imports</h4>{item.imports.length ? item.imports.map((value) => <code key={value}>{value}</code>) : <span>None</span>}</div></div></section>
  </div>;
}

function MessagesTab({ item, reviewerId, onMessage, busy }) {
  const [body, setBody] = useState("");
  const canPost = item.assignee === reviewerId && !busy;
  return <div className="messages-view"><div className="thread">{item.messages.length ? item.messages.map((message) => <article key={message.id}><div className="avatar small">{message.actor.slice(0, 2).toUpperCase()}</div><div><header><strong>{message.actor}</strong><span>{message.role} · {message.time}</span></header><p>{message.body}</p></div></article>) : <div className="empty"><MessageSquare /><strong>No messages</strong><span>No review messages have been recorded.</span></div>}{item.messagesTruncated && <small>Earlier messages are retained in the control plane.</small>}</div><form className="message-compose" onSubmit={(event) => { event.preventDefault(); if (body.trim() && canPost) onMessage(body.trim()).then((saved) => saved && setBody("")); }}><label htmlFor="review-message">Message</label><div><textarea id="review-message" value={body} onChange={(event) => setBody(event.target.value)} maxLength={2000} rows={3} disabled={!canPost} /><button className="primary icon-command" type="submit" disabled={!body.trim() || !canPost} aria-label="Send review message" title="Send review message"><Send /></button></div></form></div>;
}

function AuditTab({ item }) {
  return <div className="audit-list">{item.audit.map((event) => <div key={event.sequence}><span><History /></span><div><strong>{formatState(event.action.replaceAll(".", "-"))}</strong><small>{event.time} · resource {event.resourceVersion} · {event.actorId}</small></div></div>)}{item.decisions.map((decision) => <div key={decision.id}><span><BadgeCheck /></span><div><strong>{formatState(decision.decision)} by {decision.reviewer}</strong><small>{decision.time}{decision.reasonCodes.length ? ` · ${decision.reasonCodes.join(", ")}` : ""}</small>{decision.note && <p>{decision.note}</p>}</div></div>)}{item.auditTruncated && <small>Earlier audit events remain in append-only storage.</small>}</div>;
}

function DecisionPanel({ item, reviewerId, onDecision, busy }) {
  const [decision, setDecision] = useState("approved");
  const [reason, setReason] = useState("");
  const [note, setNote] = useState("");
  const [errors, setErrors] = useState({});
  if (item.assignee !== reviewerId) return null;
  return <form className="decision-panel" onSubmit={(event) => { event.preventDefault(); const request = { decision, reasonCodes: reason ? [reason] : [], note }; const nextErrors = validateDecision(request); setErrors(nextErrors); if (!Object.keys(nextErrors).length) onDecision(request); }}>
    <header><div><h3>{item.stage === "primary" ? "Primary decision" : "Independent decision"}</h3><p>Recorded permanently against this assignment.</p></div><BadgeCheck /></header>
    <div className="segmented decision-options" aria-label="Review decision">{["approved", "needs-changes", "rejected"].map((value) => <button className={decision === value ? "active" : ""} type="button" key={value} disabled={busy} onClick={() => { setDecision(value); setErrors({}); }}>{formatState(value)}</button>)}</div>
    {decision !== "approved" && <label>Reason code<input value={reason} onChange={(event) => setReason(event.target.value)} placeholder="privacy-disclosure" disabled={busy} />{errors.reasonCodes && <span className="field-error">{errors.reasonCodes}</span>}</label>}
    <label>Reviewer note<textarea value={note} onChange={(event) => setNote(event.target.value)} rows={3} placeholder={decision === "approved" ? "Optional internal note" : "Required actionable detail"} disabled={busy} />{errors.note && <span className="field-error">{errors.note}</span>}</label>
    <button className={decision === "rejected" ? "danger-button" : "primary"} type="submit" disabled={busy}><ClipboardCheck /> Record {formatState(decision)}</button>
  </form>;
}

function ReviewDetail({ item, reviewerId, tab, setTab, onClaim, onDecision, onMessage, mobileBack, busy, loading }) {
  return <section className="detail-panel">
    <header className="detail-header"><IconButton label="Back to queue" onClick={mobileBack}><ArrowLeft /></IconButton><div><span className="eyebrow">{item.stage === "secondary" ? "Independent review" : "Primary review"}</span><h2>{item.name} <small>{item.version}</small></h2><p>{item.appId}</p></div><div className="header-actions"><Status value={item.state} />{!item.assignee && <button className="primary" type="button" onClick={onClaim} disabled={busy || loading}><UserCheck /> Claim review</button>}{item.assignee === reviewerId && <span className="owner-chip"><UserCheck /> Assigned to you</span>}</div></header>
    <nav className="tabs" aria-label="Submission detail">{[["summary", "Summary", FileCode2], ["messages", "Messages", MessageSquare], ["audit", "Audit", History]].map(([id, label, Icon]) => <button className={tab === id ? "active" : ""} type="button" key={id} disabled={loading} onClick={() => setTab(id)}><Icon />{label}{id === "messages" && item.messages.length > 0 && <b>{item.messages.length}</b>}</button>)}</nav>
    <div className="detail-scroll">{loading || !item.detailLoaded ? <div className="empty"><LoaderCircle className="spin" /><strong>Loading submission</strong></div> : <>{tab === "summary" && <SummaryTab item={item} />}{tab === "messages" && <MessagesTab key={item.id} item={item} reviewerId={reviewerId} onMessage={onMessage} busy={busy} />}{tab === "audit" && <AuditTab item={item} />}<DecisionPanel key={item.id} item={item} reviewerId={reviewerId} onDecision={onDecision} busy={busy} /></>}</div>
  </section>;
}

function SessionGate({ api, error }) {
  return <main className="session-gate"><ShieldCheck /><h1>Store Review</h1><p>{error || "Workforce sign-in is required."}</p><button className="primary" type="button" onClick={() => window.location.assign(api.sessionClient.loginUrl())}><LogIn /> Sign in</button></main>;
}

export default function App() {
  const api = useMemo(() => new ReviewApi({
    origin: import.meta.env.VITE_REVIEW_CONTROL_ORIGIN,
    workforceOrigin: import.meta.env.VITE_REVIEW_WORKFORCE_ORIGIN,
  }), []);
  const [session, setSession] = useState(null);
  const [items, setItems] = useState([]);
  const [nextCursor, setNextCursor] = useState(null);
  const [stage, setStage] = useState("all");
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState(null);
  const [tab, setTab] = useState("summary");
  const [navOpen, setNavOpen] = useState(false);
  const [mobileDetail, setMobileDetail] = useState(false);
  const [toast, setToast] = useState("");
  const [authError, setAuthError] = useState("");
  const [loading, setLoading] = useState(true);
  const [detailLoading, setDetailLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [queueEpoch, setQueueEpoch] = useState(0);

  const announce = (message) => { setToast(message); window.setTimeout(() => setToast(""), 2800); };
  const mapQueue = (response, principalId) => response.data.items.map((item) => mapQueueItem(item, { reviewerId: principalId }));

  useEffect(() => {
    let active = true;
    (async () => {
      try {
        const nextSession = await api.sessionClient.session();
        const response = await api.listQueue();
        if (!active) return;
        const nextItems = mapQueue(response, nextSession.principal_id);
        setSession(nextSession);
        setItems(nextItems);
        setNextCursor(response.data.next_cursor);
        setSelectedId(nextItems[0]?.id ?? null);
        setQueueEpoch((value) => value + 1);
      } catch (error) {
        if (active) setAuthError(error.message);
      } finally {
        if (active) setLoading(false);
      }
    })();
    return () => { active = false; };
  }, [api]);

  useEffect(() => {
    if (!session || !selectedId) return undefined;
    let active = true;
    setDetailLoading(true);
    api.getSubmissionDetail(selectedId).then((response) => {
      if (!active) return;
      const detail = mapReviewDetail(response.data, { etag: response.etag, reviewerId: session.principal_id });
      setItems((current) => current.map((item) => item.id === selectedId ? detail : item));
    }).catch((error) => active && announce(error.message)).finally(() => active && setDetailLoading(false));
    return () => { active = false; };
  }, [api, queueEpoch, selectedId, session]);

  const reloadQueue = async () => {
    const response = await api.listQueue();
    const nextItems = mapQueue(response, session.principal_id);
    setItems(nextItems);
    setNextCursor(response.data.next_cursor);
    setSelectedId((current) => nextItems.some((item) => item.id === current) ? current : (nextItems[0]?.id ?? null));
    setQueueEpoch((value) => value + 1);
  };

  const runMutation = async (operation, success) => {
    setBusy(true);
    try {
      await operation();
      await reloadQueue();
      announce(success);
      return true;
    } catch (error) {
      announce(error.message);
      return false;
    } finally {
      setBusy(false);
    }
  };

  if (loading) return <main className="session-gate"><LoaderCircle className="spin" /><h1>Store Review</h1></main>;
  if (!session) return <SessionGate api={api} error={authError} />;

  const reviewerId = session.principal_id;
  const visible = filterQueue(items, { stage, query, reviewer: reviewerId });
  const selected = visible.find((item) => item.id === selectedId) ?? visible[0] ?? null;
  const counts = {
    all: filterQueue(items, { reviewer: reviewerId }).length,
    primary: filterQueue(items, { stage: "primary", reviewer: reviewerId }).length,
    secondary: filterQueue(items, { stage: "secondary", reviewer: reviewerId }).length,
  };
  const selectItem = (id) => { setSelectedId(id); setTab("summary"); setMobileDetail(true); };
  const loadMore = async () => {
    setBusy(true);
    try {
      const response = await api.listQueue({ cursor: nextCursor });
      const additions = mapQueue(response, reviewerId);
      setItems((current) => [...current, ...additions.filter((addition) => !current.some((item) => item.id === addition.id))]);
      setNextCursor(response.data.next_cursor);
    } catch (error) {
      announce(error.message);
    } finally {
      setBusy(false);
    }
  };
  const shortIdentity = reviewerId.slice(-6).toUpperCase();

  return <div className="app-shell">
    <aside className={navOpen ? "sidebar open" : "sidebar"}><div className="brand"><div className="brand-mark">C0</div><div><strong>Store Review</strong><span>Internal control plane</span></div><IconButton label="Close navigation" onClick={() => setNavOpen(false)}><X /></IconButton></div><nav><button className="active" type="button"><LayoutList /><span>Review queue</span><b>{counts.all}</b></button></nav><div className="sidebar-footer"><div className="avatar">{shortIdentity.slice(0, 2)}</div><div><strong>{shortIdentity}</strong><span>{formatState(session.role)}</span></div><IconButton label="Sign out" onClick={() => api.sessionClient.logout().then(() => setSession(null)).catch((error) => announce(error.message))}><LogOut /></IconButton></div></aside>
    {navOpen && <button className="scrim" type="button" aria-label="Close navigation" onClick={() => setNavOpen(false)} />}
    <main><header className="topbar"><IconButton label="Open navigation" onClick={() => setNavOpen(true)}><Menu /></IconButton><div><strong>Review queue</strong><span>Immutable submissions awaiting a decision</span></div><IconButton label="Refresh queue" disabled={busy} onClick={() => reloadQueue().catch((error) => announce(error.message))}><RefreshCw /></IconButton></header>
      <div className={`workspace ${mobileDetail ? "show-detail" : ""}`}>
        <section className="queue-panel"><header><div><span className="eyebrow">Assignments</span><h1>Claimable reviews</h1></div></header><div className="search-box"><Search /><input aria-label="Search queue" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search app, developer, version" /></div><div className="segmented stage-filter" aria-label="Review stage">{["all", "primary", "secondary"].map((value) => <button className={stage === value ? "active" : ""} type="button" key={value} onClick={() => setStage(value)}><span>{formatState(value)}</span><b>{counts[value]}</b></button>)}</div><QueueList items={visible} selectedId={selected?.id} onSelect={selectItem} />{nextCursor && <button className="load-more" type="button" disabled={busy} onClick={loadMore}>Load more</button>}</section>
        {selected && <ReviewDetail item={selected} reviewerId={reviewerId} tab={tab} setTab={setTab} mobileBack={() => setMobileDetail(false)} busy={busy} loading={detailLoading} onClaim={() => { if (canClaimReview(selected, reviewerId)) runMutation(() => api.beginReview(selected.id, selected.etag), "Review assignment claimed."); }} onMessage={(body) => runMutation(() => api.postMessage(selected.id, body), "Message appended to the review record.")} onDecision={(request) => runMutation(() => api.decideReview(selected.id, selected.etag, request), request.decision === "approved" && selected.stage === "primary" ? "Primary approval recorded. Independent review is now required." : `${formatState(request.decision)} recorded.`)} />}
      </div>
    </main>
    {toast && <div className="toast" role="status"><Check />{toast}</div>}
  </div>;
}
