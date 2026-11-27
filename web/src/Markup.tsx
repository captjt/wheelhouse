// Mark up an image before it goes to the agents: a box, an arrow, a pen
// line, text, and numbered pins with a note each. Marks are kept in the
// image's own pixels, so the image that goes out is full size, and the
// pins' notes go with it as words (src/images.rs).
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { COLORS, type Mark, type Tool, renumber, strokeFor, toBlob } from "./images.ts";
import { IconArrow, IconBox, IconPen, IconPin, IconRedo, IconText, IconUndo, IconX } from "./icons.tsx";
import { Kbd } from "./ui.tsx";

const TOOLS: { tool: Tool; label: string; key: string; icon: typeof IconBox }[] = [
  { tool: "box", label: "Box", key: "b", icon: IconBox },
  { tool: "arrow", label: "Arrow", key: "a", icon: IconArrow },
  { tool: "pen", label: "Pen", key: "p", icon: IconPen },
  { tool: "text", label: "Text", key: "t", icon: IconText },
  { tool: "pin", label: "Numbered pin", key: "n", icon: IconPin },
];

/** Draw marks over an image on a canvas the image's size. */
export function drawMarks(ctx: CanvasRenderingContext2D, marks: Mark[], w: number, h: number) {
  const s = strokeFor(w, h);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  for (const m of marks) {
    ctx.strokeStyle = m.color;
    ctx.fillStyle = m.color;
    ctx.lineWidth = s;
    switch (m.kind) {
      case "box":
        ctx.strokeRect(m.x, m.y, m.w, m.h);
        break;
      case "arrow": {
        const a = Math.atan2(m.y2 - m.y1, m.x2 - m.x1);
        const head = s * 5;
        ctx.beginPath();
        ctx.moveTo(m.x1, m.y1);
        ctx.lineTo(m.x2 - Math.cos(a) * head * 0.6, m.y2 - Math.sin(a) * head * 0.6);
        ctx.stroke();
        ctx.beginPath();
        ctx.moveTo(m.x2, m.y2);
        ctx.lineTo(m.x2 - head * Math.cos(a - 0.45), m.y2 - head * Math.sin(a - 0.45));
        ctx.lineTo(m.x2 - head * Math.cos(a + 0.45), m.y2 - head * Math.sin(a + 0.45));
        ctx.closePath();
        ctx.fill();
        break;
      }
      case "pen":
        if (m.points.length < 2) break;
        ctx.beginPath();
        ctx.moveTo(m.points[0][0], m.points[0][1]);
        for (const [x, y] of m.points.slice(1)) ctx.lineTo(x, y);
        ctx.stroke();
        break;
      case "text": {
        if (!m.text) break;
        ctx.font = `600 ${s * 7}px Inter, system-ui, sans-serif`;
        ctx.textBaseline = "top";
        // An outline in the opposite tone, so it reads on any background.
        ctx.lineWidth = s * 0.9;
        ctx.strokeStyle = m.color === "#111113" || m.color === "#0090ff" ? "rgba(255,255,255,0.9)" : "rgba(0,0,0,0.6)";
        ctx.strokeText(m.text, m.x, m.y);
        ctx.fillText(m.text, m.x, m.y);
        break;
      }
      case "pin": {
        const r = s * 4.5;
        ctx.beginPath();
        ctx.arc(m.x, m.y, r, 0, Math.PI * 2);
        ctx.fill();
        ctx.lineWidth = Math.max(2, s / 2);
        ctx.strokeStyle = "#ffffff";
        ctx.stroke();
        ctx.fillStyle = m.color === "#ffffff" ? "#111113" : "#ffffff";
        ctx.font = `700 ${r * 1.15}px Inter, system-ui, sans-serif`;
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(String(m.n), m.x, m.y + r * 0.05);
        ctx.textAlign = "start";
        break;
      }
    }
  }
}

/** The image with its marks, full size, as a PNG. */
export async function flatten(img: HTMLImageElement, marks: Mark[]): Promise<Blob> {
  const c = document.createElement("canvas");
  c.width = img.naturalWidth;
  c.height = img.naturalHeight;
  const ctx = c.getContext("2d")!;
  ctx.drawImage(img, 0, 0);
  drawMarks(ctx, marks, c.width, c.height);
  return toBlob(c, "image/png");
}

