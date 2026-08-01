import { useEffect, useMemo, useRef, useState } from "react";
import {
  AppWindow,
  ArrowLeft,
  BadgeCheck,
  Bell,
  CalendarClock,
  Check,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  Clock3,
  FileArchive,
  FileCheck2,
  Gauge,
  Image as ImageIcon,
  KeyRound,
  LayoutDashboard,
  Menu,
  Pause,
  Play,
  Plus,
  Rocket,
  Save,
  Search,
  Send,
  ShieldCheck,
  Trash2,
  UploadCloud,
  UserRoundCog,
  UsersRound,
  X,
} from "lucide-react";

import {
  AGE_RATINGS,
  CATEGORIES,
  createDemoState,
  formatState,
  listingProgress,
  releaseAction,
  validateAppDraft,
  validateListing,
} from "./model.js";

const NAV_ITEMS = [
  { id: "overview", label: "Overview", icon: LayoutDashboard },
  { id: "apps", label: "Apps", icon: AppWindow },
  { id: "submissions", label: "Submissions", icon: FileArchive },
  { id: "releases", label: "Releases", icon: Rocket },
  { id: "team", label: "Team & access", icon: UsersRound },
];

const PAGE_TITLES = {
  overview: ["Overview", "Submission health and release activity across your team."],
  apps: ["Apps", "Permanent identifiers, Store listings, assets, and privacy declarations."],
  submissions: ["Submissions", "Immutable revisions, automated checks, and review messages."],
  releases: ["Releases", "Schedule and control approved versions in the Store catalog."],
  team: ["Team & access", "Roles, two-factor authentication, and public developer keys."],
};

function Status({ value }) {
  const tone = ["approved", "published", "active", "ready"].includes(value)
    ? "success"
    : ["needs-changes", "paused", "scheduled", "suspended"].includes(value)
      ? "warning"
      : ["rejected", "removed", "publish-failed"].includes(value)
        ? "danger"
        : "neutral";
  return <span className={`status status-${tone}`}>{formatState(value)}</span>;
}

function IconButton({ label, children, className = "", ...props }) {
  return (
    <button className={`icon-button ${className}`.trim()} type="button" aria-label={label} title={label} {...props}>
      {children}
    </button>
  );
}

function EmptyState({ icon: Icon, title, body, action }) {
  return (
    <div className="empty-state">
      <Icon aria-hidden="true" />
      <h3>{title}</h3>
      <p>{body}</p>
      {action}
    </div>
  );
}

function Overview({ data, navigate }) {
  const needsAttention = data.submissions.filter((item) => item.state === "needs-changes");
  const activeReleases = data.releases.filter((item) => item.state !== "removed");
  return (
    <div className="page-stack">
      <section className="metric-grid" aria-label="Team summary">
        <article className="metric">
          <div className="metric-icon green"><AppWindow /></div>
          <div><span>Registered apps</span><strong>{data.apps.length}</strong><small>{data.apps.filter((app) => app.status === "published").length} live in Store</small></div>
        </article>
        <article className="metric">
          <div className="metric-icon amber"><Clock3 /></div>
          <div><span>In review</span><strong>{data.submissions.filter((item) => ["in-review", "pending-secondary-review"].includes(item.state)).length}</strong><small>{needsAttention.length} needs attention</small></div>
        </article>
        <article className="metric">
          <div className="metric-icon blue"><Gauge /></div>
          <div><span>Active releases</span><strong>{activeReleases.length}</strong><small>Latest catalog #{Math.max(...data.releases.map((item) => item.sequence))}</small></div>
        </article>
        <article className="metric">
          <div className="metric-icon red"><ShieldCheck /></div>
          <div><span>Security</span><strong>{data.team.members.filter((member) => member.twoFactor).length}/{data.team.members.length}</strong><small>members use 2FA</small></div>
        </article>
      </section>

      <div className="content-grid">
        <section className="panel span-2">
          <div className="panel-heading">
            <div><h2>Submission activity</h2><p>Latest immutable revisions</p></div>
            <button className="text-button" type="button" onClick={() => navigate("submissions")}>View all <ChevronRight /></button>
          </div>
          <div className="table-wrap">
            <table>
              <thead><tr><th>App</th><th>Version</th><th>Status</th><th>Submitted</th></tr></thead>
              <tbody>{data.submissions.map((item) => (
                <tr key={item.id}>
                  <td><strong>{item.appName}</strong><small>{item.appId}</small></td>
                  <td>{item.version} <span className="muted">rev {item.revision}</span></td>
                  <td><Status value={item.state} /></td>
                  <td>{item.created}</td>
                </tr>
              ))}</tbody>
            </table>
          </div>
        </section>
        <section className="panel">
          <div className="panel-heading"><div><h2>Access readiness</h2><p>Production safeguards</p></div></div>
          <div className="check-list">
            <div><Check /><span><strong>Owner 2FA enabled</strong><small>{data.viewer.email}</small></span></div>
            <div><Check /><span><strong>Developer key active</strong><small>{data.team.developerKeys.length} registered public key</small></span></div>
            <div className="attention"><CircleAlert /><span><strong>Member action required</strong><small>1 member has not enabled 2FA</small></span></div>
          </div>
          <button className="secondary full" type="button" onClick={() => navigate("team")}>Review team access</button>
        </section>
      </div>
    </div>
  );
}

