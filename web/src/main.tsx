import "@fontsource-variable/inter";
import { StrictMode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import type { Docket, Flavor } from "./api";
import { DocketScreen } from "./Docket";
import { InboxView, inboxItems } from "./Inbox";
import { IconActive, IconAgents, IconBuild, IconDone, IconHook, IconInbox, IconIssue, IconLayers, IconMenu, IconMonitor, IconMoon, IconChart, IconPlus, IconPR, IconSearch, IconSettings, IconSun, Logo, SessionIcon } from "./icons";
import { ReviewsView } from "./Reviews";
import { RecapView } from "./Recap";
import { BuildsView } from "./Builds";
import { HubProvider, LiveProvider, go, useHub, useRoute, useTheme, type Theme } from "./live";
import { NewDocket, Palette } from "./Palette";
import { PaneProvider } from "./Pane";
import { PushHint, registerWorker } from "./Push";
import { ToastProvider, typing, usePhone, useToast } from "./ui";
import { MoreSheet, TabBar, nav, popBack, tabs, usePullToRefresh, useScrolledViews, useSwipeBack } from "./mobile";
import { lastList } from "./live";
import { post } from "./api";
import { AgentsView, DocketListView } from "./views";
import { SettingsView } from "./Settings";
import { HoldDialog, RefusedDialog } from "./Verify";
import "./styles.css";

const isOpen = (d: Docket) => d.status === "todo" || d.status === "in_progress";
const FILTERS = {
  active: isOpen,
  all: () => true,
  done: (d: Docket) => d.status === "done" || d.status === "dropped",
};

/** Hooks off: dockets are named after panes. One line, and a click copies the fix. */
function HooksHint() {
  const toast = useToast();
  return (
    <button
      className="hint"
      title="Claude Code's hooks are off, so dockets are named after their panes and turns carry no message. Click to copy the command that turns them on."
      onClick={() => {
        navigator.clipboard?.writeText("wh hooks install");
        toast("Copied “wh hooks install”. Run it in a terminal, then restart your agents.");
      }}
    >
      <IconHook size={14} />
      <span className="hint-title">Hooks off</span>
      <span className="spacer" />
      <span className="hint-action">Turn on</span>
    </button>
  );
}

/** Linear isn't connected: one line, and a click copies the setup command. */
function LinearHint() {
  const toast = useToast();
  return (
    <button
      className="hint"
      title="Connect Linear to start dockets from issues and link issues to dockets. Click to copy the command."
      onClick={() => {
        navigator.clipboard?.writeText("wh setup-linear");
        toast("Copied “wh setup-linear”. Run it in a terminal, paste a Linear API key, then restart the hub.");
      }}
    >
      <IconIssue size={14} />
      <span className="hint-title">Linear off</span>
      <span className="spacer" />
      <span className="hint-action">Connect</span>
    </button>
  );
}

function ThemeSwitch() {
  const [theme, setTheme] = useTheme();
  const opts: { t: Theme; label: string; icon: React.ReactNode }[] = [
    { t: "system", label: "System", icon: <IconMonitor size={14} /> },
    { t: "light", label: "Light", icon: <IconSun size={14} /> },
    { t: "dark", label: "Dark", icon: <IconMoon size={14} /> },
  ];
  return (
    <div className="theme-seg" role="radiogroup" aria-label="Theme">
      {opts.map((o) => (
        <button key={o.t} className={theme === o.t ? "on" : ""} title={o.t === "system" ? "System: follow macOS" : o.label} role="radio" aria-checked={theme === o.t} onClick={() => setTheme(o.t)}>
          {o.icon}
        </button>
      ))}
    </div>
  );
}

/** The list (or other) view a route shows; a docket's is in `DocketScreen`. */
function RouteView({ route }: { route: string[] }) {
  const page = route[0] || "view";
  if (page === "inbox") return <InboxView />;
  if (page === "agents") return <AgentsView />;
  if (page === "reviews") return <ReviewsView />;
  if (page === "recap") return <RecapView />;
  if (page === "builds") return <BuildsView />;
  if (page === "settings") return <SettingsView section={route[1]} />;
  if (page === "session" && route[1]) {
    const s = route[1];
    return (
      <DocketListView
        key={`s-${s}`}
        path={`/session/${s}`}
        title={s}
        icon={<SessionIcon name={s} />}
        filter={(d) => isOpen(d) && d.agent?.rex_session === s}
        defaultGroup="attention"
        empty="No open dockets in this Rex session."
      />
    );
  }
  const v = (route[1] as keyof typeof FILTERS) || "active";
  const meta = {
    active: { title: "Active", icon: <IconActive size={15} />, group: "attention" as const, empty: "No open dockets. Start an agent in Rex, or press C." },
    all: { title: "All dockets", icon: <IconLayers size={15} />, group: "status" as const, empty: "No dockets yet." },
    done: { title: "Done", icon: <IconDone size={15} />, group: "status" as const, empty: "Nothing done yet." },
  }[v] ?? { title: "Active", icon: <IconActive size={15} />, group: "attention" as const, empty: "" };
  return <DocketListView key={v} path={`/view/${v}`} title={meta.title} icon={meta.icon} filter={FILTERS[v] ?? FILTERS.active} defaultGroup={meta.group} empty={meta.empty} />;
}

const routeOf = (path: string) =>
  decodeURIComponent(path.replace(/^#?\/?/, ""))
    .split("/")
    .filter(Boolean);

/** The tab's icon: the wheel, in the flavor's color. */
const favicon = (color: string) =>
  `data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'><circle cx='16' cy='16' r='11' fill='none' stroke='${color}' stroke-width='3'/><circle cx='16' cy='16' r='3' fill='${color}'/></svg>`;

/** DEV, as a nightly build marks itself, with the build it is. */
function FlavorBadge({ flavor }: { flavor: Flavor }) {
  return (
    <span className="flavor-badge" title={`Wheelhouse ${flavor.name === "dev" ? "Dev" : flavor.name}: ${flavor.branch} at ${flavor.commit.slice(0, 7)}. A copy of your dockets, with nothing live.`}>
      {flavor.name.toUpperCase()}
    </span>
  );
}

function App() {
  const hub = useHub();
  const route = useRoute();
  const [palette, setPalette] = useState(false);
  const [creating, setCreating] = useState<false | "blank" | "linear" | "agent">(false);
  const [drawer, setDrawer] = useState(false);
  const [more, setMore] = useState(false);
  const phone = usePhone();
  const page = route[0] || "view";

  const inboxCount = useMemo(() => inboxItems(hub.state?.dockets ?? [], hub.state?.messages ?? [], hub.state?.review?.prs ?? []).length, [hub.state]);
  const reviewAlerts = (hub.state?.review?.prs ?? []).filter((p) => p.attention).length;
  const buildAlerts = (hub.state?.build?.tasks ?? []).filter((t) => t.attention).length;
  const buildCount = (hub.state?.build?.tasks ?? []).filter((t) => t.state === "queued" || t.state === "active").length;
  const urgent = useMemo(
    () => (hub.state?.dockets ?? []).filter((d) => d.live?.state === "blocked" || d.live?.state === "error").length + (hub.state?.messages.length ?? 0) + reviewAlerts + buildAlerts,
    [hub.state, reviewAlerts, buildAlerts],
  );
  const reviewCount = (hub.state?.review?.prs ?? []).filter((p) => p.state === "queued" || p.state === "reviewing").length;
  const flavor = hub.state?.flavor ?? null;
  const name = flavor ? `Wheelhouse ${flavor.name === "dev" ? "Dev" : flavor.name}` : "Wheelhouse";
  // A second hub's web app wears its flavor: the brand in blue, a DEV badge, its name.
  useEffect(() => {
    const root = document.documentElement;
    if (flavor) root.dataset.flavor = flavor.name;
    else delete root.dataset.flavor;
    const icon = document.querySelector<HTMLLinkElement>("link[rel=icon]");
    if (icon) icon.href = favicon(flavor ? "%232f7dff" : "%23f6821f");
  }, [flavor?.name]);
  useEffect(() => {
    document.title = urgent > 0 ? `(${urgent}) ${name}` : name;
    // The installed app's Dock or Home Screen badge: what needs you.
    const nav = navigator as Navigator & { setAppBadge?: (n?: number) => Promise<void>; clearAppBadge?: () => Promise<void> };
    if (urgent > 0) nav.setAppBadge?.(urgent).catch(() => {});
    else nav.clearAppBadge?.().catch(() => {});
  }, [urgent]);
  useEffect(() => (setDrawer(false), setMore(false)), [route.join("/")]);
  // Where a docket goes back to: the view it was opened from, whichever.
  useEffect(() => {
    if (page === "d") return;
    const path = `/${route.map(encodeURIComponent).join("/")}` || "/view/active";
    const titles: Record<string, string> = { inbox: "Inbox", reviews: "Reviews", builds: "Builds", recap: "Recap", agents: "Agents", settings: "Settings" };
    const views: Record<string, string> = { active: "Active", all: "All dockets", done: "Done" };
    const title = page === "session" ? route[1] : page === "view" ? views[route[1] || "active"] : titles[page];
    if (!title) return;
    if (lastList.path !== path) lastList.ids = [];
    lastList.path = path === "/" ? "/view/active" : path;
    lastList.title = title;
  }, [route.join("/")]);

  // Global keys: ⌘K or / for the palette, ⌘, for Settings, C to create, G then a letter to go.
  const pending = useRef<number | null>(null);
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
        return;
      }
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key === ",") {
        e.preventDefault();
        setPalette(false);
        go("/settings");
        return;
      }
      if (typing(e) || e.metaKey || e.ctrlKey || e.altKey || document.querySelector(".modal-backdrop, .menu")) return;
      if (pending.current) {
        window.clearTimeout(pending.current);
        pending.current = null;
        const dest = { i: "/inbox", a: "/view/active", d: "/view/all", n: "/agents", x: "/view/done", r: "/reviews", p: "/recap", b: "/builds", s: "/settings" }[e.key];
        if (dest) {
          e.preventDefault();
          go(dest);
        }
        return;
      }
      if (e.key === "g") pending.current = window.setTimeout(() => (pending.current = null), 900);
      else if (e.key === "/") (e.preventDefault(), setPalette(true));
      else if (e.key === "c") (e.preventDefault(), setCreating("blank"));
    };
    window.addEventListener("keydown", on);
    return () => window.removeEventListener("keydown", on);
  }, []);

  const context = page === "d" && route[1] ? hub.byId.get(route[1]) : undefined;
  const closePalette = useCallback(() => setPalette(false), []);
  const openNew = useCallback(() => setCreating("blank"), []);
  const openNewLinear = useCallback(() => setCreating("linear"), []);
  const openNewAgent = useCallback(() => setCreating("agent"), []);

  const sessions = useMemo(() => {
    const m = new Map<string, number>();
    for (const d of hub.state?.dockets ?? []) if (isOpen(d) && d.agent?.rex_session) m.set(d.agent.rex_session, (m.get(d.agent.rex_session) ?? 0) + 1);
    return [...m.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  }, [hub.state]);
  const counts = useMemo(() => {
    const ds = hub.state?.dockets ?? [];
    return { active: ds.filter(isOpen).length, agents: hub.state?.agents.length ?? 0 };
  }, [hub.state]);

  const here = route.join("/");
  const item = (path: string, label: string, icon: React.ReactNode, count?: number, alert?: boolean) => (
    <a className={`nav-item${here === path.slice(1) || (path === "/view/active" && here === "") || (path === "/settings" && here.startsWith("settings/")) ? " on" : ""}`} href={`#${path}`}>
      {icon}
      <span>{label}</span>
      {!!count && <span className={`nav-count${alert ? " alert" : ""}`}>{count}</span>}
    </a>
  );

  const docket = page === "d" && route[1] ? route[1] : null;
  const main = docket ? <DocketScreen key={docket} id={docket} /> : <RouteView route={route} />;
  const foot = (
    <>
      {hub.state && !hub.state.hooks && <HooksHint />}
      {hub.state && !hub.state.linear && <LinearHint />}
      <PushHint />
      <div className="conn">
        <span className={`conn-dot ${hub.state ? (hub.state.rex.connected ? "on" : "off") : ""}`} />
        {hub.state ? (hub.state.rex.connected ? "Following Rex" : `Rex: ${hub.state.rex.error || "connecting"}`) : hub.error ? "Hub unreachable" : "Connecting…"}
        <span className="spacer" />
        <ThemeSwitch />
      </div>
    </>
  );
  const closeDialog = useCallback(() => hub.setDialog(null), [hub.setDialog]);
  const overlays = (
    <>
      {palette && <Palette onClose={closePalette} onNew={openNew} onNewLinear={openNewLinear} onNewAgent={openNewAgent} context={context} />}
      {creating && <NewDocket linear={creating === "linear"} agent={creating === "agent"} onClose={() => setCreating(false)} />}
      {hub.dialog?.kind === "refused" && <RefusedDialog key={hub.dialog.id} id={hub.dialog.id} onClose={closeDialog} />}
      {hub.dialog?.kind === "hold" && <HoldDialog key={hub.dialog.id} id={hub.dialog.id} onClose={closeDialog} />}
    </>
  );

  if (phone)
    return (
      <PhoneApp
        route={route}
        docket={docket}
        here={here}
        tabItems={tabs({ inbox: inboxCount, urgent: urgent > 0, reviews: reviewAlerts || reviewCount, reviewAlerts: reviewAlerts > 0, builds: buildAlerts || buildCount, buildAlerts: buildAlerts > 0, active: counts.active })}
        more={more}
        setMore={setMore}
        sessions={sessions}
        agents={counts.agents}
        foot={foot}
        onSearch={() => setPalette(true)}
        onNew={() => setCreating("blank")}
        overlays={overlays}
      />
    );

  return (
    <div className={`app${drawer ? " nav-open" : ""}`}>
      <aside className="sidebar">
        <div className="side-top">
          <a className="workspace" href="#/view/active">
            <span className="ws-logo">
              <Logo size={14} />
            </span>
            Wheelhouse
            {flavor && <FlavorBadge flavor={flavor} />}
          </a>
          <span className="spacer" />
          <button className="icon-btn" title="Search (⌘K)" onClick={() => setPalette(true)}>
            <IconSearch size={15} />
          </button>
          <button className="icon-btn new-btn" title="New docket (C)" onClick={() => setCreating("blank")}>
            <IconPlus size={15} />
          </button>
        </div>
        <nav className="nav">
          {item("/inbox", "Inbox", <IconInbox size={15} />, inboxCount, urgent > 0)}
          {item("/reviews", "Reviews", <IconPR size={15} />, reviewAlerts || reviewCount, reviewAlerts > 0)}
          {item("/builds", "Builds", <IconBuild size={15} />, buildAlerts || buildCount, buildAlerts > 0)}
          {item("/view/active", "Active", <IconActive size={15} />, counts.active)}
          {item("/view/all", "All dockets", <IconLayers size={15} />)}
          {item("/view/done", "Done", <IconDone size={15} />)}
          {item("/agents", "Agents", <IconAgents size={15} />, counts.agents)}
          {item("/recap", "Recap", <IconChart size={15} />)}
        </nav>
        {sessions.length > 0 && (
          <>
            <div className="nav-section">Rex sessions</div>
            <nav className="nav">
              {sessions.map(([s, n]) => (
                <a key={s} className={`nav-item${here === `session/${s}` ? " on" : ""}`} href={`#/session/${encodeURIComponent(s)}`}>
                  <SessionIcon name={s} />
                  <span>{s}</span>
                  <span className="nav-count">{n}</span>
                </a>
              ))}
            </nav>
          </>
        )}
        <span className="spacer-v" />
        <nav className="nav nav-foot">{item("/settings", "Settings", <IconSettings size={15} />)}</nav>
        <div className="side-foot">{foot}</div>
      </aside>
      <div className="scrim" onClick={() => setDrawer(false)} />
      <main className="main">
        <button className="icon-btn nav-toggle" onClick={() => setDrawer(true)} aria-label="Menu">
          <IconMenu size={16} />
        </button>
        {main}
      </main>
      {overlays}
    </div>
  );
}

