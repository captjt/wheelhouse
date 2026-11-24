// Who wrote an entry, in words, on a shared docket and on your own: a
// teammate's entries are named for them, never "You" (docs/teams.md).
import assert from "node:assert/strict";
import { test } from "node:test";
import { actorName, mateOf } from "../src/api.ts";

const person = (id: string) => ({ type: "person", id });
const agent = (id: string) => ({ type: "agent", id });
const hub = (id: string) => ({ type: "hub", id });

test("a teammate's person, hub and agents are named for them", () => {
  assert.equal(actorName(person("sam"), "ACME-1", "alex"), "sam");
  assert.equal(actorName(agent("codex@sam"), "ACME-1", "alex"), "sam's Codex");
  assert.equal(actorName(hub("hub@sam"), "ACME-1", "alex"), "sam's Wheelhouse");
  assert.equal(mateOf(agent("claude-code@sam"), "ACME-1", "alex"), "sam");
});

test("your own entries on a shared docket are yours", () => {
  assert.equal(actorName(person("alex"), "ACME-1", "alex"), "You");
  assert.equal(actorName(agent("claude-code@alex"), "ACME-1", "alex"), "Claude Code");
  assert.equal(mateOf(hub("hub@alex"), "ACME-1", "alex"), null);
});

test("on your own dockets, nobody is a teammate", () => {
  assert.equal(actorName(person("you"), "D-4", "alex"), "You");
  assert.equal(actorName(agent("codex@block:1"), "D-4", "alex"), "Codex");
  assert.equal(mateOf(person("sam"), "D-4", "alex"), null);
  // An agent actor's @ names its pane on a local docket, never a member.
  assert.equal(mateOf(agent("codex@local"), "ACME-1", "alex"), null);
});