export function Markup({ src, marks: initial, onDone, onClose }: { src: string; marks: Mark[]; onDone: (marks: Mark[], flat: Blob) => void; onClose: () => void }) {
  const [img, setImg] = useState<HTMLImageElement | null>(null);
  const [failed, setFailed] = useState(false);
  const [tool, setTool] = useState<Tool>("box");
  const [color, setColor] = useState(COLORS[0]);
  const [marks, setMarks] = useState<Mark[]>(initial);
  const [undone, setUndone] = useState<Mark[][]>([]);
  const [draft, setDraft] = useState<Mark | null>(null);
  const [typing, setTyping] = useState<{ x: number; y: number; left: number; top: number } | null>(null);
  const [busy, setBusy] = useState(false);
  const canvas = useRef<HTMLCanvasElement>(null);
  const textInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const i = new Image();
    i.onload = () => setImg(i);
    i.onerror = () => setFailed(true);
    i.src = src;
  }, [src]);

  const shown = useMemo(() => (draft ? [...marks, draft] : marks), [marks, draft]);
  useEffect(() => {
    const c = canvas.current;
    if (!c || !img) return;
    c.width = img.naturalWidth;
    c.height = img.naturalHeight;
    const ctx = c.getContext("2d")!;
    ctx.drawImage(img, 0, 0);
    drawMarks(ctx, shown, c.width, c.height);
  }, [img, shown]);

  const commit = useCallback((next: Mark[]) => {
    setMarks(renumber(next));
    setUndone([]);
  }, []);
  const undo = () => {
    if (!marks.length) return;
    setUndone((u) => [...u, marks]);
    setMarks(renumber(marks.slice(0, -1)));
  };
  const redo = () => {
    const last = undone[undone.length - 1];
    if (!last) return;
    setUndone(undone.slice(0, -1));
    setMarks(last);
  };

  /** A pointer's place in the image's own pixels. */
  const at = (e: { clientX: number; clientY: number }): [number, number] => {
    const c = canvas.current!;
    const r = c.getBoundingClientRect();
    return [Math.round(((e.clientX - r.left) / r.width) * c.width), Math.round(((e.clientY - r.top) / r.height) * c.height)];
  };

  const finishText = (value: string) => {
    if (typing && value.trim()) commit([...marks, { kind: "text", x: typing.x, y: typing.y, text: value.trim().slice(0, 200), color }]);
    setTyping(null);
  };

  const down = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (!img || e.button > 0) return;
    if (typing) return finishText(textInput.current?.value ?? "");
    const [x, y] = at(e);
    if (tool === "pin") return commit([...marks, { kind: "pin", x, y, n: 0, note: "", color }]);
    if (tool === "text") {
      const wrap = canvas.current!.parentElement!.getBoundingClientRect();
      setTyping({ x, y, left: e.clientX - wrap.left, top: e.clientY - wrap.top });
      setTimeout(() => textInput.current?.focus(), 0);
      return;
    }
    // Not every pointer can be captured (one a script made): drawing works without.
    try {
      e.currentTarget.setPointerCapture(e.pointerId);
    } catch {
      /* the moves still come while the pointer's over the canvas */
    }
    if (tool === "box") setDraft({ kind: "box", x, y, w: 0, h: 0, color });
    if (tool === "arrow") setDraft({ kind: "arrow", x1: x, y1: y, x2: x, y2: y, color });
    if (tool === "pen") setDraft({ kind: "pen", points: [[x, y]], color });
  };
  const move = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (!draft) return;
    const [x, y] = at(e);
    if (draft.kind === "box") setDraft({ ...draft, w: x - draft.x, h: y - draft.y });
    if (draft.kind === "arrow") setDraft({ ...draft, x2: x, y2: y });
    if (draft.kind === "pen") setDraft({ ...draft, points: [...draft.points, [x, y]] });
  };
  const up = () => {
    if (!draft || !img) return;
    const min = strokeFor(img.naturalWidth, img.naturalHeight) * 2;
    const big =
      draft.kind === "box" ? Math.abs(draft.w) > min && Math.abs(draft.h) > min : draft.kind === "arrow" ? Math.hypot(draft.x2 - draft.x1, draft.y2 - draft.y1) > min * 2 : draft.kind === "pen" ? draft.points.length > 1 : true;
    // A box drawn up or left is kept as one drawn down and right.
    const norm: Mark = draft.kind === "box" ? { ...draft, x: Math.min(draft.x, draft.x + draft.w), y: Math.min(draft.y, draft.y + draft.h), w: Math.abs(draft.w), h: Math.abs(draft.h) } : draft;
    if (big) commit([...marks, norm]);
    setDraft(null);
  };

  const done = async () => {
    if (!img || busy) return;
    setBusy(true);
    try {
      onDone(marks, await flatten(img, marks));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.tagName === "INPUT" || t.tagName === "TEXTAREA") {
        if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) (e.preventDefault(), void done());
        return;
      }
      if (e.key === "Escape") return (e.preventDefault(), e.stopPropagation(), onClose());
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "z") return (e.preventDefault(), e.shiftKey ? redo() : undo());
      if ((e.metaKey || e.ctrlKey) && e.key === "Enter") return (e.preventDefault(), void done());
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const t2 = TOOLS.find((x) => x.key === e.key.toLowerCase());
      if (t2) setTool(t2.tool);
      const n = Number(e.key);
      if (n >= 1 && n <= COLORS.length) setColor(COLORS[n - 1]);
    };
    window.addEventListener("keydown", on);
    return () => window.removeEventListener("keydown", on);
  });

  const pins = marks.flatMap((m, i) => (m.kind === "pin" ? [{ m, i }] : []));
  const setNote = (i: number, note: string) => setMarks(marks.map((m, j) => (j === i && m.kind === "pin" ? { ...m, note: note.slice(0, 500) } : m)));
  const removeAt = (i: number) => commit(marks.filter((_, j) => j !== i));

  // Its own layer, on the body: not a sheet (drawing on a phone mustn't drag
  // it closed), and not inside the phone's composer, whose backdrop blur
  // would hold a fixed layer to the composer's own box.
  return createPortal(
    <div className="modal-backdrop markup-backdrop">
      <div className="modal markup-modal markup" role="dialog" aria-modal="true" aria-label="Mark up the image">
        <div className="markup-bar">
          <div className="markup-tools" role="toolbar" aria-label="Markup tools">
            {TOOLS.map((t) => (
              <button key={t.tool} className={`icon-btn${tool === t.tool ? " on" : ""}`} aria-pressed={tool === t.tool} title={`${t.label} (${t.key.toUpperCase()})`} onClick={() => setTool(t.tool)}>
                <t.icon size={15} />
              </button>
            ))}
            <span className="markup-sep" />
            {COLORS.map((c, i) => (
              <button key={c} className={`markup-swatch${color === c ? " on" : ""}`} style={{ background: c }} aria-label={`Color ${i + 1}`} title={`Color (${i + 1})`} onClick={() => setColor(c)} />
            ))}
            <span className="markup-sep" />
            <button className="icon-btn" title="Undo (⌘Z)" disabled={!marks.length} onClick={undo}>
              <IconUndo size={15} />
            </button>
            <button className="icon-btn" title="Redo (⇧⌘Z)" disabled={!undone.length} onClick={redo}>
              <IconRedo size={15} />
            </button>
            <button className="btn btn-small" disabled={!marks.length} onClick={() => commit([])}>
              Clear
            </button>
          </div>
          <span className="spacer" />
          <button className="btn btn-small" onClick={onClose}>
            Cancel
          </button>
          <button className="btn btn-primary btn-small" disabled={!img || busy} onClick={() => void done()}>
            Done <Kbd>⌘↩</Kbd>
          </button>
        </div>
        <div className="markup-body">
          <div className="markup-stage">
            {failed ? (
              <div className="muted">This image didn't load.</div>
            ) : !img ? (
              <div className="muted">Loading…</div>
            ) : (
              <div className="markup-canvas-wrap">
                <canvas ref={canvas} className={`markup-canvas tool-${tool}`} onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={() => setDraft(null)} />
                {typing && (
                  <input
                    ref={textInput}
                    className="markup-text-input"
                    style={{ left: typing.left, top: typing.top, color }}
                    placeholder="Text, then Enter"
                    maxLength={200}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") (e.preventDefault(), finishText(e.currentTarget.value));
                      if (e.key === "Escape") (e.preventDefault(), e.stopPropagation(), setTyping(null));
                    }}
                    onBlur={(e) => finishText(e.currentTarget.value)}
                  />
                )}
              </div>
            )}
          </div>
          <aside className="markup-side">
            <div className="markup-side-head">Pins</div>
            {pins.length === 0 ? (
              <p className="muted small">Press N and click to drop a numbered pin. What you write for each goes to the agents with the image.</p>
            ) : (
              pins.map(({ m, i }) => (
                <div key={i} className="markup-pin">
                  <span className="markup-pin-n" style={{ background: m.color }}>
                    {m.kind === "pin" ? m.n : ""}
                  </span>
                  <textarea rows={2} value={m.kind === "pin" ? m.note : ""} placeholder="What about here?" onChange={(e) => setNote(i, e.target.value)} />
                  <button className="icon-btn" title="Remove this pin" onClick={() => removeAt(i)}>
                    <IconX size={13} />
                  </button>
                </div>
              ))
            )}
          </aside>
        </div>
      </div>
    </div>,
    document.body,
  );
}
