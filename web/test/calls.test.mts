// A call's proposals in words, and what Edit may change (src/calls.rs).
import assert from "node:assert/strict";
import { test } from "node:test";
import { cantApply, editable, kindWord, listeningWords, parseOptions, whatItDoes } from "../src/calls.ts";
import type { CallView, Proposal } from "../src/api.ts";

const p = (o: Partial<Proposal>): Proposal => ({ n: 1, seq: 4, at: "t", kind: "decide", options: [], quote: "q", quote_found: true, status: "open", ...o });

test("what applying a proposal would do, in a line", () => {
  assert.equal(whatItDoes(p({ docket: "D-41", subject: "Retries", choice: "5xx only" })), "Decide on D-41: Retries: 5xx only");
  assert.equal(whatItDoes(p({ docket: "D-41", choice: "5xx only" })), "Decide on D-41: 5xx only");
  assert.equal(whatItDoes(p({ kind: "ask", docket: "D-41", question: "Do 429s count?\nAnd 408s?" })), "Ask the agents on D-41: Do 429s count?");
  assert.equal(whatItDoes(p({ kind: "task", title: "Write the runbook" })), "Open a docket: Write the runbook");
  assert.equal(kindWord("ask"), "Question");
});

test("Apply waits for a docket and the words its kind needs", () => {
  assert.equal(cantApply(p({ choice: "x" })), "Pick the docket it's for: Edit");
  assert.equal(cantApply(p({ docket: "D-1", choice: " " })), "Nothing decided");
  assert.equal(cantApply(p({ kind: "ask", docket: "D-1" })), "No question");
  assert.equal(cantApply(p({ kind: "task", title: "" })), "No title");
  assert.equal(cantApply(p({ kind: "task", title: "T" })), null, "a task needs no docket");
  assert.deepEqual(editable("ask"), ["docket", "question", "options"]);
  assert.deepEqual(editable("task"), ["title", "intent"]);
  assert.deepEqual(parseOptions("Yes\n No | Maybe\r\n\n"), ["Yes", "No | Maybe"], "one per line: an option may hold a |");
});

test("how listening went, in words", () => {
  const v = (l: CallView["listening"], n = 0): CallView => ({ listening: l, proposals: Array.from({ length: n }, (_, i) => p({ n: i + 1 })) });
  assert.equal(listeningWords(v({ state: "started" }))?.tone, "busy");
  assert.match(listeningWords(v({ state: "failed", why: "it timed out" }))!.text, /it timed out/);
  assert.equal(listeningWords(v({ state: "stopped" }))?.tone, "error");
  assert.match(listeningWords(v({ state: "done" }))!.text, /nothing/);
  assert.match(listeningWords(v({ state: "done", left_out: ["x", "y"] }, 2))!.text, /^2 more/);
  assert.equal(listeningWords(v({ state: "done" }, 3)), null);
  assert.equal(listeningWords(v({ state: null })), null);
});
