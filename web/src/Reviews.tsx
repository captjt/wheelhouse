import { useMemo, useState, type ReactNode } from "react";
import { post, type Docket, type ReviewPr, type ReviewStatus } from "./api";
import { IconChevronDown, IconExternal, IconIssue, IconPause, IconPlay, IconPR, IconRefresh, LiveIcon } from "./icons";
import { ago, agoLong, stamp, useHub } from "./live";
import { InlineMd, Modal, Prose, useToast } from "./ui";
import { LargeTitle } from "./mobile";


export const verdictWord = (v?: string | null) =>
  ({ APPROVED: "Approved", CHANGES_REQUESTED: "Changes requested", DISMISSED: "Dismissed", COMMENTED: "Commented" })[v ?? ""] ?? (v ? v.replace(/_/g, " ").toLowerCase() : "");

const tokens = (n?: number | null) => (!n ? "" : n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));

/** The checked review, in a line: what it asks and what it found. */
export function draftLine(p: ReviewPr): string | null {
  const d = p.draft;
  if (!d) return null;
  const ask = d.event === "APPROVE" ? "Approve" : d.event === "REQUEST_CHANGES" ? "Request changes" : d.event;
  return [ask, `${d.blocking} blocking, ${d.non_blocking} non-blocking`, d.merge_risk && `merge risk ${d.merge_risk}`].filter(Boolean).join(" · ");
}

/** Why a checked review waits for you under a posting rule, or null. */
export function waitsWhy(p: ReviewPr): string | null {
  const d = p.post_decision;
  if (!d) return null;
  if (d.refused) return `your rule would post it, but the post was refused: ${d.refused}`;
  return d.auto ? null : d.why;
}

/** What a PR's review is doing, in a couple of words. */
export function phaseText(p: ReviewPr, paused?: boolean): string {
  if (p.attention?.kind === "ready") return "Ready to post";
  switch (p.state) {
    case "queued":
      return p.start_error ? "Didn't start" : paused ? "Queued · paused" : "Queued";
    case "reviewing":
      if (p.phase === "checking") return "Checking";
      if (p.phase === "awaiting") return "Ready to post";
      return `Reviewing ${p.reviewers_done ?? 0}/${p.reviewers_total ?? p.reviewers.length}`;
    case "posted":
      return "Waiting on the author";
    case "unrequested":
      return "No longer asked";
    case "merged":
    case "closed":
    case "cleaned":
      return p.state === "cleaned" ? "Done" : p.state === "merged" ? "Merged" : "Closed";
  }
  return p.state;
}

/** The live state a PR's review shows as. */
export function reviewLive(p: ReviewPr): string {
  const k = p.attention?.kind;
  if (k === "ready" || k === "blocked") return "blocked";
  if (k) return "error";
  if (p.state === "reviewing") return "working";
  if (p.state === "posted" || p.state === "cleaned" || p.state === "merged") return "done";
  return "idle";
}

export function RiskChip({ level }: { level?: string | null }) {
  if (!level) return null;
  return <span className={`rv-risk risk-${level}`}>Risk {level}</span>;
}

export function VerdictChip({ v }: { v?: string | null }) {
  if (!v) return null;
  const tone = v === "APPROVED" ? "ok" : v === "CHANGES_REQUESTED" ? "changes" : "muted";
  return <span className={`rv-verdict v-${tone}`}>{verdictWord(v)}</span>;
}

/** Post, Preview and Dismiss for a PR's round. Post only when the round
 * waits for you; the hub refuses anything else. */
