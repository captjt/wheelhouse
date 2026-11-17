import "@fontsource-variable/inter";
import { StrictMode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import type { Docket } from "./api";
import { DocketScreen } from "./Docket";
import { InboxView, inboxItems } from "./Inbox";
import { IconActive, IconAgents, IconDone, IconHook, IconInbox, IconIssue, IconLayers, IconMenu, IconMonitor, IconMoon, IconPlus, IconSearch, IconSun, Logo, SessionIcon } from "./icons";
import { HubProvider, LiveProvider, go, useHub, useRoute, useTheme, type Theme } from "./live";
import { NewDocket, Palette } from "./Palette";
import { PaneProvider } from "./Pane";
import { PushHint, registerWorker } from "./Push";
import { ToastProvider, typing, useToast } from "./ui";
import { AgentsView, DocketListView } from "./views";
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

function App() {
  const hub = useHub();
  const route = useRoute();
  const [palette, setPalette] = useState(false);
  const [creating, setCreating] = useState<false | "blank" | "linear" | "agent">(false);
  const [nav, setNav] = useState(false);
  const page = route[0] || "view";

  const inboxCount = useMemo(() => inboxItems(hub.state?.dockets ?? [], hub.state?.messages ?? [], hub.state?.reviews ?? []).length, [hub.state]);
  const urgent = useMemo(
    () => (hub.state?.dockets ?? []).filter((d) => d.live?.state === "blocked" || d.live?.state === "error").length + (hub.state?.messages.length ?? 0) + (hub.state?.reviews?.length ?? 0),
    [hub.state],
  );
  useEffect(() => {
    document.title = urgent > 0 ? `(${urgent}) Wheelhouse` : "Wheelhouse";
  }, [urgent]);
  useEffect(() => setNav(false), [route.join("/")]);

  // Global keys: ⌘K or / for the palette, C to create, G then a letter to go.
  const pending = useRef<number | null>(null);
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
        return;
      }
      if (typing(e) || e.metaKey || e.ctrlKey || e.altKey || document.querySelector(".modal-backdrop, .menu")) return;
      if (pending.current) {
        window.clearTimeout(pending.current);
        pending.current = null;
        const dest = { i: "/inbox", a: "/view/active", d: "/view/all", n: "/agents", x: "/view/done" }[e.key];
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
    <a className={`nav-item${here === path.slice(1) || (path === "/view/active" && here === "") ? " on" : ""}`} href={`#${path}`}>
      {icon}
      <span>{label}</span>
      {!!count && <span className={`nav-count${alert ? " alert" : ""}`}>{count}</span>}
    </a>
  );

  let main: React.ReactNode;
  if (page === "d" && route[1]) main = <DocketScreen key={route[1]} id={route[1]} />;
  else if (page === "inbox") main = <InboxView />;
  else if (page === "agents") main = <AgentsView />;
  else if (page === "session" && route[1]) {
    const s = route[1];
    main = (
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
  } else {
    const v = (route[1] as keyof typeof FILTERS) || "active";
    const meta = {
      active: { title: "Active", icon: <IconActive size={15} />, group: "attention" as const, empty: "No open dockets. Start an agent in Rex, or press C." },
      all: { title: "All dockets", icon: <IconLayers size={15} />, group: "status" as const, empty: "No dockets yet." },
      done: { title: "Done", icon: <IconDone size={15} />, group: "status" as const, empty: "Nothing done yet." },
    }[v] ?? { title: "Active", icon: <IconActive size={15} />, group: "attention" as const, empty: "" };
    main = <DocketListView key={v} path={`/view/${v}`} title={meta.title} icon={meta.icon} filter={FILTERS[v] ?? FILTERS.active} defaultGroup={meta.group} empty={meta.empty} />;
  }

  return (
    <div className={`app${nav ? " nav-open" : ""}`}>
      <aside className="sidebar">
        <div className="side-top">
          <a className="workspace" href="#/view/active">
            <span className="ws-logo">
              <Logo size={14} />
            </span>
            Wheelhouse
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
          {item("/view/active", "Active", <IconActive size={15} />, counts.active)}
          {item("/view/all", "All dockets", <IconLayers size={15} />)}
          {item("/view/done", "Done", <IconDone size={15} />)}
          {item("/agents", "Agents", <IconAgents size={15} />, counts.agents)}
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
        <div className="side-foot">
          {hub.state && !hub.state.hooks && <HooksHint />}
          {hub.state && !hub.state.linear && <LinearHint />}
          <PushHint />
          <div className="conn">
            <span className={`conn-dot ${hub.state ? (hub.state.rex.connected ? "on" : "off") : ""}`} />
            {hub.state ? (hub.state.rex.connected ? "Following Rex" : `Rex: ${hub.state.rex.error || "connecting"}`) : hub.error ? "Hub unreachable" : "Connecting…"}
            <span className="spacer" />
            <ThemeSwitch />
          </div>
        </div>
      </aside>
      <div className="scrim" onClick={() => setNav(false)} />
      <main className="main">
        <button className="icon-btn nav-toggle" onClick={() => setNav(true)} aria-label="Menu">
          <IconMenu size={16} />
        </button>
        {main}
      </main>
      {palette && <Palette onClose={closePalette} onNew={openNew} onNewLinear={openNewLinear} onNewAgent={openNewAgent} context={context} />}
      {creating && <NewDocket linear={creating === "linear"} agent={creating === "agent"} onClose={() => setCreating(false)} />}
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
