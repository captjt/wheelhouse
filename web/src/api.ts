// The hub's API, as the app sees it. See src/server.rs.

export type Live = {
  block: string;
  record?: string | null;
  /** Wheelhouse can answer this prompt with a key (a Claude Code permission). */
  answerable?: boolean;
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
  quick: boolean;
  intent?: { body: string; source: string; by: string; at: string; seq: number };
  intent_versions?: number;
  linear?: LinearRef;
  review?: ReviewRef;
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
  draft?: { event: string; verdict?: string | null; blocking: number; non_blocking: number; merge_risk?: string | null } | null;
};

export type ReviewStatus = {
  enabled: boolean;
  paused: boolean;
  post?: "ask" | "auto" | null;
  config?: { path: string; repos: string[]; reviewers: { name: string; agent: string }[]; checker: string; post: string; max_rounds: number; poll_seconds: number } | null;
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

export type HubState = {
  review?: ReviewStatus;
  messages: Msg[];
  owner: string;
  hooks: boolean;
  linear: boolean;
  rex: { connected: boolean; error?: string | null };
  dockets: Docket[];
  asks: Ask[];
  agents: (Live & { docket?: string | null })[];
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

export type Verify = {
  docket: string;
  ok: boolean;
  entries: number;
  head?: string | null;
  problems: string[];
};

export type DocketPage = { docket: Docket; entries: EntryView[]; verify: Verify };

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
    throw new Error(msg || `${res.status}`);
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
  app === "claude-code" ? "Claude Code" : app === "codex" ? "Codex" : app || "Agent";