export function ReviewActions({ p, small }: { p: ReviewPr; small?: boolean }) {
  const hub = useHub();
  const toast = useToast();
  const [busy, setBusy] = useState<string | null>(null);
  const [preview, setPreview] = useState<string | null>(null);
  // The draft you saw (in its summary, or its preview): the hub posts only that.
  const [seen, setSeen] = useState<string | undefined>(undefined);
  const cls = small ? "btn btn-small" : "btn";
  const ready = p.state === "reviewing" && p.phase === "awaiting";
  const stop = (e?: React.MouseEvent) => (e?.preventDefault(), e?.stopPropagation());
  const act = (what: "post" | "dismiss") => (e?: React.MouseEvent) => {
    stop(e);
    if (busy) return;
    setBusy(what);
    post<{ url?: string }>(`/api/review/${what}`, { key: p.full_key, digest: what === "post" ? (seen ?? p.draft?.digest) : undefined }).then(
      () => {
        toast(what === "post" ? `Posted the review of ${p.key}` : `Dismissed round ${p.round} of ${p.key}; nothing was posted`);
        setBusy(null);
        setPreview(null);
        hub.reload();
      },
      (err: Error) => (setBusy(null), toast(err.message, "error"), hub.reload()),
    );
  };
  const show = (e: React.MouseEvent) => {
    stop(e);
    setBusy("preview");
    post<{ preview: string; digest?: string }>(`/api/review/preview`, { key: p.full_key }).then(
      (v) => (setBusy(null), setPreview(v.preview), setSeen(v.digest)),
      (err: Error) => (setBusy(null), toast(err.message, "error")),
    );
  };
  if (p.state !== "reviewing") return null;
  return (
    <span className="btn-pair answer">
      {ready && (
        <button className={`${cls} btn-approve`} disabled={!!busy} onClick={act("post")} title="Post the checked review to GitHub as you">
          {busy === "post" ? "Posting…" : "Post"}
        </button>
      )}
      {(p.phase === "awaiting" || p.phase === "checking") && (
        <button className={cls} disabled={!!busy} onClick={show}>
          {busy === "preview" ? "Loading…" : "Preview"}
        </button>
      )}
      <button className={cls} disabled={!!busy} onClick={act("dismiss")} title="End this round without posting anything">
        {busy === "dismiss" ? "Dismissing…" : "Dismiss"}
      </button>
      {preview !== null && (
        <Modal onClose={() => setPreview(null)} className="preview-modal">
          <div className="nd-head">
            <span>
              Review of {p.key} · round {p.round}, as it would post
            </span>
            <span className="spacer" />
            {p.url && (
              <a className="btn btn-small btn-ghost" href={p.url} target="_blank" rel="noreferrer">
                PR <IconExternal size={11} />
              </a>
            )}
          </div>
          <div className="preview-body">
            <Prose text={preview} />
          </div>
          <div className="nd-foot">
            <span className="muted small">{ready ? "Posts from your GitHub account, with the footer above." : "The checker hasn't finished; this is its draft so far."}</span>
            <span className="spacer" />
            <button className="btn btn-small" onClick={act("dismiss")} disabled={!!busy}>
              Dismiss
            </button>
            {ready && (
              <button className="btn btn-primary btn-small" onClick={act("post")} disabled={!!busy}>
                Post review
              </button>
            )}
          </div>
        </Modal>
      )}
    </span>
  );
}

const STEPS = ["Reviewers", "Checker", "Your call", "Posted"];

function Steps({ p, auto }: { p: ReviewPr; auto: boolean }) {
  const at = p.state === "posted" || p.state === "cleaned" || p.state === "merged" || p.state === "closed" ? 4 : p.state !== "reviewing" ? -1 : p.phase === "checking" ? 1 : p.phase === "awaiting" ? 2 : 0;
  const steps = auto ? ["Reviewers", "Checker", "Posted"] : STEPS;
  const idx = auto && at >= 2 ? at - 1 : at;
  return (
    <div className="rv-steps">
      {steps.map((s, i) => (
        <span key={s} className={`rv-step${i < idx ? " done" : i === idx ? " now" : ""}`}>
          <span className="rv-dot" />
          {s === "Reviewers" && i === idx ? `Reviewers ${p.reviewers_done ?? 0}/${p.reviewers_total ?? p.reviewers.length}` : s}
        </span>
      ))}
    </div>
  );
}

/** A Review docket's PR: where its round is, who reviews it, what needs
 * you, and how earlier rounds went. */
