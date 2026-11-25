// The hub's API, as the app sees it. See src/server.rs.

/** A terminal multiplexer Wheelhouse's agents run in. See docs/runtimes.md. */
export type RuntimeName = "rex" | "herdr";

/** A runtime as the hub follows it: `runtimes` in /api/state. */
export type RuntimeLink = { name: RuntimeName; title: string; connected: boolean; error?: string | null; installed: boolean; agents: number; spawn: boolean };

/** The runtime a pane id is in: Herdr's start `herdr:`, Rex's are its own. */
export const runtimeOf = (block?: string | null): RuntimeName => (block?.startsWith("herdr:") ? "herdr" : "rex");
export const runtimeTitle = (r: RuntimeName): string => (r === "herdr" ? "Herdr" : "Rex");

/** Where a docket's agent runs, for people: "Rex", "Herdr". */
export const paneRuntime = (d?: { live?: Live | null; agent?: AgentRef | null } | null): string => runtimeTitle(d?.live?.runtime ?? runtimeOf(d?.agent?.block));

/** The agents on a docket now. An older hub's docket: its one agent, when live. */
export const agentsOn = (d?: Docket | null): DocketAgent[] => {
  if (!d) return [];
  if (d.agents) return d.agents.filter((a) => a.on);
  return d.agent && d.live ? [{ ...d.agent, session: d.session, joined_at: d.created_at, active_at: d.updated_at, on: true, live: d.live }] : [];
};

/** The agents on a docket whose panes are live now, the lead first. */
export const liveAgents = (d?: Docket | null): DocketAgent[] => {
  const on = agentsOn(d).filter((a) => a.live);
  return on.sort((a, b) => Number(b.block === d?.lead) - Number(a.block === d?.lead));
};

/** An agent, for people, beside others on its docket: its role, else its pane, else its app. */
export const agentLabel = (a?: { role?: string | null; pane?: string; app: string } | null): string => (a ? a.role || a.pane || appName(a.app) : "the agent");

/** The agent an entry names (`data.block`, or an agent actor's `block`), on a docket that may have several.
 * An entry from before agents were named (no pane) is the docket's agent's only when it has had one:
 * with several, it's no one's in particular, never the agent that happens to lead now. */
export const agentOfEntry = (d: Docket, e: { actor?: Record<string, unknown>; data?: Record<string, unknown> }): DocketAgent | AgentRef | undefined => {
  const block = entryBlock(e);
  if (block) return d.agents?.slice().reverse().find((a) => a.block === block) ?? ((d.agents?.length ?? 0) > 1 ? undefined : d.agent);
  return (d.agents?.length ?? 0) > 1 ? undefined : d.agent;
};

/** The pane an entry names as its agent's, if any. */
export const entryBlock = (e: { actor?: Record<string, unknown>; data?: Record<string, unknown> }): string | undefined =>
  (e.data?.block as string | undefined) || ((e.data?.agent as { block?: string } | undefined)?.block) || (e.actor?.block as string | undefined) || undefined;

/** Whether a prompt can be answered where it's shown cut short: an older hub doesn't say, so answerable. */
export const quick = (l?: Live | null): boolean => !!l?.answerable && (l.short ?? true);

/** The runtimes the hub follows now, for people: "Rex and Herdr". */
export const followedRuntimes = (s?: { runtimes?: RuntimeLink[] } | null): string => {
  const on = s?.runtimes ? s.runtimes.filter((r) => r.connected).map((r) => r.title) : ["Rex"];
  return on.length ? on.join(" and ") : "Rex or Herdr";
};

/** Where new agents open, for people. */
export const spawnRuntime = (s?: { runtimes?: RuntimeLink[] } | null): string => s?.runtimes?.find((r) => r.spawn)?.title ?? "Rex";

