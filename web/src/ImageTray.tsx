// The images waiting to go with what you type: in the composer, and on the
// Share page. Each one is staged as it's added; you can caption it, mark it
// up, or take it out.
import { useState } from "react";
import { type Mark, type TrayItem, sendable, stage } from "./images.ts";
import { IconEdit, IconX } from "./icons.tsx";
import { Markup } from "./Markup.tsx";

type Set = (f: (t: TrayItem[]) => TrayItem[]) => void;

/** Mark up an item: the image that goes out becomes the marked one, and
 * the one you added is kept as its original. No marks: back to the original. */
export function applyMarkup(item: TrayItem, marks: Mark[], flat: Blob, set: Set) {
  const original = item.original ?? { url: item.url, staged: item.staged, marks: [] };
  if (item.original && item.url !== original.url && item.url.startsWith("blob:")) URL.revokeObjectURL(item.url);
  if (!marks.length) {
    set((t) => t.map((x) => (x.id === item.id ? { ...x, url: original.url, staged: original.staged, original: undefined, state: original.staged ? "ready" : "failed" } : x)));
    return;
  }
  const url = URL.createObjectURL(flat);
  set((t) => t.map((x) => (x.id === item.id ? { ...x, url, staged: undefined, state: "staging", error: undefined, original: { ...original, marks } } : x)));
  // A marked-up photo is big as a PNG: redrawn within the hub's limits first.
  sendable(flat)
    .then((b) => stage(b, `marked-${item.name.replace(/\.[a-z0-9]+$/i, "")}.${b.type === "image/jpeg" ? "jpg" : "png"}`))
    .then(
    (staged) => set((t) => t.map((x) => (x.id === item.id && x.url === url ? { ...x, staged, state: "ready" } : x))),
    (e: Error) => set((t) => t.map((x) => (x.id === item.id && x.url === url ? { ...x, state: "failed", error: e.message } : x))),
  );
}

export function ImageTray({ tray, set, disabled }: { tray: TrayItem[]; set: Set; disabled?: boolean }) {
  const [marking, setMarking] = useState<TrayItem | null>(null);
  if (!tray.length) return null;
  const remove = (it: TrayItem) => {
    set((t) => t.filter((x) => x.id !== it.id));
    if (it.url.startsWith("blob:")) URL.revokeObjectURL(it.url);
    if (it.original?.url.startsWith("blob:")) URL.revokeObjectURL(it.original.url);
  };
  return (
    <>
      <ul className="img-tray" aria-label="Images to send">
        {tray.map((it) => {
          const pins = it.original?.marks.filter((m) => m.kind === "pin").length ?? 0;
          return (
            <li key={it.id} className={`img-tile ${it.state}`}>
              <button className="img-thumb" title={it.state === "failed" ? it.error : "Mark it up"} disabled={disabled || it.state === "staging"} onClick={() => setMarking(it)}>
                <img src={it.url} alt={it.caption || it.name} />
                {it.state === "staging" && <span className="img-state">Adding…</span>}
                {it.state === "failed" && <span className="img-state bad">Didn't go up</span>}
                {it.original && it.state !== "failed" && <span className="img-badge">{pins ? `${pins} pin${pins === 1 ? "" : "s"}` : "Marked"}</span>}
              </button>
              <div className="img-tile-row">
                <input
                  className="img-caption"
                  value={it.caption}
                  maxLength={500}
                  placeholder="Caption: what to see here"
                  aria-label={`Caption for ${it.name}`}
                  disabled={disabled}
                  onChange={(e) => {
                    const v = e.target.value;
                    set((t) => t.map((x) => (x.id === it.id ? { ...x, caption: v } : x)));
                  }}
                />
                <button className="icon-btn" title="Mark it up" disabled={disabled || it.state === "staging"} onClick={() => setMarking(it)}>
                  <IconEdit size={13} />
                </button>
                <button className="icon-btn" title="Take it out" disabled={disabled} onClick={() => remove(it)}>
                  <IconX size={13} />
                </button>
              </div>
              {it.state === "failed" && it.error && <div className="img-error small">{it.error}</div>}
            </li>
          );
        })}
      </ul>
      {marking && (
        <Markup
          src={marking.original?.url ?? marking.url}
          marks={marking.original?.marks ?? []}
          onClose={() => setMarking(null)}
          onDone={(marks, flat) => {
            applyMarkup(marking, marks, flat, set);
            setMarking(null);
          }}
        />
      )}
    </>
  );
}
