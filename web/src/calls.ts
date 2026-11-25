// A call's proposals, as the apps show them (src/calls.rs): what each
// would do, in words, and what Edit may change for each kind.
import type { CallView, Proposal } from "./api.ts";

export const kindWord = (k: Proposal["kind"]): string => (k === "decide" ? "Decision" : k === "ask" ? "Question" : "Task");

/** What applying it would do, in a line. */
export function whatItDoes(p: Pick<Proposal, "kind" | "docket" | "subject" | "choice" | "question" | "title">): string {
  if (p.kind === "task") return `Open a docket: ${p.title ?? ""}`;
  const on = p.docket ? ` on ${p.docket}` : "";
  if (p.kind === "decide") return `Decide${on}: ${p.subject ? `${p.subject}: ` : ""}${p.choice ?? ""}`;
  return `Ask the agents${on}: ${(p.question ?? "").split("\n")[0]}`;
}

/** Why Apply can't go yet, if it can't. */
export function cantApply(p: Pick<Proposal, "kind" | "docket" | "choice" | "question" | "title">): string | null {
  if (p.kind !== "task" && !p.docket) return "Pick the docket it's for: Edit";
  if (p.kind === "decide" && !p.choice?.trim()) return "Nothing decided";
  if (p.kind === "ask" && !p.question?.trim()) return "No question";
  if (p.kind === "task" && !p.title?.trim()) return "No title";
  return null;
}

/** The fields Edit shows for a kind, in order. */
export const editable = (k: Proposal["kind"]): (keyof Proposal)[] =>
  k === "decide" ? ["docket", "subject", "choice"] : k === "ask" ? ["docket", "question", "options"] : ["title", "intent"];

/** How listening went, in words; null when there's nothing to say. */
export function listeningWords(v: CallView): { text: string; tone: "busy" | "error" | "quiet" } | null {
  const l = v.listening;
  if (l.state === "started") return { text: "The Listener is reading the call…", tone: "busy" };
  if (l.state === "failed") return { text: `The Listener couldn't read the call: ${l.why ?? "it failed"}`, tone: "error" };
  if (l.state === "stopped") return { text: "The hub stopped while the Listener was reading. Listen again.", tone: "error" };
  if (l.state === "done" && !v.proposals.length) return { text: "The Listener found nothing the call settled.", tone: "quiet" };
  const left = l.left_out?.length ?? 0;
  if (l.state === "done" && left) return { text: `${left} more it proposed didn't hold up, and were left out.`, tone: "quiet" };
  return null;
}

/** Options as typed in Edit: one per line (an option may hold a `|`). */
export const parseOptions = (s: string): string[] =>
  s
    .split(/\r?\n/)
    .map((o) => o.trim())
    .filter(Boolean)
    .slice(0, 6);
