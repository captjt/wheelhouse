import { useMemo, useState, type ReactNode } from "react";
import { post, type BuildTask, type Docket } from "./api";
import { IconBuild, IconChevronDown, IconExternal, IconPause, IconPlay, LiveIcon } from "./icons";
import { ago, useHub } from "./live";
import { InlineMd, useMenu, useToast } from "./ui";
import { LargeTitle } from "./mobile";

/** What a Build is doing, in a few words. */
export function buildPhase(t: BuildTask): string {
  switch (t.state) {
    case "queued":
      return t.start_error ? "Didn't start" : "Queued";
    case "open":
      return "PR open";
    case "merged":
      return "Merged";
    case "closed":
      return "Closed";
    case "discarded":
      return "Discarded";
  }
  return (
    {
      planning: "Planning",
      not_ready: "Not ready: your call",
      plan_gate: "Plan: your call",
      building: "Building",
      checking: `Checking (check ${t.check_n})`,
      preview_gate: "Preview: your call",
      open_gate: "Ready to open",
      opening: "Opening",
    }[t.phase ?? ""] ?? "Working"
  );
}

const needsYou = (t: BuildTask) => !!t.attention || (t.asks.length > 0 && t.state === "active");

export function buildLive(t: BuildTask): string {
  if (t.attention) return t.attention.kind === "blocked" ? "blocked" : "error";
  if (needsYou(t)) return "blocked";
  if (t.state === "active") return "working";
  if (t.state === "open" || t.state === "merged") return "done";
  return "idle";
}

const STEPS: [string, string[]][] = [
  ["Plan", ["planning", "not_ready"]],
  ["Your call", ["plan_gate"]],
  ["Build", ["building"]],
  ["Check", ["checking", "preview_gate"]],
  ["Open", ["open_gate", "opening"]],
];

function Steps({ t }: { t: BuildTask }) {
  const at = t.state === "open" || t.state === "merged" ? STEPS.length : t.state === "queued" ? -1 : STEPS.findIndex(([, ps]) => ps.includes(t.phase ?? ""));
  return (
    <div className="rv-steps">
      {STEPS.map(([s], i) => (
        <span key={s} className={`rv-step${i < at ? " done" : i === at ? " now" : ""}`}>
          <span className="rv-dot" />
          {s === "Check" && i === at && t.check_n > 0 ? `Check ${t.check_n}` : s === "Your call" && i === at ? "Plan: your call" : s}
        </span>
      ))}
    </div>
  );
}

