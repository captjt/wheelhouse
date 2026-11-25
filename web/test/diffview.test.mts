// A docket's Changes view: its address, its file list, and when it changed.
import assert from "node:assert/strict";
import { test } from "node:test";
import { commentHere, commentPlace, countsLabel, diffPath, excerptOf, fileStatus, placeComments, sortFiles, sourceLabel, splitPath, statKey, statusesOf, type DiffComment, type DiffFile, type DiffView } from "../src/diffview.ts";

const f = (path: string, more: Partial<DiffFile> = {}): DiffFile => ({ path, added: 1, removed: 0, binary: false, untracked: false, ...more });

test("the diff's address carries only what was chosen, encoded", () => {
  assert.equal(diffPath("D-12"), "/api/dockets/D-12/diff");
  assert.equal(diffPath("D-12", { from: "base", to: "work", stat: true }), "/api/dockets/D-12/diff?from=base&to=work&stat=1");
  assert.equal(diffPath("D-1", { path: "a b/c&d.ts", old_path: "x.ts" }), "/api/dockets/D-1/diff?path=a+b%2Fc%26d.ts&old_path=x.ts");
  assert.equal(diffPath("ACME/12"), "/api/dockets/ACME%2F12/diff");
});

test("files read by folder, then name", () => {
  assert.deepEqual(splitPath("src/a/b.rs"), { dir: "src/a/", name: "b.rs" });
  assert.deepEqual(splitPath("README.md"), { dir: "", name: "README.md" });
  assert.deepEqual(
    sortFiles([f("web/z.ts"), f("src/b.rs"), f("Cargo.toml"), f("src/a.rs")]).map((x) => x.path),
    ["Cargo.toml", "src/a.rs", "src/b.rs", "web/z.ts"],
  );
});

test("a file's status comes from the patch, a rename, or being new", () => {
  const patch = "diff --git a/n.rs b/n.rs\nnew file mode 100644\n--- /dev/null\n+++ b/n.rs\n@@ -0,0 +1 @@\n+x\ndiff --git a/g.rs b/g.rs\ndeleted file mode 100644\n@@ -1 +0,0 @@\n-x\n";
  const st = statusesOf(patch);
  assert.equal(fileStatus(f("n.rs"), st), "A");
  assert.equal(fileStatus(f("g.rs"), st), "D");
  assert.equal(fileStatus(f("m.rs"), st), "M");
  assert.equal(statusesOf(null).size, 0);
  // A name with spaces, and a "new file mode" line inside a hunk, which isn't a header.
  const tricky = "diff --git a/a b.rs b/a b.rs\nnew file mode 100644\n@@ -0,0 +1 @@\n+x\ndiff --git a/c.rs b/c.rs\n@@ -1 +1 @@\n-new file mode 1\n+y\n";
  assert.deepEqual([...statusesOf(tricky)], [["a b.rs", "A"]]);
  assert.equal(fileStatus(f("new.rs", { old_path: "old.rs" })), "R");
  assert.equal(fileStatus(f("u.txt", { untracked: true })), "A");
});

test("the fingerprint changes when a file's counts or the head do", () => {
  const v = { head: "h1", from: { ref: "base", sha: "b", label: "Base" }, to: { ref: "work", sha: null, label: "Uncommitted" }, files: [f("a.rs")] };
  assert.equal(statKey(v), statKey({ ...v }));
  assert.notEqual(statKey(v), statKey({ ...v, files: [f("a.rs", { added: 2 })] }));
  assert.notEqual(statKey(v), statKey({ ...v, head: "h2" }));
  assert.equal(statKey(null), "");
});

test("labels say what changed and where it was read", () => {
  assert.equal(countsLabel(12, 3), "+12 −3");
  assert.equal(countsLabel(0, 0), "no changes");
  assert.match(sourceLabel({ source: "worktree", dir: "~/w" }), /Worktree · ~\/w/);
  assert.match(sourceLabel({ source: "record", dir: null }), /record kept/);
});

