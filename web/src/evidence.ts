// Verify, in words (as the hub works it out in src/evidence.rs): how a
// witnessed check went, how long it took, whether a docket's Verify section
// has anything to say, and which attention items aren't already asks. Pure,
// so `npm test` runs it without a browser.
import type { Attention, CheckRun, Evidence, VerdictView } from "./api";

/** How a check went, in a few words: "passed", "failed, exit 1", "timed out". */
export function checkWord(c: Pick<CheckRun, "ok" | "exit">): string {
  if (c.ok) return "passed";
  if (c.exit === null || c.exit === undefined) return "timed out";
  return `failed, exit ${c.exit}`;
}

/** A check's state, for its icon and color. */
export function checkState(c: Pick<CheckRun, "ok" | "exit" | "required">): "pass" | "fail" | "timeout" | "inform" {
  if (c.ok) return "pass";
  if (!c.required) return "inform";
  return c.exit === null || c.exit === undefined ? "timeout" : "fail";
}

/** How long something took: 153 → "0.2s", 64_000 → "1m 4s". */
export function duration(ms?: number | null): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return "";
  if (ms < 1000) return `${Math.max(0.1, Math.round(ms / 100) / 10)}s`;
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return s % 60 ? `${m}m ${s % 60}s` : `${m}m`;
}

/** A commit, short: the 7 characters git shows. */
export const shortSha = (h?: string | null) => (h ? h.slice(0, 7) : "");

/** Why a check ran, to follow "Ran because": "you asked". */
export function whyWord(why?: string | null): string {
  switch (why) {
    case "asked":
      return "you asked";
    case "build":
      return "the Build's checker passed";
    default:
      return why ?? "";
  }
}

/** Whether the Verify section has anything to say: hidden when it doesn't. */
export function verifyShown(ev?: Evidence | null): boolean {
  if (!ev) return false;
  return ev.needs || ev.evidence.length > 0 || ev.checks.length > 0 || ev.verdicts.length > 0 || !!ev.hold || !!ev.closed_on || ev.running;
}

/** Configured checks that haven't run on this docket yet. */
export function notRun(ev: Evidence): Evidence["configured"] {
  const ran = new Set(ev.checks.map((c) => c.name));
  return ev.configured.filter((c) => !ran.has(c.name));
}

/** Why Run checks can't run, or null when it can. */
export function cantRun(ev: Evidence): string | null {
  if (ev.running) return "They're running";
  if (!ev.repo) return "This docket isn't in a repo Wheelhouse knows, so there's nothing to run";
  if (!ev.configured.length) return `${ev.repo} has no checks yet: add them in Settings, under Repos`;
  return null;
}

/** Whether the checker was independent of the builder, in words; null when it can't say. */
export function independence(v: Pick<VerdictView, "independent">): string | null {
  if (v.independent === true) return "independent";
  if (v.independent === false) return "same session as the builder";
  return null;
}

/** An ask the Inbox shows, as far as attention needs to know. */
export type AskKey = { docket: string; kind: string };

/**
 * Attention that isn't already an ask: a Review that needs you is in the
 * Inbox as its review, and a Build's blocked or failed agent as that agent.
 */
export function attentionShown(att: Attention[] | null | undefined, asks: AskKey[]): Attention[] {
  const has = (docket: string | null | undefined, kinds: string[]) => !!docket && asks.some((a) => a.docket === docket && kinds.includes(a.kind));
  const seen = new Set<string>();
  return (att ?? []).filter((a) => {
    if (a.kind === "review" && (has(a.docket, ["review"]) || has(a.title, ["review"]))) return false;
    if (a.kind === "build" && has(a.docket, ["blocked", "error", "review"])) return false;
    const k = `${a.docket ?? a.title}:${a.kind}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}
