// The phone's screen: keep the app on what's visible of it, and (from the
// Home Screen) say what the screen did when the top bar ends up out of place.
//
// iOS moves the page under a fixed app: it scrolls a focused box into view,
// it can leave the page scrolled when the keyboard goes, and it restores a
// scroll when you come back to the app. WebKit scrolls the document even
// when nothing in it is taller than the screen, so the page can't be made
// unscrollable; instead the app follows the visible part of the screen
// (`--vv-top`, `--vv-h`, `--kb`) and the page goes back to the top whenever
// it has moved. Events don't always come (a Home Screen app returning from
// the background, the keyboard closing), so it also looks a few times a
// second while the app is in front.

import { post } from "./api";

const root = () => document.documentElement.style;

/** Running from the Home Screen (iOS's `navigator.standalone`, or the manifest's display mode). */
export const standalone = () =>
  window.matchMedia("(display-mode: standalone)").matches || (navigator as unknown as { standalone?: boolean }).standalone === true;

/** The safe areas as CSS has them, in px, read from a probe. */
function insets(): { top: number; bottom: number } {
  let p = document.getElementById("wh-safe-probe");
  if (!p) {
    p = document.createElement("div");
    p.id = "wh-safe-probe";
    p.setAttribute("aria-hidden", "true");
    p.style.cssText =
      "position:fixed;left:0;top:0;width:0;visibility:hidden;pointer-events:none;padding-top:env(safe-area-inset-top);padding-bottom:env(safe-area-inset-bottom)";
    document.body.appendChild(p);
  }
  const cs = getComputedStyle(p);
  return { top: parseFloat(cs.paddingTop) || 0, bottom: parseFloat(cs.paddingBottom) || 0 };
}

/** The top bar on screen now: an open sheet's (the terminal's), else the
 * pushed docket's, else the list's. */
function bar(): DOMRect | null {
  const el = document.querySelector(".ps-head") ?? document.querySelector(".sheet-head") ?? document.querySelector(".m-top .view-head") ?? document.querySelector(".m-base .view-head");
  return el ? el.getBoundingClientRect() : null;
}

/** The whole screen's height, when the app has the whole screen (a Home
 * Screen app, not split). iOS 26 and later lay a Home Screen app's page out
 * shorter than this, from the top: the strip below it isn't the page's, and
 * nothing drawn past the page's edge shows there. */
function fullHeight(): number {
  if (!standalone()) return 0;
  const [short, long] = [Math.min(screen.width, screen.height), Math.max(screen.width, screen.height)];
  const landscape = window.matchMedia("(orientation: landscape)").matches;
  const [w, h] = landscape ? [long, short] : [short, long];
  return Math.abs(window.innerWidth - w) <= 2 ? h : 0;
}

function measure(why: string) {
  const vv = window.visualViewport;
  const b = bar();
  const app = document.querySelector(".phone")?.getBoundingClientRect();
  const css = getComputedStyle(document.documentElement);
  return {
    why,
    route: location.hash,
    standalone: standalone(),
    ua: navigator.userAgent,
    screen: { w: screen.width, h: screen.height },
    inner: { w: innerWidth, h: innerHeight },
    scroll: { x: scrollX, y: scrollY },
    vv: vv ? { w: vv.width, h: vv.height, top: vv.offsetTop, pageTop: vv.pageTop, scale: vv.scale } : null,
    safe: insets(),
    css: { vvTop: css.getPropertyValue("--vv-top"), vvH: css.getPropertyValue("--vv-h"), kb: css.getPropertyValue("--kb"), safeBottom: css.getPropertyValue("--safe-bottom") },
    bar: b ? { top: b.top, bottom: b.bottom, height: b.height } : null,
    sheet: (() => {
      const m = document.querySelector(".modal, .sheet")?.getBoundingClientRect();
      return m ? { top: m.top, bottom: m.bottom, height: m.height } : null;
    })(),
    full: fullHeight(),
    bottomAt: document.elementFromPoint(innerWidth / 2, innerHeight - 2)?.className?.toString().slice(0, 40) ?? null,
    app: app ? { top: app.top, height: app.height } : null,
    focused: document.activeElement?.tagName ?? null,
    visible: document.visibilityState,
  };
}

