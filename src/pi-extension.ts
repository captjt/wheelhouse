// Wheelhouse for Pi: tells the Wheelhouse hub when this Pi session starts,
// what you asked it, and what it answered, the way Claude Code's hooks do,
// so the docket's record has the prompts, the replies and the session.
//
// Written by `wh hooks install`, and rewritten (your edits lost) on the next
// one; `wh hooks remove` deletes it. It never holds Pi up: each report runs
// `wh hook --agent pi EVENT` in the background, in order, and gives up after
// a few seconds. Outside a Rex pane or a Herdr terminal, and in print, JSON
// and RPC runs (another agent running `pi -p`), it reports nothing.
//
// wheelhouse-pi-extension v1

import { spawn } from "node:child_process";

const WH: string = "__WH__";
/** How long one report may take before it's dropped. */
const TIMEOUT_MS = 3000;
/** Reports waiting behind a slow hub; past this, new ones are dropped. */
const MAX_QUEUED = 32;

const pane = (process.env.REX_BLOCK ?? "").trim() || (process.env.HC_TERMINAL_ID ?? "").trim();

let queue: Promise<void> = Promise.resolve();
let queued = 0;

function run(event: string, payload: Record<string, unknown>): Promise<void> {
  return new Promise((resolve) => {
    let settled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const finish = () => {
      if (settled) return;
      settled = true;
      if (timer) clearTimeout(timer);
      resolve();
    };
    try {
      const child = spawn(WH, ["hook", "--agent", "pi", event], { stdio: ["pipe", "ignore", "ignore"] });
      timer = setTimeout(() => {
        child.kill();
        finish();
      }, TIMEOUT_MS);
      timer.unref?.();
      child.on("error", finish);
      child.on("close", finish);
      child.stdin?.on("error", () => {});
      child.stdin?.end(JSON.stringify(payload));
    } catch {
      finish();
    }
  });
}

function report(event: string, payload: Record<string, unknown>): void {
  if (!pane || queued >= MAX_QUEUED) return;
  queued++;
  queue = queue.then(() => run(event, payload)).finally(() => {
    queued--;
  });
}

/** The words of an assistant message, without its thinking or tool calls. */
function words(message: any): string {
  const content = message?.content;
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .filter((b: any) => b?.type === "text" && typeof b.text === "string")
    .map((b: any) => b.text)
    .join("\n\n")
    .trim();
}

/** Pi's reason a session started, as Wheelhouse's record names it. */
const SOURCES: Record<string, string> = { startup: "startup", reload: "startup", new: "clear", resume: "resume", fork: "fork" };

export default function wheelhouse(pi: any): void {
  if (!pane) return;
  // The last assistant message of the run under way: its words and how it ended.
  let last: { text: string; stop: string } | undefined;

  const session = (ctx: any) => {
    const sm = ctx?.sessionManager;
    const id = sm?.getSessionId?.();
    return {
      session_id: typeof id === "string" ? id : undefined,
      cwd: typeof ctx?.cwd === "string" ? ctx.cwd : undefined,
      transcript_path: sm?.getSessionFile?.() ?? undefined,
    };
  };
  const interactive = (ctx: any) => ctx?.mode === "tui";

  // `pi -c`, `pi -r` and `pi --session` start Pi with a session that has
  // a conversation already: Pi says "startup", but it's one resumed.
  const resumed = (ctx: any) => {
    try {
      const entries = ctx?.sessionManager?.getEntries?.();
      return Array.isArray(entries) && entries.some((e: any) => e?.type === "message");
    } catch {
      return false;
    }
  };

  pi.on("session_start", (event: any, ctx: any) => {
    if (!interactive(ctx)) return;
    last = undefined;
    const source = event?.reason === "startup" && resumed(ctx) ? "resume" : (SOURCES[event?.reason] ?? "startup");
    report("SessionStart", { ...session(ctx), source });
  });

  pi.on("input", (event: any, ctx: any) => {
    // Text another extension sent isn't yours.
    if (!interactive(ctx) || event?.source === "extension") return;
    const prompt = typeof event?.text === "string" ? event.text.trim() : "";
    if (prompt) report("UserPromptSubmit", { ...session(ctx), prompt });
  });

  pi.on("agent_start", () => {
    last = undefined;
  });

  pi.on("message_end", (event: any) => {
    const m = event?.message;
    if (m?.role === "assistant") last = { text: words(m), stop: String(m.stopReason ?? "") };
  });

  pi.on("agent_settled", (event: any, ctx: any) => {
    const done = last;
    last = undefined;
    // A turn you stopped, or one that failed, has no answer to keep.
    if (!interactive(ctx) || event?.aborted || !done || done.stop === "error" || done.stop === "aborted" || !done.text) return;
    report("Stop", { ...session(ctx), last_assistant_message: done.text });
  });
}