function AppMark({ app, size = "normal" }) {
  return <span className={`app-mark ${app.color} ${size}`} aria-hidden="true">{app.name.slice(0, 2).toUpperCase()}</span>;
}

function CreateAppDialog({ open, onClose, onCreate }) {
  const [appId, setAppId] = useState("dev.cardputerzero.");
  const [locale, setLocale] = useState("en-US");
  const [confirmed, setConfirmed] = useState(false);
  const [submitted, setSubmitted] = useState(false);
  const errors = submitted ? validateAppDraft({ appId, defaultLocale: locale }) : {};
  const valid = Object.keys(validateAppDraft({ appId, defaultLocale: locale })).length === 0 && confirmed;
  useEffect(() => {
    if (!open) {
      setAppId("dev.cardputerzero.");
      setLocale("en-US");
      setConfirmed(false);
      setSubmitted(false);
    }
  }, [open]);
  useEffect(() => {
    if (!open) return undefined;
    const closeOnEscape = (event) => { if (event.key === "Escape") onClose(); };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [open, onClose]);
  if (!open) return null;
  const submit = (event) => {
    event.preventDefault();
    setSubmitted(true);
    if (valid) onCreate(appId, locale);
  };
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className="dialog" role="dialog" aria-modal="true" aria-labelledby="create-app-title">
        <div className="dialog-heading"><div><h2 id="create-app-title">Register App ID</h2><p>The identifier permanently belongs to {"M5 Labs"}.</p></div><IconButton label="Close" onClick={onClose}><X /></IconButton></div>
        <form onSubmit={submit} className="form-stack">
          <label>App ID<input value={appId} onChange={(event) => setAppId(event.target.value)} autoFocus spellCheck="false" aria-invalid={Boolean(errors.appId)} /><small>Lowercase DNS-style identifier with at least three segments.</small>{errors.appId && <span className="field-error">{errors.appId}</span>}</label>
          <label>Default locale<select value={locale} onChange={(event) => setLocale(event.target.value)}><option>en-US</option><option>zh-CN</option><option>ja-JP</option></select>{errors.defaultLocale && <span className="field-error">{errors.defaultLocale}</span>}</label>
          <div className="availability"><BadgeCheck /><span><strong>Local format and name check</strong><small>The server performs the authoritative ownership check on creation.</small></span></div>
          <label className="check-control"><input type="checkbox" checked={confirmed} onChange={(event) => setConfirmed(event.target.checked)} /><span>I understand that this App ID cannot be renamed, recycled, or silently transferred.</span></label>
          <div className="dialog-actions"><button type="button" className="secondary" onClick={onClose}>Cancel</button><button type="submit" className="primary" disabled={!valid}>Create App ID</button></div>
        </form>
      </section>
    </div>
  );
}

