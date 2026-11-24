// The phone's screen: keep the app on what's visible of it.
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
//
// A Home Screen app has the status bar in its own band (index.html): under
// a translucent one, iOS 26 and later lay the page out short of the
// screen's bottom. If the page still ends above the home indicator (an icon
// added before that change), the indicator's inset is dropped
// (`--safe-bottom`), since it would only add empty space.

const root = () => document.documentElement.style;

/** Running from the Home Screen (iOS's `navigator.standalone`, or the manifest's display mode). */
export const standalone = () =>
  window.matchMedia("(display-mode: standalone)").matches || (navigator as unknown as { standalone?: boolean }).standalone === true;

/** The top safe area as CSS has it, in px, read from a probe. */
function safeTop(): number {
  let p = document.getElementById("wh-safe-probe");
  if (!p) {
    p = document.createElement("div");
    p.id = "wh-safe-probe";
    p.setAttribute("aria-hidden", "true");
    p.style.cssText = "position:fixed;left:0;top:0;width:0;visibility:hidden;pointer-events:none;padding-top:env(safe-area-inset-top)";
    document.body.appendChild(p);
  }
  return parseFloat(getComputedStyle(p).paddingTop) || 0;
}

/** The whole screen's height, when a Home Screen app has the whole screen (not split). */
function fullHeight(): number {
  if (!standalone()) return 0;
  const [short, long] = [Math.min(screen.width, screen.height), Math.max(screen.width, screen.height)];
  const landscape = window.matchMedia("(orientation: landscape)").matches;
  const [w, h] = landscape ? [long, short] : [short, long];
  return Math.abs(window.innerWidth - w) <= 2 ? h : 0;
}

/** Keep the app on the visible part of the screen. Returns a stop function. */
export function keepOnScreen(): () => void {
  const vv = window.visualViewport;
  const sync = () => {
    if (window.scrollX || window.scrollY) window.scrollTo(0, 0);
    if (!vv) return;
    // Zoomed in with two fingers: the page is bigger than the screen on purpose.
    if (Math.abs(vv.scale - 1) > 0.01) {
      root().removeProperty("--vv-top");
      root().removeProperty("--vv-h");
      return;
    }
    const kb = Math.max(0, Math.round(window.innerHeight - vv.height - vv.offsetTop));
    root().setProperty("--kb", kb > 80 ? `${kb}px` : "0px");
    root().setProperty("--vv-top", `${Math.max(0, Math.round(vv.offsetTop))}px`);
    // What the page is given, and no more: drawing past it is cut off.
    root().setProperty("--vv-h", `${Math.round(vv.height)}px`);
    // A page under the status bar that stops that much above the bottom.
    const full = fullHeight();
    const short = full - window.innerHeight;
    if (full && short >= 20 && safeTop() >= short - 2) root().setProperty("--safe-bottom", "0px");
    else root().removeProperty("--safe-bottom");
  };
  const later = (ms: number) => () => window.setTimeout(sync, ms);
  const focus = (e: FocusEvent) => {
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === "TEXTAREA" || t.tagName === "INPUT" || t.isContentEditable)) window.setTimeout(sync, 400);
  };
  const blur = () => (sync(), window.setTimeout(sync, 300));
  const shown = () => document.visibilityState === "visible" && (sync(), window.setTimeout(sync, 500));
  // A few times a second while the app is in front: events don't always come.
  const tick = window.setInterval(() => document.visibilityState === "visible" && sync(), 250);
  const rotate = later(300);
  const pageshow = later(100);
  sync();
  vv?.addEventListener("resize", sync);
  vv?.addEventListener("scroll", sync);
  window.addEventListener("scroll", sync, { passive: true });
  window.addEventListener("focusin", focus);
  window.addEventListener("focusout", blur);
  window.addEventListener("orientationchange", rotate);
  window.addEventListener("pageshow", pageshow);
  document.addEventListener("visibilitychange", shown);
  return () => {
    window.clearInterval(tick);
    vv?.removeEventListener("resize", sync);
    vv?.removeEventListener("scroll", sync);
    window.removeEventListener("scroll", sync);
    window.removeEventListener("focusin", focus);
    window.removeEventListener("focusout", blur);
    window.removeEventListener("orientationchange", rotate);
    window.removeEventListener("pageshow", pageshow);
    document.removeEventListener("visibilitychange", shown);
    for (const k of ["--kb", "--vv-top", "--vv-h", "--safe-bottom"]) root().removeProperty(k);
  };
}
