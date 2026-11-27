// Images you share with a docket's agents (src/images.rs): staged with
// `POST /api/images`, then sent with what you typed in the composer, each
// with a caption and, when you marked it up, its numbered pins and the
// original. Here is what doesn't draw: the tray's items, the marks, what
// goes to the hub, and the Markdown an image takes in the intent.
import type { EntryView } from "./api.ts";

/** What the hub takes: a PNG, JPEG, GIF or WebP (never SVG). */
export const SENDABLE = ["image/png", "image/jpeg", "image/gif", "image/webp"] as const;
/** The hub's limits (src/images.rs). */
export const MAX_BYTES = 15 << 20;
export const MAX_SIDE = 20_000;
export const MAX_PER = 10;
export const MAX_CAPTION = 500;

/** What the hub answers when it stages an image. */
export type Staged = { digest: string; name: string; media_type: string; width: number; height: number; size: number };

export type Pin = { n: number; note: string };

/** A mark on an image, in the image's own pixels. */
export type Mark =
  | { kind: "box"; x: number; y: number; w: number; h: number; color: string }
  | { kind: "arrow"; x1: number; y1: number; x2: number; y2: number; color: string }
  | { kind: "pen"; points: [number, number][]; color: string }
  | { kind: "text"; x: number; y: number; text: string; color: string }
  | { kind: "pin"; x: number; y: number; n: number; note: string; color: string };

export type Tool = Mark["kind"];

/** One image in the composer's tray. */
export type TrayItem = {
  id: string;
  /** What's shown: the marked-up one once you've marked it. */
  url: string;
  name: string;
  caption: string;
  state: "staging" | "ready" | "failed";
  error?: string;
  staged?: Staged;
  /** Before markup: the image you added, staged, and its marks. */
  original?: { url: string; staged?: Staged; marks: Mark[] };
};

/** What the compose request carries for each image (src/images.rs `Ref`). */
export type ImageRef = { digest: string; name: string; caption: string; original?: string; pins?: Pin[] };

/** The markup colors: red first, the one marks start in. */
export const COLORS = ["#e5484d", "#f5a524", "#30a46c", "#0090ff", "#ffffff", "#111113"];

/** The images a paste or a drop carries, sendable or not yet. */
export function imageFiles(dt: { items?: ArrayLike<{ kind: string; type: string; getAsFile(): File | null }>; files?: ArrayLike<File> } | null | undefined): File[] {
  if (!dt) return [];
  const out: File[] = [];
  const items = dt.items ? Array.from(dt.items) : [];
  for (const it of items) {
    if (it.kind !== "file" || !it.type.startsWith("image/")) continue;
    const f = it.getAsFile();
    if (f) out.push(f);
  }
  if (!out.length && dt.files) for (const f of Array.from(dt.files)) if (f.type.startsWith("image/")) out.push(f);
  return out;
}

/** Whether a file can go to the hub as it is, or has to be redrawn first
 * (HEIC from a phone, AVIF, BMP, SVG, or too big). */
export function needsRedraw(type: string, size: number): boolean {
  return !(SENDABLE as readonly string[]).includes(type) || size > MAX_BYTES;
}

/** The size to redraw an image at: within the hub's limits, and no more
 * than 4096 on a side, which agents read well. */
export function fitWithin(w: number, h: number, max = 4096): { w: number; h: number } {
  const s = Math.min(1, max / Math.max(w, h, 1));
  return { w: Math.max(1, Math.round(w * s)), h: Math.max(1, Math.round(h * s)) };
}

/** A name for a pasted image, which has none of its own. */
export function pastedName(now = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `pasted-${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}${p(now.getSeconds())}.png`;
}

/** The pins on an image, in order, numbered 1, 2, 3 with no gaps. */
export function renumber(marks: Mark[]): Mark[] {
  let n = 0;
  return marks.map((m) => (m.kind === "pin" ? { ...m, n: ++n } : m));
}

export const pinsOf = (marks: Mark[]): Pin[] => marks.flatMap((m) => (m.kind === "pin" ? [{ n: m.n, note: m.note.trim().slice(0, MAX_CAPTION) }] : []));

/** How thick a mark is on an image this size: it reads the same on a phone
 * shot and on a 5K one. */
export const strokeFor = (w: number, h: number): number => Math.max(3, Math.round(Math.min(w, h) / 220));

