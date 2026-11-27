// An image on a docket, as the viewer shows it: its caption and pins, the
// original before markup, and a reference against what was built: side by
// side, a swipe, or one over the other (`wh attach shot.png --against N`).
import { useMemo, useState } from "react";
import type { EntryView } from "./api.ts";
import { compareFor, imageUrl, imagesOf, type CompareMode } from "./images.ts";

const src = (e: EntryView) => imageUrl(String(e.data?.digest || ""), e.data?.name || "image.png");

const label = (e: EntryView) => `#${e.seq} · ${e.data?.caption || e.body || e.data?.name}`;

export function ImageView({ e, entries, onOpen }: { e: EntryView; entries: EntryView[]; onOpen?: (e: EntryView) => void }) {
  const images = useMemo(() => imagesOf(entries), [entries]);
  const [other, setOther] = useState<number | null>(() => compareFor(e, images)?.seq ?? null);
  const [mode, setMode] = useState<CompareMode | null>(() => (compareFor(e, images) ? "side" : null));
  const [cut, setCut] = useState(50);
  const [fade, setFade] = useState(50);
  const pins: { n: number; note: string }[] = Array.isArray(e.data?.pins) ? e.data.pins : [];
  // By digest: a shared docket's entries have no number until they're ordered.
  const original = e.data?.original ? images.find((x) => x.data?.digest === e.data.original && x.data?.image === "original") : undefined;
  const marked = e.data?.image === "original" ? images.find((x) => x.data?.original === e.data?.digest) : undefined;
  const that = other != null ? images.find((x) => x.seq === other) : undefined;
  // The reference on the left, what was built on the right.
  const [left, right] = that ? (e.data?.against === that.seq || (e.data?.image === "attached" && that.data?.image !== "attached") ? [that, e] : [e, that]) : [e, e];
  const choices = images.filter((x) => x.seq !== e.seq);

  return (
    <div className="imgv">
      <div className="imgv-bar">
        <div className="seg seg-text" role="tablist" aria-label="View">
          <button role="tab" aria-selected={!mode} className={!mode ? "on" : ""} onClick={() => setMode(null)}>
            Image
          </button>
          {(["side", "swipe", "onion"] as CompareMode[]).map((m) => (
            <button key={m} role="tab" aria-selected={mode === m} className={mode === m ? "on" : ""} disabled={!choices.length} onClick={() => (setMode(m), other == null && choices[0] && setOther(choices[0].seq))}>
              {m === "side" ? "Side by side" : m === "swipe" ? "Swipe" : "Onion skin"}
            </button>
          ))}
        </div>
        {mode && (
          <select className="imgv-pick" value={other ?? ""} aria-label="Compare with" onChange={(x) => setOther(Number(x.target.value))}>
            {choices.map((c) => (
              <option key={c.seq} value={c.seq}>
                {label(c)}
              </option>
            ))}
          </select>
        )}
        {mode === "swipe" && <input type="range" min={0} max={100} value={cut} aria-label="Swipe" onChange={(x) => setCut(Number(x.target.value))} />}
        {mode === "onion" && <input type="range" min={0} max={100} value={fade} aria-label="Opacity of the top image" onChange={(x) => setFade(Number(x.target.value))} />}
      </div>
      {!mode || !that ? (
        <img className="art-img" src={src(e)} alt={e.data?.caption || e.body || e.data?.name} />
      ) : mode === "side" ? (
        <div className="imgv-side">
          {[left, right].map((x) => (
            <figure key={x.seq}>
              <img src={src(x)} alt={label(x)} />
              <figcaption className="muted small">{label(x)}</figcaption>
            </figure>
          ))}
        </div>
      ) : (
        <div className="imgv-stack">
          <img src={src(left)} alt={label(left)} />
          <img
            className="imgv-top"
            src={src(right)}
            alt={label(right)}
            style={mode === "swipe" ? { clipPath: `inset(0 0 0 ${cut}%)` } : { opacity: fade / 100 }}
          />
          {mode === "swipe" && <span className="imgv-cut" style={{ left: `${cut}%` }} />}
          <div className="imgv-legend small">
            <span>{label(left)}</span>
            <span>{label(right)}</span>
          </div>
        </div>
      )}
      {(e.data?.caption || pins.length > 0 || original || marked) && (
        <div className="imgv-notes">
          {e.data?.caption && <p>{e.data.caption}</p>}
          {pins.length > 0 && (
            <ol className="imgv-pins">
              {pins.map((p) => (
                <li key={p.n}>
                  <span className="markup-pin-n">{p.n}</span>
                  {p.note || <span className="muted">No note</span>}
                </li>
              ))}
            </ol>
          )}
          {original && onOpen && (
            <button className="link-btn act-link small" onClick={() => onOpen(original)}>
              Before markup (#{original.seq})
            </button>
          )}
          {marked && onOpen && (
            <button className="link-btn act-link small" onClick={() => onOpen(marked)}>
              Marked up (#{marked.seq})
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/** A row of thumbnails: an entry's images, or one image artifact. */
export function Thumbs({ items, onOpen }: { items: EntryView[]; onOpen: (e: EntryView) => void }) {
  if (!items.length) return null;
  return (
    <div className="thumbs">
      {items.map((x) => (
        <button key={x.seq} className="thumb" title={label(x)} onClick={() => onOpen(x)}>
          <img src={src(x)} alt={x.data?.caption || x.data?.name} loading="lazy" />
          {Array.isArray(x.data?.pins) && x.data.pins.length > 0 && <span className="img-badge">{x.data.pins.length}</span>}
        </button>
      ))}
    </div>
  );
}
