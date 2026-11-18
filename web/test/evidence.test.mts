// Verify, in words, and the Inbox's attention beside your asks.
import assert from "node:assert/strict";
import { test } from "node:test";
import { attentionShown, cantRun, checkState, checkWord, duration, independence, notRun, shortSha, verifyShown, whyWord } from "../src/evidence.ts";
import type { Attention, Evidence } from "../src/api.ts";

const ev = (e: Partial<Evidence> = {}): Evidence => ({ needs: false, evidence: [], checks: [], repo: "acme/api", configured: [], running: false, verdicts: [], hold: null, closed_on: null, ...e });

test("a check, in words: passed, failed with its exit, or timed out", () => {
  assert.equal(checkWord({ ok: true, exit: 0 }), "passed");
  assert.equal(checkWord({ ok: false, exit: 1 }), "failed, exit 1");
  assert.equal(checkWord({ ok: false, exit: null }), "timed out");
  // An optional one that fails only informs.
  assert.equal(checkState({ ok: false, exit: 1, required: false }), "inform");
  assert.equal(checkState({ ok: false, exit: 1, required: true }), "fail");
  assert.equal(checkState({ ok: false, exit: null, required: true }), "timeout");
  assert.equal(checkState({ ok: true, exit: 0, required: false }), "pass");
});

test("how long it took, and a short commit", () => {
  assert.equal(duration(153), "0.2s");
  assert.equal(duration(20), "0.1s");
  assert.equal(duration(4200), "4s");
  assert.equal(duration(64_000), "1m 4s");
  assert.equal(duration(120_000), "2m");
  assert.equal(duration(null), "");
  assert.equal(shortSha("30297c98165f9d7e98fa5dc0bc553e2b7d096670"), "30297c9");
  assert.equal(whyWord("asked"), "you asked");
});

test("Verify shows only when it has something to say", () => {
  // A repo with checks configured, none run, nothing needed: quiet.
  assert.ok(!verifyShown(ev({ configured: [{ name: "test", run: "./t", required: true }] })));
  assert.ok(!verifyShown(null));
  assert.ok(verifyShown(ev({ needs: true })));
  assert.ok(verifyShown(ev({ hold: "Waiting on the DBA" })));
  assert.ok(verifyShown(ev({ closed_on: "Checked by hand" })));
  assert.ok(verifyShown(ev({ running: true })));
  assert.ok(verifyShown(ev({ evidence: [{ how: "checkable", what: "The review is on GitHub" }] })));
});

test("checks not run yet, and why Run checks can't run", () => {
  const configured = [
    { name: "lint", run: "./lint", required: false },
    { name: "test", run: "./run-tests", required: true },
  ];
  const ran = { name: "test", command: "./run-tests", required: true, exit: 0, ok: true, ms: 1, head: "abc", seq: 1, at: "" };
  assert.deepEqual(notRun(ev({ configured, checks: [ran] })).map((c) => c.name), ["lint"]);
  assert.equal(cantRun(ev({ configured })), null);
  assert.match(cantRun(ev())!, /acme\/api has no checks yet/);
  assert.match(cantRun(ev({ repo: null }))!, /isn't in a repo/);
  assert.match(cantRun(ev({ configured, running: true }))!, /running/);
});

test("a verdict's independence, in words, or nothing when it can't say", () => {
  assert.equal(independence({ independent: true }), "independent");
  assert.equal(independence({ independent: false }), "same session as the builder");
  assert.equal(independence({ independent: null }), null);
});

test("attention leaves out what's already an ask, and keeps the rest in order", () => {
  const att: Attention[] = [
    { docket: "D-9", title: "storefront#212", kind: "review", text: "Review of storefront#212: ready" },
    { docket: "D-4", title: "Bundle discounts", kind: "build", text: "Build: an agent is blocked" },
    { docket: "D-2", title: "Webhooks", kind: "failing", text: "Failing: `test`" },
    { docket: "D-3", title: "Region column", kind: "held", text: "On hold: the DBA" },
    { docket: null, title: "acme/api#7", kind: "review", text: "Review of acme/api#7: stalled" },
    { docket: "D-1", title: "Checkout", kind: "word", text: "Closed on your word: by hand" },
  ];
  const asks = [
    { docket: "D-9", kind: "review" },
    { docket: "D-4", kind: "blocked" },
    { docket: "D-3", kind: "error" },
    { docket: "acme/api#7", kind: "review" },
  ];
  assert.deepEqual(
    attentionShown(att, asks).map((a) => `${a.docket}:${a.kind}`),
    ["D-2:failing", "D-3:held", "D-1:word"],
    "a held docket whose agent failed is both: the failure is the ask, the hold is attention",
  );
  assert.deepEqual(attentionShown(undefined, []), []);
  assert.equal(attentionShown([att[2], att[2]], []).length, 1, "the same thing once");
});