function Apps({ data, setData, toast, focusedId }) {
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState(data.apps[0]?.id);
  const [createOpen, setCreateOpen] = useState(false);
  const [tab, setTab] = useState("listing");
  useEffect(() => { if (focusedId && data.apps.some((app) => app.id === focusedId)) setSelectedId(focusedId); }, [data.apps, focusedId]);
  const filtered = data.apps.filter((app) => `${app.name} ${app.id}`.toLowerCase().includes(query.toLowerCase()));
  const selected = data.apps.find((app) => app.id === selectedId) ?? filtered[0];
  const setListing = (patch) => setData((current) => ({
    ...current,
    apps: current.apps.map((app) => app.id === selected.id ? { ...app, listing: { ...app.listing, ...patch } } : app),
  }));
  const createApp = (id, locale) => {
    const name = id.split(".").at(-1).replaceAll("-", " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
    const app = {
      id, name, locale, latestVersion: "No version", status: "draft", updated: "Now", color: "blue",
      listing: { name, subtitle: "", description: "", releaseNotes: "", category: "utilities", ageRating: "4+", privacyUrl: "", supportUrl: "", locale, keywords: [], iconReady: false, screenshotCount: 0, privacy: { dataCollection: false, tracking: false, encryption: false } },
    };
    setData((current) => ({ ...current, apps: [app, ...current.apps] }));
    setSelectedId(id);
    setCreateOpen(false);
    toast("App ID registered. Ownership is now permanent.");
  };
  return (
    <>
      <div className="master-detail">
        <section className="panel master-list">
          <div className="list-toolbar"><label className="search-box"><Search /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search apps" aria-label="Search apps" /></label><IconButton label="Register App ID" onClick={() => setCreateOpen(true)}><Plus /></IconButton></div>
          <div className="item-list" role="list">{filtered.map((app) => (
            <button type="button" className={`list-item ${selected?.id === app.id ? "selected" : ""}`} key={app.id} onClick={() => setSelectedId(app.id)}>
              <AppMark app={app} /><span><strong>{app.name}</strong><small>{app.id}</small></span><Status value={app.status} /><ChevronRight />
            </button>
          ))}</div>
          {!filtered.length && <EmptyState icon={Search} title="No apps found" body="Try another name or identifier." />}
        </section>
        {selected ? <section className="panel detail-panel">
          <div className="detail-title"><AppMark app={selected} size="large" /><div><span className="eyebrow">{selected.id}</span><h2>{selected.name}</h2><p>{listingProgress(selected.listing)}% listing complete</p></div><button className="primary save-button" type="button" onClick={() => toast("Draft saved locally. No submission revision was changed.")}><Save /> Save draft</button></div>
          <div className="progress-track" aria-label={`${listingProgress(selected.listing)} percent complete`}><span style={{ width: `${listingProgress(selected.listing)}%` }} /></div>
          <div className="tabs" role="tablist">
            {[{ id: "listing", label: "Listing" }, { id: "assets", label: "Assets" }, { id: "privacy", label: "Privacy" }].map((item) => <button type="button" role="tab" aria-selected={tab === item.id} className={tab === item.id ? "active" : ""} key={item.id} onClick={() => setTab(item.id)}>{item.label}</button>)}
          </div>
          {tab === "listing" && <ListingEditor listing={selected.listing} update={setListing} />}
          {tab === "assets" && <AssetsEditor listing={selected.listing} update={setListing} />}
          {tab === "privacy" && <PrivacyEditor listing={selected.listing} update={setListing} />}
        </section> : <section className="panel"><EmptyState icon={AppWindow} title="Select an app" body="Choose an app to edit its Store presence." /></section>}
      </div>
      <CreateAppDialog open={createOpen} onClose={() => setCreateOpen(false)} onCreate={createApp} />
    </>
  );
}

function ListingEditor({ listing, update }) {
  const errors = validateListing(listing);
  return (
    <div className="editor-grid">
      <label>Display name <span>{listing.name.length}/32</span><input value={listing.name} maxLength="32" onChange={(event) => update({ name: event.target.value })} />{errors.name && <small className="field-error">{errors.name}</small>}</label>
      <label>Default locale<select value={listing.locale} onChange={(event) => update({ locale: event.target.value })}><option>en-US</option><option>zh-CN</option><option>ja-JP</option></select></label>
      <label className="wide">Subtitle <span>{listing.subtitle.length}/48</span><input value={listing.subtitle} maxLength="48" onChange={(event) => update({ subtitle: event.target.value })} />{errors.subtitle && <small className="field-error">{errors.subtitle}</small>}</label>
      <label>Category<select value={listing.category} onChange={(event) => update({ category: event.target.value })}>{CATEGORIES.map((category) => <option key={category} value={category}>{formatState(category)}</option>)}</select></label>
      <label>Age rating<select value={listing.ageRating} onChange={(event) => update({ ageRating: event.target.value })}>{AGE_RATINGS.map((rating) => <option key={rating}>{rating}</option>)}</select></label>
      <label className="wide">Description <span>{listing.description.length}/1024</span><textarea rows="5" maxLength="1024" value={listing.description} onChange={(event) => update({ description: event.target.value })} />{errors.description && <small className="field-error">{errors.description}</small>}</label>
      <label className="wide">Release notes <span>{listing.releaseNotes.length}/512</span><textarea rows="3" maxLength="512" value={listing.releaseNotes} onChange={(event) => update({ releaseNotes: event.target.value })} />{errors.releaseNotes && <small className="field-error">{errors.releaseNotes}</small>}</label>
      <label>Privacy URL<input type="url" value={listing.privacyUrl} onChange={(event) => update({ privacyUrl: event.target.value })} />{errors.privacyUrl && <small className="field-error">{errors.privacyUrl}</small>}</label>
      <label>Support URL<input type="url" value={listing.supportUrl} onChange={(event) => update({ supportUrl: event.target.value })} />{errors.supportUrl && <small className="field-error">{errors.supportUrl}</small>}</label>
    </div>
  );
}

function AssetsEditor({ listing, update }) {
  return (
    <div className="asset-layout">
      <div className="asset-block"><div className="section-title"><div><h3>App icon</h3><p>One 48x48 PNG. Transparency is allowed.</p></div><Status value={listing.iconReady ? "ready" : "draft"} /></div><button type="button" className={`upload-zone icon-zone ${listing.iconReady ? "has-asset" : ""}`} onClick={() => update({ iconReady: !listing.iconReady })}>{listing.iconReady ? <><FileCheck2 /><strong>icon-48.png</strong><small>48x48, verified</small></> : <><UploadCloud /><strong>Choose icon</strong><small>PNG only</small></>}</button></div>
      <div className="asset-block"><div className="section-title"><div><h3>Device screenshots</h3><p>One to five 320x170 PNG images.</p></div><span className="asset-count">{listing.screenshotCount}/5</span></div><div className="screenshot-row">{Array.from({ length: listing.screenshotCount }, (_, index) => <div className="screenshot-preview" key={index}><ImageIcon /><span>Screen {index + 1}</span></div>)}{listing.screenshotCount < 5 && <button type="button" className="add-screenshot" onClick={() => update({ screenshotCount: listing.screenshotCount + 1 })}><Plus /><span>Add screen</span></button>}</div>{listing.screenshotCount > 0 && <button type="button" className="text-button danger-text" onClick={() => update({ screenshotCount: listing.screenshotCount - 1 })}><Trash2 /> Remove last</button>}</div>
    </div>
  );
}

function PrivacyEditor({ listing, update }) {
  const privacy = listing.privacy;
  const setPrivacy = (patch) => update({ privacy: { ...privacy, ...patch } });
  return (
    <div className="privacy-layout">
      <div className="notice"><ShieldCheck /><div><strong>Runtime permissions stay separate</strong><p>Store privacy declarations are reviewed metadata. They never grant device capabilities.</p></div></div>
      <div className="setting-row"><div><strong>Collects user data</strong><small>Data sent off the device or retained by you</small></div><label className="switch"><input type="checkbox" checked={privacy.dataCollection} onChange={(event) => setPrivacy({ dataCollection: event.target.checked })} /><span /></label></div>
      <div className="setting-row"><div><strong>Uses tracking</strong><small>Links activity across apps, services, or organizations</small></div><label className="switch"><input type="checkbox" checked={privacy.tracking} onChange={(event) => setPrivacy({ tracking: event.target.checked })} /><span /></label></div>
      <div className="setting-row"><div><strong>Encrypted network traffic</strong><small>All remote endpoints use authenticated encryption</small></div><label className="switch"><input type="checkbox" checked={privacy.encryption} onChange={(event) => setPrivacy({ encryption: event.target.checked })} /><span /></label></div>
    </div>
  );
}

function Submissions({ data, setData, toast, focusedId }) {
  const [selectedId, setSelectedId] = useState(data.submissions[0]?.id);
  const [file, setFile] = useState(null);
  const [upload, setUpload] = useState(null);
  const inputRef = useRef(null);
  useEffect(() => { if (focusedId && data.submissions.some((item) => item.id === focusedId)) setSelectedId(focusedId); }, [data.submissions, focusedId]);
  const selected = data.submissions.find((item) => item.id === selectedId) ?? data.submissions[0];
  const checks = selected?.state === "processing"
    ? [["Package format", "complete"], ["Developer signature", "complete"], ["Manifest and capabilities", "running"], ["Store assets", "pending"]]
    : [["Package format", "complete"], ["Developer signature", "complete"], ["Manifest and capabilities", "complete"], ["Store assets", "complete"]];
  useEffect(() => {
    if (!upload || upload.progress >= 100) return undefined;
    const timer = window.setTimeout(() => setUpload((current) => ({ ...current, progress: Math.min(100, current.progress + 8) })), 120);
    return () => window.clearTimeout(timer);
  }, [upload]);
  useEffect(() => {
    if (upload?.progress !== 100 || upload.recorded) return;
    const app = data.apps[0];
    const submission = { id: `sub_local${Date.now()}`, appId: app.id, appName: app.name, version: app.latestVersion, revision: 3, state: "processing", created: "Just now", digest: "Computing...", messages: [{ actor: "Portal", body: "Upload completed. Automated checks started.", time: "Now" }] };
    setData((current) => ({ ...current, submissions: [submission, ...current.submissions] }));
    setSelectedId(submission.id);
    setUpload((current) => ({ ...current, recorded: true }));
    toast("Upload complete. A new immutable revision is processing.");
  }, [upload, data.apps, setData, toast]);
  const startUpload = () => {
    if (!file) return;
    setUpload({ name: file.name, progress: 0, recorded: false });
  };
  return (
    <div className="master-detail submissions-layout">
      <section className="panel master-list">
        <div className="list-toolbar"><div><strong>All revisions</strong><small>{data.submissions.length} submissions</small></div><IconButton label="Upload submission" onClick={() => inputRef.current?.click()}><UploadCloud /></IconButton><input ref={inputRef} className="visually-hidden" type="file" accept=".capp" onChange={(event) => setFile(event.target.files?.[0] ?? null)} /></div>
        {file && !upload && <div className="upload-prompt"><FileArchive /><span><strong>{file.name}</strong><small>{Math.max(1, Math.round(file.size / 1024))} KiB, kept in this session</small></span><button className="primary compact" type="button" onClick={startUpload}>Upload</button></div>}
        {upload && upload.progress < 100 && <div className="upload-progress"><div><span>{upload.name}</span><strong>{upload.progress}%</strong></div><div className="progress-track"><span style={{ width: `${upload.progress}%` }} /></div><small>Resumable upload. Closing this demo clears local state.</small></div>}
        <div className="item-list">{data.submissions.map((item) => <button type="button" className={`list-item submission-item ${selected?.id === item.id ? "selected" : ""}`} key={item.id} onClick={() => setSelectedId(item.id)}><span><strong>{item.appName}</strong><small>{item.version} · revision {item.revision}</small></span><Status value={item.state} /><ChevronRight /></button>)}</div>
      </section>
      {selected && <section className="panel detail-panel">
        <div className="detail-heading-line"><div><span className="eyebrow">Revision {selected.revision}</span><h2>{selected.appName} {selected.version}</h2><p className="mono">{selected.id}</p></div><Status value={selected.state} /></div>
        <div className="detail-facts"><div><span>Created</span><strong>{selected.created}</strong></div><div><span>Content digest</span><strong className="mono">{selected.digest}</strong></div><div><span>Package</span><strong>Developer signed</strong></div></div>
        <div className="subsection"><div className="section-title"><div><h3>Automated checks</h3><p>Each result is bound to this exact revision.</p></div></div><div className="scan-grid">{checks.map(([label, state]) => <div key={label} className={state}><span>{state === "complete" ? <Check /> : state === "running" ? <Clock3 /> : <span className="pending-dot" />}</span><div><strong>{label}</strong><small>{formatState(state)}</small></div></div>)}</div></div>
        <div className="subsection"><div className="section-title"><div><h3>Review timeline</h3><p>Messages are append-only audit events.</p></div></div><div className="timeline">{selected.messages.length ? selected.messages.map((message, index) => <div key={`${message.time}-${index}`}><span className="timeline-dot" /><div><strong>{message.actor}</strong><time>{message.time}</time><p>{message.body}</p></div></div>) : <p className="muted">No review messages for this revision.</p>}</div></div>
        {["draft", "uploading", "processing", "ready-for-review", "in-review", "pending-secondary-review"].includes(selected.state) && <div className="danger-zone"><div><strong>Withdraw revision</strong><p>Stops review. The immutable files remain in the audit history.</p></div><button className="danger-button" type="button" onClick={() => { setData((current) => ({ ...current, submissions: current.submissions.map((item) => item.id === selected.id ? { ...item, state: "withdrawn" } : item) })); toast("Submission withdrawn. A new revision is required for changes."); }}>Withdraw</button></div>}
      </section>}
    </div>
  );
}

function Releases({ data, setData, toast, focusedId }) {
  const [selectedId, setSelectedId] = useState(data.releases[0]?.id);
  const [rollout, setRollout] = useState(100);
  const [schedule, setSchedule] = useState("");
  useEffect(() => { if (focusedId && data.releases.some((item) => item.id === focusedId)) setSelectedId(focusedId); }, [data.releases, focusedId]);
  const selected = data.releases.find((item) => item.id === selectedId) ?? data.releases[0];
  useEffect(() => { if (selected) setRollout(selected.rollout); }, [selected]);
  const mutate = (next, message) => {
    setData((current) => ({ ...current, releases: current.releases.map((item) => item.id === selected.id ? { ...item, state: next, rollout, scheduled: next === "scheduled" ? schedule : item.scheduled, sequence: item.sequence + 1 } : item) }));
    toast(message);
  };
  const action = releaseAction(selected?.state);
  return (
    <div className="master-detail releases-layout">
      <section className="panel master-list"><div className="list-toolbar"><div><strong>Catalog releases</strong><small>Approved submissions only</small></div></div><div className="item-list">{data.releases.map((release) => <button type="button" className={`list-item release-item ${selected?.id === release.id ? "selected" : ""}`} key={release.id} onClick={() => setSelectedId(release.id)}><span><strong>{release.appName}</strong><small>{release.version} · {release.rollout}% rollout</small></span><Status value={release.state} /><ChevronRight /></button>)}</div></section>
      {selected && <section className="panel detail-panel release-detail">
        <div className="detail-heading-line"><div><span className="eyebrow">Catalog #{selected.sequence}</span><h2>{selected.appName} {selected.version}</h2><p className="mono">{selected.id}</p></div><Status value={selected.state} /></div>
        <div className="notice"><BadgeCheck /><div><strong>Approved submission verified</strong><p>Release controls cannot modify the reviewed package, listing, or assets.</p></div></div>
        <div className="release-controls">
          <div className="control-block"><div className="range-heading"><label htmlFor="rollout">Phased rollout</label><strong>{rollout}%</strong></div><input id="rollout" type="range" min="5" max="100" step="5" value={rollout} onChange={(event) => setRollout(Number(event.target.value))} disabled={["publishing", "removed"].includes(selected.state)} /><div className="range-labels"><span>5%</span><span>100%</span></div><p>New catalog snapshots keep the last approved package immutable.</p></div>
          <div className="control-block"><label htmlFor="schedule">Schedule publication</label><div className="input-with-icon"><CalendarClock /><input id="schedule" type="datetime-local" value={schedule} onChange={(event) => setSchedule(event.target.value)} disabled={!['ready', 'scheduled'].includes(selected.state)} /></div><button className="secondary" type="button" disabled={!schedule || !['ready', 'scheduled'].includes(selected.state)} onClick={() => mutate("scheduled", "Release scheduled with a new catalog sequence.")}>Save schedule</button></div>
        </div>
        <div className="release-actions"><div><strong>Release state</strong><p>Every action creates an auditable catalog event.</p></div><div>{action === "publish" && <button className="primary" type="button" onClick={() => mutate("published", "Release published to the mock catalog.")}><Rocket /> Publish now</button>}{action === "pause" && <button className="secondary" type="button" onClick={() => mutate("paused", "Release paused with a higher catalog sequence.")}><Pause /> Pause</button>}{action === "resume" && <button className="primary" type="button" onClick={() => mutate("published", "Release resumed with a higher catalog sequence.")}><Play /> Resume</button>}<button className="danger-button" type="button" disabled={selected.state === "removed"} onClick={() => mutate("removed", "Release removed. Published objects remain immutable.")}><Trash2 /> Remove</button></div></div>
      </section>}
    </div>
  );
}

function Team({ data, setData, toast }) {
  const [keyDialog, setKeyDialog] = useState(false);
  const [keyName, setKeyName] = useState("");
  const [publicKey, setPublicKey] = useState("");
  const [memberToRemove, setMemberToRemove] = useState(null);
  const ownerCount = data.team.members.filter((member) => member.role === "Owner" && member.state === "active").length;
  useEffect(() => {
    if (!keyDialog && !memberToRemove) return undefined;
    const closeOnEscape = (event) => {
      if (event.key !== "Escape") return;
      setKeyDialog(false);
      setMemberToRemove(null);
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [keyDialog, memberToRemove]);
  const addKey = (event) => {
    event.preventDefault();
    const normalized = publicKey.trim();
    if (!keyName.trim() || !/^ssh-ed25519 [A-Za-z0-9+/=]{32,}(?: .*)?$/.test(normalized)) return;
    const key = { id: `key_local_${Date.now()}`, name: keyName.trim(), algorithm: "Ed25519", fingerprint: "Pending server verification", created: "2026-08-01", lastUsed: "Never", status: "active" };
    setData((current) => ({ ...current, team: { ...current.team, developerKeys: [...current.team.developerKeys, key] } }));
    setKeyDialog(false); setKeyName(""); setPublicKey(""); toast("Public developer key registered. No private key was requested.");
  };
  const removeMember = () => {
    const removedName = memberToRemove.name;
    setData((current) => ({
      ...current,
      team: {
        ...current.team,
        members: current.team.members.filter((member) => member.id !== memberToRemove.id),
      },
    }));
    setMemberToRemove(null);
    toast(`${removedName} was removed and all active sessions were revoked.`);
  };
  const setMemberState = (member, state) => {
    setData((current) => ({
      ...current,
      team: {
        ...current.team,
        members: current.team.members.map((item) => item.id === member.id ? { ...item, state } : item),
      },
    }));
    toast(state === "suspended"
      ? `${member.name} was suspended and all active sessions were revoked.`
      : `${member.name} was restored and must sign in again.`);
  };
  return (
    <div className="page-stack">
      <section className="panel">
        <div className="panel-heading"><div><h2>{data.team.name}</h2><p>Team ID <span className="mono">{data.team.id}</span></p></div><Status value="active" /></div>
        <div className="table-wrap"><table className="team-table">
          <thead><tr><th>Member</th><th>Role</th><th>Two-factor authentication</th><th><span className="visually-hidden">Actions</span></th></tr></thead>
          <tbody>{data.team.members.map((member) => {
            const suspended = member.state === "suspended";
            const finalOwner = member.role === "Owner" && member.state === "active" && ownerCount === 1;
            return <tr key={member.id}>
              <td><div className="member-heading"><strong>{member.name}</strong><Status value={member.state} /></div><small>{member.email}</small></td>
              <td><select className="table-select" aria-label={`${member.name} role`} value={member.role} disabled={member.role === "Owner" || suspended} onChange={(event) => setData((current) => ({ ...current, team: { ...current.team, members: current.team.members.map((item) => item.id === member.id ? { ...item, role: event.target.value } : item) } }))}><option>Owner</option><option>Developer</option><option>Release Manager</option><option>Viewer</option></select></td>
              <td>{member.twoFactor ? <span className="verified"><ShieldCheck /> Enabled</span> : <span className="not-verified"><CircleAlert /> Required before release access</span>}</td>
              <td><div className="team-actions">{suspended
                ? <IconButton label={`Restore ${member.name}`} onClick={() => setMemberState(member, "active")}><Play /></IconButton>
                : <IconButton label={finalOwner ? "The final Owner cannot be suspended" : `Suspend ${member.name}`} disabled={finalOwner} onClick={() => setMemberState(member, "suspended")}><Pause /></IconButton>}
              <IconButton className="danger-icon" label={finalOwner ? "The final Owner cannot be removed" : `Remove ${member.name}`} disabled={finalOwner} onClick={() => setMemberToRemove(member)}><Trash2 /></IconButton></div></td>
            </tr>;
          })}</tbody>
        </table></div>
      </section>
      <section className="panel"><div className="panel-heading"><div><h2>Developer public keys</h2><p>Keys verify package authorship. Private keys stay on developer workstations.</p></div><button className="primary" type="button" onClick={() => setKeyDialog(true)}><Plus /> Register public key</button></div><div className="key-list">{data.team.developerKeys.map((key) => <div className="key-row" key={key.id}><div className="key-icon"><KeyRound /></div><div><strong>{key.name}</strong><small>{key.algorithm} · <span className="mono">{key.fingerprint}</span></small><small>Created {key.created} · Last used {key.lastUsed}</small></div><Status value={key.status} /><button className="text-button danger-text" type="button" onClick={() => { setData((current) => ({ ...current, team: { ...current.team, developerKeys: current.team.developerKeys.map((item) => item.id === key.id ? { ...item, status: "revoked" } : item) } })); toast("Public key revoked. Existing audit records are unchanged."); }}>Revoke</button></div>)}</div></section>
      {keyDialog && <div className="modal-backdrop" role="presentation"><section className="dialog" role="dialog" aria-modal="true" aria-labelledby="key-title"><div className="dialog-heading"><div><h2 id="key-title">Register public key</h2><p>Only the Ed25519 public key is accepted.</p></div><IconButton label="Close" onClick={() => setKeyDialog(false)}><X /></IconButton></div><form className="form-stack" onSubmit={addKey}><label>Key name<input value={keyName} onChange={(event) => setKeyName(event.target.value)} placeholder="Release workstation" autoFocus /></label><label>SSH Ed25519 public key<textarea rows="4" value={publicKey} onChange={(event) => setPublicKey(event.target.value)} placeholder="ssh-ed25519 AAAA... developer@example" /><small>Never paste a private key, seed, passphrase, or token.</small></label><div className="dialog-actions"><button className="secondary" type="button" onClick={() => setKeyDialog(false)}>Cancel</button><button className="primary" type="submit" disabled={!keyName.trim() || !publicKey.trim()}>Register key</button></div></form></section></div>}
      {memberToRemove && <div className="modal-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && setMemberToRemove(null)}><section className="dialog compact-dialog" role="alertdialog" aria-modal="true" aria-labelledby="remove-member-title" aria-describedby="remove-member-description"><div className="dialog-heading"><div><h2 id="remove-member-title">Remove {memberToRemove.name}</h2><p id="remove-member-description">Access and active sessions will be revoked immediately.</p></div><IconButton label="Close" onClick={() => setMemberToRemove(null)}><X /></IconButton></div><div className="form-stack"><div className="notice attention"><CircleAlert /><div><strong>This action cannot be undone</strong><p>The membership remains in the audit history, but it cannot be reactivated.</p></div></div><div className="dialog-actions"><button className="secondary" type="button" autoFocus onClick={() => setMemberToRemove(null)}>Cancel</button><button className="danger-button" type="button" onClick={removeMember}><Trash2 /> Remove member</button></div></div></section></div>}
    </div>
  );
}

export default function App() {
  const [data, setData] = useState(createDemoState);
  const [page, setPage] = useState("overview");
  const [navOpen, setNavOpen] = useState(false);
  const [toast, setToast] = useState("");
  const [globalQuery, setGlobalQuery] = useState("");
  const [searchOpen, setSearchOpen] = useState(false);
  const [focusedTarget, setFocusedTarget] = useState(null);
  const title = PAGE_TITLES[page];
  const toastMessage = useMemo(() => toast, [toast]);
  const globalResults = useMemo(() => {
    const query = globalQuery.trim().toLowerCase();
    if (!query) return [];
    return [
      ...data.apps.map((item) => ({ id: `app-${item.id}`, targetId: item.id, label: item.name, meta: item.id, page: "apps", type: "App" })),
      ...data.submissions.map((item) => ({ id: `submission-${item.id}`, targetId: item.id, label: `${item.appName} ${item.version}`, meta: `Revision ${item.revision} · ${formatState(item.state)}`, page: "submissions", type: "Submission" })),
      ...data.releases.map((item) => ({ id: `release-${item.id}`, targetId: item.id, label: `${item.appName} ${item.version}`, meta: `${item.rollout}% · ${formatState(item.state)}`, page: "releases", type: "Release" })),
    ].filter((item) => `${item.label} ${item.meta} ${item.type}`.toLowerCase().includes(query)).slice(0, 6);
  }, [data.apps, data.releases, data.submissions, globalQuery]);
  useEffect(() => {
    if (!toastMessage) return undefined;
    const timer = window.setTimeout(() => setToast(""), 4000);
    return () => window.clearTimeout(timer);
  }, [toastMessage]);
  useEffect(() => {
    if (!navOpen) return undefined;
    const closeOnEscape = (event) => { if (event.key === "Escape") setNavOpen(false); };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [navOpen]);
  const navigate = (next) => { setPage(next); setNavOpen(false); setSearchOpen(false); };
  const chooseGlobalResult = (result) => {
    setFocusedTarget({ page: result.page, id: result.targetId });
    navigate(result.page);
    setGlobalQuery("");
  };
  return (
    <div className="app-shell">
      <aside className={navOpen ? "sidebar open" : "sidebar"}>
        <div className="brand"><div className="brand-mark">C0</div><div><strong>CardputerZero</strong><span>Developer</span></div><IconButton label="Close navigation" onClick={() => setNavOpen(false)}><X /></IconButton></div>
        <nav aria-label="Primary navigation">{NAV_ITEMS.map(({ id, label, icon: Icon }) => <button type="button" className={page === id ? "active" : ""} key={id} onClick={() => navigate(id)}><Icon /><span>{label}</span>{id === "submissions" && <b>{data.submissions.filter((item) => ["in-review", "pending-secondary-review", "needs-changes"].includes(item.state)).length}</b>}</button>)}</nav>
        <div className="sidebar-footer"><div className="avatar">LB</div><div><strong>{data.viewer.name}</strong><span>{data.team.name} · {data.team.role}</span></div><ChevronDown /></div>
      </aside>
      {navOpen && <button className="nav-scrim" aria-label="Close navigation" type="button" onClick={() => setNavOpen(false)} />}
      <main>
        <header className="topbar"><IconButton label="Open navigation" onClick={() => setNavOpen(true)}><Menu /></IconButton><div className="top-search" onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) setSearchOpen(false); }}><Search /><input aria-label="Search portal" placeholder="Search apps, versions, App IDs" value={globalQuery} onChange={(event) => { setGlobalQuery(event.target.value); setSearchOpen(true); }} onFocus={() => setSearchOpen(true)} onKeyDown={(event) => { if (event.key === "Escape") setSearchOpen(false); if (event.key === "Enter" && globalResults[0]) chooseGlobalResult(globalResults[0]); }} aria-expanded={searchOpen && Boolean(globalQuery.trim())} />{searchOpen && globalQuery.trim() && <div className="global-results" aria-label="Portal search results">{globalResults.length ? globalResults.map((result) => <button type="button" key={result.id} onClick={() => chooseGlobalResult(result)}><span>{result.type}</span><strong>{result.label}</strong><small>{result.meta}</small></button>) : <p>No apps, submissions, or releases found.</p>}</div>}</div><IconButton label="Notifications" onClick={() => setToast("No unread notifications.")}><Bell /></IconButton><div className="avatar small">LB</div></header>
        <div className="page-header"><div>{page !== "overview" && <button className="mobile-back" type="button" onClick={() => navigate("overview")} aria-label="Back to overview"><ArrowLeft /></button>}<span className="eyebrow">M5 Labs</span><h1>{title[0]}</h1><p>{title[1]}</p></div>{page === "apps" && <button className="primary page-action" type="button" onClick={() => document.querySelector('[aria-label="Register App ID"]')?.click()}><Plus /> Register App ID</button>}</div>
        <div className="page-content">
          {page === "overview" && <Overview data={data} navigate={navigate} />}
          {page === "apps" && <Apps data={data} setData={setData} toast={setToast} focusedId={focusedTarget?.page === "apps" ? focusedTarget.id : null} />}
          {page === "submissions" && <Submissions data={data} setData={setData} toast={setToast} focusedId={focusedTarget?.page === "submissions" ? focusedTarget.id : null} />}
          {page === "releases" && <Releases data={data} setData={setData} toast={setToast} focusedId={focusedTarget?.page === "releases" ? focusedTarget.id : null} />}
          {page === "team" && <Team data={data} setData={setData} toast={setToast} />}
        </div>
      </main>
      {toastMessage && <div className="toast" role="status"><Check />{toastMessage}</div>}
    </div>
  );
}
