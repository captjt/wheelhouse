import { useEffect, useRef, type ReactNode, type RefObject } from "react";
import { IconActive, IconBuild, IconInbox, IconMore, IconPR } from "./icons";
import { go, lastList } from "./live";
import { Sheet } from "./ui";

/**
 * The phone, native: a tab bar, large titles, a stack the docket slides
 * onto (swipe right to go back), rows that swipe to act and long-press for
 * their menu, sheets, and pull to refresh.
 */

const vibrate = (ms = 8) => (navigator as Navigator & { vibrate?: (n: number) => boolean }).vibrate?.(ms);

// ---- going back ----

/** Back from the docket to its list. The app swaps in an animated one on a phone. */
export const nav: { back: () => void } = { back: () => go(lastList.path) };

// ---- the tab bar ----

export type Tab = { path: string; label: string; icon: ReactNode; count?: number; alert?: boolean };

export function tabs(c: { inbox: number; urgent: boolean; reviews: number; reviewAlerts: boolean; builds: number; buildAlerts: boolean; active: number }): Tab[] {
  return [
    { path: "/inbox", label: "Inbox", icon: <IconInbox size={22} />, count: c.inbox, alert: c.urgent },
    // Open dockets aren't a worry: no badge for them.
    { path: "/view/active", label: "Active", icon: <IconActive size={22} /> },
    { path: "/reviews", label: "Reviews", icon: <IconPR size={22} />, count: c.reviews, alert: c.reviewAlerts },
    { path: "/builds", label: "Builds", icon: <IconBuild size={22} />, count: c.builds, alert: c.buildAlerts },
  ];
}

/** The views a tab is for: More holds the rest. */
const onTab = (here: string, path: string) => here === path.slice(1) || (path === "/view/active" && (here === "" || here === "view"));

export function TabBar({ here, items, onMore }: { here: string; items: Tab[]; onMore: () => void }) {
  const more = !items.some((t) => onTab(here, t.path));
  return (
    <nav className="tabbar" aria-label="Views">
      {items.map((t) => {
        const on = onTab(here, t.path);
        return (
          <a
            key={t.path}
            className={`tab${on ? " on" : ""}`}
            href={`#${t.path}`}
            onClick={(e) => {
              // The tab you're on: back to the top, as on iOS.
              if (on) {
                e.preventDefault();
                document.querySelector(".m-base .view")?.scrollTo({ top: 0, behavior: "smooth" });
              }
            }}
          >
            <span className="tab-icon">
              {t.icon}
              {!!t.count && <span className={`tab-badge${t.alert ? " alert" : ""}`}>{t.count > 99 ? "99+" : t.count}</span>}
            </span>
            <span className="tab-label">{t.label}</span>
          </a>
        );
      })}
      <button className={`tab${more ? " on" : ""}`} onClick={onMore}>
        <span className="tab-icon">
          <IconMore size={22} />
        </span>
        <span className="tab-label">More</span>
      </button>
    </nav>
  );
}

export type MoreLink = { path: string; label: string; icon: ReactNode; count?: number };

/** More: the other views, the Rex sessions, search, a new docket, and the hub's state. */
export function MoreSheet({
  here,
  links,
  sessions,
  actions,
  footer,
  onClose,
}: {
  here: string;
  links: MoreLink[];
  sessions: MoreLink[];
  actions: { key: string; label: string; icon: ReactNode; run: () => void }[];
  footer: ReactNode;
  onClose: () => void;
}) {
  return (
    <Sheet onClose={onClose} className="more-sheet">
      {(close) => (
        <>
          <div className="more-actions">
            {actions.map((a) => (
              <button key={a.key} className="more-action" onClick={() => (close(), window.setTimeout(a.run, 230))}>
                {a.icon}
                <span>{a.label}</span>
              </button>
            ))}
          </div>
          <div className="sheet-group">
            {links.map((l) => (
              <a key={l.path} className={`sheet-item${here === l.path.slice(1) ? " on" : ""}`} href={`#${l.path}`} onClick={close}>
                <span className="sheet-item-icon">{l.icon}</span>
                <span className="sheet-item-label">{l.label}</span>
                {!!l.count && <span className="sheet-item-count">{l.count}</span>}
              </a>
            ))}
          </div>
          {sessions.length > 0 && (
            <>
              <div className="sheet-section">Rex sessions</div>
              <div className="sheet-group">
                {sessions.map((l) => (
                  <a key={l.path} className={`sheet-item${here === l.path.slice(1) ? " on" : ""}`} href={`#${l.path}`} onClick={close}>
                    <span className="sheet-item-icon">{l.icon}</span>
                    <span className="sheet-item-label">{l.label}</span>
                    {!!l.count && <span className="sheet-item-count">{l.count}</span>}
                  </a>
                ))}
              </div>
            </>
          )}
          <div className="more-foot">{footer}</div>
        </>
      )}
    </Sheet>
  );
}

