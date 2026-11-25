// Pi's extension (src/pi-extension.ts), run as Pi runs it: its handlers get
// Pi's events, and it reports them through `wh hook --agent pi` (here a
// stand-in that writes down what it got).
import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { after, test } from "node:test";

const dir = mkdtempSync(join(tmpdir(), "wh-pi-ext-"));
after(() => rmSync(dir, { recursive: true, force: true }));

const source = readFileSync(new URL("../../src/pi-extension.ts", import.meta.url), "utf8");

/** A stand-in `wh` that appends `EVENT <stdin>` to `log`. */
function fakeWh(name: string, body = 'printf "%s %s\\n" "$4" "$(cat)" >> "$LOG"'): { wh: string; log: string } {
  const wh = join(dir, `${name}.sh`);
  const log = join(dir, `${name}.log`);
  writeFileSync(wh, `#!/bin/sh\nLOG=${JSON.stringify(log)}\n${body}\n`);
  chmodSync(wh, 0o755);
  return { wh, log };
}

/** The extension calling `wh`, loaded fresh with this environment. */
async function load(name: string, wh: string, env: Record<string, string | undefined>) {
  const file = join(dir, `${name}.ts`);
  assert.ok(source.includes('"__WH__"'));
  writeFileSync(file, source.replace('"__WH__"', JSON.stringify(wh)));
  const saved = { REX_BLOCK: process.env.REX_BLOCK, HC_TERMINAL_ID: process.env.HC_TERMINAL_ID };
  for (const [k, v] of Object.entries(env)) {
    if (v === undefined) delete process.env[k];
    else process.env[k] = v;
  }
  try {
    const mod = await import(pathToFileURL(file).href);
    const handlers = new Map<string, (event: any, ctx: any) => unknown>();
    mod.default({ on: (name: string, fn: (event: any, ctx: any) => unknown) => handlers.set(name, fn) });
    return { handlers, emit: (event: string, e: any = {}, ctx: any = tui) => handlers.get(event)?.(e, ctx) };
  } finally {
    for (const [k, v] of Object.entries(saved)) {
      if (v === undefined) delete process.env[k];
      else process.env[k] = v;
    }
  }
}

const session = { getSessionId: () => "01a21302-9d46", getSessionFile: () => "/Users/you/.pi/agent/sessions/--w--/x_01a21302-9d46.jsonl" };
const tui = { mode: "tui", cwd: "/Users/you/git/api", sessionManager: session };
const print = { ...tui, mode: "print" };
const assistant = (text: string, stopReason = "stop") => ({ message: { role: "assistant", stopReason, content: [{ type: "thinking", thinking: "hmm" }, { type: "text", text }] } });

async function lines(log: string, n: number): Promise<{ event: string; payload: any }[]> {
  for (let i = 0; i < 100; i++) {
    const got = existsSync(log) ? readFileSync(log, "utf8").split("\n").filter(Boolean) : [];
    if (got.length >= n) {
      return got.map((l) => {
        const at = l.indexOf(" ");
        return { event: l.slice(0, at), payload: JSON.parse(l.slice(at + 1)) };
      });
    }
    await new Promise((r) => setTimeout(r, 30));
  }
  assert.fail(`${log}: fewer than ${n} reports`);
}

test("a session, a prompt and its answer reach the hub in order, as Claude Code's hooks do", async () => {
  const { wh, log } = fakeWh("order");
  const pi = await load("order", wh, { REX_BLOCK: "block:1", HC_TERMINAL_ID: undefined });
  pi.emit("session_start", { reason: "resume" });
  pi.emit("input", { text: "  Add Pi support ", source: "interactive" });
  pi.emit("agent_start");
  pi.emit("message_end", assistant("Reading the hub."));
  pi.emit("message_end", { message: { role: "toolResult", content: [{ type: "text", text: "ok" }] } });
  pi.emit("message_end", assistant("Pi is **in**."));
  pi.emit("agent_settled", { aborted: false });
  const got = await lines(log, 3);
  assert.deepEqual(
    got.map((g) => g.event),
    ["SessionStart", "UserPromptSubmit", "Stop"],
  );
  assert.deepEqual(got[0].payload, { session_id: "01a21302-9d46", cwd: "/Users/you/git/api", transcript_path: session.getSessionFile(), source: "resume" });
  assert.equal(got[1].payload.prompt, "Add Pi support");
  assert.equal(got[2].payload.last_assistant_message, "Pi is **in**.", "the last message's words, not its thinking");
});

