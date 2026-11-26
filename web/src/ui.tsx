import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { renderInline, renderMarkdown } from "./markdown";

// ---- the phone ----

const PHONE = "(max-width: 760px)";

/** Whether the screen is a phone's: the app lays out natively there. */
export const isPhone = () => typeof window !== "undefined" && window.matchMedia(PHONE).matches;

export function usePhone() {
  const [p, setP] = useState(isPhone);
  useEffect(() => {
    const m = window.matchMedia(PHONE);
    const on = () => setP(m.matches);
    m.addEventListener("change", on);
    return () => m.removeEventListener("change", on);
  }, []);
  return p;
}

// ---- toasts ----

/** A toast's one action, such as Undo. */
export type ToastAction = { label: string; run: () => void };
type Toast = { id: number; text: string; tone?: "error"; action?: ToastAction };
const ToastCtx = createContext<(text: string, tone?: "error", action?: ToastAction) => void>(() => {});

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const drop = useCallback((id: number) => setToasts((t) => t.filter((x) => x.id !== id)), []);
  const push = useCallback(
    (text: string, tone?: "error", action?: ToastAction) => {
      const id = Date.now() + Math.random();
      setToasts((t) => [...t.slice(-3), { id, text, tone, action }]);
      window.setTimeout(() => drop(id), action ? 5500 : tone === "error" ? 6000 : 3200);
    },
    [drop],
  );
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="toasts" role="status">
        {toasts.map((t) => (
          <div key={t.id} className={`toast${t.tone ? ` toast-${t.tone}` : ""}`}>
            <span className="toast-text">{t.text}</span>
            {t.action && (
              <button className="toast-action" onClick={() => (t.action!.run(), drop(t.id))}>
                {t.action.label}
              </button>
            )}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

export const useToast = () => useContext(ToastCtx);

// ---- keys ----

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}

export const typing = (e: KeyboardEvent) => {
  const t = e.target as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
};

/** Keys while `active`: a map from key to handler. Ignored while typing. */
// When G was last pressed: the key after it goes somewhere (G then D is All
// dockets), so a list's own keys leave it alone. Without this, G D in the
// Inbox would also deny the prompt under the cursor. Recorded in the
// capture phase, before any list sees the key.
let gAt = 0;
if (typeof window !== "undefined") {
  window.addEventListener(
    "keydown",
    (e) => {
      if (e.key === "g" && !typing(e) && !e.metaKey && !e.ctrlKey && !e.altKey) gAt = Date.now();
    },
    true,
  );
}
/** Whether this key is the second of a G chord. */
export const afterG = (e: KeyboardEvent) => e.key !== "g" && Date.now() - gAt < 900;

export function useKeys(map: Record<string, (e: KeyboardEvent) => void>, active = true) {
  const ref = useRef(map);
  ref.current = map;
  useEffect(() => {
    if (!active) return;
    const on = (e: KeyboardEvent) => {
      if (typing(e) || e.metaKey || e.ctrlKey || e.altKey || afterG(e)) return;
      if (document.querySelector(".modal-backdrop, .menu")) return;
      const f = ref.current[e.key];
      if (f) {
        e.preventDefault();
        f(e);
      }
    };
    window.addEventListener("keydown", on);
    return () => window.removeEventListener("keydown", on);
  }, [active]);
}

// ---- menus ----

export type MenuItem = { key: string; label: ReactNode; icon?: ReactNode; hint?: string; checked?: boolean; onPick: () => void };

/** A menu: a popover under `anchor`, or on a phone, a sheet from the bottom. */
export function Menu(p: { anchor: DOMRect; items: MenuItem[]; onClose: () => void; title?: string }) {
  return usePhone() ? <MenuSheet {...p} /> : <MenuPopover {...p} />;
}

function MenuSheet({ items, onClose, title }: { items: MenuItem[]; onClose: () => void; title?: string }) {
  return (
    <Sheet onClose={onClose} title={title} className="menu-sheet">
      {(close) =>
        items.map((it) => (
          <button
            key={it.key}
            className={`sheet-item${it.checked ? " on" : ""}`}
            onClick={() => {
              it.onPick();
              close();
            }}
          >
            {it.icon && <span className="sheet-item-icon">{it.icon}</span>}
            <span className="sheet-item-label">{it.label}</span>
            {it.checked && <span className="menu-check">✓</span>}
          </button>
        ))
      }
    </Sheet>
  );
}

/** A popover menu under `anchor`, with arrow keys, numbers, and Escape. */
function MenuPopover({ anchor, items, onClose, title }: { anchor: DOMRect; items: MenuItem[]; onClose: () => void; title?: string }) {
  const [sel, setSel] = useState(Math.max(0, items.findIndex((i) => i.checked)));
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ top: anchor.bottom + 4, left: anchor.left });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({
      top: anchor.bottom + 4 + r.height > window.innerHeight ? Math.max(8, anchor.top - r.height - 4) : anchor.bottom + 4,
      left: Math.min(anchor.left, window.innerWidth - r.width - 8),
    });
  }, [anchor]);
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      else if (e.key === "ArrowDown" || e.key === "j") setSel((s) => (s + 1) % items.length);
      else if (e.key === "ArrowUp" || e.key === "k") setSel((s) => (s - 1 + items.length) % items.length);
      else if (e.key === "Enter") {
        items[sel]?.onPick();
        onClose();
      } else if (/^[1-9]$/.test(e.key) && items[Number(e.key) - 1]) {
        items[Number(e.key) - 1].onPick();
        onClose();
      } else return;
      e.preventDefault();
      e.stopPropagation();
    };
    const away = (e: MouseEvent) => !ref.current?.contains(e.target as Node) && onClose();
    window.addEventListener("keydown", on, true);
    window.addEventListener("mousedown", away);
    return () => {
      window.removeEventListener("keydown", on, true);
      window.removeEventListener("mousedown", away);
    };
  }, [items, sel, onClose]);
  return (
    <div className="menu" ref={ref} style={pos} role="menu">
      {title && <div className="menu-title">{title}</div>}
      {items.map((it, i) => (
        <button
          key={it.key}
          className={`menu-item${i === sel ? " sel" : ""}`}
          onMouseEnter={() => setSel(i)}
          onClick={() => {
            it.onPick();
            onClose();
          }}
          role="menuitem"
        >
          {it.icon && <span className="menu-icon">{it.icon}</span>}
          <span className="menu-label">{it.label}</span>
          {it.checked && <span className="menu-check">✓</span>}
          {it.hint && <span className="menu-hint">{it.hint}</span>}
        </button>
      ))}
    </div>
  );
}