/** What the compose request sends: the tray's ready images, in order. */
export function refsOf(tray: TrayItem[]): ImageRef[] {
  return tray.flatMap((t) => {
    if (t.state !== "ready" || !t.staged) return [];
    const r: ImageRef = { digest: t.staged.digest, name: t.staged.name, caption: t.caption.trim().slice(0, MAX_CAPTION) };
    if (t.original) {
      // The original goes too when it went up; the pins' words always do.
      if (t.original.staged) r.original = t.original.staged.digest;
      const pins = pinsOf(t.original.marks);
      if (pins.length) r.pins = pins;
    }
    return [r];
  });
}

/** Whether the tray can be sent: nothing still staging, nothing failed. */
export function trayState(tray: TrayItem[]): { ok: boolean; why?: string } {
  if (tray.some((t) => t.state === "staging")) return { ok: false, why: "Adding images…" };
  const bad = tray.filter((t) => t.state === "failed").length;
  if (bad) return { ok: false, why: `${bad === 1 ? "An image" : `${bad} images`} didn't go up: remove ${bad === 1 ? "it" : "them"} or try again` };
  if (tray.length > MAX_PER) return { ok: false, why: `At most ${MAX_PER} images at once` };
  return { ok: true };
}

/** The URL an image has on the hub (`GET /api/blobs/…`). */
export const imageUrl = (digest: string, name: string) => `/api/blobs/${digest}?name=${encodeURIComponent(name)}`;

/** An image in the intent's Markdown: the hub keeps it with the docket,
 * and `wh brief` gives agents its path. */
export function markdownImage(s: Pick<Staged, "digest" | "name">, alt: string): string {
  const a = alt.replace(/[\[\]\n\r]/g, " ").replace(/\s+/g, " ").trim();
  return `![${a}](${imageUrl(s.digest, s.name)})`;
}

/** Stage an image: its bytes to the hub, checked there. */
export async function stage(blob: Blob, name: string): Promise<Staged> {
  const res = await fetch(`/api/images?name=${encodeURIComponent(name)}`, {
    method: "POST",
    headers: { "Content-Type": "application/octet-stream", "X-Wheelhouse": "web" },
    body: blob,
  });
  const text = await res.text();
  let body: unknown = null;
  try {
    body = JSON.parse(text);
  } catch {
    /* plain text */
  }
  if (!res.ok) {
    const msg = body && typeof body === "object" && "error" in body ? String((body as { error: unknown }).error) : text || `The hub answered ${res.status}`;
    throw new Error(res.status === 413 ? `That image is over ${MAX_BYTES >> 20} MB` : msg);
  }
  return body as Staged;
}

/** An image the hub can take: as it is, or redrawn (a phone's HEIC as a
 * JPEG, something huge made smaller). Throws when the browser can't read it. */
export async function sendable(file: Blob): Promise<Blob> {
  if (!needsRedraw(file.type, file.size)) return file;
  const bmp = await createImageBitmap(file).catch(() => {
    throw new Error("This browser can't read that image: try a PNG or JPEG");
  });
  const { w, h } = fitWithin(bmp.width, bmp.height);
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  c.getContext("2d")!.drawImage(bmp, 0, 0, w, h);
  bmp.close();
  // A photo as JPEG; a screenshot (which a PNG was) stays sharp as PNG unless it's too big.
  const png = file.type === "image/png";
  let out = await toBlob(c, png ? "image/png" : "image/jpeg", 0.9);
  if (out.size > MAX_BYTES) out = await toBlob(c, "image/jpeg", 0.85);
  if (out.size > MAX_BYTES) throw new Error(`That image is over ${MAX_BYTES >> 20} MB even made smaller`);
  return out;
}

export function toBlob(c: HTMLCanvasElement, type: string, q?: number): Promise<Blob> {
  return new Promise((ok, bad) => c.toBlob((b) => (b ? ok(b) : bad(new Error("Couldn't draw the image"))), type, q));
}

let n = 0;
export const trayId = () => `img-${Date.now().toString(36)}-${(n++).toString(36)}`;

/** Add files to a tray: each shown at once, staged, then ready or failed.
 * `set` is a state setter; `prefix` names pasted images. */