const PATCH = [
  "diff --git a/src/retry.rs b/src/retry.rs",
  "index 1111111..2222222 100644",
  "--- a/src/retry.rs",
  "+++ b/src/retry.rs",
  "@@ -10,4 +10,5 @@ fn retry() {",
  "     let a = 1;",
  "-    let tries = 5;",
  "+    let tries = 3;",
  "+    log(tries);",
  "     go(a, tries)",
  "diff --git a/old.txt b/old.txt",
  "deleted file mode 100644",
  "--- a/old.txt",
  "+++ /dev/null",
  "@@ -1,2 +0,0 @@",
  "-gone",
  "-too",
  "",
].join("\n");

test("the code a comment is on, from the patch: either side, a span, a deleted file", () => {
  assert.equal(excerptOf(PATCH, "src/retry.rs", "additions", 11), "    let tries = 3;");
  assert.equal(excerptOf(PATCH, "src/retry.rs", "additions", 11, 12), "    let tries = 3;\n    log(tries);");
  assert.equal(excerptOf(PATCH, "src/retry.rs", "additions", 13, 10), "    let a = 1;\n    let tries = 3;\n    log(tries);\n    go(a, tries)", "a span given backwards");
  assert.equal(excerptOf(PATCH, "src/retry.rs", "deletions", 11), "    let tries = 5;");
  assert.equal(excerptOf(PATCH, "src/retry.rs", "deletions", 12), "    go(a, tries)", "context counts on both sides");
  assert.equal(excerptOf(PATCH, "old.txt", "deletions", 2), "too");
  assert.equal(excerptOf(PATCH, "src/retry.rs", "additions", 99), "");
  assert.equal(excerptOf(PATCH, "nope.rs", "additions", 11), "");
  assert.equal(excerptOf(null, "src/retry.rs", "additions", 11), "");
});

const view = (o: Partial<DiffView> = {}): Pick<DiffView, "from" | "to" | "head" | "files"> => ({
  from: { ref: "base", sha: "aaaaaaaaaaaa", label: "Base" },
  to: { ref: "head", sha: "bbbbbbbbbbbb", label: "HEAD" },
  head: "bbbbbbbbbbbb",
  files: [{ path: "src/retry.rs", added: 2, removed: 1, binary: false, untracked: false }],
  ...o,
});
const comment = (d: Partial<DiffComment["diff"]>): DiffComment => ({ seq: 1, at: "t", by: "you", body: "x", diff: { path: "src/retry.rs", side: "additions", line: 11, from: "aaaaaaa", to: "bbbbbbb", ...d } });

test("a comment shows on the version it was written on, and is listed apart otherwise", () => {
  assert.equal(commentHere(comment({}), view()), true, "a short sha names the full one");
  assert.equal(commentHere(comment({ to: "ccccccc" }), view()), false, "a later commit's lines aren't these");
  assert.equal(commentHere(comment({ side: "deletions", to: "ccccccc" }), view()), true, "the old side's lines are from's");
  assert.equal(commentHere(comment({ side: "deletions", from: "ddddddd" }), view()), false);
  const work = view({ to: { ref: "work", sha: null, label: "Uncommitted" } });
  assert.equal(commentHere(comment({ to: null, head: "bbbbbbb" }), work), true, "the working tree, on the same HEAD");
  assert.equal(commentHere(comment({ to: null, head: "eeeeeee" }), work), false, "on another HEAD it may have moved");
  assert.equal(commentHere(comment({}), work), false);
  const { here, elsewhere } = placeComments([comment({}), comment({ to: "ccccccc" }), comment({ path: "gone.rs" })], view());
  assert.equal(here.get("src/retry.rs")?.length, 1);
  assert.equal(elsewhere.length, 2, "another version's, and a file this diff doesn't have");
  assert.equal(commentPlace({ path: "a.rs", line: 3, end_line: 5, side: "deletions" }), "a.rs:3–5 (old)");
  assert.equal(commentPlace({ path: "a.rs", line: 3, end_line: 3, side: "additions" }), "a.rs:3");
});
