// The parts of the Markdown renderer that don't need a browser:
// `npm test` (node --test, which runs TypeScript as it is).
import assert from "node:assert/strict";
import { test } from "node:test";
import { hardWrapped, toggleTask } from "../src/markdown.ts";

test("a task box ticks in the text, skipping code blocks", () => {
  const t = "- [x] a\n```\n- [ ] in code\n```\n> - [ ] quoted\n1. [ ] numbered\n  - [X] nested";
  assert.equal(toggleTask(t, 1, true, 4), t.replace("> - [ ] quoted", "> - [x] quoted"));
  assert.equal(toggleTask(t, 3, false, 4), t.replace("  - [X] nested", "  - [ ] nested"));
  // The rendered boxes don't match the text's: refuse, rather than tick the wrong one.
  assert.equal(toggleTask(t, 0, false, 5), null);
});

test("wrapped prose joins; a line ended on purpose keeps its break", () => {
  assert.ok(hardWrapped("This paragraph was hard-wrapped by an agent at seventy-two columns, the\nway a brief or a commit message is, and it should read as one paragraph\nrather."));
  assert.ok(hardWrapped("The fix holds for the 5xx case, but the 4xx path still retries once because\nthe guard reads the wrong status."));
  assert.ok(!hardWrapped("Owner: Jordan\nDue: Friday"));
  assert.ok(!hardWrapped("Close out P-GRD-130 (R1.4: Agent Certification PDF), now including GRD-438.\nOwner: Jordan"));
  assert.ok(!hardWrapped("Ship retries for failed webhooks, so a flaky endpoint doesn't drop events. Tracks D-26 and GRD-142 here.\nOwner"));
});
