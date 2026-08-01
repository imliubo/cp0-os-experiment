import { useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  CalendarDays,
  Check,
  ChevronRight,
  Clock3,
  FileWarning,
  LayoutDashboard,
  Menu,
  Newspaper,
  Plus,
  RefreshCw,
  Save,
  Search,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";

import {
  DECISION_REASONS,
  applyDecision,
  createOperationsData,
  filterReports,
  formatCode,
  validateDecision,
  validateEditorial,
} from "./model.js";

function IconButton({ label, children, ...props }) {
  return <button className="icon-button" type="button" aria-label={label} title={label} {...props}>{children}</button>;
}

function Status({ tone = "neutral", children }) {
  return <span className={`status ${tone}`}>{children}</span>;
}

function ReleaseMark({ release, small = false }) {
  return <span className={`release-mark ${release.accent} ${small ? "small" : ""}`} aria-hidden="true">{release.name.split(" ").map((part) => part[0]).join("").slice(0, 2)}</span>;
}

function DevicePreview({ editorial, releases }) {
  const byId = new Map(releases.map((release) => [release.releaseId, release]));
  const featured = byId.get(editorial.featured_release_id);
  return <div className="device-frame" role="img" aria-label="320 by 170 Today preview">
    <div className="device-status"><span>STORE</span><span>10:24</span></div>
    <div className="device-tabs"><b>Today</b><span>Apps</span><span>Search</span><span>Updates</span></div>
    <div className="device-content">
      <small>FEATURED</small>
      <strong>{editorial.headline || "Today"}</strong>
      <div><span>{featured?.name ?? "Select a Release"}</span><em>{featured?.version ?? ""}</em></div>
      <footer>{editorial.collections.map((collection, index) => <span key={`${index}-${collection.title}`}>{collection.title || "Collection"}</span>)}</footer>
    </div>
  </div>;
}

function ReleasePicker({ releases, selected, excluded, onChange }) {
  return <select value={selected} onChange={(event) => onChange(event.target.value)}>
    <option value="">Select Release</option>
    {releases.map((release) => <option key={release.releaseId} value={release.releaseId} disabled={excluded.has(release.releaseId) && release.releaseId !== selected}>
      {release.name} {release.version}
    </option>)}
  </select>;
}

function TodayView({ data, setData, announce }) {
  const [draft, setDraft] = useState(() => structuredClone(data.editorial));
  const [errors, setErrors] = useState({});
  const selectedIds = new Set([draft.featured_release_id, ...draft.collections.flatMap((collection) => collection.release_ids)]);
  const updateCollection = (index, transform) => setDraft((current) => ({
    ...current,
    collections: current.collections.map((collection, itemIndex) => itemIndex === index ? transform(collection) : collection),
  }));
  const save = (event) => {
    event.preventDefault();
    const nextErrors = validateEditorial(draft, data.releases);
    setErrors(nextErrors);
    if (Object.keys(nextErrors).length) return;
    setData((current) => ({ ...current, editorial: { ...structuredClone(draft), resource_version: current.editorial.resource_version + 1, updated_unix_seconds: Math.floor(Date.now() / 1000) } }));
    announce("Today revision queued for Catalog rebuild");
  };
  return <div className="page-scroll">
    <header className="page-heading"><div><span className="eyebrow">Discovery</span><h1>Today editorial</h1><p>Revision {data.editorial.resource_version} · published Releases only</p></div><Status tone="success"><ShieldCheck /> Ready</Status></header>
    <form className="today-layout" onSubmit={save}>
      <section className="editor-column">
        <div className="section-heading"><div><h2>Hero</h2><p>Primary placement</p></div><Newspaper /></div>
        <label>Headline<input value={draft.headline} maxLength={48} onChange={(event) => setDraft({ ...draft, headline: event.target.value })} />{errors.headline && <span className="field-error">{errors.headline}</span>}</label>
        <label>Featured Release<ReleasePicker releases={data.releases} selected={draft.featured_release_id} excluded={selectedIds} onChange={(value) => setDraft({ ...draft, featured_release_id: value })} />{errors.featured_release_id && <span className="field-error">{errors.featured_release_id}</span>}</label>
        <div className="section-heading collections-heading"><div><h2>Collections</h2><p>One or two ordered groups</p></div><button className="secondary" type="button" disabled={draft.collections.length >= 2} onClick={() => setDraft({ ...draft, collections: [...draft.collections, { title: "", release_ids: [] }] })}><Plus /> Add</button></div>
        {draft.collections.map((collection, collectionIndex) => <fieldset className="collection-editor" key={collectionIndex}>
          <legend>Collection {collectionIndex + 1}</legend>
          <IconButton label={`Remove collection ${collectionIndex + 1}`} disabled={draft.collections.length === 1} onClick={() => setDraft({ ...draft, collections: draft.collections.filter((_, index) => index !== collectionIndex) })}><Trash2 /></IconButton>
          <label>Title<input value={collection.title} maxLength={32} onChange={(event) => updateCollection(collectionIndex, (current) => ({ ...current, title: event.target.value }))} />{errors[`collection.${collectionIndex}.title`] && <span className="field-error">{errors[`collection.${collectionIndex}.title`]}</span>}</label>
          <div className="release-fields">{[0, 1, 2, 3].map((releaseIndex) => releaseIndex < collection.release_ids.length || releaseIndex === collection.release_ids.length ? <label key={releaseIndex}>Release {releaseIndex + 1}<ReleasePicker releases={data.releases} selected={collection.release_ids[releaseIndex] ?? ""} excluded={selectedIds} onChange={(value) => updateCollection(collectionIndex, (current) => ({ ...current, release_ids: value ? [...current.release_ids.slice(0, releaseIndex), value, ...current.release_ids.slice(releaseIndex + 1)] : current.release_ids.filter((_, index) => index !== releaseIndex) }))} /></label> : null)}</div>
          {errors[`collection.${collectionIndex}.release_ids`] && <span className="field-error">{errors[`collection.${collectionIndex}.release_ids`]}</span>}
        </fieldset>)}
        <div className="form-actions"><button className="secondary" type="button" onClick={() => { setDraft(structuredClone(data.editorial)); setErrors({}); }}><RefreshCw /> Reset</button><button className="primary" type="submit"><Save /> Save revision</button></div>
      </section>
      <aside className="preview-column">
        <div className="section-heading"><div><h2>Device preview</h2><p>320 x 170</p></div><LayoutDashboard /></div>
        <DevicePreview editorial={draft} releases={data.releases} />
        <div className="release-library"><h3>Published Releases</h3>{data.releases.map((release) => <div className="release-row" key={release.releaseId}><ReleaseMark release={release} small /><span><strong>{release.name}</strong><small>{release.appId}</small></span><em>{release.version}</em></div>)}</div>
      </aside>
    </form>
  </div>;
}

function ReportList({ items, selectedId, onSelect }) {
  if (!items.length) return <div className="empty"><ShieldCheck /><strong>Queue clear</strong><span>No open reports match this view.</span></div>;
  return <div className="report-list">{items.map((report) => <button className={`report-row ${selectedId === report.reportId ? "selected" : ""}`} type="button" key={report.reportId} onClick={() => onSelect(report.reportId)}>
    <span className={`priority-dot ${report.slaClass}`} />
    <span><strong>{report.appName}</strong><small>{formatCode(report.reasonCode)} · {report.received}</small><em>{report.appId} {report.version}</em></span>
    <Status tone={report.slaClass === "security" ? "danger" : "warning"}>{report.slaClass}</Status>
    <ChevronRight />
  </button>)}</div>;
}

function DecisionForm({ report, onDecision }) {
  const [disposition, setDisposition] = useState("developer-notice");
  const [reasonCodes, setReasonCodes] = useState(["policy-violation"]);
  const [errors, setErrors] = useState({});
  const toggleReason = (reason) => setReasonCodes((current) => current.includes(reason) ? current.filter((value) => value !== reason) : current.length < 4 ? [...current, reason] : current);
  return <form className="decision-form" onSubmit={(event) => { event.preventDefault(); const request = { disposition, reason_codes: reasonCodes }; const next = validateDecision(request); setErrors(next); if (!Object.keys(next).length) onDecision(request); }}>
    <div className="section-heading"><div><h2>Disposition</h2><p>Report revision {report.resourceVersion}</p></div><ShieldAlert /></div>
    <div className="segmented disposition" aria-label="Moderation disposition">{["no-action", "developer-notice", "security-escalation"].map((value) => <button className={disposition === value ? "active" : ""} type="button" key={value} onClick={() => setDisposition(value)}>{formatCode(value)}</button>)}</div>
    {errors.disposition && <span className="field-error">{errors.disposition}</span>}
    <fieldset className="reason-picker"><legend>Reason codes</legend>{DECISION_REASONS.map((reason) => <label key={reason}><input type="checkbox" checked={reasonCodes.includes(reason)} onChange={() => toggleReason(reason)} /><span>{reasonCodes.includes(reason) && <Check />}</span>{formatCode(reason)}</label>)}</fieldset>
    {errors.reason_codes && <span className="field-error">{errors.reason_codes}</span>}
    <button className={disposition === "security-escalation" ? "danger-button" : "primary"} type="submit"><ShieldCheck /> Commit decision</button>
  </form>;
}

function ModerationView({ data, setData, announce }) {
  const [query, setQuery] = useState("");
  const [sla, setSla] = useState("all");
  const [selectedId, setSelectedId] = useState(data.reports[0]?.reportId ?? null);
  const [mobileDetail, setMobileDetail] = useState(false);
  const items = useMemo(() => filterReports(data.reports, { query, sla }), [data.reports, query, sla]);
  const selected = items.find((report) => report.reportId === selectedId) ?? items[0] ?? null;
  const choose = (id) => { setSelectedId(id); setMobileDetail(true); };
  const decide = (request) => {
    const appName = selected.appName;
    setData((current) => ({ ...current, reports: applyDecision(current.reports, selected.reportId, request) }));
    setMobileDetail(false);
    announce(`${appName}: ${formatCode(request.disposition)}`);
  };
  return <div className={`moderation-workspace ${mobileDetail ? "show-detail" : ""}`}>
    <section className="report-panel"><header><div><span className="eyebrow">Safety operations</span><h1>Moderation queue</h1></div><Status tone={items.some((item) => item.slaClass === "security") ? "danger" : "neutral"}><AlertTriangle /> {items.length}</Status></header><div className="search-box"><Search /><input aria-label="Search reports" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search app, reason, version" /></div><div className="segmented queue-filter">{["all", "security", "standard"].map((value) => <button className={sla === value ? "active" : ""} type="button" key={value} onClick={() => setSla(value)}>{formatCode(value)}</button>)}</div><ReportList items={items} selectedId={selected?.reportId} onSelect={choose} /></section>
    <section className="report-detail">{selected ? <>
      <header><IconButton label="Back to moderation queue" onClick={() => setMobileDetail(false)}><ArrowLeft /></IconButton><div><span className="eyebrow">{formatCode(selected.reasonCode)}</span><h2>{selected.appName} <small>{selected.version}</small></h2><p>{selected.reportId}</p></div><Status tone={selected.slaClass === "security" ? "danger" : "warning"}>{selected.slaClass}</Status></header>
      <div className="detail-scroll"><section className="report-summary"><div><FileWarning /><span><small>App ID</small><strong>{selected.appId}</strong></span></div><div><CalendarDays /><span><small>Release</small><strong>{selected.releaseId}</strong></span></div><div><Clock3 /><span><small>Acknowledge by</small><strong>{new Date(selected.acknowledgementDue * 1000).toLocaleString()}</strong></span></div><div><ShieldAlert /><span><small>Resolve by</small><strong>{new Date(selected.resolutionDue * 1000).toLocaleString()}</strong></span></div></section><DecisionForm key={selected.reportId} report={selected} onDecision={decide} /></div>
    </> : <div className="empty"><ShieldCheck /><strong>Queue clear</strong><span>No report is selected.</span></div>}</section>
  </div>;
}

export default function App() {
  const [data, setData] = useState(createOperationsData);
  const [view, setView] = useState("today");
  const [navOpen, setNavOpen] = useState(false);
  const [toast, setToast] = useState("");
  const openReports = filterReports(data.reports).length;
  const announce = (message) => { setToast(message); window.setTimeout(() => setToast(""), 2800); };
  const navigate = (target) => { setView(target); setNavOpen(false); };
  return <div className="app-shell">
    <aside className={navOpen ? "sidebar open" : "sidebar"}><div className="brand"><div className="brand-mark">C0</div><div><strong>Store Operations</strong><span>Publishing control plane</span></div><IconButton label="Close navigation" onClick={() => setNavOpen(false)}><X /></IconButton></div><nav><button className={view === "today" ? "active" : ""} type="button" onClick={() => navigate("today")}><Newspaper /><span>Today</span></button><button className={view === "moderation" ? "active" : ""} type="button" onClick={() => navigate("moderation")}><ShieldAlert /><span>Moderation</span><b>{openReports}</b></button></nav><div className="sidebar-footer"><span className="operator-avatar">MS</span><span><strong>Mina Seo</strong><small>Store operator</small></span></div></aside>
    {navOpen && <button className="scrim" type="button" aria-label="Close navigation" onClick={() => setNavOpen(false)} />}
    <main><header className="topbar"><IconButton label="Open navigation" onClick={() => setNavOpen(true)}><Menu /></IconButton><div><strong>{view === "today" ? "Today editorial" : "Moderation"}</strong><span>Production preview · 2FA verified</span></div><Status tone="success"><ShieldCheck /> Active</Status></header>{view === "today" ? <TodayView data={data} setData={setData} announce={announce} /> : <ModerationView data={data} setData={setData} announce={announce} />}</main>
    {toast && <div className="toast" role="status"><Check />{toast}</div>}
  </div>;
}