// ---- large titles ----

/** A list's large title, at the top of what scrolls; it moves into the bar as you scroll. */
export function LargeTitle({ title, count, children }: { title: ReactNode; count?: number; children?: ReactNode }) {
  return (
    <div className="m-large">
      <h1>
        {title}
        {count !== undefined && count > 0 && <span className="m-large-count">{count}</span>}
      </h1>
      {children}
    </div>
  );
}

/** Mark each scrolled view, so its bar shows the title and a hairline. */
export function useScrolledViews(on: boolean) {
  useEffect(() => {
    if (!on) return;
    const handle = (e: Event) => {
      const v = e.target as HTMLElement;
      if (v instanceof HTMLElement && v.classList.contains("view")) v.classList.toggle("m-scrolled", v.scrollTop > 34);
    };
    document.addEventListener("scroll", handle, true);
    return () => document.removeEventListener("scroll", handle, true);
  }, [on]);
}

// ---- gestures ----

/** Whether a touch on `el` belongs to something that scrolls or edits sideways. */
function sideways(el: HTMLElement | null, stop: HTMLElement): boolean {
  for (let n = el; n && n !== stop; n = n.parentElement) {
    if (n.matches("textarea, input, select, [contenteditable], .md-tools, .no-swipe")) return true;
    const o = getComputedStyle(n).overflowX;
    if ((o === "auto" || o === "scroll") && n.scrollWidth > n.clientWidth + 1) return true;
  }
  return false;
}

export type SwipeAction = { key: string; label: string; icon?: ReactNode; tone: string; run: () => void };

let openRow: { close: () => void; el: HTMLElement } | null = null;
const ACTION_W = 78;
// A touch anywhere but the open row closes it.
if (typeof document !== "undefined")
  document.addEventListener(
    "touchstart",
    (e) => {
      if (openRow && !openRow.el.contains(e.target as Node)) openRow.close();
    },
    { passive: true, capture: true },
  );

/**
 * A row that swipes, as in Mail: swipe right for `leading`, left for
 * `trailing`. Swipe far enough with `full`, and the first action on that
 * side runs; otherwise the buttons stay open to tap. Long-press runs
 * `onLongPress` (its menu). Touch only; the mouse is as before.
 */
