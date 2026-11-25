// A docket's code changes, as GET /api/dockets/{id}/diff answers (src/diffs.rs),
// and the small pure helpers the Changes view needs. Pure, so `npm test` runs
// them without a browser.

export type DiffPoint = { ref: string; sha: string | null; label: string; at?: string; seq?: number };
export type DiffSide = { ref: string; sha: string | null; label: string };
export type DiffFile = { path: string; old_path?: string | null; added: number; removed: number; binary: boolean; untracked: boolean };
export type DiffView = {
  docket: string;
  /** worktree, folder, checkout (the worktree went), or record (only the kept patches). */
  source: "worktree" | "folder" | "checkout" | "record";
  live: boolean;
  repo?: string | null;
  dir?: string | null;
  head: string;
  base?: string | null;
  from: DiffSide;
  to: DiffSide;
  points: DiffPoint[];
  files: DiffFile[];
  added: number;
  removed: number;
  /** Null when only the counts were asked for, or the whole patch is over the hub's cap. */
  patch: string | null;
  too_big: boolean;
  untracked_more: number;
  /** Every comment on the docket's diff, every version's (src/comments.rs). */
  comments?: DiffComment[];
};

/** Where a comment is: a file's line or lines, on the new side (`additions`) or the old (`deletions`), between two commits (`to` none for the working tree, on `head`). */
export type CommentPlace = {
  path: string;
  old_path?: string | null;
  side: "additions" | "deletions";
  line: number;
  end_line?: number | null;
  from: string;
  to?: string | null;
  head?: string | null;
  excerpt?: string | null;
};
export type DiffComment = { seq: number; at: string; by: string; body: string; diff: CommentPlace };

/** The diff's address: from and to, when chosen; one file, when asked. */
export function diffPath(id: string, q: { from?: string | null; to?: string | null; path?: string | null; old_path?: string | null; stat?: boolean } = {}): string {
  const p = new URLSearchParams();
  if (q.from) p.set("from", q.from);
  if (q.to) p.set("to", q.to);
  if (q.path) p.set("path", q.path);
  if (q.old_path) p.set("old_path", q.old_path);
  if (q.stat) p.set("stat", "1");
  const s = p.toString();
  return `/api/dockets/${encodeURIComponent(id)}/diff${s ? `?${s}` : ""}`;
}

/** A file's folder and name, for a list that reads by name. */
export function splitPath(path: string): { dir: string; name: string } {
  const i = path.lastIndexOf("/");
  return i < 0 ? { dir: "", name: path } : { dir: path.slice(0, i + 1), name: path.slice(i + 1) };
}

/** What happened to a file: added, deleted, renamed, or changed. */
export function fileStatus(f: DiffFile, statuses?: Map<string, "A" | "D">): "A" | "D" | "R" | "M" {
  if (f.untracked) return "A";
  if (f.old_path && f.old_path !== f.path) return "R";
  return statuses?.get(f.path) ?? "M";
}

/** The files a patch adds or deletes, read in one pass over its headers. */
export function statusesOf(patch?: string | null): Map<string, "A" | "D"> {
  const out = new Map<string, "A" | "D">();
  if (!patch) return out;
  let cur: string | null = null;
  for (const line of patch.split("\n")) {
    if (line.startsWith("diff --git ")) cur = nameOf(line.slice(11));
    else if (!cur) continue;
    else if (line.startsWith("new file mode")) out.set(cur, "A");
    else if (line.startsWith("deleted file mode")) out.set(cur, "D");
    else if (line.startsWith("@@")) cur = null;
  }
  return out;
}

/** `a/X b/X`: X, when both halves name the same file (a rename has its own lines). */
function nameOf(rest: string): string | null {
  const n = (rest.length - 5) / 2;
  if (!rest.startsWith("a/") || !Number.isInteger(n) || n < 1) return null;
  const name = rest.slice(2, 2 + n);
  return rest.slice(2 + n, 5 + n) === " b/" && rest.slice(5 + n) === name ? name : null;
}