export function ReviewPanel({ d }: { d: Docket }) {
  const hub = useHub();
  const rv = hub.state?.review;
  const p = rv?.prs.find((x) => x.docket === d.id);
  if (!rv?.enabled || !p) return null;
  const auto = (p.post ?? rv.post) === "auto";
  const att = p.attention;
  const why = waitsWhy(p);
  return (
    <section className={`rv-panel${att ? " rv-alert" : ""}`}>
      <div className="rv-head">
        <IconPR size={14} />
        <a className="rv-key" href={p.url || "#"} target="_blank" rel="noreferrer">
          {p.repo}#{p.pr} <IconExternal size={10} />
        </a>
        {p.author && <span className="muted">by {p.author}</span>}
        <span className="act-dot">·</span>
        <span className="muted">round {p.round || 1}</span>
        <span className="spacer" />
        <VerdictChip v={p.state === "reviewing" ? null : p.verdict} />
        <RiskChip level={p.merge_risk} />
      </div>
      {p.state === "reviewing" || p.state === "queued" ? (
        <Steps p={p} auto={auto} />
      ) : (
        <div className="rv-state muted small">{phaseText(p, rv.paused)}{p.posted_at ? ` · last posted ${agoLong(p.posted_at)}` : ""}</div>
      )}
      {(p.reviewers.length > 0 || p.checker_effort) && p.state === "reviewing" && (
        <div className="rv-who">
          {p.reviewers.map((r) => (
            <span key={r.name} className="rv-agent" title={`${r.agent}${r.effort ? ` at ${r.effort} effort` : ""}`}>
              {r.name}
              {r.effort && <span className="muted"> · {r.effort}</span>}
            </span>
          ))}
          {p.checker_effort && (
            <span className="rv-agent">
              checker<span className="muted"> · {p.checker_effort}</span>
            </span>
          )}
        </div>
      )}
      {att && (
        <div className={`callout ${att.kind === "ready" || att.kind === "blocked" ? "callout-blocked" : "callout-error"}`}>
          <LiveIcon state={att.kind === "ready" || att.kind === "blocked" ? "blocked" : "error"} />
          <div>
            <strong>{att.kind === "ready" ? `Round ${att.round} is ready to post` : att.kind === "start-failed" ? `Round ${att.round} didn't start` : att.kind === "stalled" ? "The round stalled" : att.kind === "pane-closed" ? "A pane closed mid-round" : att.kind === "blocked" ? "An agent is blocked" : "Something went wrong"}</strong>
            {att.kind === "ready" && p.draft ? (
              <div className="rv-draft">
                <span className="rv-draft-line">{draftLine(p)}</span>
                {p.draft.verdict && <span className="rv-draft-verdict">{p.draft.verdict}</span>}
                {why && <span className="rv-draft-why">Waits for you: {why}</span>}
              </div>
            ) : (
              <div className="callout-msg">{att.text}</div>
            )}
          </div>
          <span className="spacer" />
          <span className="callout-actions">
            <ReviewActions p={p} small />
          </span>
        </div>
      )}
      {!att && p.state === "reviewing" && (
        <div className="rv-actions">
          <ReviewActions p={p} small />
        </div>
      )}
      {p.implementation && <div className="rv-impl"><InlineMd text={p.implementation} /></div>}
      {p.history.length > 0 && <History p={p} />}
    </section>
  );
}

function History({ p }: { p: ReviewPr }) {
  const [open, setOpen] = useState(false);
  const rows = open ? p.history : p.history.slice(0, 2);
  return (
    <div className="rv-hist">
      {rows.map((h) => {
        const found = Object.entries(h.reviewers ?? {})
          .map(([n, r]) => `${n}: ${r.kept ?? r.drafted ?? 0}${r.only ? ` (${r.only} alone)` : ""}`)
          .join(" · ");
        return (
          <div key={`${h.round}-${h.at}`} className="rv-hist-row">
            <span className="rv-hist-round">R{h.round}</span>
            <VerdictChip v={h.verdict} />
            <span className="rv-hist-counts muted">
              {h.blocking != null ? `${h.blocking} blocking · ${h.non_blocking ?? 0} non-blocking` : h.by === "you" ? "by you" : ""}
            </span>
            <span>
              <RiskChip level={h.merge_risk} />
            </span>
            <span className="rv-hist-found muted" title="Findings kept from each reviewer">
              {found}
            </span>
            <span className="rv-hist-tok muted">{h.tokens?.total ? `${tokens(h.tokens.total)} tok` : ""}</span>
            <span className="rv-hist-at muted" title={h.at ? stamp(h.at) : ""}>
              {ago(h.at)}
            </span>
            {h.url ? (
              <a className="icon-btn" href={h.url} target="_blank" rel="noreferrer" title="The review on GitHub">
                <IconExternal size={12} />
              </a>
            ) : (
              <span className="icon-btn" />
            )}
          </div>
        );
      })}
      {p.history.length > 2 && (
        <button className="btn btn-ghost btn-small rv-more" onClick={() => setOpen((o) => !o)}>
          {open ? "Fewer rounds" : `All ${p.history.length} rounds`}
        </button>
      )}
    </div>
  );
}

type Group = { key: string; label: string; items: ReviewPr[]; icon: ReactNode };