export type Live = {
  block: string;
  /** The runtime the pane is in (older hubs leave it out: Rex). */
  runtime?: RuntimeName;
  record?: string | null;
  /** Wheelhouse can answer this prompt with a key (a Claude Code permission). */
  answerable?: boolean;
  /** It can also be answered where it's shown cut short (a menu, a row, the palette): one short line. */
  short?: boolean;
  /** It takes a message mid-turn (steering): Claude Code, Codex. */
  steer?: boolean;
  app: string;
  state: string;
  kind?: string | null;
  msg?: string | null;
  since?: string | null;
  rex_session: string;
  pane: string;
  cwd?: string | null;
};

export type AgentRef = {
  app: string;
  host: string;
  block: string;
  rex_session: string;
  pane: string;
  cwd?: string | null;
};

/** One agent on a docket (src/record/fold.rs `DocketAgent`), as the hub serves it. */
export type DocketAgent = AgentRef & {
  session?: string | null;
  /** What it's there for: `Build builder`, `claude reviewer`, `tests`. */
  role?: string | null;
  round?: number | null;
  joined_at: string;
  active_at: string;
  left_at?: string | null;
  left?: string | null;
  /** Still on the docket. */
  on: boolean;
  runtime?: RuntimeName;
  /** What it's doing now, when its pane is live. */
  live?: Live | null;
  /** Its message address, when live. */
  addr?: string | null;
  /** On a shared docket, the workspace member whose agent it is; none for this hub's own dockets. */
  member?: string | null;
};

export type Docket = {
  id: string;
  title: string;
  title_source: string;
  status: "todo" | "in_progress" | "done" | "dropped";
  protocol: string;
  agent?: AgentRef;
  session?: string;
  prompts: number;
  turns: number;
  notes: number;
  last_prompt?: string;
  last_turn?: string;
  released?: string;
  created_at: string;
  updated_at: string;
  ended_at?: string;
  entries: number;
  head: string;
  short_head: string;
  live: Live | null;
  /** Every agent that's been on it (on /api/state, only those on it now). Older hubs leave it out. */
  agents?: DocketAgent[];
  /** The pane of the agent shown first: the one that needs you most. */
  lead?: string | null;
  quick: boolean;
  intent?: { body: string; source: string; by: string; at: string; seq: number };
  intent_versions?: number;
  linear?: LinearRef;
  /** Other issues linked beside it: a PR can address more than one. */
  linear_also?: { key: string; url?: string | null; title?: string | null }[];
  review?: ReviewRef;
  /** Why it's on hold (Build and Review take no steps), or null. */
  hold?: string | null;
  /** Closed with nothing witnessed behind it: your reason. */
  closed_on?: string | null;
  /** Its witnessed checks, the latest of each. */
  checks?: Record<string, { ok: boolean; head: string; at: string; required: boolean }> | null;
  /** A local docket that was shared: the shared docket it went on as (`ACME-31`). */
  shared_as?: string | null;
  /** A shared docket: who shared it, and from which of their dockets (`jordan/D-12`). */
  shared_from?: string | null;
  /** A shared docket's owners and members, by handle. */
  owners?: string[];
  members?: string[];
};

/** One witnessed check: the hub ran it itself, at a commit (src/checks.rs). */
export type CheckRun = {
  name: string;
  command: string;
  required: boolean;
  /** null: it timed out. */
  exit: number | null;
  ok: boolean;
  ms: number;
  head: string;
  dirty?: boolean;
  /** `sha256:<hex>` of what it printed. */
  output?: string | null;
  run?: string;
  why?: string | null;
  seq: number;
  at: string;
  repo?: string | null;
};

/** A checker's judgment, said by the checker. */
export type VerdictView = {
  seq: number;
  at: string;
  n?: number | null;
  verdict?: string | null;
  findings?: number | null;
  summary?: string | null;
  by: string;
  role?: string | null;
  /** From a session other than the builder's; null when it can't say. */
  independent?: boolean | null;
  head?: string | null;
  mark: Mark;
};

