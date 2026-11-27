// The composer on a docket's Activity (src/compose.rs reads what's sent):
// a note, `@agent` to instruct one, `/ask`, `/decide`, `/task`. Here is
// only what the box shows while you type: which it is, the button, a hint,
// and what `/` and `@` can complete to.
import { appName, liveAgents, type Docket, type DocketAgent } from "./api.ts";

export type ComposeKind = "note" | "instruct" | "ask" | "decide" | "task";

/** What the hub answers (`POST /api/dockets/{id}/compose`). */
export type ComposeResult = (
  | { did: "note"; seq: number }
  | { did: "instructed"; sent: { to: string; ok: boolean; queued?: boolean; error?: string }[] }
  | { did: "asked"; message: number; options: string[] }
  | { did: "answered"; key: string; message: number; choice?: string | null }
  | { did: "decided"; key: string; seq: number; told: string[] }
  | { did: "task"; prompt: string }
) & { images?: { digest: string; name: string }[] };

export const COMMANDS: { word: string; kind: ComposeKind; what: string }[] = [
  { word: "ask", kind: "ask", what: "Ask the agents here: Which store? | Postgres | SQLite" },
  { word: "decide", kind: "decide", what: "Record a decision (Subject: answer), or answer a question (#12 …)" },
  { word: "task", kind: "task", what: "Start another agent here with this prompt" },
  { word: "note", kind: "note", what: "A note that starts with / or @" },
];

/** Which it'll be, by its first word, as the hub reads it. A path (`/tmp/x`) or a lone `@` is a note. */
export function composeKind(text: string): ComposeKind {
  const t = text.trimStart();
  const m = /^([/@])(\S*)/.exec(t);
  if (!m) return "note";
  const word = m[2];
  if (m[1] === "@") return /^[A-Za-z0-9._-]+$/.test(word) ? "instruct" : "note";
  if (!/^[A-Za-z]+$/.test(word)) return "note";
  return COMMANDS.find((c) => c.word === word.toLowerCase())?.kind ?? "note";
}

export const composeButton = (k: ComposeKind): string =>
  k === "instruct" ? "Send" : k === "ask" ? "Ask" : k === "decide" ? "Decide" : k === "task" ? "Add an agent…" : "Note";

export function composeHint(k: ComposeKind, id: string): string {
  switch (k) {
    case "instruct":
      return "A prompt to that agent, queued while it works";
    case "ask":
      return `Asks the agents on ${id}; their answer stands under Decisions`;
    case "decide":
      return `Stands under Decisions, and the agents on ${id} are told`;
    case "task":
      return "Opens Add an agent with this prompt";
    default:
      return "Signed as you · @agent, /ask, /decide, /task";
  }
}

/** What happens to the images in the tray, by what you're sending. */
export function imagesHint(k: ComposeKind, n: number): string {
  const them = n === 1 ? "The image goes" : `The ${n} images go`;
  switch (k) {
    case "instruct":
      return `${them} to that agent as ${n === 1 ? "a file" : "files"} it opens`;
    case "ask":
    case "decide":
      return `${them} to the agents with it`;
    case "task":
      return `${them} with the new agent's prompt`;
    default:
      return `${n === 1 ? "Kept" : "All kept"} with the docket: agents see ${n === 1 ? "it" : "them"} in wh brief`;
  }
}

/** What an agent answers to after `@`: its pane's name, else its role, else its app (src/compose.rs `names`). */
export const atName = (a: DocketAgent): string =>
  (a.pane.trim() || a.role?.trim() || (a.app === "claude-code" ? "claude" : a.app)).split(/\s+/).join("-").toLowerCase();

/** Something the hub would refuse, said before you send it; `blocked` when sending can't work. */
export function composeWarning(text: string, d?: Docket | null): { text: string; blocked: boolean } | null {
  const m = /^([/@])(\S*)\s/.exec(text.trimStart());
  if (!m) return null;
  const word = m[2].toLowerCase();
  if (m[1] === "/" && /^[a-z]+$/.test(word) && !COMMANDS.some((c) => c.word === word))
    return { text: `There's no /${word}: try /ask, /decide, /task, or /note to keep it as a note`, blocked: true };
  if (m[1] === "@" && composeKind(text) === "instruct" && !["all", "agents", "everyone"].includes(word)) {
    const agents = liveAgents(d);
    if (!agents.length) return { text: "No agent is on this docket now: /note keeps it as a note", blocked: true };
    if (!agents.some((a) => atName(a) === word || a.role?.toLowerCase().split(/\s+/).includes(word) || atName({ ...a, pane: "", role: null }) === word || (a.app === "claude-code" && word === "claude-code")))
      return { text: `No agent here is @${word}: /note keeps it as a note`, blocked: false };
  }
  return null;
}

export type Suggestion = { insert: string; label: string; sub: string };

/** While only the first word is typed: the commands `/` could be, or the agents `@` could name. */
export function suggestions(text: string, d?: Docket | null): Suggestion[] {
  const m = /^([/@])(\S*)$/.exec(text.trimStart());
  if (!m) return [];
  const typed = m[2].toLowerCase();
  if (m[1] === "/") return COMMANDS.filter((c) => c.word.startsWith(typed)).map((c) => ({ insert: `/${c.word} `, label: `/${c.word}`, sub: c.what }));
  const agents = liveAgents(d);
  const out: Suggestion[] = agents.map((a) => ({ insert: `@${atName(a)} `, label: `@${atName(a)}`, sub: [appName(a.app), a.role].filter(Boolean).join(" · ") }));
  if (agents.length > 1) out.push({ insert: "@all ", label: "@all", sub: `Every agent on ${d?.id ?? "it"}` });
  return out.filter((s, i) => s.label.slice(1).startsWith(typed) && out.findIndex((x) => x.label === s.label) === i);
}

/** What happened, for a toast; none for a note (it's in the Activity) or a task (its dialog opens). */
export function composeToast(r: ComposeResult): { text: string; error?: boolean } | null {
  switch (r.did) {
    case "instructed": {
      const ok = r.sent.filter((s) => s.ok);
      const failed = r.sent.filter((s) => !s.ok);
      const parts = ok.map((s) => `${s.queued ? "queued for" : "sent to"} ${s.to}`);
      if (failed.length) parts.push(`not ${failed.map((s) => `${s.to} (${s.error})`).join(", ")}`);
      const text = parts.join("; ");
      return { text: text.charAt(0).toUpperCase() + text.slice(1), error: failed.length > 0 };
    }
    case "asked":
      return { text: `Asked the agents (#${r.message})${r.options.length ? `: ${r.options.join(" / ")}` : ""}` };
    case "answered":
      return { text: `Answered ${r.key}${r.choice ? `: ${r.choice}` : ""}` };
    case "decided":
      return { text: r.told.length ? `Decided, and told ${r.told.length === 1 ? r.told[0] : `${r.told.length} agents`}` : "Decided. No agent is on it now; the next one reads it in its brief" };
    default:
      return null;
  }
}
