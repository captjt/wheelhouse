// The line diff: an intent's versions, and a guide or brief against its
// built-in text.
import assert from "node:assert/strict";
import { test } from "node:test";
import { diffCounts, foldDiff, lineDiff } from "../src/diff.ts";

test("a line diff keeps what's the same and marks what's added and taken out", () => {
  const d = lineDiff("a\nb\nc", "a\nB\nc\nd");
  assert.deepEqual(d, [
    { op: "same", text: "a" },
    { op: "del", text: "b" },
    { op: "add", text: "B" },
    { op: "same", text: "c" },
    { op: "add", text: "d" },
  ]);
  assert.deepEqual(diffCounts(d), { added: 2, removed: 1 });
  assert.deepEqual(lineDiff("", "x"), [{ op: "add", text: "x" }]);
  assert.deepEqual(lineDiff("x", ""), [{ op: "del", text: "x" }]);
  assert.deepEqual(diffCounts(lineDiff("same\ntext", "same\ntext")), { added: 0, removed: 0 });
});

test("a long diff folds its unchanged runs, keeping a few lines around each change", () => {
  const before = Array.from({ length: 20 }, (_, i) => `line ${i + 1}`).join("\n");
  const after = before.replace("line 10", "line ten");
  const folded = foldDiff(lineDiff(before, after), 2);
  assert.deepEqual(folded[0], { op: "skip", count: 7 });
  assert.deepEqual(
    folded.slice(1, 7).map((l) => ("text" in l ? l.text : "")),
    ["line 8", "line 9", "line 10", "line ten", "line 11", "line 12"],
  );
  assert.deepEqual(folded[7], { op: "skip", count: 8 });
  assert.equal(folded.length, 8);
  // Nothing changed: one fold.
  assert.deepEqual(foldDiff(lineDiff("a\nb", "a\nb")), [{ op: "skip", count: 2 }]);
});
