import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { renderInline, renderMarkdown } from "./markdown";

// ---- toasts ----

type Toast = { id: number; text: string; tone?: "error" };
const ToastCtx = createContext<(text: string, tone?: "error") => void>(() => {});

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const push = useCallback((text: string, tone?: "error") => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-3), { id, text, tone }]);
    window.setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), tone === "error" ? 6000 : 3200);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="toasts" role="status">
        {toasts.map((t) => (
          <div key={t.id} className={`toast${t.tone ? ` toast-${t.tone}` : ""}`}>
            {t.text}
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
export function useKeys(map: Record<string, (e: KeyboardEvent) => void>, active = true) {
  const ref = useRef(map);
  ref.current = map;
  useEffect(() => {
    if (!active) return;
    const on = (e: KeyboardEvent) => {
      if (typing(e) || e.metaKey || e.ctrlKey || e.altKey) return;
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

/** A popover menu under `anchor`, with arrow keys, numbers, and Escape. */
export function Menu({ anchor, items, onClose, title }: { anchor: DOMRect; items: MenuItem[]; onClose: () => void; title?: string }) {
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

export function Modal({ children, onClose, className }: { children: ReactNode; onClose: () => void; className?: string }) {
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

// ---- text ----

/** Markdown, fully: GFM, and the HTML PRs and issues carry, sanitized. */
export function Prose({ text }: { text: string }) {
  const html = useMemo(() => renderMarkdown(text), [text]);
  return <div className="prose" dangerouslySetInnerHTML={{ __html: html }} />;
}

/** One line of Markdown, inline: for summaries and rows. */
export function InlineMd({ text, links = true, className }: { text: string; links?: boolean; className?: string }) {
  const html = useMemo(() => renderInline(text, links), [text, links]);
  return <span className={`md-inline${className ? ` ${className}` : ""}`} dangerouslySetInnerHTML={{ __html: html }} />;
}
