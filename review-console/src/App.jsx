import { useMemo, useState } from "react";
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
  Menu,
  MessageSquare,
  Search,
  Send,
  ShieldCheck,
  UserCheck,
  X,
} from "lucide-react";

import { applyDecision, canClaimReview, createReviewData, filterQueue, formatState, validateDecision } from "./model.js";

const CURRENT_REVIEWER = "Liang Bo";

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

function ScreenPreview({ variant }) {
  if (variant === "signal") {
    return <div className="device-screen signal-screen" role="img" aria-label="Signal Lab submitted screenshot"><div className="screen-bar"><span>Signal Lab</span><b>915.0</b></div><div className="signal-plot"><i /><i /><i /><i /><i /><i /></div><div className="screen-footer"><span>RSSI -71</span><span>SF 7</span><span>RX</span></div></div>;
  }
  if (variant === "calc") {
    return <div className="device-screen calc-screen" role="img" aria-label="Pocket Calc submitted screenshot"><div className="calc-display"><small>128 x 24</small><strong>3,072</strong></div><div className="calc-keys">{["7", "8", "9", "+", "4", "5", "6", "-", "1", "2", "3", "="].map((key) => <i key={key}>{key}</i>)}</div></div>;
  }
  if (variant === "snake") {
    return <div className="device-screen snake-screen" role="img" aria-label="Neon Snake submitted screenshot"><div className="snake-score">SCORE 01840</div><div className="snake-board"><i className="food" /><i className="snake a" /><i className="snake b" /><i className="snake c" /><i className="snake d" /></div></div>;
  }
  return <div className="device-screen notes-screen" role="img" aria-label="Field Notes submitted screenshot"><div className="screen-bar"><span>Field Notes</span><b>12:08</b></div><div className="note-lines"><strong>Site inspection</strong><i /><i /><i /><i /></div><div className="camera-chip"><ImageIcon /> Add photo</div></div>;
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
  return <div className="detail-stack">
    <section className="summary-grid">
      <ScreenPreview variant={item.screen} />
      <dl>
        <div><dt>Developer</dt><dd>{item.developer}</dd></div>
        <div><dt>Category</dt><dd>{item.category}</dd></div>
        <div><dt>Package SHA-256</dt><dd>{item.packageSha}</dd></div>
        <div><dt>Listing SHA-256</dt><dd>{item.listingSha}</dd></div>
      </dl>
    </section>
    <section className="section-band"><header><div><h3>Automated checks</h3><p>Bound to this immutable revision.</p></div><FileCheck2 /></header><div className="check-grid">{item.checks.map((check) => <div key={check.name}><span className={`check-icon ${check.status}`}>{check.status === "passed" ? <Check /> : <CircleAlert />}</span><span><strong>{check.name}</strong><small>{check.detail}</small></span></div>)}</div></section>
    <section className="section-band"><header><div><h3>Risk assessment</h3><p>Policy version {item.risk.policyVersion} · bound to the scan result.</p></div><ShieldCheck /></header><div className="risk-summary"><RiskStatus risk={item.risk} /><div>{item.risk.reasons.length ? item.risk.reasons.map((reason) => <code key={reason}>{reason}</code>) : <span>No elevated capability signals</span>}</div></div></section>
    <section className="section-band"><header><div><h3>Capabilities</h3><p>Declared permissions and exact WASM host imports.</p></div><ShieldCheck /></header><div className="capability-grid"><div><h4>Permissions</h4>{item.permissions.map((value) => <code key={value}>{value}</code>)}</div><div><h4>Imports</h4>{item.imports.map((value) => <code key={value}>{value}</code>)}</div></div></section>
  </div>;
}

