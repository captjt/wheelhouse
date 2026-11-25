// The composer: which command you're typing, its button and hint, what `/`
// and `@` complete to, and what the hub's answer says (src/compose.rs).
import assert from "node:assert/strict";
import { test } from "node:test";
import { atName, composeButton, composeHint, composeKind, composeToast, composeWarning, suggestions } from "../src/compose.ts";
import type { Docket, DocketAgent, Live } from "../src/api.ts";

const live = (block: string): Live => ({ block, state: "idle", app: "claude-code", rex_session: "wheelhouse", pane: block });
const agent = (block: string, o: Partial<DocketAgent> = {}): DocketAgent => ({ app: "claude-code", host: "local", block, rex_session: "wheelhouse", pane: block, joined_at: "t1", active_at: "t1", on: true, live: live(block), ...o });
const docket = (agents: DocketAgent[]): Docket =>
  ({ id: "D-12", title: "Retries", title_source: "person", status: "in_progress", protocol: "none", prompts: 0, turns: 0, notes: 0, created_at: "t0", updated_at: "t9", entries: 1, head: "", short_head: "", live: null, quick: false, agents, lead: agents[0]?.block }) as Docket;

test("what you're typing is, as the hub reads it", () => {
  assert.equal(composeKind("Looks right."), "note");
  assert.equal(composeKind("@builder run it"), "instruct");
  assert.equal(composeKind("  /ask Which? | a | b"), "ask");
  assert.equal(composeKind("/DECIDE Retries: 5xx"), "decide");
  assert.equal(composeKind("/task write tests"), "task");
  assert.equal(composeKind("/note @all isn't a command"), "note");
  // A path, a lone @, and an @ that isn't a name are notes.
  assert.equal(composeKind("/tmp/x is full"), "note");
  assert.equal(composeKind("@ noon"), "note");
  assert.equal(composeKind("@sam's idea"), "note");
  assert.equal(composeButton("instruct"), "Send");
  assert.equal(composeButton("task"), "Add an agent…");
  assert.equal(composeButton("note"), "Note");
  assert.match(composeHint("decide", "D-12"), /agents on D-12 are told/);
});

test("/ completes to the commands, @ to the agents here", () => {
  assert.deepEqual(suggestions("/", null).map((s) => s.label), ["/ask", "/decide", "/task", "/note"]);
  assert.deepEqual(suggestions("/de", null).map((s) => s.insert), ["/decide "]);
  assert.deepEqual(suggestions("/decide Retries", null), [], "only while the first word is typed");
  assert.deepEqual(suggestions("/zz", null), []);
  const d = docket([agent("block:1", { pane: "builder", role: "Build builder" }), agent("block:2", { pane: "", role: "tests", app: "codex" }), agent("block:3", { on: false })]);
  assert.deepEqual(suggestions("@", d).map((s) => s.label), ["@builder", "@tests", "@all"]);
  assert.equal(suggestions("@", d)[1].sub, "Codex · tests");
  assert.deepEqual(suggestions("@b", d).map((s) => s.insert), ["@builder "]);
  assert.deepEqual(suggestions("@", docket([agent("block:1", { pane: "D-12" })])).map((s) => s.label), ["@d-12"], "one agent: no @all");
  assert.deepEqual(suggestions("@", docket([])), []);
  assert.equal(atName(agent("b", { pane: "", role: "", app: "claude-code" })), "claude");
});

test("what the hub did, in a toast", () => {
  assert.equal(composeToast({ did: "note", seq: 3 }), null);
  assert.equal(composeToast({ did: "task", prompt: "x" }), null);
  assert.deepEqual(composeToast({ did: "instructed", sent: [{ to: "builder (Claude Code)", ok: true, queued: true }] }), { text: "Queued for builder (Claude Code)", error: false });
  const some = composeToast({ did: "instructed", sent: [{ to: "a", ok: true }, { to: "b", ok: false, error: "the pane closed" }] });
  assert.deepEqual(some, { text: "Sent to a; not b (the pane closed)", error: true });
  assert.equal(composeToast({ did: "asked", message: 7, options: ["Postgres", "SQLite"] })?.text, "Asked the agents (#7): Postgres / SQLite");
  assert.equal(composeToast({ did: "answered", key: "#3", message: 9, choice: "Postgres" })?.text, "Answered #3: Postgres");
  assert.equal(composeToast({ did: "decided", key: "=ab", seq: 4, told: ["wheelhouse/D-12"] })?.text, "Decided, and told wheelhouse/D-12");
  assert.match(composeToast({ did: "decided", key: "=ab", seq: 4, told: [] })!.text, /next one reads it in its brief/);
});

test("what the hub would refuse is said before it's sent", () => {
  const d = docket([agent("block:1", { pane: "my builder", role: "Build builder" }), agent("block:2", { pane: "", role: "tests", app: "codex" })]);
  assert.deepEqual(suggestions("@", d).map((s) => s.label), ["@my-builder", "@tests", "@all"], "a name with a space, as the hub takes it");
  assert.deepEqual(composeWarning("/decied 5xx", d), { text: "There's no /decied: try /ask, /decide, /task, or /note to keep it as a note", blocked: true });
  assert.equal(composeWarning("/decide 5xx", d), null);
  assert.equal(composeWarning("/tmp/x is full", d), null, "a path is a note");
  assert.equal(composeWarning("@my-builder go", d), null);
  assert.equal(composeWarning("@builder go", d), null, "a word of its role");
  assert.equal(composeWarning("@codex go", d), null, "its app");
  assert.equal(composeWarning("@all go", d), null);
  assert.deepEqual(composeWarning("@sam can you look", d), { text: "No agent here is @sam: /note keeps it as a note", blocked: false });
  assert.equal(composeWarning("@sam can you look", docket([]))?.blocked, true);
  assert.equal(composeWarning("@builder", d), null, "only once there's more than the name");
});