/** Open a menu at an element or at the pointer. */
export function useMenu() {
  const [m, setM] = useState<{ anchor: DOMRect; items: MenuItem[]; title?: string } | null>(null);
  const open = useCallback((at: HTMLElement | DOMRect | { x: number; y: number }, items: MenuItem[], title?: string) => {
    const anchor =
      at instanceof HTMLElement ? at.getBoundingClientRect() : at instanceof DOMRect ? at : new DOMRect(at.x, at.y, 0, 0);
    setM({ anchor, items, title });
  }, []);
  const node = m ? <Menu anchor={m.anchor} items={m.items} title={m.title} onClose={() => setM(null)} /> : null;
  return { open, node };
}

// ---- modals ----

/** A dialog: centered, or on a phone, a sheet from the bottom (the pane's
 * view stays full screen). */
export function Modal(p: { children: ReactNode; onClose: () => void; className?: string }) {
  const phone = usePhone();
  if (phone && !p.className?.includes("pane-sheet")) {
    const tall = /palette|new-docket|linear-modal|history-modal|preview-modal|artifact-modal|record-modal|doc-modal/.test(p.className ?? "");
    return (
      <Sheet onClose={p.onClose} className={`modal-sheet ${p.className ?? ""}`} tall={tall}>
        {p.children}
      </Sheet>
    );
  }
  return <CenteredModal {...p} />;
}

function CenteredModal({ children, onClose, className }: { children: ReactNode; onClose: () => void; className?: string }) {
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", on);
    return () => window.removeEventListener("keydown", on);
  }, [onClose]);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`modal ${className || ""}`} role="dialog">
        {children}
      </div>
    </div>
  );
}

// ---- sheets ----

/** The nearest element that scrolls vertically, from `el` up to `stop`. */
function scrollerOf(el: HTMLElement | null, stop: HTMLElement): HTMLElement | null {
  for (let n = el; n && n !== stop.parentElement; n = n.parentElement) {
    const o = getComputedStyle(n).overflowY;
    if ((o === "auto" || o === "scroll") && n.scrollHeight > n.clientHeight + 1) return n;
  }
  return null;
}

/**
 * A sheet that rises from the bottom, as on iOS: drag it down (or tap
 * outside, or press Escape) to close it. `children` may be a function of
 * `close`, to close it with its animation.
 */