export function addFiles(files: File[], set: (f: (t: TrayItem[]) => TrayItem[]) => void, have: number): string | null {
  const room = MAX_PER - have;
  if (room <= 0) return `At most ${MAX_PER} images at once`;
  const take = files.slice(0, room);
  for (const f of take) {
    const id = trayId();
    const name = f.name && f.name !== "image.png" ? f.name : pastedName();
    const url = URL.createObjectURL(f);
    set((t) => [...t, { id, url, name, caption: "", state: "staging" }]);
    sendable(f)
      .then((b) => stage(b, name))
      .then(
        (staged) => set((t) => t.map((x) => (x.id === id ? { ...x, staged, name: staged.name, state: "ready" } : x))),
        (e: Error) => set((t) => t.map((x) => (x.id === id ? { ...x, state: "failed", error: e.message } : x))),
      );
  }
  return files.length > take.length ? `Only ${MAX_PER} images go at once: ${files.length - take.length} left out` : null;
}

/** Let go of a tray's object URLs. */
export function release(tray: TrayItem[]) {
  for (const t of tray) {
    if (t.url.startsWith("blob:")) URL.revokeObjectURL(t.url);
    if (t.original?.url.startsWith("blob:")) URL.revokeObjectURL(t.original.url);
  }
}

export type CompareMode = "side" | "swipe" | "onion";

/** The images among a docket's entries (artifacts a browser can show), newest first. */
export function imagesOf(entries: EntryView[]): EntryView[] {
  return entries.filter((e) => e.kind === "artifact" && /^image\/(png|jpeg|gif|webp)$/.test(String(e.data?.media_type || ""))).sort((a, b) => b.seq - a.seq);
}

/** What an image compares with at first: the reference it was kept
 * against, else the newest screenshot kept against it. */
export function compareFor(e: EntryView, images: EntryView[]): EntryView | undefined {
  // The newest image kept against this one, or the one it was kept against.
  if (e.data?.against != null) return images.find((x) => x.seq === e.data.against);
  return images.find((x) => x.data?.against === e.seq);
}

/** A prompt's text without the block that names its images' paths (the
 * hub adds it, src/images.rs `block`), and how many images it named. */
export function splitImageBlock(text: string): { text: string; images: number } {
  const at = text.search(/\n\n(An image comes|Images come) with this \(open /);
  if (at < 0) return { text, images: 0 };
  const rest = text.slice(at + 2);
  const n = rest.split("\n").filter((l) => /^\d+\. /.test(l)).length;
  return { text: text.slice(0, at).trimEnd(), images: n };
}

type Carried = { digest: string; name: string; caption?: string; width?: number; height?: number; pins?: Pin[]; original?: string };

/** The images an entry carries (`data.images`, by digest: a shared
 * docket's entries have no number until they're ordered), each as the
 * artifact it was kept as, or as it was sent while that isn't here yet. */
export function entryImages(e: EntryView, entries: EntryView[]): EntryView[] {
  const list: Carried[] = Array.isArray(e.data?.images) ? e.data.images.filter((i: Carried) => typeof i?.digest === "string") : [];
  return list.map((i, n) => {
    const kept = entries.find((x) => x.kind === "artifact" && x.data?.digest === i.digest && x.data?.image === "reference" && (x.data?.caption ?? "") === (i.caption ?? ""));
    return kept ?? ({ seq: -(e.seq * 100 + n + 1), kind: "artifact", at: e.at, actor: e.actor, body: i.caption || i.name, data: { ...i, image: "reference", media_type: "image/png" } } as unknown as EntryView);
  });
}

/** Whether an image artifact is shown already, under the entry that carried it. */
export function carriedBy(e: EntryView, entries: EntryView[]): boolean {
  const d = e.data?.digest;
  if (e.kind !== "artifact" || !d || !["reference", "original"].includes(e.data?.image)) return false;
  // An original is opened from its marked-up image.
  if (e.data?.image === "original" && entries.some((x) => x.kind === "artifact" && x.data?.original === d)) return true;
  return entries.some((x) => x.kind !== "artifact" && Array.isArray(x.data?.images) && x.data.images.some((i: Carried) => i?.digest === d || i?.original === d));
}

/** Whether Markdown still has an image going up in it (the editor's placeholder). */
export const stillAdding = (md: string) => /!\[Adding [^\]\n]*…\]\(\)/.test(md);

/** Images handed to a docket's composer from elsewhere (the Share page):
 * waiting until that docket's composer takes them. */
let handed: { id: string; files: File[]; text: string } | null = null;
export function handOn(id: string, files: File[], text = "") {
  handed = { id, files, text };
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent("wh-share-images", { detail: handed }));
}
/** The images handed to this docket, once. */
export function takeHanded(id: string): { files: File[]; text: string } | null {
  if (!handed || handed.id !== id) return null;
  const h = handed;
  handed = null;
  return h;
}
