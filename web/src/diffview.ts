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
};

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
