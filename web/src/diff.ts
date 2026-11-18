// A line diff, for what changed between two texts: an intent's versions,
// or a guide or brief against its built-in text. Pure, so `npm test` can
// run it without a browser.

export type DiffLine = { op: "same" | "add" | "del"; text: string };

/** A line diff (longest common subsequence): enough for a spec's versions. */
export function lineDiff(a: string, b: string): DiffLine[] {
  const x = a ? a.split("\n") : [];
  const y = b ? b.split("\n") : [];
  const n = x.length;
  const m = y.length;
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) lcs[i][j] = x[i] === y[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (x[i] === y[j]) (out.push({ op: "same", text: x[i] }), i++, j++);
    else if (lcs[i + 1][j] >= lcs[i][j + 1]) out.push({ op: "del", text: x[i++] });
    else out.push({ op: "add", text: y[j++] });
  }
  while (i < n) out.push({ op: "del", text: x[i++] });
  while (j < m) out.push({ op: "add", text: y[j++] });
  return out;
}

/** How many lines a diff adds and takes away. */
export function diffCounts(d: DiffLine[]): { added: number; removed: number } {
  return { added: d.filter((l) => l.op === "add").length, removed: d.filter((l) => l.op === "del").length };
}

/**
 * A long diff, folded: changed lines with `context` lines around them, and
 * each run of unchanged lines between as one `skip` with its count.
 */
export function foldDiff(d: DiffLine[], context = 3): (DiffLine | { op: "skip"; count: number })[] {
  const near = new Array(d.length).fill(false);
  d.forEach((l, i) => {
    if (l.op === "same") return;
    for (let k = Math.max(0, i - context); k <= Math.min(d.length - 1, i + context); k++) near[k] = true;
  });
  const out: (DiffLine | { op: "skip"; count: number })[] = [];
  let skipped = 0;
  d.forEach((l, i) => {
    if (near[i]) {
      if (skipped) out.push({ op: "skip", count: skipped });
      skipped = 0;
      out.push(l);
    } else skipped++;
  });
  if (skipped) out.push({ op: "skip", count: skipped });
  return out;
}