/** Every PR the Review protocol follows, by what it needs. */
export function ReviewsView() {
  const hub = useHub();
  const toast = useToast();
  const rv = hub.state?.review;
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set(["done"]));
  const set = (body: Record<string, unknown>, done?: string) =>
    post<ReviewStatus>("/api/review/settings", body).then(
      () => (done && toast(done), hub.reload()),
      (e: Error) => toast(e.message, "error"),
    );
  const groups: Group[] = useMemo(() => {
    const prs = rv?.prs ?? [];
    const by = (f: (p: ReviewPr) => boolean) => prs.filter(f).sort((a, b) => (b.updated_at ?? "").localeCompare(a.updated_at ?? ""));
    return [
      { key: "needs", label: "Needs you", icon: <LiveIcon state="blocked" size={13} />, items: by((p) => !!p.attention) },
      { key: "reviewing", label: "Reviewing", icon: <LiveIcon state="working" size={13} />, items: by((p) => !p.attention && p.state === "reviewing") },
      { key: "queued", label: "Queued", icon: <LiveIcon state="idle" size={13} />, items: by((p) => !p.attention && p.state === "queued") },
      { key: "posted", label: "Waiting on the author", icon: <LiveIcon state="done" size={13} />, items: by((p) => !p.attention && p.state === "posted") },
      { key: "done", label: "Done this week", icon: <LiveIcon state="idle" size={13} />, items: by((p) => !p.attention && ["merged", "closed", "cleaned", "unrequested"].includes(p.state)) },
    ].filter((g) => g.items.length > 0);
  }, [rv]);
  const live = (rv?.prs ?? []).filter((p) => ["queued", "reviewing", "posted"].includes(p.state)).length;
  const cfg = rv?.config;
  // Rules: a rule, or a repo that posts its own way. The switch shows the
  // override (wh review post-mode) when there is one, else the config.
  const ruled = cfg?.post === "rule" || Object.keys(cfg?.repo_posts ?? {}).length > 0;
  const posting = rv?.post_override ?? (ruled ? "rule" : cfg?.post === "auto" ? "auto" : "ask");

  return (
    <div className="view">
      <header className="view-head">
        <span className="view-title">
          <IconPR size={15} /> Reviews <span className="view-count">{live}</span>
        </span>
        <span className="spacer" />
        {rv?.enabled && (
          <>
            <div className="seg seg-text" title={ruled ? "Who posts a checked review: you (Ask), the checker (Auto), or your rules, repo by repo (Rules, in Settings)" : "Who posts a checked review: you (Ask), or the checker (Auto)"}>
              {(["ask", "auto"] as const).map((m) => (
                <button key={m} className={posting === m ? "on" : ""} onClick={() => posting !== m && set({ post: rv.post_override && !ruled && cfg?.post === m ? "config" : m }, m === "auto" ? "The checker posts each review" : "You post each review")}>
                  {m === "ask" ? "Ask" : "Auto"}
                </button>
              ))}
              {ruled && (
                <button className={posting === "rule" ? "on" : ""} onClick={() => posting !== "rule" && set({ post: "config" }, "Each repo posts as your rules say")}>
                  Rules
                </button>
              )}
            </div>
            <button className="btn btn-ghost" onClick={() => set({ paused: !rv.paused }, rv.paused ? "Reviews resumed" : "Reviews paused: no new rounds start")}>
              {rv.paused ? <IconPlay size={13} /> : <IconPause size={13} />} <span className="rv-btn-label">{rv.paused ? "Resume" : "Pause"}</span>
            </button>
            <button className="btn btn-ghost" onClick={() => post("/api/review/check", {}).then(() => toast("Checking GitHub"), (e: Error) => toast(e.message, "error"))}>
              <IconRefresh size={13} /> <span className="rv-btn-label">Check now</span>
            </button>
          </>
        )}
      </header>
      <LargeTitle title="Reviews" count={live} />
      {rv?.enabled && (
        <div className="rv-bar">
          <span className={`rv-pill ${rv.paused ? "paused" : "on"}`}>{rv.paused ? "Paused" : "On"}</span>
          {rv.doing ? (
            <span className="rv-doing">{rv.doing}…</span>
          ) : rv.scan_error ? (
            <span className="rv-err">GitHub: {rv.scan_error}</span>
          ) : (
            <span>{!rv.last_scan_at ? "Not checked GitHub yet" : ago(rv.last_scan_at) === "now" ? "Checked GitHub just now" : `Checked GitHub ${ago(rv.last_scan_at)} ago`}</span>
          )}
          {cfg && (
            <span className="rv-cfg">
              <span className="act-dot">·</span>
              <span title={cfg.path}>
                {cfg.reviewers.map((r) => r.name).join(", ")} → checker {cfg.checker}
              </span>
              <span className="act-dot">·</span>
              <span>up to {cfg.max_rounds} at once</span>
              <span className="act-dot">·</span>
              <span title={cfg.repos.join("\n")}>
                {cfg.repos.length} repo{cfg.repos.length === 1 ? "" : "s"}
              </span>
            </span>
          )}
          {rv.config_error && <span className="rv-err">Config: {rv.config_error}</span>}
        </div>
      )}
      {!rv ? (
        <div className="view-empty">{hub.error ? `Can't reach the hub: ${hub.error}` : ""}</div>
      ) : !rv.enabled ? (
        <div className="view-empty rv-off">
          <IconPR size={22} />
          <p>
            <strong>Reviews run in Wheelhouse.</strong> Each PR that asks you for a review becomes a Review docket: blind reviewers in panes of their own, a checker that merges their drafts and rates the merge risk, and a post you approve (or let the checker make).
          </p>
          <p className="muted small">
            {cfg ? <>Its settings are in {cfg.path}: repos, reviewers, the checker, and your effort policy.</> : <>Set it up in ~/.config/wheelhouse/review.lua (docs/review.md). Moving from rex-reviewer? <code>wh import rex-reviewer</code> brings its config and history over.</>}
            {rv.rex_reviewer_running ? " rex-reviewer is still running: stop it first with rex-reviewer down." : ""}
          </p>
          {rv.config_error && <p className="rv-err small">{rv.config_error}</p>}
          <button className="btn btn-primary" disabled={rv.rex_reviewer_running || !!rv.config_error || !cfg} onClick={() => set({ enabled: true }, "Reviews are on, paused until you resume")}>
            Turn on reviews
          </button>
        </div>
      ) : groups.length === 0 ? (
        <div className="view-empty">No PR asks you for a review right now.</div>
      ) : (
        <div className="list" style={{ ["--idch" as string]: Math.max(3, ...(rv.prs ?? []).map((p) => (p.docket || "").length)) }}>
          {groups.map((g) => (
            <div key={g.key}>
              <button
                className="group-head"
                onClick={() =>
                  setCollapsed((c) => {
                    const n = new Set(c);
                    if (n.has(g.key)) n.delete(g.key);
                    else n.add(g.key);
                    return n;
                  })
                }
              >
                <span className={`caret${collapsed.has(g.key) ? " closed" : ""}`}>
                  <IconChevronDown size={12} />
                </span>
                {g.icon}
                <span className="group-label">{g.label}</span>
                <span className="view-count">{g.items.length}</span>
              </button>
              {!collapsed.has(g.key) && g.items.map((p) => <ReviewRow key={p.full_key} p={p} paused={rv.paused} />)}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function ReviewRow({ p, paused }: { p: ReviewPr; paused: boolean }) {
  const last = p.history[0];
  const issue = useHub().byId.get(p.docket ?? "")?.linear;
  return (
    <a className="row rv-row" href={p.docket ? `#/d/${p.docket}` : p.url || undefined} target={p.docket ? undefined : "_blank"} rel="noreferrer">
      <span className="row-live" title={phaseText(p, paused)}>
        <LiveIcon state={reviewLive(p)} />
      </span>
      <span className="row-id">{p.docket || "—"}</span>
      <span className="rv-key-cell">{p.key}</span>
      <span className="row-title">{p.title || p.key}</span>
      {p.attention?.kind === "ready" ? (
        <span className="row-note">{draftLine(p) ?? p.attention.text}</span>
      ) : p.attention ? (
        <span className="row-note">{p.attention.text}</span>
      ) : p.author ? (
        <span className="rv-author muted">{p.author}</span>
      ) : null}
      <span className="row-end">
        {issue && (
          <span className="rv-issue" title={`${issue.key}${issue.title ? `: ${issue.title}` : ""}`}>
            <IconIssue size={12} />
            {issue.key}
          </span>
        )}
        {p.attention?.kind === "ready" && (
          <span className="rv-row-actions" onClick={(e) => e.preventDefault()}>
            <ReviewActions p={p} small />
          </span>
        )}
        {(p.state === "reviewing" || p.state === "queued") && !p.attention && <span className="rv-phase">{phaseText(p, paused)}</span>}
        {p.round > 0 && <span className="rv-round">R{p.round}</span>}
        <VerdictChip v={p.state === "reviewing" ? null : p.verdict ?? last?.verdict} />
        <RiskChip level={p.merge_risk} />
        <span className="row-age" title={p.updated_at ?? ""}>
          {ago(p.updated_at)}
        </span>
      </span>
    </a>
  );
}