/** A docket's Build: where it is, what it's built on, and what's decided. */
export function BuildPanel({ d }: { d: Docket }) {
  const hub = useHub();
  const toast = useToast();
  const t = hub.state?.build?.tasks.find((x) => x.docket === d.id);
  const [busy, setBusy] = useState(false);
  if (!t) return null;
  const discard = () => {
    if (busy) return;
    setBusy(true);
    post("/api/build/discard", { docket: d.id }).then(
      () => (setBusy(false), toast(`Discarded ${d.id}'s Build; its branch stays`), hub.reload()),
      (e: Error) => (setBusy(false), toast(e.message, "error")),
    );
  };
  const c = t.last_check;
  return (
    <section className={`rv-panel${t.attention ? " rv-alert" : ""}`}>
      <div className="rv-head">
        <IconBuild size={14} />
        <span className="rv-key">Build</span>
        <span className="muted">into {t.repo}</span>
        {t.branch && (
          <>
            <span className="act-dot">·</span>
            <span className="mono-s muted" title={`from ${t.base ?? "main"}`}>
              {t.branch}
            </span>
          </>
        )}
        <span className="spacer" />
        {t.pr_url && (
          <a className="btn btn-small btn-ghost" href={t.pr_url} target="_blank" rel="noreferrer">
            PR #{t.pr_number} <IconExternal size={11} />
          </a>
        )}
        {(t.state === "active" || t.state === "queued") && (
          <button className="btn btn-small btn-ghost" onClick={discard} disabled={busy} title="Stop the Build: its panes close and its worktree goes; the branch stays">
            Discard
          </button>
        )}
      </div>
      {t.state === "active" || t.state === "queued" || t.state === "open" ? <Steps t={t} /> : <div className="rv-state muted small">{buildPhase(t)}{t.why ? `: ${t.why}` : ""}</div>}
      {t.attention && (
        <div className={`callout ${t.attention.kind === "blocked" ? "callout-blocked" : "callout-error"}`}>
          <LiveIcon state={t.attention.kind === "blocked" ? "blocked" : "error"} />
          <div>
            <strong>{t.attention.kind === "stalled" ? "The Build stalled" : t.attention.kind === "start-failed" ? "The Build didn't start" : t.attention.kind === "pane-closed" ? "An agent's pane closed" : t.attention.kind === "blocked" ? "An agent is blocked" : "Something went wrong"}</strong>
            <div className="callout-msg">{t.attention.text}</div>
          </div>
        </div>
      )}
      <div className="rv-who">
        <span className="rv-agent">
          builder · {t.builder.agent || "claude"}
          {t.builder.effort && <span className="muted"> · {t.builder.effort}</span>}
        </span>
        <span className="rv-agent">
          checker · {t.checker.agent || "claude"}
          {t.checker.effort && <span className="muted"> · {t.checker.effort}</span>}
        </span>
        {t.plan_version > 0 && <span className="rv-agent">plan v{t.plan_version}</span>}
        {t.fix_used > 0 && <span className="rv-agent">{t.fix_used} fix round{t.fix_used === 1 ? "" : "s"}</span>}
      </div>
      {c && (
        <div className="rv-impl">
          <strong>Check {c.n}:</strong> {c.verdict === "pass" ? "passed" : "found problems"}. {c.summary && <InlineMd text={c.summary} />}
          {(c.findings ?? []).length > 0 && <span className="muted"> ({(c.findings ?? []).length} finding{(c.findings ?? []).length === 1 ? "" : "s"})</span>}
        </div>
      )}
      {t.decisions.length > 0 && (
        <div className="rv-hist">
          {t.decisions.map((x) => (
            <div key={x.id} className="bd-dec">
              <span className="rv-hist-round">{x.id}</span>
              <span className="bd-q">{x.question}</span>
              <span className={`bd-a${x.answered ? "" : " open"}`}>{x.answered ? x.label : "open"}</span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

/** "Build it": on a docket with an intent and no Build yet. */
export function BuildIt({ d }: { d: Docket }) {
  const hub = useHub();
  const toast = useToast();
  const menu = useMenu();
  const repos = hub.state?.build?.config?.repos ?? [];
  if (hub.state?.build?.tasks.some((t) => t.docket === d.id && !["merged", "closed", "discarded"].includes(t.state))) return null;
  if (!d.intent?.body?.trim()) return null;
  const start = (repo: string) =>
    post("/api/build/start", { from: d.id, repo }).then(
      () => (toast(`${d.id} is queued to build into ${repo}`), hub.reload()),
      (e: Error) => toast(e.message, "error"),
    );
  return (
    <>
      <button
        className="prop-btn"
        title="Give this docket's intent to a builder and a checker"
        onClick={(e) => (repos.length === 1 ? start(repos[0]) : menu.open(e.currentTarget, repos.map((r) => ({ key: r, label: r, onPick: () => start(r) })), "Build into"))}
      >
        <IconBuild size={14} /> Build it…
      </button>
      {menu.node}
    </>
  );
}

type Group = { key: string; label: string; items: BuildTask[]; icon: ReactNode };

export function BuildsView() {
  const hub = useHub();
  const toast = useToast();
  const bs = hub.state?.build;
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set(["done"]));
  const groups: Group[] = useMemo(() => {
    const ts = bs?.tasks ?? [];
    const by = (f: (t: BuildTask) => boolean) => ts.filter(f).sort((a, b) => (b.updated_at ?? "").localeCompare(a.updated_at ?? ""));
    return [
      { key: "needs", label: "Needs you", icon: <LiveIcon state="blocked" size={13} />, items: by(needsYou) },
      { key: "working", label: "Working", icon: <LiveIcon state="working" size={13} />, items: by((t) => !needsYou(t) && t.state === "active") },
      { key: "queued", label: "Queued", icon: <LiveIcon state="idle" size={13} />, items: by((t) => !needsYou(t) && t.state === "queued") },
      { key: "open", label: "PRs open", icon: <LiveIcon state="done" size={13} />, items: by((t) => t.state === "open") },
      { key: "done", label: "Done this week", icon: <LiveIcon state="idle" size={13} />, items: by((t) => ["merged", "closed", "discarded"].includes(t.state)) },
    ].filter((g) => g.items.length > 0);
  }, [bs]);
  const live = (bs?.tasks ?? []).filter((t) => ["queued", "active", "open"].includes(t.state)).length;
  const cfg = bs?.config;
  const pause = () =>
    post("/api/build/settings", { paused: !bs?.paused }).then(
      () => (toast(bs?.paused ? "Builds resumed" : "Builds paused: nothing new starts"), hub.reload()),
      (e: Error) => toast(e.message, "error"),
    );
  return (
    <div className="view">
      <header className="view-head">
        <span className="view-title">
          <IconBuild size={15} /> Builds <span className="view-count">{live}</span>
        </span>
        <span className="spacer" />
        {bs && (
          <button className="btn btn-ghost" onClick={pause}>
            {bs.paused ? <IconPlay size={13} /> : <IconPause size={13} />} <span className="rv-btn-label">{bs.paused ? "Resume" : "Pause"}</span>
          </button>
        )}
      </header>
      <LargeTitle title="Builds" count={live} />
      {bs && (
        <div className="rv-bar">
          <span className={`rv-pill ${bs.paused ? "paused" : "on"}`}>{bs.paused ? "Paused" : "On"}</span>
          {bs.doing && <span className="rv-doing">{bs.doing}…</span>}
          {cfg && (
            <span className="rv-cfg">
              <span>
                builder {cfg.builder} → checker {cfg.checker}
              </span>
              <span className="act-dot">·</span>
              <span>
                plan {cfg.gates.plan ?? "ask"}, open {cfg.gates.open ?? "ask"}
              </span>
              <span className="act-dot">·</span>
              <span title={cfg.repos.join("\n")}>
                {cfg.repos.length} repo{cfg.repos.length === 1 ? "" : "s"}
              </span>
            </span>
          )}
          {bs.config_error && <span className="rv-err">Config: {bs.config_error}</span>}
        </div>
      )}
      {!bs ? (
        <div className="view-empty">{hub.error ? `Can't reach the hub: ${hub.error}` : ""}</div>
      ) : groups.length === 0 ? (
        <div className="view-empty rv-off">
          <IconBuild size={22} />
          <p>
            <strong>Nothing's building.</strong> Open a docket, write what it's for in its intent, and press <b>Build it</b>: a builder plans it, you approve the plan, a checker checks the work, and you approve the PR before it opens.
          </p>
          <p className="muted small">
            Or from a terminal: <code>wh build start D-12</code>, or a GitHub issue (<code>owner/name#45</code>).
          </p>
        </div>
      ) : (
        <div className="list" style={{ ["--idch" as string]: Math.max(3, ...(bs.tasks ?? []).map((t) => t.docket.length)) }}>
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
              {!collapsed.has(g.key) &&
                g.items.map((t) => (
                  <a key={t.docket} className="row rv-row" href={`#/d/${t.docket}`}>
                    <span className="row-live" title={buildPhase(t)}>
                      <LiveIcon state={buildLive(t)} />
                    </span>
                    <span className="row-id">{t.docket}</span>
                    {t.key !== t.docket && <span className="rv-key-cell">{t.key}</span>}
                    <span className="row-title">{t.title}</span>
                    {t.attention ? <span className="row-note">{t.attention.text}</span> : t.branch ? <span className="rv-author muted mono-s">{t.branch}</span> : null}
                    <span className="row-end">
                      <span className="rv-phase">{buildPhase(t)}</span>
                      {t.pr_url && (
                        <span className="rc-gh" onClick={(e) => (e.preventDefault(), window.open(t.pr_url!, "_blank"))} title="The PR on GitHub">
                          <IconExternal size={12} />
                        </span>
                      )}
                      <span className="row-age">{ago(t.updated_at)}</span>
                    </span>
                  </a>
                ))}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