/** What a docket's done rests on, and what checks it (src/evidence.rs). */
export type Evidence = {
  /** Its intent has acceptance items, so done needs evidence. */
  needs: boolean;
  evidence: { how: "witnessed" | "checkable"; what: string; seq?: number | null; at?: string | null }[];
  checks: CheckRun[];
  repo?: string | null;
  configured: { name: string; run: string; required: boolean }[];
  running: boolean;
  verdicts: VerdictView[];
  hold?: string | null;
  closed_on?: string | null;
};

/** What needs your attention beyond your asks, worst first. */
export type Attention = {
  docket?: string | null;
  title: string;
  kind: "build" | "review" | "failing" | "held" | "long" | "quiet" | "word" | string;
  text: string;
  at?: string | null;
};

export type LinearRef = {
  key: string;
  id?: string | null;
  url?: string | null;
  title?: string | null;
  state?: string | null;
  state_type?: string | null;
  priority?: string | null;
  assignee?: string | null;
  branch?: string | null;
  updated_at?: string | null;
  description?: string | null;
};

export type LinearIssue = {
  id: string;
  identifier: string;
  title: string;
  url: string;
  state?: string | null;
  state_type?: string | null;
  priority?: string | null;
  assignee?: string | null;
  team?: string | null;
  updated_at: string;
};

/** Linear's state type as a docket status, for the icon. */
export const linearStatus = (t?: string | null) =>
  t === "started" ? "in_progress" : t === "completed" ? "done" : t === "canceled" ? "dropped" : "todo";

export type Ask = {
  docket: string;
  /** The asking agent's pane (older hubs leave it out). */
  block?: string;
  title?: string | null;
  what: "blocked" | "failed";
  kind?: string | null;
  msg?: string | null;
  since?: string | null;
  app: string;
};

export type Msg = {
  id: number;
  docket?: string | null;
  from: string;
  from_app?: string | null;
  /** A teammate's agent's message (docs/teams.md): whose agent it is. */
  from_member?: string | null;
  to: string;
  subject?: string | null;
  body: string;
  options: string[];
  reply_to?: number | null;
  choice?: string | null;
  status: string;
  created_at: string;
  preview?: { url: string; local: string; https_port: number; app_port: number; down?: string } | null;
};

/** One round of a PR's review, as its history records it. */
export type ReviewRound = {
  round: number;
  verdict: string;
  by?: string;
  head?: string;
  at?: string;
  url?: string | null;
  blocking?: number;
  non_blocking?: number;
  merge_risk?: string | null;
  checker_effort?: string | null;
  tokens?: { total?: number } | null;
  reviewers?: Record<string, { drafted?: number; kept?: number; only?: number; effort?: string | null; tokens?: number }> | null;
};

/** A PR in the Review protocol. */
export type ReviewPr = {
  key: string;
  full_key: string;
  repo: string;
  pr: number;
  url?: string | null;
  title?: string | null;
  author?: string | null;
  state: string;
  phase?: string | null;
  round: number;
  verdict?: string | null;
  merge_risk?: string | null;
  review_url?: string | null;
  implementation?: string | null;
  docket?: string | null;
  head?: string | null;
  round_head?: string | null;
  reviewers_done?: number | null;
  reviewers_total?: number | null;
  reviewers: { name: string; agent: string; effort?: string | null }[];
  checker_effort?: string | null;
  start_error?: string | null;
  attention?: { kind: string; text: string; round: number } | null;
  posted_at?: string | null;
  updated_at?: string | null;
  history: ReviewRound[];
  /** The checked review of the round in progress, once it's written. */
  /** The checked review, in brief; `digest` names its exact text, so a post is of what you saw. */
  draft?: { event: string; verdict?: string | null; blocking: number; non_blocking: number; merge_risk?: string | null; digest?: string } | null;
  /** How a checked review posts in this PR's repo now: `ask`, `auto` or `rule`. */
  post?: "ask" | "auto" | "rule" | null;
  /** Under a rule, what the hub decided for this round's checked review, and why; `refused` if its post was. */
  post_decision?: { auto: boolean; why: string; refused?: string | null } | null;
};