export function SwipeRow({
  leading = [],
  trailing = [],
  fullLeading,
  fullTrailing,
  onLongPress,
  children,
}: {
  leading?: SwipeAction[];
  trailing?: SwipeAction[];
  fullLeading?: boolean;
  fullTrailing?: boolean;
  onLongPress?: () => void;
  children: ReactNode;
}) {
  const root = useRef<HTMLDivElement>(null);
  const front = useRef<HTMLDivElement>(null);
  const suppress = useRef(false);
  const offset = useRef(0);
  const acts = useRef({ leading, trailing, fullLeading, fullTrailing, onLongPress });
  acts.current = { leading, trailing, fullLeading, fullTrailing, onLongPress };

  useEffect(() => {
    const el = root.current;
    const f = front.current;
    if (!el || !f) return;
    let x0 = 0;
    let y0 = 0;
    let t0 = 0;
    let dx = 0;
    let axis: "x" | "y" | null = null;
    let press: number | undefined;
    let pressed = false;
    const set = (x: number, animate: boolean) => {
      f.style.transition = animate ? "transform 0.26s cubic-bezier(0.2, 0.8, 0.2, 1)" : "none";
      f.style.transform = x ? `translateX(${x}px)` : "";
      el.classList.toggle("lead-open", x > 0);
      el.classList.toggle("trail-open", x < 0);
      const w = el.offsetWidth;
      const a = acts.current;
      el.classList.toggle("armed-lead", !!a.fullLeading && x > w * 0.55);
      el.classList.toggle("armed-trail", !!a.fullTrailing && -x > w * 0.55);
      el.style.setProperty("--sw", `${Math.abs(x)}px`);
    };
    const close = () => {
      offset.current = 0;
      set(0, true);
      if (openRow?.el === el) openRow = null;
    };
    const start = (e: TouchEvent) => {
      if (e.touches.length !== 1) return;
      if (openRow && openRow.el !== el) openRow.close();
      x0 = e.touches[0].clientX;
      y0 = e.touches[0].clientY;
      t0 = Date.now();
      dx = 0;
      axis = null;
      pressed = false;
      window.clearTimeout(press);
      if (acts.current.onLongPress && !offset.current)
        press = window.setTimeout(() => {
          press = undefined;
          if (axis) return;
          pressed = true;
          suppress.current = true;
          vibrate(10);
          el.classList.add("pressed");
          window.setTimeout(() => el.classList.remove("pressed"), 220);
          acts.current.onLongPress?.();
        }, 480);
    };
    const move = (e: TouchEvent) => {
      if (e.touches.length !== 1) return;
      const x = e.touches[0].clientX - x0;
      const y = e.touches[0].clientY - y0;
      if (!axis) {
        if (Math.abs(x) < 8 && Math.abs(y) < 8) return;
        window.clearTimeout(press);
        const a = acts.current;
        const can = x > 0 ? a.leading.length > 0 || offset.current < 0 : a.trailing.length > 0 || offset.current > 0;
        axis = Math.abs(x) > Math.abs(y) * 1.2 && can && !sideways(e.target as HTMLElement, el) ? "x" : "y";
        if (axis === "x" && openRow?.el !== el) openRow?.close();
      }
      if (axis !== "x") return;
      e.preventDefault();
      const a = acts.current;
      const w = el.offsetWidth;
      let next = offset.current + x;
      const lead = a.leading.length * ACTION_W;
      const trail = a.trailing.length * ACTION_W;
      // Past the buttons it resists, unless a full swipe can run the action.
      if (next > 0) next = !a.leading.length ? 0 : next > lead && !a.fullLeading ? lead + (next - lead) * 0.25 : Math.min(next, w);
      else next = !a.trailing.length ? 0 : -next > trail && !a.fullTrailing ? -(trail + (-next - trail) * 0.25) : Math.max(next, -w);
      dx = next;
      set(next, false);
    };
    const end = (e: TouchEvent) => {
      window.clearTimeout(press);
      // The finger lifting after a long press isn't a tap on what opened under it.
      if (pressed && e.cancelable) e.preventDefault();
      if (axis !== "x") return;
      suppress.current = true;
      const a = acts.current;
      const w = el.offsetWidth;
      const v = (dx - offset.current) / Math.max(1, Date.now() - t0);
      const fullL = a.fullLeading && a.leading[0] && (dx > w * 0.55 || (v > 1.1 && dx > ACTION_W));
      const fullT = a.fullTrailing && a.trailing[0] && (-dx > w * 0.55 || (v < -1.1 && -dx > ACTION_W));
      if (fullL || fullT) {
        const act = fullL ? a.leading[0] : a.trailing[0];
        set(fullL ? w : -w, true);
        vibrate(12);
        window.setTimeout(() => {
          act.run();
          offset.current = 0;
          set(0, false);
          if (openRow?.el === el) openRow = null;
        }, 240);
        return;
      }
      const lead = a.leading.length * ACTION_W;
      const trail = a.trailing.length * ACTION_W;
      if (dx > lead / 2 && lead) {
        offset.current = lead;
        set(lead, true);
        openRow = { close, el };
      } else if (-dx > trail / 2 && trail) {
        offset.current = -trail;
        set(-trail, true);
        openRow = { close, el };
      } else close();
    };
    el.addEventListener("touchstart", start, { passive: true });
    el.addEventListener("touchmove", move, { passive: false });
    el.addEventListener("touchend", end);
    el.addEventListener("touchcancel", end);
    return () => {
      window.clearTimeout(press);
      el.removeEventListener("touchstart", start);
      el.removeEventListener("touchmove", move);
      el.removeEventListener("touchend", end);
      el.removeEventListener("touchcancel", end);
      if (openRow?.el === el) openRow = null;
    };
  }, []);

  const runAction = (a: SwipeAction) => {
    a.run();
    offset.current = 0;
    if (front.current) {
      front.current.style.transition = "transform 0.26s cubic-bezier(0.2, 0.8, 0.2, 1)";
      front.current.style.transform = "";
    }
    root.current?.classList.remove("lead-open", "trail-open");
    openRow = null;
  };

  return (
    <div
      ref={root}
      className="sw"
      onClickCapture={(e) => {
        // The tap that ends a swipe or a long press, or closes an open row, isn't a click.
        if (suppress.current || offset.current) {
          e.preventDefault();
          e.stopPropagation();
          suppress.current = false;
          if (offset.current && !(e.target as HTMLElement).closest(".sw-act")) openRow?.close();
        }
      }}
      onContextMenu={(e) => {
        // iOS's own long-press menu for links: ours instead.
        if (onLongPress && window.matchMedia("(pointer: coarse)").matches) e.preventDefault();
      }}
    >
      {leading.length > 0 && (
        <div className="sw-side sw-lead">
          {leading.map((a) => (
            <button key={a.key} className={`sw-act tone-${a.tone}`} onClick={() => runAction(a)}>
              {a.icon}
              <span>{a.label}</span>
            </button>
          ))}
        </div>
      )}
      {trailing.length > 0 && (
        <div className="sw-side sw-trail">
          {trailing.map((a) => (
            <button key={a.key} className={`sw-act tone-${a.tone}`} onClick={() => runAction(a)}>
              {a.icon}
              <span>{a.label}</span>
            </button>
          ))}
        </div>
      )}
      <div ref={front} className="sw-front">
        {children}
      </div>
    </div>
  );
}