test("Pi's reasons a session starts are the record's", async () => {
  const { wh, log } = fakeWh("reasons");
  const pi = await load("reasons", wh, { REX_BLOCK: undefined, HC_TERMINAL_ID: "t2y" });
  for (const reason of ["startup", "reload", "new", "resume", "fork", "something-new"]) pi.emit("session_start", { reason });
  // `pi -c`: started with a conversation already in the session.
  const had = (entries: unknown) => ({ ...tui, sessionManager: { ...session, getEntries: () => entries } });
  pi.emit("session_start", { reason: "startup" }, had([{ type: "model_change" }, { type: "message", message: { role: "user" } }]));
  pi.emit("session_start", { reason: "startup" }, had([{ type: "model_change" }]));
  pi.emit("session_start", { reason: "reload" }, had([{ type: "message" }]));
  pi.emit("session_start", { reason: "startup" }, { ...tui, sessionManager: { ...session, getEntries: () => { throw new Error("closed"); } } });
  const got = await lines(log, 10);
  assert.deepEqual(
    got.map((g) => g.payload.source),
    ["startup", "startup", "clear", "resume", "fork", "startup", "resume", "startup", "startup", "startup"],
  );
});

test("a stopped or failed turn, another extension's text, and a run without Pi's UI aren't reported", async () => {
  const { wh, log } = fakeWh("quiet");
  const pi = await load("quiet", wh, { REX_BLOCK: "block:1" });
  // `pi -p` run by another agent in its pane: never that pane's prompts.
  pi.emit("session_start", { reason: "startup" }, print);
  pi.emit("input", { text: "Summarize", source: "interactive" }, print);
  pi.emit("message_end", assistant("Summary."), print);
  pi.emit("agent_settled", { aborted: false }, print);
  pi.emit("input", { text: "from a plugin", source: "extension" });
  pi.emit("input", { text: "   ", source: "interactive" });
  pi.emit("agent_start");
  pi.emit("message_end", assistant("Half an answer"));
  pi.emit("agent_settled", { aborted: true });
  pi.emit("agent_start");
  pi.emit("message_end", assistant("", "error"));
  pi.emit("agent_settled", { aborted: false });
  pi.emit("agent_start");
  pi.emit("message_end", { message: { role: "assistant", stopReason: "stop", content: [{ type: "toolCall", name: "bash" }] } });
  pi.emit("agent_settled", { aborted: false });
  // Then one that counts, so the test knows the rest had their chance.
  pi.emit("input", { text: "Now this", source: "interactive" });
  const got = await lines(log, 1);
  await new Promise((r) => setTimeout(r, 200));
  assert.deepEqual(
    readFileSync(log, "utf8").split("\n").filter(Boolean).map((l) => l.split(" ")[0]),
    ["UserPromptSubmit"],
  );
  assert.equal(got[0].payload.prompt, "Now this");
});

test("outside a Rex pane or a Herdr terminal it does nothing at all", async () => {
  const { wh, log } = fakeWh("outside");
  const pi = await load("outside", wh, { REX_BLOCK: "  ", HC_TERMINAL_ID: undefined });
  assert.equal(pi.handlers.size, 0);
  assert.equal(existsSync(log), false);
});

test("reports wait their turn, at most 32 of them, and handlers never wait for wh", async () => {
  const { wh, log } = fakeWh("burst");
  const pi = await load("burst", wh, { REX_BLOCK: "block:1" });
  const t0 = Date.now();
  for (let i = 0; i < 40; i++) pi.emit("input", { text: `prompt ${i}`, source: "interactive" });
  assert.ok(Date.now() - t0 < 500, "handlers return at once");
  const got = await lines(log, 32);
  await new Promise((r) => setTimeout(r, 300));
  assert.equal(readFileSync(log, "utf8").split("\n").filter(Boolean).length, 32, "past 32 waiting, new ones are dropped");
  assert.deepEqual(
    got.slice(0, 3).map((g) => g.payload.prompt),
    ["prompt 0", "prompt 1", "prompt 2"],
  );
});

test("a wh that hangs is given up on, and the next report still goes", async () => {
  // Hangs on its first call only.
  const { wh, log } = fakeWh("hang", 'if [ ! -e "$LOG.once" ]; then : > "$LOG.once"; exec sleep 30; fi\nprintf "%s %s\\n" "$4" "$(cat)" >> "$LOG"');
  const pi = await load("hang", wh, { REX_BLOCK: "block:1" });
  pi.emit("input", { text: "lost", source: "interactive" });
  pi.emit("input", { text: "after the hang", source: "interactive" });
  const got = await lines(log, 1);
  assert.equal(got[0].payload.prompt, "after the hang");
  const missing = await load("missing", join(dir, "no-such-wh"), { REX_BLOCK: "block:1" });
  missing.emit("input", { text: "still fine", source: "interactive" });
  await new Promise((r) => setTimeout(r, 200));
});