function MessagesTab({ item, onMessage }) {
  const [body, setBody] = useState("");
  return <div className="messages-view"><div className="thread">{item.messages.length ? item.messages.map((message, index) => <article key={`${message.time}-${index}`}><div className="avatar small">{message.actor.split(" ").map((part) => part[0]).join("")}</div><div><header><strong>{message.actor}</strong><span>{message.role} · {message.time}</span></header><p>{message.body}</p></div></article>) : <div className="empty"><MessageSquare /><strong>No messages</strong><span>Start a revision-bound review thread after claiming.</span></div>}</div><form className="message-compose" onSubmit={(event) => { event.preventDefault(); if (body.trim() && item.assignee === CURRENT_REVIEWER) { onMessage(body.trim()); setBody(""); } }}><label htmlFor="review-message">Message</label><div><textarea id="review-message" value={body} onChange={(event) => setBody(event.target.value)} maxLength={2000} rows={3} disabled={item.assignee !== CURRENT_REVIEWER} /><button className="primary icon-command" type="submit" disabled={!body.trim() || item.assignee !== CURRENT_REVIEWER} aria-label="Send review message" title="Send review message"><Send /></button></div></form></div>;
}

function AuditTab({ item }) {
  return <div className="audit-list">{item.audit.map((event, index) => <div key={event}><span><History /></span><div><strong>{event}</strong><small>Revision {item.version} · event {index + 1}</small></div></div>)}</div>;
}

function DecisionPanel({ item, onDecision }) {
  const [decision, setDecision] = useState("approved");
  const [reason, setReason] = useState("");
  const [note, setNote] = useState("");
  const [errors, setErrors] = useState({});
  if (item.assignee !== CURRENT_REVIEWER) return null;
  return <form className="decision-panel" onSubmit={(event) => { event.preventDefault(); const request = { decision, reasonCodes: reason ? [reason] : [], note }; const nextErrors = validateDecision(request); setErrors(nextErrors); if (!Object.keys(nextErrors).length) onDecision(request); }}>
    <header><div><h3>{item.stage === "primary" ? "Primary decision" : "Independent decision"}</h3><p>Recorded permanently against this assignment.</p></div><BadgeCheck /></header>
    <div className="segmented decision-options" aria-label="Review decision">{["approved", "needs-changes", "rejected"].map((value) => <button className={decision === value ? "active" : ""} type="button" key={value} onClick={() => { setDecision(value); setErrors({}); }}>{formatState(value)}</button>)}</div>
    {decision !== "approved" && <label>Reason code<input value={reason} onChange={(event) => setReason(event.target.value)} placeholder="privacy-disclosure" />{errors.reasonCodes && <span className="field-error">{errors.reasonCodes}</span>}</label>}
    <label>Reviewer note<textarea value={note} onChange={(event) => setNote(event.target.value)} rows={3} placeholder={decision === "approved" ? "Optional internal note" : "Required actionable detail"} />{errors.note && <span className="field-error">{errors.note}</span>}</label>
    <button className={decision === "rejected" ? "danger-button" : "primary"} type="submit"><ClipboardCheck /> Record {formatState(decision)}</button>
  </form>;
}

function ReviewDetail({ item, tab, setTab, onClaim, onDecision, onMessage, mobileBack }) {
  return <section className="detail-panel">
    <header className="detail-header"><IconButton label="Back to queue" onClick={mobileBack}><ArrowLeft /></IconButton><div><span className="eyebrow">{item.stage === "secondary" ? "Independent review" : "Primary review"}</span><h2>{item.name} <small>{item.version}</small></h2><p>{item.appId}</p></div><div className="header-actions"><Status value={item.state} />{!item.assignee && <button className="primary" type="button" onClick={onClaim}><UserCheck /> Claim review</button>}{item.assignee === CURRENT_REVIEWER && <span className="owner-chip"><UserCheck /> Assigned to you</span>}</div></header>
    <nav className="tabs" aria-label="Submission detail">{[["summary", "Summary", FileCode2], ["messages", "Messages", MessageSquare], ["audit", "Audit", History]].map(([id, label, Icon]) => <button className={tab === id ? "active" : ""} type="button" key={id} onClick={() => setTab(id)}><Icon />{label}{id === "messages" && item.messages.length > 0 && <b>{item.messages.length}</b>}</button>)}</nav>
    <div className="detail-scroll">{tab === "summary" && <SummaryTab item={item} />}{tab === "messages" && <MessagesTab key={item.id} item={item} onMessage={onMessage} />}{tab === "audit" && <AuditTab item={item} />}<DecisionPanel key={item.id} item={item} onDecision={onDecision} /></div>
  </section>;
}

