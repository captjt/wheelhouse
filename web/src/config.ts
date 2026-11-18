// Settings, as the hub shows them (GET /api/settings, src/settings.rs), and
// the patches that change them. A patch is shaped like the Lua configs and
// laid over them: objects merge key by key, `null` takes a value set here
// away (back to build.lua's, review.lua's, or the default), and anything
// else replaces. Pure, so `npm test` runs it without a browser.

export type Gate = { key: string; label: string; value: string; ask: string; auto: string };
export type CheckSpec = { name: string; run: string; required: boolean; timeout: number };
export type BuildRepo = { checkout: string; notes: string; base: string | null; plan: string | null; checks: CheckSpec[] };
export type BuildConfig = {
  gates: Gate[];
  builder: string;
  checker: string;
  prereview: string[];
  prereview_loops: number;
  fix_loops: number;
  max_tasks: number;
  update: string;
  pr: { draft: boolean; reviewers: string[] };
  github_label: string | null;
  linear: unknown;
  guides: { plan_format: unknown; pr_style: unknown };
  repos: Record<string, BuildRepo>;
};
export type ReviewRepo = { checkout: string; notes: string; guides: Record<string, unknown> | null };
export type ReviewConfig = {
  reviewers: { name: string; agent: string }[];
  checker: string;
  post: string;
  max_rounds: number;
  enabled: boolean;
  guides: unknown;
  repos: Record<string, ReviewRepo>;
};
export type Part<C> = { lua: string; lua_exists: boolean; error: string | null; config: C | null };
export type Order = { protocol: "review" | "build" | "verify" | string; on: boolean; paused: boolean; text: string };
export type SettingsDoc = {
  id: string;
  kind: "guide" | "brief";
  protocol: "review" | "build" | "";
  title: string;
  about: string;
  builtin: boolean;
  overridden: boolean;
  path: string;
  used_by: string[];
};
export type DocText = SettingsDoc & { text: string; original: string | null };
export type Settings = {
  file: string;
  set: Record<string, unknown>;
  set_error: string | null;
  build: Part<BuildConfig>;
  review: Part<ReviewConfig>;
  orders: Order[];
  docs: SettingsDoc[];
  history: string;
};

/** A path into the configs: `["build", "repos", "acme/api", "notes"]` (a slug has a slash, so not dots). */
export type Path = string[];

/** The patch that sets one value: `patchAt(["build", "gates", "open"], "auto")`. `null` takes it away. */
export function patchAt(path: Path, value: unknown): Record<string, unknown> {
  if (!path.length) throw new Error("a patch needs a path");
  let out: unknown = value;
  for (let i = path.length - 1; i >= 0; i--) out = { [path[i]]: out };
  return out as Record<string, unknown>;
}

/** Several patches, merged into one (later ones win where they meet). */
export function mergePatches(...ps: Record<string, unknown>[]): Record<string, unknown> {
  const merge = (a: Record<string, unknown>, b: Record<string, unknown>) => {
    for (const [k, v] of Object.entries(b)) {
      const cur = a[k];
      if (v && typeof v === "object" && !Array.isArray(v) && cur && typeof cur === "object" && !Array.isArray(cur)) merge(cur as Record<string, unknown>, v as Record<string, unknown>);
      else a[k] = v;
    }
    return a;
  };
  return ps.reduce<Record<string, unknown>>((acc, p) => merge(acc, structuredClone(p)), {});
}

/** The value at `path`, or undefined when there's none. */
export function valueAt(obj: unknown, path: Path): unknown {
  let v: unknown = obj;
  for (const k of path) {
    if (!v || typeof v !== "object" || Array.isArray(v) || !(k in (v as object))) return undefined;
    v = (v as Record<string, unknown>)[k];
  }
  return v;
}

/** Whether Settings set this value (it's in settings.json), rather than the Lua or the default. */
export const setHere = (set: Record<string, unknown> | null | undefined, path: Path) => valueAt(set ?? {}, path) !== undefined;

/** The agents, by their config names. */
export const AGENTS: { key: string; label: string }[] = [
  { key: "claude", label: "Claude Code" },
  { key: "codex", label: "Codex" },
];
export const agentLabel = (a?: string | null) => AGENTS.find((x) => x.key === a)?.label ?? a ?? "";

/**
 * A role (builder, checker) set to an agent: as a table, so an effort
 * function build.lua gives the role stays with it.
 */
export const rolePatch = (part: "build" | "review", role: string, agent: string) => patchAt([part, role], { agent });

/** How much of the plan goes on the PR, and what each means. */
export const PLAN_MODES: { key: "full" | "summary" | "off"; label: string; text: string }[] = [
  { key: "full", label: "Full", text: "The plan goes on the PR as written: Intent, Acceptance, Out of scope and Notes." },
  { key: "summary", label: "Summary", text: "The builder writes the plan in its own words, without quoting the issue: for a public repo." },
  { key: "off", label: "Off", text: "No plan on the PR." },
];
/** A plan mode as build.lua writes it: `full`, `summary`, or `false`. */
export const planValue = (mode: string) => (mode === "off" ? false : mode);

