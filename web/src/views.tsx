import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { appName, followedRuntimes, liveAgents, quick, paneRuntime, runtimeOf, runtimeTitle, type Docket } from "./api";
import { AppAvatar, IconBoard, IconChevronDown, IconEye, SessionChip, SessionIcon, TitleText, shortPath, IconCopy, IconDisplay, IconList, IconTerminal, LiveIcon, StatusIcon, liveWord } from "./icons";
import { ago, go, lastList, store, useHub } from "./live";
import { useKeys, useMenu, usePhone, type MenuItem } from "./ui";
import { LargeTitle, SwipeRow, type SwipeAction } from "./mobile";
import { IconAgents, IconCheck, IconPause, IconPlay, IconX } from "./icons";
import { onThisMac, usePane } from "./Pane";

export const STATUSES: { key: Docket["status"]; label: string }[] = [
  { key: "todo", label: "To do" },
  { key: "in_progress", label: "In progress" },
  { key: "done", label: "Done" },
  { key: "dropped", label: "Dropped" },
];

export function statusItems(d: Docket, set: (s: Docket["status"]) => void): MenuItem[] {
  return STATUSES.map((s, i) => ({
    key: s.key,
    label: s.label,
    icon: <StatusIcon status={s.key} />,
    hint: String(i + 1),
    checked: d.status === s.key,
    onPick: () => d.status !== s.key && set(s.key),
  }));
}

export function rowMenu(d: Docket, hub: ReturnType<typeof useHub>, pane: ReturnType<typeof usePane>): MenuItem[] {
  const items: MenuItem[] = [
    { key: "open", label: "Open", hint: "↩", onPick: () => go(`/d/${d.id}`) },
  ];
  if (d.live) {
    if (onThisMac()) items.push({ key: "rex", label: `Show pane in ${paneRuntime(d)}`, icon: <IconTerminal size={14} />, hint: "O", onPick: () => hub.openInRex(d.id) });
    items.push({ key: "peek", label: onThisMac() ? "Peek at pane" : "View pane", icon: <IconEye size={14} />, hint: onThisMac() ? undefined : "O", onPick: () => pane.view(d.id) });
    if (quick(d.live)) {
      items.push({ key: "approve", label: <>Approve <span className="menu-sub">{d.live.msg}</span></>, onPick: () => hub.answer(d, true) });
      items.push({ key: "deny", label: "Deny", onPick: () => hub.answer(d, false) });
    }
  }
  items.push(
    ...statusItems(d, (s) => hub.setStatus(d.id, s)).map((i) => ({ ...i, key: `s-${i.key}`, hint: undefined })),
    d.hold
      ? { key: "resume", label: <>Resume <span className="menu-sub">on hold: {d.hold}</span></>, icon: <IconPlay size={14} />, onPick: () => void hub.resume(d.id).catch(() => {}) }
      : { key: "hold", label: "Hold…", icon: <IconPause size={14} />, onPick: () => hub.setDialog({ kind: "hold", id: d.id }) },
    { key: "copy", label: "Copy ID", icon: <IconCopy size={14} />, onPick: () => navigator.clipboard?.writeText(d.id) },
  );
  if (canStartAgent(d)) items.splice(1, 0, { key: "start-agent", label: startLabel(d), icon: <IconAgents size={14} />, onPick: () => hub.setDialog({ kind: "agent", id: d.id }) });
  return items;
}

/** Whether a docket can take an agent now: open, and not a Review's. One with agents on it takes another beside them. */
export const canStartAgent = (d: Docket) => (d.status === "todo" || d.status === "in_progress") && d.protocol !== "review" && !d.review;

/** "Start an agent…", or with agents on it already, "Add an agent…". */
export const startLabel = (d: Docket) => (liveAgents(d).length ? "Add an agent…" : "Start an agent…");

// ---- grouping ----

export type GroupBy = "attention" | "status" | "session";

type Group = { key: string; label: string; icon: ReactNode; items: Docket[] };