/** Files in the order a reader wants them: by folder, then name. */
export function sortFiles(files: DiffFile[]): DiffFile[] {
  return [...files].sort((a, b) => {
    const x = splitPath(a.path);
    const y = splitPath(b.path);
    return x.dir === y.dir ? x.name.localeCompare(y.name) : x.dir.localeCompare(y.dir);
  });
}

/** A short fingerprint of a diff's files and counts, to tell when it changed. */
export function statKey(v: Pick<DiffView, "files" | "head" | "from" | "to"> | null): string {
  if (!v) return "";
  return [v.head, v.from.sha, v.to.sha ?? "work", ...v.files.map((f) => `${f.path}:${f.added}:${f.removed}`)].join("|");
}

/** "+12 −3", or "no changes". */
export function countsLabel(added: number, removed: number): string {
  return added || removed ? `+${added} −${removed}` : "no changes";
}

/** Where the code was read from, for the line under the title. */
export function sourceLabel(v: Pick<DiffView, "source" | "dir">): string {
  switch (v.source) {
    case "worktree":
      return `Worktree · ${v.dir ?? ""}`;
    case "folder":
      return `Folder · ${v.dir ?? ""}`;
    case "checkout":
      return `The worktree is gone; read from ${v.dir ?? "the repo's checkout"}`;
    case "record":
      return "The checkout is gone; this is the patch the record kept with its checks";
  }
}

/** Two commits are one when one names the other (a short sha and a full one). */
const sameSha = (a?: string | null, b?: string | null) => !!a && !!b && (a.startsWith(b) || b.startsWith(a));