/**
 * The phone: a tab bar, and a stack. The list stays mounted under a docket
 * (so going back is instant and keeps your place); the docket slides on,
 * and a swipe right takes it off.
 */
function PhoneApp({
  route,
  docket,
  here,
  tabItems,
  more,
  setMore,
  sessions,
  agents,
  foot,
  onSearch,
  onNew,
  overlays,
}: {
  route: string[];
  docket: string | null;
  here: string;
  tabItems: ReturnType<typeof tabs>;
  more: boolean;
  setMore: (b: boolean) => void;
  sessions: [string, number][];
  agents: number;
  foot: React.ReactNode;
  onSearch: () => void;
  onNew: () => void;
  overlays: React.ReactNode;
}) {
  const base = useRef<HTMLDivElement>(null);
  const top = useRef<HTMLDivElement>(null);
  const shade = useRef<HTMLDivElement>(null);
  const ptr = useRef<HTMLDivElement>(null);
  const back = useCallback(() => go(lastList.path), []);
  useEffect(() => {
    nav.back = () => popBack(top.current, base.current, shade.current, back);
    return () => {
      nav.back = back;
    };
  }, [back]);
  useSwipeBack(top, base, shade, docket ?? false, back);
  useScrolledViews(true);
  // The on-screen keyboard: how much of the screen it covers, so the docket
  // (and its conversation's box) stays above it. Safari scrolls the page
  // when a box takes focus; it goes back to the top.
  useEffect(() => {
    const vv = window.visualViewport;
    if (!vv) return;
    const on = () => {
      const kb = Math.max(0, Math.round(window.innerHeight - vv.height));
      document.documentElement.style.setProperty("--kb", kb > 80 ? `${kb}px` : "0px");
      if (kb > 80 && window.scrollY) window.scrollTo(0, 0);
    };
    vv.addEventListener("resize", on);
    vv.addEventListener("scroll", on);
    return () => {
      vv.removeEventListener("resize", on);
      vv.removeEventListener("scroll", on);
      document.documentElement.style.removeProperty("--kb");
    };
  }, []);
  const page = route[0] || "view";
  usePullToRefresh(base, ptr, !docket, () => {
    window.dispatchEvent(new Event("wh:refresh"));
    return page === "reviews" ? post("/api/review/check", {}).catch(() => {}) : Promise.resolve();
  });
  // Under a docket, the list it came from.
  const under = docket ? routeOf(lastList.path) : route;
  return (
    <div className={`app phone${docket ? " pushed" : ""}`}>
      <main className="main m-main">
        <div ref={base} className={`m-base${docket ? " covered" : ""}`} aria-hidden={docket ? true : undefined}>
          <RouteView route={under} />
        </div>
        <div ref={ptr} className="ptr" aria-hidden="true">
          <span className="ptr-spin" />
        </div>
        {docket && (
          <>
            <div ref={shade} className="m-shade" />
            <div ref={top} className="m-top" key={docket}>
              <DocketScreen key={docket} id={docket} />
            </div>
          </>
        )}
      </main>
      {!docket && <TabBar here={here} items={tabItems} onMore={() => setMore(true)} />}
      {more && (
        <MoreSheet
          here={here}
          onClose={() => setMore(false)}
          actions={[
            { key: "search", label: "Search", icon: <IconSearch size={18} />, run: onSearch },
            { key: "new", label: "New docket", icon: <IconPlus size={18} />, run: onNew },
          ]}
          links={[
            { path: "/view/all", label: "All dockets", icon: <IconLayers size={18} /> },
            { path: "/view/done", label: "Done", icon: <IconDone size={18} /> },
            { path: "/agents", label: "Agents", icon: <IconAgents size={18} />, count: agents },
            { path: "/recap", label: "Recap", icon: <IconChart size={18} /> },
            { path: "/settings", label: "Settings", icon: <IconSettings size={18} /> },
          ]}
          sessions={sessions.map(([s, n]) => ({ path: `/session/${encodeURIComponent(s)}`, label: s, icon: <SessionIcon name={s} size={18} />, count: n }))}
          footer={foot}
        />
      )}
      {overlays}
    </div>
  );
}

registerWorker();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ToastProvider>
      <LiveProvider>
        <HubProvider>
          <PaneProvider>
            <App />
          </PaneProvider>
        </HubProvider>
      </LiveProvider>
    </ToastProvider>
  </StrictMode>,
);