const ATTENTION: { key: string; label: string; icon: ReactNode; test: (d: Docket) => boolean }[] = [
  { key: "blocked", label: "Needs you", icon: <LiveIcon state="blocked" />, test: (d) => d.live?.state === "blocked" },
  { key: "error", label: "Failed", icon: <LiveIcon state="error" />, test: (d) => d.live?.state === "error" },
  { key: "working", label: "Working", icon: <LiveIcon state="working" />, test: (d) => d.live?.state === "working" || d.live?.state === "starting" },
  { key: "done", label: "Finished, your move", icon: <LiveIcon state="done" />, test: (d) => d.live?.state === "done" },
  { key: "idle", label: "Idle", icon: <LiveIcon state="idle" />, test: (d) => !!d.live },
  { key: "todo", label: "To do", icon: <StatusIcon status="todo" />, test: (d) => d.status === "todo" || d.status === "in_progress" },
  { key: "closed", label: "Done", icon: <StatusIcon status="done" />, test: (d) => d.status === "done" },
  { key: "dropped", label: "Dropped", icon: <StatusIcon status="dropped" />, test: () => true },
];

export function groupDockets(list: Docket[], by: GroupBy): Group[] {
  if (by === "status")
    return [
      ...["in_progress", "todo", "done", "dropped"].map((s) => ({
        key: s,
        label: STATUSES.find((x) => x.key === s)!.label,
        icon: <StatusIcon status={s} />,
        items: list.filter((d) => d.status === s),
      })),
    ].filter((g) => g.items.length > 0);
  if (by === "session") {
    const m = new Map<string, Docket[]>();
    for (const d of list) {
      const k = d.agent?.rex_session || "By hand";
      m.set(k, [...(m.get(k) ?? []), d]);
    }
    return [...m.entries()]
      .sort((a, b) => a[0].localeCompare(b[0]))
      .map(([k, items]) => ({ key: k, label: k, icon: <SessionIcon name={k} size={14} />, items }));
  }
  const groups: Group[] = ATTENTION.map((a) => ({ key: a.key, label: a.label, icon: a.icon, items: [] as Docket[] }));
  for (const d of list) {
    const i = ATTENTION.findIndex((a) => a.test(d));
    groups[i].items.push(d);
  }
  return groups.filter((g) => g.items.length > 0);
}

// ---- a list view ----