/** A docket's Build. */
export type BuildTask = {
  docket: string;
  key: string;
  title: string;
  url?: string | null;
  source: string;
  repo: string;
  state: string;
  phase?: string | null;
  branch?: string | null;
  base?: string | null;
  plan_version: number;
  check_n: number;
  fix_used: number;
  last_check?: { n: number; verdict: string; summary?: string; findings?: { id: string; severity: string; what: string }[] } | null;
  checked_head?: string | null;
  pushed_head?: string | null;
  pr_number?: number | null;
  pr_url?: string | null;
  builder: { agent?: string; effort?: string | null };
  checker: { agent?: string; effort?: string | null };
  decisions: { id: string; question: string; answered: boolean; label?: string | null }[];
  asks: string[];
  why?: string | null;
  start_error?: string | null;
  updated_at?: string | null;
  tokens?: number | null;
  attention?: { kind: string; text: string } | null;
  /** Address rounds since the PR opened, and the one in progress. */
  rounds?: number;
  round?: { n: number; summary?: string | null; update?: string | null } | null;
  /** Things open on the PR waiting on "address it?". */
  pending?: number | null;
  /** Reviewers who review the branch blind before the PR opens. */
  prereview?: { n: number; reviewers?: string[] | null; blocking?: number | null; non_blocking?: number | null } | null;
};

export type BuildStatus = {
  paused: boolean;
  doing?: string | null;
  config?: { path?: string | null; repos: string[]; builder: string; checker: string; gates: Record<string, string>; max_tasks: number } | null;
  config_error?: string | null;
  tasks: BuildTask[];
};

export type ReviewStatus = {
  enabled: boolean;
  paused: boolean;
  /** What posting does now, by default, after any override. */
  post?: "ask" | "auto" | "rule" | null;
  /** `text` ends "A checked review …". */
  post_rule?: { mode: string; text: string } | null;
  /** `wh review post-mode ask|auto`, over every repo's rule. */
  post_override?: "ask" | "auto" | null;
  config?: {
    path: string;
    repos: string[];
    reviewers: { name: string; agent: string }[];
    checker: string;
    /** The config's own posting by default, without the override. */
    post: string;
    /** Repos that post their own way. */
    repo_posts?: Record<string, { mode: string; text: string }>;
    max_rounds: number;
    poll_seconds: number;
  } | null;
  config_error?: string | null;
  last_scan_at?: string | null;
  scan_error?: string | null;
  doing?: string | null;
  rex_reviewer_running: boolean;
  prs: ReviewPr[];
};


export type ReviewRef = {
  key: string;
  repo: string;
  pr: number;
  url?: string | null;
  title?: string | null;
  author?: string | null;
  state?: string | null;
  phase?: string | null;
  round?: number | null;
  verdict?: string | null;
  merge_risk?: string | null;
  review_url?: string | null;
  implementation?: string | null;
};

/** A second hub beside the daily one (Wheelhouse Dev), and the build it runs; null on the daily hub. */
export type Flavor = { name: string; commit: string; branch: string };

export type HubState = {
  review?: ReviewStatus;
  build?: BuildStatus;
  messages: Msg[];
  owner: string;
  flavor?: Flavor | null;
  hooks: boolean;
  linear: boolean;
  rex: { connected: boolean; error?: string | null };
  /** Every runtime, followed or not (older hubs leave it out). */
  runtimes?: RuntimeLink[];
  /** The workspace this hub is a member of (docs/teams.md); `workspace` is null when it's in none. */
  team?: TeamStatus;
  dockets: Docket[];
  asks: Ask[];
  agents: (Live & { docket?: string | null; role?: string | null })[];
  attention?: Attention[];
};

