// A docket's Changes view: its address, its file list, and when it changed.
import assert from "node:assert/strict";
import { test } from "node:test";
import { countsLabel, diffPath, fileStatus, sortFiles, sourceLabel, splitPath, statKey, statusesOf, type DiffFile } from "../src/diffview.ts";

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
