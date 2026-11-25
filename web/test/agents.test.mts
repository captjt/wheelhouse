// A docket's agents as the apps show them: who's on it, who leads, and
// which agent an entry was.
import assert from "node:assert/strict";
import { test } from "node:test";
import { agentLabel, agentOfEntry, agentsOn, appName, appOfName, entryBlock, liveAgents, startAppFor, startApps, type Docket, type DocketAgent, type Live } from "../src/api.ts";

const live = (block: string, state: string): Live => ({ block, state, app: "claude-code", rex_session: "wheelhouse", pane: block });
const agent = (block: string, o: Partial<DocketAgent> = {}): DocketAgent => ({
  app: "claude-code",
  host: "local",
  block,
  rex_session: "wheelhouse",
  pane: block,
  joined_at: "t1",
  active_at: "t1",
  on: true,
  ...o,
});
const docket = (o: Partial<Docket>): Docket =>
  ({ id: "D-1", title: "Shared", title_source: "person", status: "in_progress", protocol: "none", prompts: 0, turns: 0, notes: 0, created_at: "t0", updated_at: "t9", entries: 1, head: "", short_head: "", live: null, quick: false, ...o }) as Docket;

test("the agents on a docket, the lead first", () => {
  const d = docket({
    agents: [agent("block:1", { live: live("block:1", "idle") }), agent("herdr:t2", { live: live("herdr:t2", "blocked"), role: "reviewer" }), agent("block:3", { on: false, left: "left" }), agent("block:4")],
    lead: "herdr:t2",
  });
  assert.deepEqual(agentsOn(d).map((a) => a.block), ["block:1", "herdr:t2", "block:4"]);
  assert.deepEqual(liveAgents(d).map((a) => a.block), ["herdr:t2", "block:1"]);
  assert.equal(agentLabel(liveAgents(d)[0]), "reviewer");
  assert.equal(agentLabel(agent("block:1", { pane: "" })), "Claude Code");
});

test("an older hub's docket: its one agent, while live", () => {
  const ref = { app: "codex", host: "local", block: "block:1", rex_session: "api", pane: "builder" };
  assert.deepEqual(agentsOn(docket({ agent: ref, live: live("block:1", "working") })).map((a) => a.block), ["block:1"]);
  assert.deepEqual(agentsOn(docket({ agent: ref, live: null })), []);
});

test("an entry names its agent by pane, or falls back to the docket's", () => {
  const d = docket({ agents: [agent("block:1"), agent("herdr:t2", { role: "tests" })], agent: agent("block:1") });
  assert.equal(entryBlock({ data: { block: "herdr:t2" } }), "herdr:t2");
  assert.equal(entryBlock({ data: { agent: { block: "block:1" } } }), "block:1");
  assert.equal(entryBlock({ actor: { type: "agent", block: "herdr:t2" }, data: {} }), "herdr:t2");
  assert.equal(agentOfEntry(d, { data: { block: "herdr:t2" } })?.block, "herdr:t2");
  assert.equal(agentOfEntry(d, { data: {} }), undefined, "an old entry on a docket that's had several: no one's in particular");
  const one = docket({ agents: [agent("block:1")], agent: agent("block:1") });
  assert.equal(agentOfEntry(one, { data: {} })?.block, "block:1", "an old entry on a docket with one: its agent");
});

test("each agent Wheelhouse starts has its name, and a message's name gives its app back", () => {
  assert.deepEqual(startApps.map(appName), ["Claude Code", "Codex", "Antigravity", "Pi"]);
  for (const a of startApps) assert.equal(appOfName(appName(a)), a);
  assert.equal(appOfName(null), "claude-code");
  // A new agent on a docket is the kind already there.
  assert.equal(startAppFor("pi"), "pi");
  assert.equal(startAppFor("antigravity"), "agy");
  assert.equal(startAppFor("rex"), "claude-code");
  assert.equal(startAppFor(undefined), "claude-code");
});
