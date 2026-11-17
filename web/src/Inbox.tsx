import { useEffect, useMemo, useState } from "react";
import { appName, type Docket } from "./api";
import { DocketScreen } from "./Docket";
import { AppAvatar, IconInbox, LiveIcon, TitleText } from "./icons";
import { ago, go, lastList, useHub } from "./live";
import { useKeys, useMenu } from "./ui";
import { rowMenu } from "./views";
import { AnswerButtons, usePane } from "./Pane";

type Item = { d: Docket; kind: "blocked" | "error" | "done"; line: string; at?: string | null };

/** What needs you, newest worry first: waiting on you, failed, then agents
 * that finished a turn and wait for your next word. */
export function inboxItems(dockets: Docket[]): Item[] {
  const items: Item[] = [];
  for (const d of dockets) {
    const s = d.live?.state;
    if (s === "blocked") items.push({ d, kind: "blocked", line: d.live?.msg || `${appName(d.agent?.app)} needs you`, at: d.live?.since });
    else if (s === "error") items.push({ d, kind: "error", line: d.live?.msg || "Failed", at: d.live?.since });
    else if (s === "done") items.push({ d, kind: "done", line: firstLine(d.last_turn) || "Finished its turn", at: d.live?.since || d.updated_at });
  }
  const rank = { blocked: 0, error: 1, done: 2 };
  return items.sort((a, b) => rank[a.kind] - rank[b.kind] || (b.at || "").localeCompare(a.at || ""));
}

function firstLine(s?: string) {
  return s
    ?.split("\n")
    .map((l) => l.replace(/^[#>*\-\s]+/, "").replace(/\*\*/g, "").trim())
    .find(Boolean);
}

export function InboxView() {
  const hub = useHub();
  const pane = usePane();
  const items = useMemo(() => inboxItems(hub.state?.dockets ?? []), [hub.state]);
  const [sel, setSel] = useState<string | null>(null);
  const menu = useMenu();
  const current = items.find((i) => i.d.id === sel) ?? items[0];

  useEffect(() => {
    lastList.path = "/inbox";
    lastList.title = "Inbox";
    lastList.ids = items.map((i) => i.d.id);
  }, [items]);

  const move = (delta: number) => {
    if (!items.length) return;
    const i = Math.max(0, items.findIndex((x) => x.d.id === current?.d.id));
    const next = items[Math.max(0, Math.min(items.length - 1, i + delta))];
    setSel(next.d.id);
    document.querySelector(`[data-inbox="${next.d.id}"]`)?.scrollIntoView({ block: "nearest" });
  };
  useKeys({
    j: () => move(1),
    ArrowDown: () => move(1),
    k: () => move(-1),
    ArrowUp: () => move(-1),
    Enter: () => current && go(`/d/${current.d.id}`),
    o: () => current && pane.act(current.d.id),
    e: () => current && hub.setStatus(current.d.id, "done"),
  });

  const counts = { blocked: items.filter((i) => i.kind === "blocked").length, error: items.filter((i) => i.kind === "error").length };
  return (
    <div className="view inbox">
      <header className="view-head">
        <span className="view-title">
          <IconInbox size={15} /> Inbox <span className="view-count">{items.length}</span>
        </span>
        <span className="spacer" />
        {counts.blocked > 0 && <span className="inbox-sum blocked">{counts.blocked} waiting on you</span>}
        {counts.error > 0 && <span className="inbox-sum error">{counts.error} failed</span>}
      </header>
      {items.length === 0 ? (
        <div className="view-empty inbox-zero">
          <IconInbox size={28} />
          <div className="inbox-zero-title">You're all caught up</div>
          <div className="muted">Nothing is waiting on you. Agents that block, fail, or finish a turn show up here.</div>
        </div>
      ) : (
        <div className="split">
          <div className="split-list">
            {items.map((it) => (
              <a
                key={it.d.id}
                data-inbox={it.d.id}
                className={`inbox-item${current?.d.id === it.d.id ? " sel" : ""} kind-${it.kind}`}
                href={`#/d/${it.d.id}`}
                onClick={(e) => {
                  e.preventDefault();
                  // A phone has no room for the detail beside the list: open the docket.
                  if (window.matchMedia("(max-width: 760px)").matches) go(`/d/${it.d.id}`);
                  else setSel(it.d.id);
                }}
                onDoubleClick={() => go(`/d/${it.d.id}`)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  menu.open({ x: e.clientX, y: e.clientY }, rowMenu(it.d, hub, pane));
                }}
              >
                <span className="inbox-avatar">
                  <AppAvatar app={it.d.agent?.app} size={22} />
                  <span className="inbox-badge">
                    <LiveIcon state={it.kind} size={11} />
                  </span>
                </span>
                <span className="inbox-main">
                  <span className="inbox-top">
                    <span className="inbox-title">
                      <TitleText d={it.d} />
                    </span>
                    <span className="inbox-age">{ago(it.at)}</span>
                  </span>
                  <span className="inbox-line">
                    <span className="row-id">{it.d.id}</span>
                    {it.kind === "blocked" ? (it.d.live?.kind ? `${it.d.live.kind}: ` : "Needs you: ") : it.kind === "error" ? "Failed: " : ""}
                    {it.line}
                  </span>
                  {it.d.live?.answerable && (
                    <span className="inbox-actions">
                      <AnswerButtons d={it.d} small />
                    </span>
                  )}
                </span>
              </a>
            ))}
          </div>
          <div className="split-detail">{current && <DocketScreen key={current.d.id} id={current.d.id} embedded />}</div>
        </div>
      )}
      {menu.node}
    </div>
  );
}
