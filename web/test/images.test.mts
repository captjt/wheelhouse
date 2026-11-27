// Images you share with a docket's agents (src/images.rs): what the tray
// sends, the marks' pins, what's redrawn before it goes up, and the
// Markdown an image takes in the intent.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  MAX_PER,
  carriedBy,
  compareFor,
  entryImages,
  fitWithin,
  imageFiles,
  markdownImage,
  needsRedraw,
  pastedName,
  pinsOf,
  refsOf,
  renumber,
  splitImageBlock,
  stillAdding,
  strokeFor,
  trayState,
  imagesOf,
  type Mark,
  type TrayItem,
} from "../src/images.ts";
import type { EntryView } from "../src/api.ts";

const staged = (n: number) => ({ digest: `sha256:${String(n).repeat(64).slice(0, 64)}`, name: `shot-${n}.png`, media_type: "image/png", width: 10, height: 10, size: 99 });
const item = (o: Partial<TrayItem> = {}): TrayItem => ({ id: "a", url: "blob:x", name: "shot.png", caption: "", state: "ready", staged: staged(1), ...o });

test("the tray sends its ready images, with captions, originals and pins", () => {
  const pin = (n: number, note: string): Mark => ({ kind: "pin", x: 1, y: 1, n, note, color: "#e5484d" });
  const marked = item({ id: "b", caption: "  The header  ", staged: staged(2), original: { url: "blob:o", staged: staged(3), marks: [pin(1, " Primary "), { kind: "box", x: 0, y: 0, w: 5, h: 5, color: "#fff" }, pin(2, "")] } });
  assert.deepEqual(refsOf([item(), marked, item({ id: "c", state: "staging", staged: undefined })]), [
    { digest: staged(1).digest, name: "shot-1.png", caption: "" },
    { digest: staged(2).digest, name: "shot-2.png", caption: "The header", original: staged(3).digest, pins: [{ n: 1, note: "Primary" }, { n: 2, note: "" }] },
  ]);
  // An original that didn't go up: the pins' words still go.
  const lost = item({ original: { url: "blob:o", marks: [pin(1, "Here")] } });
  assert.deepEqual(refsOf([lost])[0].pins, [{ n: 1, note: "Here" }]);
  assert.equal(refsOf([lost])[0].original, undefined);
});

test("the tray can't be sent while an image is going up, or one failed", () => {
  assert.deepEqual(trayState([item()]), { ok: true });
  assert.equal(trayState([item({ state: "staging" })]).ok, false);
  assert.match(trayState([item({ state: "failed" }), item({ state: "failed" })]).why ?? "", /2 images didn't go up/);
  assert.equal(trayState(Array.from({ length: MAX_PER + 1 }, (_, i) => item({ id: String(i) }))).ok, false);
});

test("pins are numbered in order, with no gaps after one is taken out", () => {
  const p = (note: string): Mark => ({ kind: "pin", x: 0, y: 0, n: 9, note, color: "#e5484d" });
  const marks = renumber([p("a"), { kind: "arrow", x1: 0, y1: 0, x2: 1, y2: 1, color: "#fff" }, p("b"), p("c")]);
  assert.deepEqual(pinsOf(marks).map((x) => x.n), [1, 2, 3]);
  assert.deepEqual(pinsOf(renumber(marks.filter((m) => !(m.kind === "pin" && m.note === "b")))), [
    { n: 1, note: "a" },
    { n: 2, note: "c" },
  ]);
});

test("a mark reads the same on a phone shot and a 5K one", () => {
  assert.equal(strokeFor(100, 100), 3);
  assert.equal(strokeFor(5120, 2880), 13);
});

test("what's redrawn before it goes up: a phone's HEIC, SVG, or too big", () => {
  assert.equal(needsRedraw("image/png", 1000), false);
  assert.equal(needsRedraw("image/heic", 1000), true);
  assert.equal(needsRedraw("image/svg+xml", 10), true);
  assert.equal(needsRedraw("image/jpeg", 16 << 20), true);
  assert.deepEqual(fitWithin(8000, 4000), { w: 4096, h: 2048 });
  assert.deepEqual(fitWithin(800, 600), { w: 800, h: 600 });
});

test("only images come from a paste or a drop", () => {
  const f = (type: string) => ({ kind: "file", type, getAsFile: () => ({ type, name: "x" }) as unknown as File });
  assert.equal(imageFiles({ items: [f("image/png"), f("text/plain"), { kind: "string", type: "text/plain", getAsFile: () => null }] }).length, 1);
  assert.equal(imageFiles({ files: [{ type: "image/jpeg" } as File, { type: "application/pdf" } as File] }).length, 1);
  assert.deepEqual(imageFiles(null), []);
  assert.equal(pastedName(new Date(2026, 10, 27, 9, 4, 5)), "pasted-2026-11-27-090405.png");
});

test("an image in the intent is Markdown the hub keeps, and the editor waits for one going up", () => {
  assert.equal(markdownImage({ digest: "sha256:ab", name: "a b.png" }, "The [new] page\n"), "![The new page](/api/blobs/sha256:ab?name=a%20b.png)");
  assert.equal(stillAdding("Text\n![Adding shot.png (3)…]()\n"), true);
  assert.equal(stillAdding("![Done](/api/blobs/sha256:ab?name=a.png)"), false);
});

test("a prompt's paths are for the agent: the record shows how many images went", () => {
  const p = "match this\n\nImages come with this (open each one to see them: Read in Claude Code, view_image in Codex):\n1. /x/a.png (10×10): The header\n   Numbered pins on it: 1. Big\n2. /x/b.png\n";
  assert.deepEqual(splitImageBlock(p), { text: "match this", images: 2 });
  assert.deepEqual(splitImageBlock("plain"), { text: "plain", images: 0 });
});

test("a screenshot opens against the reference it was kept against, and the other way", () => {
  const art = (seq: number, data: Record<string, unknown>): EntryView => ({ seq, kind: "artifact", data: { media_type: "image/png", digest: "sha256:x", name: "x.png", ...data } }) as unknown as EntryView;
  const ref = art(3, { image: "reference" });
  const shot = art(7, { image: "attached", against: 3 });
  const svg = art(8, { media_type: "image/svg+xml" });
  const note = { seq: 4, kind: "note", data: { images: [{ digest: "sha256:x", name: "x.png" }, { digest: "sha256:y", name: "y.png", caption: "Not kept yet" }] } } as unknown as EntryView;
  const all = imagesOf([ref, shot, svg, note]);
  assert.deepEqual(all.map((e) => e.seq), [7, 3], "newest first, never SVG");
  assert.equal(compareFor(shot, all)?.seq, 3);
  assert.equal(compareFor(ref, all)?.seq, 7);
  // By digest: the kept reference, or the image as it was sent until it's kept (a shared docket's).
  const carried = entryImages(note, [ref, shot, note]);
  assert.equal(carried[0].seq, 3);
  assert.equal(carried[1].data.digest, "sha256:y");
  assert.ok(carried[1].seq < 0);
  // A reference a message shows already isn't shown again; a screenshot is.
  assert.equal(carriedBy(ref, [ref, shot, note]), true);
  assert.equal(carriedBy(shot, [ref, shot, note]), false);
  const orig = art(2, { image: "original", digest: "sha256:o" });
  const marked = art(5, { image: "reference", digest: "sha256:m", original: "sha256:o" });
  assert.equal(carriedBy(orig, [orig, marked]), true, "opened from its marked-up image");
});