/** This hub's workspace, as `/api/team` and `/api/state` serve it (src/team/member.rs `status`). */
export type TeamStatus = {
  workspace: { name: string; prefix: string; url: string; id: string; you: string } | null;
  online?: boolean;
  error?: string | null;
  members?: { handle: string; name: string; role: "admin" | "member"; login?: string | null; active: boolean; joined_at: string; you: boolean }[];
  dockets?: {
    id: string;
    title?: string | null;
    status?: Docket["status"] | null;
    owners: string[];
    members: string[];
    agents?: { member?: string | null; app: string; pane: string; role?: string | null }[] | null;
    halted?: string | null;
  }[];
  aliases?: Record<string, string>;
  outbox?: { n: number; docket: string; status: "queued" | "held" | "refused"; why?: string | null; at: string }[];
  halted?: Record<string, string>;
  log?: { entries: number; head: string };
};

export type Mark = "said" | "witnessed" | "checkable";

export type EntryView = {
  seq: number;
  id: string;
  chain: string;
  kind: string;
  at: string;
  actor: { type: string; id: string; [k: string]: unknown };
  body?: string | null;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  data: any;
  mark: Mark;
  key?: string | null;
};

/** The record as the record view reads it: each entry with its key, mark and problems. */
export type RecordEntry = EntryView & { blobs: string[]; replaces?: string | null; keyid?: string | null; problems: string[] };
export type RecordView = {
  docket: string;
  title: string;
  head: string;
  short_head: string;
  keys: { keyid: string; name?: string | null; entries: number }[];
  verify: Verify;
  entries: RecordEntry[];
};

export type Verify = {
  docket: string;
  ok: boolean;
  entries: number;
  head?: string | null;
  problems: string[];
};

/** A question an agent put to you, and the answer that stands, or a decision of your own from the composer (no `message`) (src/decisions.rs). */
export type Decision = {
  key: string;
  question: string;
  options: string[];
  asked_by: string;
  asked_at: string;
  message?: number | null;
  answer?: string | null;
  note?: string | null;
  /** `you`, an agent's app, or `stood` (the recommendation, with the plan). */
  by?: string | null;
  at?: string | null;
};

/** What a call proposes for a docket (src/calls.rs `Proposal`), and what became of it. */
export type Proposal = {
  n: number;
  seq: number;
  at: string;
  kind: "decide" | "ask" | "task";
  docket?: string | null;
  subject?: string | null;
  choice?: string | null;
  question?: string | null;
  options: string[];
  title?: string | null;
  intent?: string | null;
  quote: string;
  quote_found: boolean;
  why?: string | null;
  status: "open" | "applied" | "ignored";
  outcome?: { seq: number; at: string; applied?: { docket?: string; did?: string } | null; edited?: Record<string, unknown> | null };
};

/** A call's proposals and how the Listener's reading went (src/calls.rs `view`). */
export type CallView = {
  listening: { state: "started" | "done" | "failed" | "stopped" | null; at?: string; why?: string | null; left_out?: string[] | null; proposals?: number | null };
  proposals: Proposal[];
};

export type DocketPage = { docket: Docket; entries: EntryView[]; verify: Verify; queued?: { id: number; body: string; at: string }[]; decisions?: Decision[]; evidence?: Evidence; call?: CallView };

/** A refusal from the hub: its sentence, and its status (409: not now, or not on what was said). */
export type ApiError = Error & { status?: number };

async function handle<T>(res: Response): Promise<T> {
  const text = await res.text();
  let body: unknown = text;
  try {
    body = JSON.parse(text);
  } catch {
    /* not JSON */
  }
  if (!res.ok) {
    const msg = typeof body === "object" && body && "error" in body ? String((body as { error: unknown }).error) : text;
    const err: ApiError = new Error(msg || `${res.status}`);
    err.status = res.status;
    throw err;
  }
  return body as T;
}