// ---- the stack: swipe right to go back ----

const EASE = "cubic-bezier(0.2, 0.8, 0.2, 1)";

/** Animate the docket off to the right, then go back to its list. */
export function popBack(top: HTMLElement | null, base: HTMLElement | null, shade: HTMLElement | null, done: () => void) {
  if (!top) return done();
  const t = `transform 0.26s ${EASE}`;
  top.style.transition = t;
  top.style.transform = "translateX(100%)";
  if (base) {
    base.style.transition = t;
    base.style.transform = "translateX(0)";
  }
  if (shade) {
    shade.style.transition = "opacity 0.26s linear";
    shade.style.opacity = "0";
  }
  window.setTimeout(done, 250);
}

/**
 * Swipe right on the docket to go back, as on iOS: it follows your finger,
 * with its list underneath. From the screen's left edge anywhere; elsewhere
 * on the page too, unless the touch is on something that scrolls or edits
 * sideways.
 */
export function useSwipeBack(top: RefObject<HTMLElement | null>, base: RefObject<HTMLElement | null>, shade: RefObject<HTMLElement | null>, active: string | false, onBack: () => void) {
  const back = useRef(onBack);
  back.current = onBack;
  useEffect(() => {
    const el = top.current;
    const b = base.current;
    const s = shade.current;
    // A new docket on the stack: the list goes back under it.
    if (b) {
      b.style.transition = "";
      b.style.transform = "";
    }
    if (!active || !el) return;
    let x0 = 0;
    let y0 = 0;
    let t0 = 0;
    let dx = 0;
    let axis: "x" | "y" | null = null;
    let edge = false;
    let blocked = false;
    const w = () => el.offsetWidth || window.innerWidth;
    const paint = (x: number) => {
      const p = Math.min(1, Math.max(0, x / w()));
      el.style.transform = `translateX(${x}px)`;
      if (b) b.style.transform = `translateX(${-22 * (1 - p)}%)`;
      if (s) s.style.opacity = String(1 - p);
    };
    const start = (e: TouchEvent) => {
      if (e.touches.length !== 1 || document.querySelector(".sheet-backdrop, .modal-backdrop")) {
        blocked = true;
        return;
      }
      x0 = e.touches[0].clientX;
      y0 = e.touches[0].clientY;
      t0 = Date.now();
      dx = 0;
      axis = null;
      edge = x0 < 28;
      blocked = !edge && sideways(e.target as HTMLElement, el);
    };
    const move = (e: TouchEvent) => {
      if (blocked || e.touches.length !== 1) return;
      const x = e.touches[0].clientX - x0;
      const y = e.touches[0].clientY - y0;
      if (!axis) {
        if (Math.abs(x) < 10 && Math.abs(y) < 10) return;
        axis = x > 0 && Math.abs(x) > Math.abs(y) * (edge ? 1 : 1.6) ? "x" : "y";
        if (axis === "x") {
          el.style.transition = "none";
          if (b) b.style.transition = "none";
          if (s) s.style.transition = "none";
        }
      }
      if (axis !== "x") return;
      e.preventDefault();
      dx = Math.max(0, x);
      paint(dx);
    };
    const end = () => {
      if (blocked || axis !== "x") return;
      const v = dx / Math.max(1, Date.now() - t0);
      if (dx > w() * 0.33 || (v > 0.35 && dx > 40)) popBack(el, b, s, () => back.current());
      else {
        const t = `transform 0.24s ${EASE}`;
        el.style.transition = t;
        el.style.transform = "";
        if (b) {
          b.style.transition = t;
          b.style.transform = "";
        }
        if (s) {
          s.style.transition = "opacity 0.24s linear";
          s.style.opacity = "";
        }
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
  }, [top, base, shade, active]);
}

// ---- pull to refresh ----

/**
 * Pull a list down from its top to refresh it. `indicator` is the spinner
 * that shows how far it's pulled.
 */
export function usePullToRefresh(area: RefObject<HTMLElement | null>, indicator: RefObject<HTMLElement | null>, active: boolean, onRefresh: () => Promise<unknown>) {
  const refresh = useRef(onRefresh);
  refresh.current = onRefresh;
  useEffect(() => {
    const el = area.current;
    const ind = indicator.current;
    if (!active || !el || !ind) return;
    const ARM = 64;
    let y0 = 0;
    let x0 = 0;
    let pull = 0;
    let axis: "down" | "no" | null = null;
    let view: HTMLElement | null = null;
    let busy = false;
    const show = (d: number, animate: boolean) => {
      const t = animate ? `transform 0.28s ${EASE}` : "none";
      if (view) {
        view.style.transition = t;
        view.style.transform = d ? `translateY(${d}px)` : "";
      }
      ind.style.transition = animate ? `opacity 0.2s, transform 0.28s ${EASE}` : "none";
      ind.style.opacity = String(Math.min(1, d / ARM));
      ind.style.transform = `translateY(${Math.min(d, ARM) * 0.6}px) rotate(${d * 4}deg)`;
      ind.classList.toggle("armed", d >= ARM);
    };
    const start = (e: TouchEvent) => {
      axis = null;
      pull = 0;
      if (busy || e.touches.length !== 1 || document.querySelector(".sheet-backdrop, .modal-backdrop")) {
        axis = "no";
        return;
      }
      view = (e.target as HTMLElement).closest(".view");
      if (!view || view.scrollTop > 0 || (e.target as HTMLElement).closest("textarea, input")) {
        axis = "no";
        return;
      }
      y0 = e.touches[0].clientY;
      x0 = e.touches[0].clientX;
    };
    const move = (e: TouchEvent) => {
      if (axis === "no" || e.touches.length !== 1) return;
      const y = e.touches[0].clientY - y0;
      const x = e.touches[0].clientX - x0;
      if (!axis) {
        if (Math.abs(y) < 8 && Math.abs(x) < 8) return;
        axis = y > 0 && Math.abs(y) > Math.abs(x) && (view?.scrollTop ?? 1) <= 0 ? "down" : "no";
      }
      if (axis !== "down") return;
      e.preventDefault();
      // It resists more the further it goes.
      pull = Math.min(130, y * 0.5);
      show(pull, false);
    };
    const end = () => {
      if (axis !== "down") return;
      axis = null;
      if (pull < ARM) return show(0, true);
      busy = true;
      vibrate(10);
      ind.classList.add("spinning");
      show(ARM, true);
      const started = Date.now();
      Promise.resolve(refresh.current())
        .catch(() => {})
        .then(() => new Promise((r) => window.setTimeout(r, Math.max(0, 650 - (Date.now() - started)))))
        .then(() => {
          ind.classList.remove("spinning");
          show(0, true);
          busy = false;
        });
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
  }, [area, indicator, active]);
}