/** Whether the top bar is out of place: moved above the visible top (it
 * starts there, its own padding keeping its buttons clear of the clock),
 * off the screen, or, from the Home Screen, with no safe area to keep it
 * from under the clock. */
function misplaced(): boolean {
  const b = bar();
  if (!b) return false;
  const top = window.visualViewport?.offsetTop ?? 0;
  return b.top < top - 1 || b.bottom <= top + 1 || (standalone() && insets().top === 0);
}

let sent = 0;
let last = 0;
/** Tell the hub what the screen did: from the Home Screen only, at most one a second, 300 a visit. */
function report(why: string, force = false) {
  if (!standalone() || sent >= 300) return;
  const now = Date.now();
  if (!force && now - last < 1000) return;
  last = now;
  sent++;
  void post("/api/diag/viewport", measure(why)).catch(() => {});
}

/** Keep the app on the visible part of the screen. Returns a stop function. */
export function keepOnScreen(): () => void {
  const vv = window.visualViewport;
  let wasOff = false;
  const sync = (why: string) => {
    if (window.scrollX || window.scrollY) window.scrollTo(0, 0);
    if (vv) {
      // Zoomed in with two fingers: the page is bigger than the screen on purpose.
      if (Math.abs(vv.scale - 1) > 0.01) {
        root().removeProperty("--vv-top");
        root().removeProperty("--vv-h");
      } else {
        const kb = Math.max(0, Math.round(window.innerHeight - vv.height - vv.offsetTop));
        // What the page is given, and no more: from the Home Screen, iOS 26
        // gives it less than the screen, and drawing past that is cut off.
        const h = vv.height;
        root().setProperty("--kb", kb > 80 ? `${kb}px` : "0px");
        root().setProperty("--vv-top", `${Math.max(0, Math.round(vv.offsetTop))}px`);
        root().setProperty("--vv-h", `${Math.round(h)}px`);
        // A page that ends well above the screen's bottom (a Home Screen
        // app on iOS 26 and later) doesn't reach the home indicator: its
        // inset would only add empty space under the tab bar and the box.
        // It starts under the status bar (its top inset covers the
        // shortfall) and so stops that much above the bottom.
        const full = fullHeight();
        const short = full - window.innerHeight;
        if (full && short >= 20 && insets().top >= short - 2) root().setProperty("--safe-bottom", "0px");
        else root().removeProperty("--safe-bottom");
      }
    }
    // Out of place: say so once when it happens (and again if it stays).
    const off = misplaced();
    if (off) report(`misplaced:${why}`, !wasOff);
    wasOff = off;
  };
  const later = (why: string, ms: number) => () => window.setTimeout(() => sync(why), ms);
  const on = () => sync("viewport");
  const onScroll = () => sync("scroll");
  const blur = () => {
    sync("blur");
    window.setTimeout(() => (sync("blur+300"), report("keyboard closed")), 300);
  };
  const focus = (e: FocusEvent) => {
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "TEXTAREA" || t.tagName === "INPUT" || t.isContentEditable)) window.setTimeout(() => (sync("focus+400"), report("keyboard open")), 400);
  };
  const shown = () => {
    if (document.visibilityState !== "visible") return;
    sync("visible");
    window.setTimeout(() => (sync("visible+500"), report("back to the app")), 500);
  };
  // A few times a second while the app is in front: events don't always come.
  const tick = window.setInterval(() => document.visibilityState === "visible" && sync("tick"), 250);
  sync("open");
  window.setTimeout(() => (sync("open+800"), report("open", true)), 800);
  vv?.addEventListener("resize", on);
  vv?.addEventListener("scroll", on);
  window.addEventListener("scroll", onScroll, { passive: true });
  window.addEventListener("focusin", focus);
  window.addEventListener("focusout", blur);
  window.addEventListener("orientationchange", later("rotate", 300));
  window.addEventListener("pageshow", later("pageshow", 100));
  document.addEventListener("visibilitychange", shown);
  return () => {
    window.clearInterval(tick);
    vv?.removeEventListener("resize", on);
    vv?.removeEventListener("scroll", on);
    window.removeEventListener("scroll", onScroll);
    window.removeEventListener("focusin", focus);
    window.removeEventListener("focusout", blur);
    document.removeEventListener("visibilitychange", shown);
    for (const k of ["--kb", "--vv-top", "--vv-h", "--safe-bottom"]) root().removeProperty(k);
  };
}
