import { useEffect, useMemo, useRef, useState } from "react";
import { get } from "./api";
import { AppAvatar, IconBuild, IconChart, IconClipboard, IconExternal, StatusIcon } from "./icons";
import { ago, useHub } from "./live";
import { RiskChip } from "./Reviews";
import { InlineMd, useToast } from "./ui";
import { LargeTitle } from "./mobile";

type PrRecap = {
  key: string;
  full_key: string;
  title?: string | null;
  url?: string | null;
  author?: string | null;
  docket?: string | null;
  rounds: number;
  verdicts: string[];
  merge_risk?: string | null;
  tokens?: number | null;
  additions?: number | null;
  deletions?: number | null;
  implementation?: string | null;
  state: string;
  outcome?: string | null;
  ended_at?: string | null;
  last_at?: string | null;
};

type RecapData = {
  since: string;
  until: string;
  days: number;
  markdown: string;
  reviews: {
    posted: number;
    prev_posted: number;
    per_day: number[];
    approved: number;
    changes: number;
    dismissed: number;
    merged: number;
    prev_merged: number;
    median_to_review?: number | null;
    tokens: number;
    cached?: number | null;
    prs: { merged: PrRecap[]; open: PrRecap[]; closed: PrRecap[] };
  };
  agents: { started: number; turns: number; per_day: number[]; prompts: number; done: { id: string; title: string; app?: string | null; turns: number; at: string; summary?: string | null }[] };
  builds?: {
    opened: number;
    rounds: number;
    replies: number;
    merged: number;
    list: { id: string; title: string; key?: string; pr?: number | null; url?: string | null; opened: boolean; rounds: number; replies: number; outcome?: string | null; at: string }[];
  };
  you: { decisions: { docket: string; title: string; choice: string; at: string }[]; approved: number; denied: number; asked: number; answered: number; intents: number };
};

type Period = "today" | "week" | "30d";
const PERIODS: { key: Period; label: string; short: string; words: string; before: string }[] = [
  { key: "today", label: "Today", short: "Today", words: "today", before: "yesterday" },
  { key: "week", label: "This week", short: "Week", words: "this week", before: "the week before" },
  { key: "30d", label: "30 days", short: "30d", words: "the last 30 days", before: "the 30 days before" },
];

/** The period's bounds in this browser's time zone. */
function bounds(p: Period): { since: Date; until: Date } {
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  const until = new Date(start);
  until.setDate(until.getDate() + 1);
  if (p === "week") start.setDate(start.getDate() - 6);
  if (p === "30d") start.setDate(start.getDate() - 29);
  return { since: start, until };
}

const compact = (n?: number | null) => (!n ? "0" : n >= 1e9 ? `${(n / 1e9).toFixed(1)}B` : n >= 1e6 ? `${(n / 1e6).toFixed(n >= 1e7 ? 0 : 1)}M` : n >= 1e4 ? `${Math.round(n / 1e3)}k` : n.toLocaleString());
const dur = (s?: number | null) => (s == null ? "" : s < 3600 ? `${Math.max(1, Math.round(s / 60))}m` : s < 86400 ? `${Math.floor(s / 3600)}h ${Math.round((s % 3600) / 60)}m` : `${Math.floor(s / 86400)}d ${Math.round((s % 86400) / 3600)}h`);
const pct = (a: number, b: number) => (b ? `${Math.round((a / b) * 100)}%` : "–");

function Delta({ now, before, words }: { now: number; before: number; words: string }) {
  const d = now - before;
  return <span className={`rc-delta ${d > 0 ? "up" : d < 0 ? "down" : ""}`}>{`${d > 0 ? "+" : d < 0 ? "−" : "±"}${Math.abs(d)} vs ${words}`}</span>;
}

