import { useEffect, useMemo, useRef, useState } from "react";
import { get } from "./api";
import { AppAvatar, IconBuild, IconChart, IconClipboard, IconExternal, IconPR, StatusIcon } from "./icons";
import { COST_FLOOR, costOf, costWords, money, moneyDelta, nothingPriced, priceWords, sparkTop, type Price } from "./cost";
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
  usage?: Usage;
};

type Split = { input: number; cache_read: number; cache_write: number; output: number; cache_write_1h?: number };
type Work = { agents: number; builds: number; reviews: number };
/** Tokens, and their cost in US dollars at API prices (src/pricing.rs). `unpriced` counts the tokens of models with no price, which the cost leaves out. */
type Usage = {
  tokens: number;
  prev_tokens: number;
  split: Split;
  cached?: number | null;
  per_day: number[];
  providers: { provider: string; name: string; tokens: number; cost?: number; unpriced?: number }[];
  models: { model: string; provider: string; provider_name: string; tokens: number; split: Split; dockets: number; cost?: number | null; unpriced?: number; price?: Price | null }[];
  work: Work;
  dockets: { id: string; title: string; kind: string; tokens: number; models: string[]; cost?: number; unpriced?: number }[];
  cost?: number;
  prev_cost?: number;
  cost_per_day?: number[];
  work_cost?: Work;
  unpriced?: { tokens: number; prev_tokens: number; models: { model: string; provider: string; provider_name: string; tokens: number }[] };
  price_notes?: string[];
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

function Delta({ now, before, words, format = String, neutral = false }: { now: number; before: number; words: string; format?: (n: number) => string; neutral?: boolean }) {
  const d = now - before;
  return <span className={`rc-delta ${neutral ? "" : d > 0 ? "up" : d < 0 ? "down" : ""}`}>{`${d > 0 ? "+" : d < 0 ? "−" : "±"}${format(Math.abs(d))} vs ${words}`}</span>;
}

/** A per-day sparkline: the past in the muted ink, today in the accent. */
function Spark({ values, since, what, format = String, floor = 1 }: { values: number[]; since: string; what: string; format?: (n: number) => string; floor?: number }) {
  const [hover, setHover] = useState<number | null>(null);
  if (values.length < 2) return null;
  const W = 160;
  const H = 28;
  const max = sparkTop(values, floor);
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
          {day(hover)}: {format(values[hover])} {what}
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

/** A provider's color: the same as its agent's avatar. */
const PROVIDER: Record<string, string> = { anthropic: "var(--p-anthropic)", openai: "var(--p-openai)", google: "var(--p-google)" };
const color = (provider: string) => PROVIDER[provider] ?? "var(--progress)";
const share = (n: number, of: number) => (of ? `${n / of >= 0.995 ? 100 : (n / of) * 100 >= 1 ? Math.round((n / of) * 100) : "<1"}%` : "–");
const WORK: { key: keyof Usage["work"]; label: string }[] = [
  { key: "agents", label: "Agents" },
  { key: "builds", label: "Builds" },
  { key: "reviews", label: "Reviews" },
];

/** Tokens across all the work: by provider and model, kind of work, and the dockets that took the most. */
function UsageSection({ u, words }: { u: Usage; words: string }) {
  if (!u.tokens) {
    return (
      <section className="rc-section">
        <h2>Tokens</h2>
        <p className="muted rc-empty">No model calls {words}.</p>
      </section>
    );
  }
  const workMax = Math.max(1, ...WORK.map((w) => u.work[w.key]));
  return (
    <section className="rc-section">
      <h2>Tokens</h2>
      <p className="rc-line">
        {[
          `${compact(u.tokens)} tokens ${words}`,
          costWords(u, compact),
          u.cached != null && `${Math.round(u.cached * 100)}% of input from the cache`,
          `${compact(u.split.output)} output`,
          `${u.models.length} model${u.models.length === 1 ? "" : "s"} from ${u.providers.length} provider${u.providers.length === 1 ? "" : "s"}`,
        ]
          .filter(Boolean)
          .join(" · ")}
      </p>
      <div className="rc-bar" role="img" aria-label={u.models.map((m) => `${m.model}: ${share(m.tokens, u.tokens)}`).join(", ")}>
        {u.models.map((m, i) => (
          <span
            key={`${m.provider}/${m.model}`}
            style={{ flexGrow: m.tokens, background: color(m.provider), opacity: 1 - Math.min(i, 3) * 0.18 }}
            title={`${m.model} (${m.provider_name}): ${compact(m.tokens)}, ${share(m.tokens, u.tokens)}`}
          />
        ))}
      </div>
      <div className="rc-legend">
        {u.providers.map((p) => (
          <span key={p.provider} className="rc-legend-item">
            <span className="rc-swatch" style={{ background: color(p.provider) }} />
            {p.name}{" "}
            <span className="muted">
              {compact(p.tokens)} · {share(p.tokens, u.tokens)}
              {p.cost != null && ` · ${costOf(p)}`}
            </span>
          </span>
        ))}
      </div>
      {(u.unpriced?.tokens ?? 0) > 0 && (
        <p className="rc-line" title="Add a price under prices in settings.json">
          {compact(u.unpriced!.tokens)} tokens unpriced, not in the cost: {u.unpriced!.models.map((m) => `${m.model} (${m.provider_name})`).join(", ")}
        </p>
      )}
      {(u.price_notes?.length ?? 0) > 0 && <p className="rc-line rc-warn">Prices in settings.json not used: {u.price_notes!.join("; ")}</p>}
      <div className="rc-cols">
        <div className="rc-group">
          <div className="rc-group-head">Models</div>
          <div className="rc-table" role="table">
            {u.models.map((m) => {
              const input = m.split.input + m.split.cache_read + m.split.cache_write;
              return (
                <div key={`${m.provider}/${m.model}`} className="rc-row" role="row">
                  <span className="rc-swatch" style={{ background: color(m.provider) }} />
                  <span className="rc-row-name" role="cell">
                    {m.model} <span className="muted">{m.provider_name}</span>
                  </span>
                  <span className="rc-row-meta muted" role="cell" title="Input from the cache, and output">
                    {input ? `${Math.round((m.split.cache_read / input) * 100)}% cached · ` : ""}
                    {compact(m.split.output)} out · {m.dockets} docket{m.dockets === 1 ? "" : "s"}
                  </span>
                  <span className="rc-row-num" role="cell">
                    {compact(m.tokens)}
                  </span>
                  <span className={`rc-row-cost${m.cost == null ? " muted" : ""}`} role="cell" title={priceWords(m.price)}>
                    {costOf(m)}
                  </span>
                  <span className="rc-row-pct muted" role="cell">
                    {share(m.tokens, u.tokens)}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
        <div className="rc-group">
          <div className="rc-group-head">By kind of work</div>
          {WORK.map((w) => (
            <div key={w.key} className="rc-work">
              <span className="rc-work-label">{w.label}</span>
              <span className="rc-work-track">
                <span style={{ width: `${(u.work[w.key] / workMax) * 100}%` }} />
              </span>
              <span className="rc-row-num">{compact(u.work[w.key])}</span>
              {u.work_cost && <span className="rc-row-cost">{money(u.work_cost[w.key])}</span>}
              <span className="rc-row-pct muted">{share(u.work[w.key], u.tokens)}</span>
            </div>
          ))}
        </div>
      </div>
      <Group title="Most tokens" n={u.dockets.length}>
        {u.dockets.map((d) => (
          <a key={d.id} className="rc-pr" href={`#/d/${d.id}`}>
            <span className="rc-pr-icon">{d.kind === "builds" ? <IconBuild size={14} /> : d.kind === "reviews" ? <IconPR size={14} /> : <StatusIcon status="in_progress" size={14} />}</span>
            <span className="rc-pr-main">
              <span className="rc-pr-top">
                <span className="rc-pr-key">{d.id}</span>
                <span className="rc-pr-title">{d.title}</span>
                <span className="spacer" />
                <span className="rc-pr-at rc-row-num">
                  {compact(d.tokens)}
                  {d.cost != null && <span className="muted"> · {costOf(d)}</span>}
                </span>
              </span>
              <span className="rc-pr-meta">
                {[
                  WORK.find((w) => w.key === d.kind)?.label.replace(/s$/, ""),
                  d.models.join(", "),
                  `${share(d.tokens, u.tokens)} of the tokens`,
                  d.unpriced && d.cost != null && d.unpriced < d.tokens ? `${compact(d.unpriced)} tokens unpriced` : null,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
            </span>
          </a>
        ))}
      </Group>
    </section>
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
  const asked = useRef(0);
  const load = useMemo(
    () => () => {
      const b = bounds(period);
      const q = new URLSearchParams({ since: b.since.toISOString(), until: b.until.toISOString(), label: meta.words });
      last.current = Date.now();
      // Only the latest ask's answer shows: a slow first read for one
      // period doesn't land under another's.
      const ask = ++asked.current;
      get<RecapData>(`/api/recap?${q}`).then(
        (d) => ask === asked.current && (setData(d), setError(null)),
        (e: Error) => ask === asked.current && setError(e.message),
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
              {data.usage && (
                <Tile label="Tokens" value={compact(data.usage.tokens)}>
                  <Delta now={data.usage.tokens} before={data.usage.prev_tokens} words={meta.before} format={compact} neutral />
                  <Spark values={data.usage.per_day} since={data.since} what="tokens" format={compact} />
                </Tile>
              )}
              {data.usage?.cost != null &&
                (nothingPriced(data.usage) ? (
                  <Tile label="Cost" value="–">
                    <span className="rc-sub">{costWords(data.usage, compact)}</span>
                  </Tile>
                ) : (
                  <Tile label="Cost" value={money(data.usage.cost)}>
                    <span className="rc-delta">{`${moneyDelta(data.usage.cost, data.usage.prev_cost)} vs ${meta.before}`}</span>
                    <span className="rc-sub" title="What the tokens cost at each model's API price">
                      {(data.usage.unpriced?.tokens ?? 0) > 0 ? `${compact(data.usage.unpriced!.tokens)} tokens unpriced` : "at API prices"}
                    </span>
                    <Spark values={data.usage.cost_per_day ?? []} since={data.since} what="spent" format={money} floor={COST_FLOOR} />
                  </Tile>
                ))}
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

            {data.usage && <UsageSection u={data.usage} words={meta.words} />}

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