export default function App() {
  const [items, setItems] = useState(createReviewData);
  const [stage, setStage] = useState("all");
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState(items[0].id);
  const [tab, setTab] = useState("summary");
  const [navOpen, setNavOpen] = useState(false);
  const [mobileDetail, setMobileDetail] = useState(false);
  const [toast, setToast] = useState("");
  const visible = useMemo(() => filterQueue(items, { stage, query, reviewer: CURRENT_REVIEWER }), [items, stage, query]);
  const selected = visible.find((item) => item.id === selectedId) ?? visible[0] ?? null;
  const counts = {
    all: filterQueue(items, { reviewer: CURRENT_REVIEWER }).length,
    primary: filterQueue(items, { stage: "primary", reviewer: CURRENT_REVIEWER }).length,
    secondary: filterQueue(items, { stage: "secondary", reviewer: CURRENT_REVIEWER }).length,
  };
  const selectItem = (id) => { setSelectedId(id); setTab("summary"); setMobileDetail(true); };
  const updateSelected = (transform) => setItems((current) => current.map((item) => item.id === selected.id ? transform(item) : item));
  const announce = (message) => { setToast(message); window.setTimeout(() => setToast(""), 2800); };
  return <div className="app-shell">
    <aside className={navOpen ? "sidebar open" : "sidebar"}><div className="brand"><div className="brand-mark">C0</div><div><strong>Store Review</strong><span>Internal control plane</span></div><IconButton label="Close navigation" onClick={() => setNavOpen(false)}><X /></IconButton></div><nav><button className="active" type="button"><LayoutList /><span>Review queue</span><b>{counts.all}</b></button></nav><div className="sidebar-footer"><div className="avatar">LB</div><div><strong>{CURRENT_REVIEWER}</strong><span>Senior reviewer</span></div><ChevronDown /></div></aside>
    {navOpen && <button className="scrim" type="button" aria-label="Close navigation" onClick={() => setNavOpen(false)} />}
    <main><header className="topbar"><IconButton label="Open navigation" onClick={() => setNavOpen(true)}><Menu /></IconButton><div><strong>Review queue</strong><span>Immutable submissions awaiting a decision</span></div></header>
      <div className={`workspace ${mobileDetail ? "show-detail" : ""}`}>
        <section className="queue-panel"><header><div><span className="eyebrow">Assignments</span><h1>Claimable reviews</h1></div></header><div className="search-box"><Search /><input aria-label="Search queue" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search app, developer, version" /></div><div className="segmented stage-filter" aria-label="Review stage">{["all", "primary", "secondary"].map((value) => <button className={stage === value ? "active" : ""} type="button" key={value} onClick={() => setStage(value)}><span>{formatState(value)}</span><b>{counts[value]}</b></button>)}</div><QueueList items={visible} selectedId={selected?.id} onSelect={selectItem} /></section>
        {selected && <ReviewDetail item={selected} tab={tab} setTab={setTab} mobileBack={() => setMobileDetail(false)} onClaim={() => { if (!canClaimReview(selected, CURRENT_REVIEWER)) return; updateSelected((item) => ({ ...item, state: "in-review", assignee: CURRENT_REVIEWER, etag: `"${Number(item.etag.replaceAll('"', '')) + 1}"` })); announce("Review assignment claimed."); }} onMessage={(body) => { updateSelected((item) => ({ ...item, messages: [...item.messages, { actor: CURRENT_REVIEWER, role: item.stage === "primary" ? "Primary reviewer" : "Independent reviewer", time: "Now", body }] })); announce("Message appended to the review record."); }} onDecision={(request) => { updateSelected((item) => applyDecision(item, request.decision)); announce(request.decision === "approved" && selected.stage === "primary" ? "Primary approval recorded. Independent review is now required." : `${formatState(request.decision)} recorded.`); }} />}
      </div>
    </main>
    {toast && <div className="toast" role="status"><Check />{toast}</div>}
  </div>;
}
