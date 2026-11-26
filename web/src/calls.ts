// A call's proposals, as the apps show them (src/calls.rs): what each
// would do, in words, and what Edit may change for each kind.
import type { CallView, Proposal } from "./api.ts";

const WORDS: Record<Proposal["kind"], string> = { intent: "Intent", decide: "Decision", instruct: "Instruction", ask: "Question", task: "Task" };
export const kindWord = (k: Proposal["kind"]): string => WORDS[k] ?? "Task";

const first = (s?: string | null) => (s ?? "").split("\n")[0];

/** What applying it would do, in a line. */
export function whatItDoes(p: Pick<Proposal, "kind" | "docket" | "subject" | "choice" | "question" | "text" | "title">): string {
  const on = p.docket ? ` on ${p.docket}` : "";
  switch (p.kind) {
    case "intent":
      return `Revise the intent${on}${p.subject ? `: ${p.subject}` : ""}`;
    case "decide":
      return `Decide${on}: ${p.subject ? `${p.subject}: ` : ""}${p.choice ?? ""}`;
    case "instruct":
      return `Tell the agents${on}: ${first(p.text)}`;
    case "ask":
      return `Ask the agents${on}: ${first(p.question)}`;
    default:
      return `Open a docket: ${p.title ?? ""}`;
  }
}

/** Why Apply can't go yet, if it can't. `versions`: the docket's intent
 * versions now, to tell an intent change read against an older one. */
export function cantApply(p: Pick<Proposal, "kind" | "docket" | "choice" | "question" | "text" | "title" | "intent" | "base">, versions?: number): string | null {
  if (p.kind !== "task" && !p.docket) return "Pick the docket it's for: Edit";
  if (p.kind === "intent" && !p.intent?.trim()) return "No intent";
  if (p.kind === "intent" && versions !== undefined && p.base != null && p.base !== versions) return "The intent changed since the call was read: Listen again, or Edit";
  if (p.kind === "decide" && !p.choice?.trim()) return "Nothing decided";
  if (p.kind === "instruct" && !p.text?.trim()) return "Nothing to do";
  if (p.kind === "ask" && !p.question?.trim()) return "No question";
  if (p.kind === "task" && !p.title?.trim()) return "No title";
  return null;
}

const EDITABLE: Record<Proposal["kind"], (keyof Proposal)[]> = {
  intent: ["subject", "intent"],
  decide: ["docket", "subject", "choice"],
  instruct: ["docket", "text"],
  ask: ["docket", "question", "options"],
  task: ["title", "intent"],
};
/** The fields Edit shows for a kind, in order. An intent change is only
 * ever for the call's own docket. */
export const editable = (k: Proposal["kind"]): (keyof Proposal)[] => EDITABLE[k] ?? EDITABLE.task;

/** How listening went, in words; null when there's nothing to say. */
export function listeningWords(v: Pick<CallView, "listening" | "proposals">): { text: string; tone: "busy" | "error" | "quiet" } | null {
  const l = v.listening;
  if (l.state === "started") return { text: "The Listener is reading the call…", tone: "busy" };
  if (l.state === "failed") return { text: `The Listener couldn't read the call: ${l.why ?? "it failed"}`, tone: "error" };
  if (l.state === "stopped") return { text: "The hub stopped while the Listener was reading. Listen again.", tone: "error" };
  if (l.state === "done" && !v.proposals.length) return { text: "The Listener found nothing the call settled.", tone: "quiet" };
  const left = l.left_out?.length ?? 0;
  if (l.state === "done" && left) return { text: `${left} more it proposed didn't hold up, and were left out.`, tone: "quiet" };
  return null;
}

/** A call's proposals in a line: how many are open, and applied. */
export function tally(v: Pick<CallView, "proposals">): string {
  const open = v.proposals.filter((p) => p.status === "open").length;
  const applied = v.proposals.filter((p) => p.status === "applied").length;
  if (!v.proposals.length) return "no proposals";
  return [open && `${open} open`, applied && `${applied} applied`, !open && !applied && "all ignored"].filter(Boolean).join(" · ");
}

/** Options as typed in Edit: one per line (an option may hold a `|`). */
export const parseOptions = (s: string): string[] =>
  s
    .split(/\r?\n/)
    .map((o) => o.trim())
    .filter(Boolean)
    .slice(0, 6);
