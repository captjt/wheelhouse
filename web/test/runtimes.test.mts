// Runtimes as the apps show them (src/runtime/): which one a pane is in,
// where new agents open, and the footer's line.
import assert from "node:assert/strict";
import { test } from "node:test";
import { connLine, followedRuntimes, paneRuntime, runtimeOf, spawnRuntime, type RuntimeLink } from "../src/api.ts";

const rex = (o: Partial<RuntimeLink> = {}): RuntimeLink => ({ name: "rex", title: "Rex", connected: true, installed: true, agents: 2, spawn: true, ...o });
const herdr = (o: Partial<RuntimeLink> = {}): RuntimeLink => ({ name: "herdr", title: "Herdr", connected: true, installed: true, agents: 1, spawn: false, ...o });
const off = { connected: false, error: null };

test("a pane id names its runtime", () => {
  assert.equal(runtimeOf("block:01a20af7"), "rex");
  assert.equal(runtimeOf("herdr:t2h"), "herdr");
  assert.equal(runtimeOf(undefined), "rex");
  assert.equal(paneRuntime({ agent: { app: "codex", host: "local", block: "herdr:t2h", rex_session: "wheelhouse", pane: "D-1" } }), "Herdr");
  assert.equal(paneRuntime({ live: { block: "herdr:t2h", runtime: "herdr", app: "codex", state: "idle", rex_session: "s", pane: "p" } }), "Herdr");
  assert.equal(paneRuntime(null), "Rex");
});

test("new agents open where the hub says, Rex before it said", () => {
  assert.equal(spawnRuntime({ runtimes: [rex({ spawn: false }), herdr({ spawn: true })] }), "Herdr");
  assert.equal(spawnRuntime({}), "Rex");
  assert.equal(followedRuntimes({ runtimes: [rex(), herdr()] }), "Rex and Herdr");
  assert.equal(followedRuntimes({ runtimes: [rex(), herdr({ connected: false })] }), "Rex");
});

test("the footer is calm about a runtime that isn't installed, not one that's down", () => {
  assert.deepEqual(connLine({ rex: off, runtimes: [rex(), herdr()] }), { ok: true, text: "Rex · Herdr", title: "Following Rex and Herdr" });
  assert.deepEqual(connLine({ rex: off, runtimes: [rex(), herdr({ connected: false, installed: false })] }), { ok: true, text: "Following Rex", title: "Following Rex" });
  // Herdr installed but down, while new agents open in Rex: still fine.
  assert.equal(connLine({ rex: off, runtimes: [rex(), herdr({ connected: false })] }).ok, true);
  // The one new agents open in is down: that's trouble.
  assert.deepEqual(connLine({ rex: off, runtimes: [rex({ connected: false, error: "no server" }), herdr()] }), { ok: false, text: "Rex: no server" });
  assert.deepEqual(connLine({ rex: off, runtimes: [rex({ connected: false, installed: false }), herdr({ connected: false, installed: false })] }), { ok: false, text: "Rex isn't installed" });
  // An older hub says only Rex's.
  assert.deepEqual(connLine({ rex: { connected: true } }), { ok: true, text: "Following Rex", title: "Following Rex" });
});