/** A path as git writes it after `--- ` or `+++ `: `a/` or `b/` off, the tab git adds after a name with a space off, and a quoted name (`"b/a\"b.txt"`) read back. */
export function patchName(raw: string): string {
  let n = raw.replace(/\t.*$/, "");
  if (n.startsWith('"') && n.endsWith('"') && n.length >= 2) n = unquote(n.slice(1, -1));
  return n.replace(/^[ab]\//, "");
}

/** Git's C-style quoting: `\"`, `\\`, `\t`, `\n` and octal bytes (`\303\251` is é). */
function unquote(s: string): string {
  const bytes: number[] = [];
  const esc: Record<string, number> = { n: 10, t: 9, r: 13, '"': 34, "\\": 92, a: 7, b: 8, f: 12, v: 11 };
  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    if (ch === "\\" && i + 1 < s.length) {
      const o = /^[0-7]{3}/.exec(s.slice(i + 1));
      if (o) {
        bytes.push(parseInt(o[0], 8));
        i += 3;
        continue;
      }
      const e = esc[s[i + 1]];
      if (e !== undefined) {
        bytes.push(e);
        i += 1;
        continue;
      }
    }
    bytes.push(...new TextEncoder().encode(ch));
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}

/**
 * Every line one side of a file shows in a unified patch, by its number: the
 * code a comment is on, and whether its line is in the diff at all. Headers
 * are read only between hunks, and each hunk is counted down by its lengths,
 * so a removed `-- comment` or an added `++x` is content, never a header.
 */
export function sideLines(patch: string | null | undefined, path: string, side: "additions" | "deletions"): Map<number, string> {
  const out = new Map<number, string>();
  if (!patch) return out;
  let inFile = false;
  let oldName = "";
  let oldN = 0;
  let newN = 0;
  let oldLeft = 0;
  let newLeft = 0;
  for (const raw of patch.split("\n")) {
    if (oldLeft > 0 || newLeft > 0) {
      if (raw.startsWith("\\")) continue;
      const mark = raw[0];
      const text = raw.slice(1);
      if (mark === "+") {
        if (inFile && side === "additions") out.set(newN, text);
        newN++;
        newLeft--;
      } else if (mark === "-") {
        if (inFile && side === "deletions") out.set(oldN, text);
        oldN++;
        oldLeft--;
      } else {
        // Context (a space, or an empty line some tools leave for a blank one).
        if (inFile) out.set(side === "additions" ? newN : oldN, text);
        oldN++;
        newN++;
        oldLeft--;
        newLeft--;
      }
      continue;
    }
    if (raw.startsWith("diff --git ")) {
      inFile = false;
      continue;
    }
    if (raw.startsWith("--- ")) {
      oldName = patchName(raw.slice(4));
      continue;
    }
    if (raw.startsWith("+++ ")) {
      // A deleted file's new name is /dev/null: it's known by its old one.
      const name = raw.slice(4).replace(/\t.*$/, "") === "/dev/null" ? oldName : patchName(raw.slice(4));
      inFile = name === path;
      continue;
    }
    const h = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(raw);
    if (h) {
      oldN = Number(h[1]);
      oldLeft = h[2] === undefined ? 1 : Number(h[2]);
      newN = Number(h[3]);
      newLeft = h[4] === undefined ? 1 : Number(h[4]);
    }
  }
  return out;
}

/** The lines `line` to `end` of one side of a file, from a unified patch: the code a comment is on. Empty when the patch doesn't show them. */
export function excerptOf(patch: string | null | undefined, path: string, side: "additions" | "deletions", line: number, end = line): string {
  const lines = sideLines(patch, path, side);
  const [lo, hi] = line <= end ? [line, end] : [end, line];
  const out: string[] = [];
  for (let n = lo; n <= hi; n++) if (lines.has(n)) out.push(lines.get(n)!);
  return out.join("\n");
}

/**
 * Whether a comment's lines are the ones shown: the new side's are the same
 * when `to` is, the old side's when `from` is, and the working tree's while
 * HEAD and the code it was on are the same (an agent may have changed them
 * since). Its last line must be one the diff shows, or there's nowhere to put it.
 */
export function commentHere(c: DiffComment, v: Pick<DiffView, "from" | "to" | "head">, patch?: string | null): boolean {
  const d = c.diff;
  const same = d.side === "deletions" ? sameSha(d.from, v.from.sha) : d.to ? sameSha(d.to, v.to.sha) : v.to.ref === "work" && sameSha(d.head, v.head);
  if (!same || patch === undefined) return same;
  const lines = sideLines(patch, d.path, d.side);
  const last = d.end_line ?? d.line;
  if (!lines.has(last)) return false;
  if (!d.to && d.side === "additions" && d.excerpt) return excerptOf(patch, d.path, d.side, d.line, last) === d.excerpt;
  return true;
}

/** The comments on what's shown, by file, and those on other versions (or on lines this diff doesn't show). */
export function placeComments(comments: DiffComment[] | undefined, v: Pick<DiffView, "from" | "to" | "head" | "files">, patch?: string | null): { here: Map<string, DiffComment[]>; elsewhere: DiffComment[] } {
  const here = new Map<string, DiffComment[]>();
  const elsewhere: DiffComment[] = [];
  const paths = new Set(v.files.map((f) => f.path));
  for (const c of comments ?? []) {
    if (paths.has(c.diff.path) && commentHere(c, v, patch)) here.set(c.diff.path, [...(here.get(c.diff.path) ?? []), c]);
    else elsewhere.push(c);
  }
  return { here, elsewhere };
}

/** Where a comment is, for people: `src/a.rs:12`, `src/a.rs:12–14`, and `(old)` on the removed lines (as src/comments.rs says it). */
export function commentPlace(d: Pick<CommentPlace, "path" | "line" | "end_line" | "side">): string {
  return `${d.path}:${d.end_line && d.end_line !== d.line ? `${d.line}–${d.end_line}` : d.line}${d.side === "deletions" ? " (old)" : ""}`;
}

/** A Markdown code fence longer than any run of backticks in the code. */
export function fenceFor(code: string): string {
  const longest = Math.max(0, ...(code.match(/`+/g) ?? []).map((r) => r.length));
  return "`".repeat(Math.max(3, longest + 1));
}

/** Where a drag in the gutter picked: one side's lines, low to high. A drag across sides (an old line onto its new one) is its last line alone, since the two sides' numbers don't mix. */
export function pickedLines(range: { start: number; end: number; side?: "additions" | "deletions"; endSide?: "additions" | "deletions" }): { side: "additions" | "deletions"; line: number; end: number } {
  const startSide = range.side ?? "additions";
  const endSide = range.endSide ?? startSide;
  if (endSide !== startSide) return { side: endSide, line: range.end, end: range.end };
  return { side: startSide, line: Math.min(range.start, range.end), end: Math.max(range.start, range.end) };
}