/** The default for a check's timeout, in seconds, and its bounds. */
export const CHECK_TIMEOUT = 1200;
export const TIMEOUT_MIN = 10;
export const TIMEOUT_MAX = 7200;

/**
 * A check as the patch writes it: just its command when it's required with
 * the default timeout (`test = "cargo test"`), or the whole table. The whole
 * table, never a part: laid over build.lua's, nothing of the old one stays.
 */
export function checkValue(c: Pick<CheckSpec, "run" | "required" | "timeout">): string | { run: string; required: boolean; timeout: number } {
  const run = c.run.trim();
  if (c.required && c.timeout === CHECK_TIMEOUT) return run;
  return { run, required: c.required, timeout: c.timeout };
}

/** What's wrong with a check before it's sent, in a sentence, or null. */
export function checkProblem(c: Pick<CheckSpec, "name" | "run" | "timeout">, others: string[] = []): string | null {
  const name = c.name.trim();
  if (!name) return "Give it a name, like test or lint.";
  if (!/^[A-Za-z0-9][A-Za-z0-9_-]*$/.test(name)) return "A check's name is letters, digits, dashes and underscores.";
  if (others.includes(name)) return `There's already a check named ${name}.`;
  if (!c.run.trim()) return "Give it a command to run, like cargo test.";
  if (!Number.isInteger(c.timeout) || c.timeout < TIMEOUT_MIN || c.timeout > TIMEOUT_MAX) return `Its timeout is ${TIMEOUT_MIN} to ${TIMEOUT_MAX} seconds.`;
  return null;
}

/** A timeout, readably: 1200 → "20m", 90 → "1m 30s". */
export function timeoutWord(s: number): string {
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return s % 60 ? `${m}m ${s % 60}s` : `${m}m`;
}

/** Every repo either protocol knows, sorted. */
export function repoSlugs(s: Pick<Settings, "build" | "review">): string[] {
  return [...new Set([...Object.keys(s.build.config?.repos ?? {}), ...Object.keys(s.review.config?.repos ?? {})])].sort();
}

/** A new guide's id from the name you type: `API review` won't do, `api-review` → `guide/api-review.md`. */
export function guideId(name: string): { id: string } | { error: string } {
  const n = name.trim().replace(/\.md$/, "");
  if (!n) return { error: "Name it: lowercase letters, digits and dashes, like api-review." };
  if (!/^[a-z0-9][a-z0-9-]*$/.test(n)) return { error: "A guide's name is lowercase letters, digits and dashes, like api-review." };
  if (n.length > 61) return { error: "That name is too long: keep it under 60 characters." };
  return { id: `guide/${n}.md` };
}

/** The `{{VARIABLES}}` a brief uses, in order, once each. */
export function briefVars(text: string): string[] {
  return [...new Set(text.match(/\{\{[A-Z][A-Z0-9_]*\}\}/g) ?? [])];
}

/** What an edit to a brief does to its variables: ones it lost, and ones Wheelhouse won't fill. */
export function varChanges(original: string, text: string): { missing: string[]; unknown: string[] } {
  const was = briefVars(original);
  const now = briefVars(text);
  return { missing: was.filter((v) => !now.includes(v)), unknown: now.filter((v) => !was.includes(v)) };
}

export type DocGroup = { key: string; label: string; note?: string; docs: SettingsDoc[] };

/** Guides and briefs, grouped: yours, built-in guides, Build's briefs, Review's briefs. */
export function docGroups(docs: SettingsDoc[]): DocGroup[] {
  const parts = (d: SettingsDoc) => d.id.split("/").length;
  const briefs = (p: string) => docs.filter((d) => d.kind === "brief" && d.protocol === p).sort((a, b) => parts(a) - parts(b));
  return [
    { key: "own", label: "Your guides", docs: docs.filter((d) => d.kind === "guide" && !d.builtin) },
    { key: "guides", label: "Built-in guides", docs: docs.filter((d) => d.kind === "guide" && d.builtin) },
    { key: "build", label: "Build's briefs", docs: briefs("build") },
    { key: "review", label: "Review's briefs", docs: briefs("review") },
  ];
}

/** A guide's slot, in words. */
const SLOTS: Record<string, string> = { review: "review", style: "style", merge_risk: "merge-risk", check: "checker's", plan_format: "plan format", pr_style: "PR style" };
const slotWord = (s: string) => SLOTS[s] ?? s.replace(/_/g, " ");

/** Where a guide is used, in words: `review: repos.acme/api.guides.review` → `acme/api's review guide`. */
export function usedWord(u: string): string {
  const m = /^(review|build): (.*)$/.exec(u);
  if (!m) return u;
  const [, part, at] = m;
  const repo = /^repos\.(.+)\.guides\.(\w+)$/.exec(at);
  if (repo) return `${repo[1]}'s ${slotWord(repo[2])} guide`;
  const slot = /^guides\.(\w+)$/.exec(at);
  if (slot) return `${part === "review" ? "Review" : "Build"}'s ${slotWord(slot[1])} guide`;
  return `${part}.lua: ${at}`;
}