export function DocketListView({
  path,
  title,
  icon,
  filter,
  defaultGroup,
  empty,
}: {
  path: string;
  title: string;
  icon: ReactNode;
  filter: (d: Docket) => boolean;
  defaultGroup: GroupBy;
  empty: ReactNode;
}) {
  const hub = useHub();
  const pane = usePane();
  const [savedGroup, saveGroup] = store<GroupBy>(`group:${path}`, defaultGroup);
  const [savedLayout, saveLayout] = store<"list" | "board">(`layout:${path}`, "list");
  const [group, setGroup] = useState<GroupBy>(savedGroup);
  const [layout, setLayout] = useState(savedLayout);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [sel, setSel] = useState<string | null>(null);
  const menu = useMenu();
  const phone = usePhone();
  // A phone shows the list; the board needs the width.
  const shown = phone ? "list" : layout;

  const list = useMemo(() => (hub.state?.dockets ?? []).filter(filter), [hub.state, filter]);
  const groups = useMemo(() => groupDockets(list, group), [list, group]);
  const visible = useMemo(() => groups.flatMap((g) => (collapsed.has(g.key) ? [] : g.items)), [groups, collapsed]);
  const idch = Math.max(3, ...list.map((d) => d.id.length));

  useEffect(() => {
    lastList.path = path;
    lastList.title = title;
    lastList.ids = visible.map((d) => d.id);
  }, [visible, path, title]);

  const move = (delta: number) => {
    if (visible.length === 0) return;
    const i = sel ? visible.findIndex((d) => d.id === sel) : -1;
    const next = visible[Math.max(0, Math.min(visible.length - 1, i < 0 ? 0 : i + delta))];
    setSel(next.id);
    document.querySelector(`[data-id="${next.id}"]`)?.scrollIntoView({ block: "nearest" });
  };
  const current = sel ? hub.byId.get(sel) : undefined;
  useKeys({
    j: () => move(1),
    ArrowDown: () => move(1),
    k: () => move(-1),
    ArrowUp: () => move(-1),
    Enter: () => current && go(`/d/${current.id}`),
    o: () => current?.live && pane.act(current.id),
    s: () => {
      if (!current) return;
      const el = document.querySelector<HTMLElement>(`[data-id="${current.id}"] .row-status`);
      if (el) menu.open(el, statusItems(current, (s) => hub.setStatus(current.id, s)), "Change status");
    },
  });

  const displayMenu = (el: HTMLElement) =>
    menu.open(
      el,
      [
        { key: "attention", label: "Group by attention", checked: group === "attention", onPick: () => (setGroup("attention"), saveGroup("attention")) },
        { key: "status", label: "Group by status", checked: group === "status", onPick: () => (setGroup("status"), saveGroup("status")) },
        { key: "session", label: "Group by session", checked: group === "session", onPick: () => (setGroup("session"), saveGroup("session")) },
      ],
      "Display",
    );

  return (
    <div className="view">
      <header className="view-head">
        <span className="view-title">
          {icon}
          {title}
          <span className="view-count">{list.length}</span>
        </span>
        <span className="spacer" />
        <div className="seg">
          <button className={layout === "list" ? "on" : ""} title="List" onClick={() => (setLayout("list"), saveLayout("list"))}>
            <IconList size={14} />
          </button>
          <button className={layout === "board" ? "on" : ""} title="Board" onClick={() => (setLayout("board"), saveLayout("board"))}>
            <IconBoard size={14} />
          </button>
        </div>
        <button className="btn btn-ghost" onClick={(e) => displayMenu(e.currentTarget)} aria-label="Display">
          <IconDisplay size={14} /> <span className="m-hide">Display</span>
        </button>
      </header>
      <LargeTitle title={title} count={list.length} />

      {!hub.state ? (
        <div className="view-empty">{hub.error ? `Can't reach the hub: ${hub.error}` : ""}</div>
      ) : list.length === 0 ? (
        <div className="view-empty">{empty}</div>
      ) : shown === "board" ? (
        <div className="board">
          {groups.map((g) => (
            <div className="board-col" key={g.key}>
              <div className="board-col-head">
                {g.icon}
                <span>{g.label}</span>
                <span className="view-count">{g.items.length}</span>
              </div>
              <div className="board-cards">
                {g.items.map((d) => (
                  <Card key={d.id} d={d} onMenu={(e) => (e.preventDefault(), menu.open({ x: e.clientX, y: e.clientY }, rowMenu(d, hub, pane)))} />
                ))}
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="list" style={{ ["--idch" as string]: idch }}>
          {groups.map((g) => (
            <div key={g.key}>
              <button
                className="group-head"
                onClick={() =>
                  setCollapsed((c) => {
                    const n = new Set(c);
                    if (n.has(g.key)) n.delete(g.key);
                    else n.add(g.key);
                    return n;
                  })
                }
              >
                <span className={`caret${collapsed.has(g.key) ? " closed" : ""}`}>
                  <IconChevronDown size={12} />
                </span>
                {g.icon}
                <span className="group-label">{g.label}</span>
                <span className="view-count">{g.items.length}</span>
              </button>
              {!collapsed.has(g.key) &&
                g.items.map((d) => {
                  const row = (
                    <Row
                      key={d.id}
                      d={d}
                      selected={!phone && sel === d.id}
                      onHover={() => setSel(d.id)}
                      onStatus={(el) => menu.open(el, statusItems(d, (s) => hub.setStatus(d.id, s, d.status)), "Change status")}
                      onMenu={(e) => {
                        e.preventDefault();
                        setSel(d.id);
                        menu.open({ x: e.clientX, y: e.clientY }, rowMenu(d, hub, pane));
                      }}
                    />
                  );
                  return phone ? (
                    <SwipeRow
                      key={d.id}
                      leading={[{ key: "status", label: "Status", icon: <StatusIcon status="in_progress" />, tone: "status", run: () => menu.open(new DOMRect(), statusItems(d, (s) => hub.setStatus(d.id, s, d.status)), `${d.id} · Change status`) }]}
                      trailing={swipeEnd(d, hub)}
                      fullTrailing
                      onLongPress={() => menu.open(new DOMRect(), rowMenu(d, hub, pane), d.id)}
                    >
                      {row}
                    </SwipeRow>
                  ) : (
                    row
                  );
                })}
            </div>
          ))}
        </div>
      )}
      {menu.node}
    </div>
  );
}

/** A row's swipe to the left: Done (or Reopen), then Drop. */
export function swipeEnd(d: Docket, hub: ReturnType<typeof useHub>): SwipeAction[] {
  const open = d.status === "todo" || d.status === "in_progress";
  if (!open) return [{ key: "reopen", label: "Reopen", icon: <StatusIcon status="in_progress" />, tone: "status", run: () => void hub.setStatus(d.id, "in_progress", d.status) }];
  return [
    { key: "done", label: "Done", icon: <IconCheck size={18} />, tone: "done", run: () => void hub.setStatus(d.id, "done", d.status) },
    { key: "drop", label: "Drop", icon: <IconX size={18} />, tone: "drop", run: () => void hub.setStatus(d.id, "dropped", d.status) },
  ];
}

export function Row({
  d,
  selected,
  onHover,
  onStatus,
  onMenu,
}: {
  d: Docket;
  selected?: boolean;
  onHover?: () => void;
  onStatus?: (el: HTMLElement) => void;
  onMenu?: (e: React.MouseEvent) => void;
}) {
  const where = d.agent?.rex_session;
  const statusRef = useRef<HTMLButtonElement>(null);
  return (
    <a className={`row${selected ? " sel" : ""}`} href={`#/d/${d.id}`} data-id={d.id} onMouseMove={onHover} onContextMenu={onMenu}>
      <span className="row-live" title={liveWord(d.live?.state)}>
        <LiveIcon state={d.live?.state} />
      </span>
      <span className="row-id">{d.id}</span>
      <button
        ref={statusRef}
        className="row-status"
        title="Change status (S)"
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          if (statusRef.current) onStatus?.(statusRef.current);
        }}
      >
        <StatusIcon status={d.status} />
      </button>
      <span className="row-title">
        <TitleText d={d} />
      </span>
      {d.live?.state === "blocked" && d.live.msg && <span className="row-note">{d.live.msg}</span>}
      <span className="spacer" />
      <span className="row-end">
        {d.turns > 0 && (
          <span className="chip-mini" title={`${d.prompts} prompts, ${d.turns} turns`}>
            {d.turns} turn{d.turns === 1 ? "" : "s"}
          </span>
        )}
        {where && <SessionChip name={where} />}
        {d.agent ? <AppAvatar app={d.agent.app} /> : <span className="avatar avatar-none" />}
        <span className="row-age" title={d.updated_at}>
          {ago(d.updated_at)}
        </span>
      </span>
    </a>
  );
}

function Card({ d, onMenu }: { d: Docket; onMenu: (e: React.MouseEvent) => void }) {
  return (
    <a className="card" href={`#/d/${d.id}`} onContextMenu={onMenu}>
      <div className="card-top">
        <span className="row-id">{d.id}</span>
        <span className="spacer" />
        {d.agent && <AppAvatar app={d.agent.app} size={16} />}
      </div>
      <div className="card-title">
        <TitleText d={d} />
      </div>
      <div className="card-foot">
        <StatusIcon status={d.status} size={13} />
        {d.live && (
          <span className="card-live">
            <LiveIcon state={d.live.state} size={12} />
            {liveWord(d.live.state)}
          </span>
        )}
        <span className="spacer" />
        {d.agent?.rex_session && <SessionChip name={d.agent.rex_session} />}
      </div>
    </a>
  );
}

// ---- agents ----

const AGENTS_BY = "wh.agents.by";

export function AgentsView() {
  const hub = useHub();
  const pane = usePane();
  const menu = useMenu();
  // By the runtime session each pane is in, or by the docket each is on.
  const [by, setBy] = useState<"session" | "docket">(() => {
    try {
      return localStorage.getItem(AGENTS_BY) === "docket" ? "docket" : "session";
    } catch {
      return "session";
    }
  });
  const pick = (b: "session" | "docket") => {
    setBy(b);
    try {
      localStorage.setItem(AGENTS_BY, b);
    } catch {
      /* a private window: it's only for this visit */
    }
  };
  const groups = useMemo(() => {
    const m = new Map<string, NonNullable<typeof hub.state>["agents"]>();
    const keyOf = (a: NonNullable<typeof hub.state>["agents"][number]) => (by === "docket" ? a.docket || "—" : a.rex_session || "—");
    for (const a of hub.state?.agents ?? []) m.set(keyOf(a), [...(m.get(keyOf(a)) ?? []), a]);
    // Dockets newest first, those with the most agents on top; sessions by name.
    const n = (k: string) => Number(k.replace(/^D-/, "")) || 0;
    return [...m.entries()].sort((a, b) => (by === "docket" ? b[1].length - a[1].length || n(b[0]) - n(a[0]) : a[0].localeCompare(b[0])));
  }, [hub.state, by]);
  const total = hub.state?.agents.length ?? 0;
  return (
    <div className="view">
      <header className="view-head">
        <span className="view-title">
          <IconTerminal size={15} /> Agents <span className="view-count">{total}</span>
        </span>
        <span className="spacer" />
        <span className="muted small m-hide">Every Claude Code, Codex and Antigravity pane in {followedRuntimes(hub.state)}, live</span>
        <span className="seg seg-text" role="tablist" aria-label="Group by">
          <button className={by === "session" ? "on" : ""} onClick={() => pick("session")} role="tab" aria-selected={by === "session"}>
            Session
          </button>
          <button className={by === "docket" ? "on" : ""} onClick={() => pick("docket")} role="tab" aria-selected={by === "docket"}>
            Docket
          </button>
        </span>
      </header>
      <LargeTitle title="Agents" count={total} />
      {total === 0 ? (
        <div className="view-empty">No agents running in {followedRuntimes(hub.state)}.</div>
      ) : (
        <div className="list" style={{ ["--idch" as string]: Math.max(3, ...(hub.state?.agents ?? []).map((a) => (a.docket || "").length)) }}>
          {groups.map(([session, list]) => (
            <div key={session}>
              <div className="group-head static">
                {by === "docket" ? (
                  <>
                    <span className="row-id">{session}</span>
                    <span className="group-label">{hub.byId.get(session)?.title ?? (session === "—" ? "On no docket" : "")}</span>
                  </>
                ) : (
                  <>
                    <SessionIcon name={session} size={14} />
                    <span className="group-label">{session}</span>
                  </>
                )}
                <span className="view-count">{list.length}</span>
                {[...new Set(list.map((a) => runtimeTitle(a.runtime ?? runtimeOf(a.block))))].sort().map((r) => (
                  <span key={r} className="runtime-tag" title={`In ${r}`}>
                    {r}
                  </span>
                ))}
              </div>
              {list.map((a) => {
                const d = a.docket ? hub.byId.get(a.docket) : undefined;
                return (
                  <a
                    key={a.block}
                    className="row agent-row"
                    href={a.docket ? `#/d/${a.docket}` : undefined}
                    onContextMenu={(e) => {
                      if (!d) return;
                      e.preventDefault();
                      menu.open({ x: e.clientX, y: e.clientY }, rowMenu(d, hub, pane));
                    }}
                  >
                    <span className="row-live">
                      <LiveIcon state={a.state} />
                    </span>
                    <span className="row-id">{by === "docket" ? "" : a.docket || "—"}</span>
                    <AppAvatar app={a.app} />
                    <span className="row-title">
                      {by === "docket" ? (
                        <>
                          {a.role || appName(a.app)}
                          <span className="muted">
                            {" "}
                            · {a.rex_session || "—"} · {runtimeTitle(a.runtime ?? runtimeOf(a.block))}
                          </span>
                        </>
                      ) : d ? (
                        <TitleText d={d} />
                      ) : (
                        appName(a.app)
                      )}
                      {by === "session" && a.role ? <span className="muted"> · {a.role}</span> : null}
                    </span>
                    <span className={`agent-state state-${a.state}`} title={a.msg || ""}>
                      {a.state === "blocked" && a.msg ? a.msg : liveWord(a.state)}
                    </span>
                    <span className="agent-pane" title={`Pane ${a.pane}`}>
                      {a.pane || "pane"}
                    </span>
                    <span className="row-path" title={a.cwd || ""}>
                      {shortPath(a.cwd)}
                    </span>
                    <button
                      className="icon-btn"
                      title={onThisMac() ? `Show pane in ${runtimeTitle(a.runtime ?? runtimeOf(a.block))}` : "View pane"}
                      onClick={(e) => {
                        e.preventDefault();
                        if (a.docket) pane.act(a.docket, a.block);
                      }}
                    >
                      {onThisMac() ? <IconTerminal size={14} /> : <IconEye size={14} />}
                    </button>
                  </a>
                );
              })}
            </div>
          ))}
        </div>
      )}
      {menu.node}
    </div>
  );
}

