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
};

export type HubState = {
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