export function Sheet({
  children,
  onClose,
  title,
  className,
  tall,
}: {
  children: ReactNode | ((close: () => void) => ReactNode);
  onClose: () => void;
  title?: ReactNode;
  className?: string;
  tall?: boolean;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const back = useRef<HTMLDivElement>(null);
  const [closing, setClosing] = useState(false);
  const close = useCallback(() => setClosing(true), []);
  // A tap that lands in its first moment belongs to what opened it.
  const opened = useRef(Date.now());
  useEffect(() => {
    if (!closing) return;
    const t = window.setTimeout(onClose, 220);
    return () => window.clearTimeout(t);
  }, [closing, onClose]);
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopPropagation();
      close();
    };
    window.addEventListener("keydown", on, true);
    return () => window.removeEventListener("keydown", on, true);
  }, [close]);
  useEffect(() => {
    const el = panel.current;
    if (!el) return;
    let x0 = 0;
    let y0 = 0;
    let t0 = 0;
    let dy = 0;
    let decided = false;
    let dragging = false;
    let scroller: HTMLElement | null = null;
    let target: HTMLElement | null = null;
    const start = (e: TouchEvent) => {
      if (e.touches.length !== 1) return;
      x0 = e.touches[0].clientX;
      y0 = e.touches[0].clientY;
      t0 = Date.now();
      dy = 0;
      decided = false;
      dragging = false;
      target = e.target as HTMLElement;
      scroller = scrollerOf(target, el);
    };
    const move = (e: TouchEvent) => {
      if (e.touches.length !== 1) return;
      const y = e.touches[0].clientY;
      const x = e.touches[0].clientX;
      if (!decided) {
        if (Math.abs(y - y0) < 6 && Math.abs(x - x0) < 6) return;
        decided = true;
        const typingIn = !!target?.closest("textarea, input, [contenteditable]") && !target.closest(".sheet-grab");
        dragging = y - y0 > 0 && Math.abs(y - y0) > Math.abs(x - x0) && (!scroller || scroller.scrollTop <= 0) && !typingIn;
      }
      if (!dragging) return;
      e.preventDefault();
      dy = Math.max(0, y - y0);
      el.style.transition = "none";
      el.style.transform = `translateY(${dy}px)`;
      if (back.current) back.current.style.opacity = String(1 - Math.min(1, dy / Math.max(1, el.offsetHeight)));
    };
    const end = () => {
      if (!dragging) return;
      dragging = false;
      const v = dy / Math.max(1, Date.now() - t0);
      el.style.transition = "";
      if (dy > el.offsetHeight * 0.28 || (v > 0.5 && dy > 30)) close();
      else {
        el.style.transform = "";
        if (back.current) back.current.style.opacity = "";
      }
    };
    el.addEventListener("touchstart", start, { passive: true });
    el.addEventListener("touchmove", move, { passive: false });
    el.addEventListener("touchend", end);
    el.addEventListener("touchcancel", end);
    return () => {
      el.removeEventListener("touchstart", start);
      el.removeEventListener("touchmove", move);
      el.removeEventListener("touchend", end);
      el.removeEventListener("touchcancel", end);
    };
  }, [close]);
  return (
    <div ref={back} className={`sheet-backdrop${closing ? " closing" : ""}`} onClick={(e) => e.target === e.currentTarget && Date.now() - opened.current > 400 && close()}>
      <div ref={panel} className={`sheet${tall ? " tall" : ""}${closing ? " closing" : ""} ${className ?? ""}`} role="dialog" aria-modal="true">
        <div className="sheet-grab" aria-hidden="true">
          <span />
        </div>
        {title && <div className="sheet-title">{title}</div>}
        <div className="sheet-body">{typeof children === "function" ? children(close) : children}</div>
      </div>
    </div>
  );
}

// ---- text ----

/**
 * Markdown, fully: GFM, footnotes, alerts, and the HTML PRs and issues
 * carry, sanitized. With `onTask`, its task boxes can be ticked: it gets the
 * box's index and new state, and resolves false to put the box back.
 */
/** Markdown, sanitized. `images: false` for text a model wrote from
 * something untrusted (a call's Listener): no image, so nothing on the page
 * fetches a URL it chose. */
export function Prose({ text, onTask, images = true }: { text: string; onTask?: (index: number, checked: boolean) => Promise<boolean> | boolean; images?: boolean }) {
  const frag = useMemo(() => renderMarkdown(text, { images }), [text, images]);
  const ref = useRef<HTMLDivElement>(null);
  // The sanitized nodes go in as nodes: no HTML string is parsed here.
  useLayoutEffect(() => {
    ref.current?.replaceChildren(frag.cloneNode(true));
  }, [frag]);
  useEffect(() => {
    const boxes = ref.current?.querySelectorAll<HTMLInputElement>('input[type="checkbox"]') ?? [];
    boxes.forEach((b) => {
      b.disabled = !onTask;
      if (onTask) b.title = b.checked ? "Done: untick it" : "Tick it off";
    });
  }, [frag, onTask]);
  const click = (e: React.MouseEvent<HTMLDivElement>) => {
    const t = e.target as HTMLElement;
    const a = t.closest("a");
    // A footnote's link stays on the page; the app's own routes are #/….
    const href = a?.getAttribute("href") ?? "";
    if (a && href.startsWith("#") && !href.startsWith("#/")) {
      e.preventDefault();
      ref.current?.querySelector(`[id="user-content-${CSS.escape(href.slice(1))}"]`)?.scrollIntoView({ block: "center", behavior: "smooth" });
      return;
    }
    if (onTask && t instanceof HTMLInputElement && t.type === "checkbox") {
      const all = [...(ref.current?.querySelectorAll('input[type="checkbox"]') ?? [])];
      const box = t;
      Promise.resolve(onTask(all.indexOf(box), box.checked)).then(
        (ok) => !ok && (box.checked = !box.checked),
        () => (box.checked = !box.checked),
      );
    }
  };
  return <div ref={ref} className="prose" onClick={click} />;
}

/** One line of Markdown, inline: for summaries and rows. */
export function InlineMd({ text, links = true, className }: { text: string; links?: boolean; className?: string }) {
  const frag = useMemo(() => renderInline(text, links), [text, links]);
  const ref = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    ref.current?.replaceChildren(frag.cloneNode(true));
  }, [frag]);
  return <span ref={ref} className={`md-inline${className ? ` ${className}` : ""}`} />;
}