/** A per-day sparkline: the past in the muted ink, today in the accent. */
function Spark({ values, since, what }: { values: number[]; since: string; what: string }) {
  const [hover, setHover] = useState<number | null>(null);
  if (values.length < 2) return null;
  const W = 160;
  const H = 28;
  const max = Math.max(1, ...values);
  const x = (i: number) => (i / (values.length - 1)) * (W - 6) + 3;
  const y = (v: number) => H - 4 - (v / max) * (H - 8);
  const pts = values.map((v, i) => `${x(i)},${y(v)}`).join(" ");
  const day = (i: number) => {
    const d = new Date(since);
    d.setDate(d.getDate() + i);
    return d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
  };
  const last = values.length - 1;
  return (
    <div className="rc-spark" onMouseLeave={() => setHover(null)}>
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height={H} preserveAspectRatio="none" role="img" aria-label={`${what} per day`}>
        <polyline points={pts} fill="none" stroke="var(--faint)" strokeWidth="1.5" strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
        {values.map((_, i) => (
          <rect key={i} x={x(i) - W / values.length / 2} y={0} width={W / values.length} height={H} fill="transparent" onMouseEnter={() => setHover(i)} />
        ))}
      </svg>
      <span className="rc-dot" style={{ left: `${(x(hover ?? last) / W) * 100}%`, top: y(values[hover ?? last]) }} />
      {hover !== null && (
        <span className="rc-tip" style={{ left: `${(x(hover) / W) * 100}%` }}>
          {day(hover)}: {values[hover]} {what}
        </span>
      )}
    </div>
  );
}

function Tile({ label, value, children }: { label: string; value: string | number; children?: React.ReactNode }) {
  return (
    <div className="rc-tile">
      <div className="rc-label">{label}</div>
      <div className="rc-value">{value}</div>
      {children}
    </div>
  );
}

const MARK: Record<string, string> = { APPROVED: "✓", CHANGES_REQUESTED: "✕" };

function PrItem({ p, kind }: { p: PrRecap; kind: "merged" | "open" | "closed" }) {
  const trail = p.verdicts.map((v) => MARK[v] ?? "·").join(" → ");
  const last = p.verdicts[p.verdicts.length - 1];
  const meta = [
    kind === "open" ? (last === "APPROVED" ? "approved, not merged yet" : last === "CHANGES_REQUESTED" ? "changes requested" : p.state === "reviewing" ? "in review" : "queued") : trail,
    `${p.rounds} round${p.rounds === 1 ? "" : "s"}`,
    p.tokens ? `${compact(p.tokens)} tokens${kind === "open" ? " so far" : ""}` : null,
    p.author ? `@${p.author}` : null,
  ].filter(Boolean);
  return (
    <a className="rc-pr" href={p.docket ? `#/d/${p.docket}` : p.url || undefined} target={p.docket ? undefined : "_blank"} rel="noreferrer">
      <span className="rc-pr-icon">
        <StatusIcon status={kind === "merged" ? "done" : kind === "closed" ? "dropped" : "in_progress"} size={14} />
      </span>
      <span className="rc-pr-main">
        <span className="rc-pr-top">
          <span className="rc-pr-key">{p.key}</span>
          <span className="rc-pr-title">{p.title || p.key}</span>
          <span className="spacer" />
          <span className="rc-pr-at muted">{ago(p.ended_at || p.last_at)}</span>
        </span>
        <span className="rc-pr-meta">
          {meta.join(" · ")}
          {p.merge_risk && <RiskChip level={p.merge_risk} />}
          {(p.additions != null || p.deletions != null) && (
            <span className="rc-diff">
              <span className="add">+{p.additions ?? 0}</span> <span className="del">−{p.deletions ?? 0}</span>
            </span>
          )}
          {p.url && (
            <span className="rc-gh" onClick={(e) => (e.preventDefault(), e.stopPropagation(), window.open(p.url!, "_blank"))} title="The PR on GitHub">
              <IconExternal size={11} />
            </span>
          )}
        </span>
        {p.implementation && <InlineMd className="rc-pr-impl" text={p.implementation} links={false} />}
      </span>
    </a>
  );
}

function Group({ title, n, children }: { title: string; n: number; children: React.ReactNode }) {
  if (!n) return null;
  return (
    <div className="rc-group">
      <div className="rc-group-head">
        {title} <span className="view-count">{n}</span>
      </div>
      {children}
    </div>
  );
}