/** A GET's body as text, so a caller can tell an unchanged answer cheaply. */
export async function getText(path: string): Promise<string> {
  const res = await fetch(path, { headers: { Accept: "application/json" } });
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      const b = JSON.parse(text);
      if (b && typeof b === "object" && "error" in b) msg = String(b.error);
    } catch {
      /* plain text */
    }
    throw new Error(msg || `${res.status}`);
  }
  return text;
}

export async function get<T>(path: string): Promise<T> {
  return handle<T>(await fetch(path, { headers: { Accept: "application/json" } }));
}

export async function post<T>(path: string, body: unknown = {}): Promise<T> {
  return handle<T>(
    await fetch(path, {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Wheelhouse": "web" },
      body: JSON.stringify(body),
    }),
  );
}

export const appName = (app?: string | null) =>
  app === "claude-code" ? "Claude Code" : app === "codex" ? "Codex" : app === "agy" ? "Antigravity" : app === "pi" ? "Pi" : app || "Agent";

/** The agents Wheelhouse starts (src/start.rs), in the order its pickers show them. */
export const startApps = ["claude-code", "codex", "agy", "pi"] as const;
export type StartApp = (typeof startApps)[number];

/** The agent to start on a docket: the kind already on it, else Claude Code. */
export const startAppFor = (app?: string | null): StartApp =>
  app === "antigravity" ? "agy" : (startApps as readonly string[]).includes(app ?? "") ? (app as StartApp) : "claude-code";

/** An agent's app from the name a message gives it (`from_app`: "Codex"). */
export const appOfName = (name?: string | null): string => startApps.find((a) => appName(a) === name) ?? "claude-code";

/** On a shared docket (docs/teams.md), the teammate an entry's writer
 * belongs to (`sam`, `hub@sam`, `codex@sam`); null when it's yours, or the
 * docket isn't shared. `me` is your handle in the workspace. */
export function mateOf(actor: { type: string; id: unknown }, docket: string, me?: string | null): string | null {
  if (docket.startsWith("D-")) return null;
  const id = String(actor.id);
  const member = id.includes("@") ? id.split("@").pop() : actor.type === "person" ? id : undefined;
  return member && member !== "local" && member !== me ? member : null;
}

/** Who wrote an entry, in words: "You", "sam", "Codex", "sam's Codex", "Wheelhouse". */
export function actorName(actor: { type: string; id: unknown }, docket: string, me?: string | null): string {
  const mate = mateOf(actor, docket, me);
  if (actor.type === "person") return mate ?? "You";
  const whose = mate ? `${mate}'s ` : "";
  if (actor.type === "agent") return whose + appName(String(actor.id).split("@")[0]);
  return whose + "Wheelhouse";
}

/** The footer's line: the runtimes followed, or the one that's down. A
 * runtime that isn't installed is no trouble; one that is and isn't
 * followed is, and so is having none. Older hubs only say Rex's. */
export function connLine(s: Pick<HubState, "rex" | "runtimes">): { ok: boolean; text: string; title?: string } {
  const rs = s.runtimes ?? [{ name: "rex" as const, title: "Rex", connected: s.rex.connected, error: s.rex.error, installed: true, agents: 0, spawn: true }];
  const down = rs.find((r) => r.installed && !r.connected && (r.spawn || rs.every((x) => !x.connected)));
  if (down) return { ok: false, text: `${down.title}: ${down.error || "connecting"}` };
  const on = rs.filter((r) => r.connected).map((r) => r.title);
  if (!on.length) return { ok: false, text: `${spawnRuntime(s)} isn't installed` };
  // Two fit the sidebar's footer as "Rex · Herdr"; the title says it whole.
  return { ok: true, text: on.length > 1 ? on.join(" · ") : `Following ${on[0]}`, title: `Following ${on.join(" and ")}` };
}