export function RecapView() {
  const hub = useHub();
  const toast = useToast();
  const [period, setPeriod] = useState<Period>(() => (localStorage.getItem("wh:recap") as Period) || "week");
  const [data, setData] = useState<RecapData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const meta = PERIODS.find((p) => p.key === period)!;
  const last = useRef(0);
  const load = useMemo(
    () => () => {
      const b = bounds(period);
      const q = new URLSearchParams({ since: b.since.toISOString(), until: b.until.toISOString(), label: meta.words });
      last.current = Date.now();
      get<RecapData>(`/api/recap?${q}`).then(
        (d) => (setData(d), setError(null)),
        (e: Error) => setError(e.message),
      );
    },
    [period, meta.words],
  );
  useEffect(() => {
    try {
      localStorage.setItem("wh:recap", period);
    } catch {
      /* private mode */
    }
    load();
  }, [period, load]);
  // Follow the hub, at most every 15 seconds.
  useEffect(() => {
    if (Date.now() - last.current > 15000) load();
  }, [hub.state, load]);

  const copy = () => {
    if (!data) return;
    navigator.clipboard?.writeText(data.markdown).then(
      () => toast(`Copied the recap for ${meta.words} as Markdown`),
      () => toast("Couldn't reach the clipboard", "error"),
    );
  };

  const r = data?.reviews;
  const ag = data?.agents;
  const you = data?.you;
  return (
    <div className="view">
      <header className="view-head">
        <span className="view-title">
          <IconChart size={15} /> Recap
        </span>
        <div className="seg seg-text rc-periods">
          {PERIODS.map((p) => (
            <button key={p.key} className={period === p.key ? "on" : ""} onClick={() => setPeriod(p.key)}>
              <span className="rc-long">{p.label}</span>
              <span className="rc-short">{p.short}</span>
            </button>
          ))}
        </div>
        <span className="spacer" />
        <button className="btn btn-ghost" onClick={copy} disabled={!data}>
          <IconClipboard size={14} /> <span className="rv-btn-label">Copy as Markdown</span>
        </button>
      </header>
      <LargeTitle title="Recap" />
      {!data ? (
        <div className="view-empty">{error ? `Couldn't make the recap: ${error}` : ""}</div>
      ) : (
        <div className="rc-scroll">
          <div className="rc">
            <div className="rc-tiles">
              <Tile label="Reviews posted" value={r!.posted}>
                <Delta now={r!.posted} before={r!.prev_posted} words={meta.before} />
                <Spark values={r!.per_day} since={data.since} what="reviews" />
              </Tile>
              <Tile label="Approved" value={r!.approved}>
                <span className="rc-sub">{pct(r!.approved, r!.posted)} of reviews</span>
              </Tile>
              <Tile label="Changes requested" value={r!.changes}>
                <span className="rc-sub">{pct(r!.changes, r!.posted)} of reviews</span>
              </Tile>
              <Tile label="PRs merged" value={r!.merged}>
                <Delta now={r!.merged} before={r!.prev_merged} words={meta.before} />
              </Tile>
              <Tile label="Agent turns" value={ag!.turns}>
                <span className="rc-sub">
                  {ag!.done.length} docket{ag!.done.length === 1 ? "" : "s"} done · {ag!.prompts} prompt{ag!.prompts === 1 ? "" : "s"}
                </span>
                <Spark values={ag!.per_day} since={data.since} what="turns" />
              </Tile>
            </div>
            {(r!.median_to_review != null || r!.tokens > 0 || r!.dismissed > 0) && (
              <div className="rc-line">
                {[
                  r!.median_to_review != null && `Median time from request to review: ${dur(r!.median_to_review)}`,
                  r!.tokens > 0 && `${compact(r!.tokens)} review tokens${r!.cached != null ? `, ${Math.round(r!.cached * 100)}% cached` : ""}`,
                  r!.dismissed > 0 && `${r!.dismissed} dismissed`,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </div>
            )}

            <section className="rc-section">
              <h2>Reviews</h2>
              {r!.prs.merged.length + r!.prs.open.length + r!.prs.closed.length === 0 ? (
                <p className="muted rc-empty">No reviews {meta.words}.</p>
              ) : (
                <>
                  <Group title="Merged" n={r!.prs.merged.length}>
                    {r!.prs.merged.map((p) => (
                      <PrItem key={p.full_key} p={p} kind="merged" />
                    ))}
                  </Group>
                  <Group title="Waiting on their authors" n={r!.prs.open.length}>
                    {r!.prs.open.map((p) => (
                      <PrItem key={p.full_key} p={p} kind="open" />
                    ))}
                  </Group>
                  <Group title="Closed without merging" n={r!.prs.closed.length}>
                    {r!.prs.closed.map((p) => (
                      <PrItem key={p.full_key} p={p} kind="closed" />
                    ))}
                  </Group>
                </>
              )}
            </section>

            <section className="rc-section">
              <h2>Agents</h2>
              <p className="rc-line">
                {ag!.started} docket{ag!.started === 1 ? "" : "s"} started · {ag!.done.length} done · {ag!.turns} turns · {ag!.prompts} prompt{ag!.prompts === 1 ? "" : "s"} from you
              </p>
              {ag!.done.map((d) => (
                <a key={d.id} className="rc-pr" href={`#/d/${d.id}`}>
                  <span className="rc-pr-icon">
                    <AppAvatar app={d.app} size={16} />
                  </span>
                  <span className="rc-pr-main">
                    <span className="rc-pr-top">
                      <span className="rc-pr-key">{d.id}</span>
                      <span className="rc-pr-title">{d.title}</span>
                      <span className="spacer" />
                      <span className="rc-pr-at muted">{ago(d.at)}</span>
                    </span>
                    <span className="rc-pr-meta">
                      {d.turns} turn{d.turns === 1 ? "" : "s"} {meta.words}
                    </span>
                    {d.summary && <InlineMd className="rc-pr-impl" text={d.summary} links={false} />}
                  </span>
                </a>
              ))}
            </section>

            {(data.builds?.list.length ?? 0) > 0 && (
              <section className="rc-section">
                <h2>Builds</h2>
                <p className="rc-line">
                  {data.builds!.opened} PR{data.builds!.opened === 1 ? "" : "s"} opened · {data.builds!.rounds} address round{data.builds!.rounds === 1 ? "" : "s"} ({data.builds!.replies} repl{data.builds!.replies === 1 ? "y" : "ies"}) · {data.builds!.merged} merged
                </p>
                {data.builds!.list.map((b) => (
                  <a key={b.id} className="rc-pr" href={`#/d/${b.id}`}>
                    <span className="rc-pr-icon">
                      <IconBuild size={14} />
                    </span>
                    <span className="rc-pr-main">
                      <span className="rc-pr-top">
                        <span className="rc-pr-key">{b.id}</span>
                        <span className="rc-pr-title">{b.title}</span>
                        <span className="spacer" />
                        <span className="rc-pr-at muted">{ago(b.at)}</span>
                      </span>
                      <span className="rc-pr-meta">
                        {[b.pr ? `#${b.pr}` : null, b.opened ? "opened" : null, b.rounds ? `${b.rounds} round${b.rounds === 1 ? "" : "s"}` : null, b.outcome].filter(Boolean).join(" · ")}
                      </span>
                    </span>
                  </a>
                ))}
              </section>
            )}
            <section className="rc-section">
              <h2>You</h2>
              <p className="rc-line">
                {you!.decisions.length} decision{you!.decisions.length === 1 ? "" : "s"} · {you!.approved} approval{you!.approved === 1 ? "" : "s"} · {you!.denied} denied · {you!.intents} intent edit{you!.intents === 1 ? "" : "s"}
                {you!.asked > 0 ? ` · agents asked you ${you!.asked} time${you!.asked === 1 ? "" : "s"}` : ""}
              </p>
              {you!.decisions.map((d, i) => (
                <a key={`${d.docket}-${i}`} className="rc-decision" href={`#/d/${d.docket}`}>
                  <span className="rc-pr-key">{d.docket}</span>
                  <span className="rc-pr-title">{d.title}</span>
                  <span className="rc-choice">“{d.choice}”</span>
                  <span className="rc-pr-at muted">{ago(d.at)}</span>
                </a>
              ))}
            </section>
          </div>
        </div>
      )}
    </div>
  );
}
