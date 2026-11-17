import { useEffect, useMemo, useState } from "react";
import { appName, type Docket, type Msg, type ReviewReady } from "./api";
import { ReviewButtons } from "./Reviews";
import { MessageCard, msgFrom, msgTitle } from "./Messages";
import { DocketScreen } from "./Docket";
import { AppAvatar, IconInbox, LiveIcon, TitleText } from "./icons";
import { ago, go, lastList, useHub } from "./live";
import { useKeys, useMenu } from "./ui";
import { rowMenu } from "./views";
import { AnswerButtons, usePane } from "./Pane";

type Item = { d: Docket; kind: "blocked" | "error" | "done" | "message" | "review"; line: string; at?: string | null; m?: Msg; r?: ReviewReady; key: string };

/** What needs you, newest worry first: waiting on you, failed, then agents
 * that finished a turn and wait for your next word. */
const placeholder = (id: string, title: string, at: string): Docket =>
  ({ id, title, title_source: "person", status: "in_progress", protocol: "none", prompts: 0, turns: 0, notes: 0, created_at: at, updated_at: at, entries: 0, head: "", short_head: "", live: null, quick: false }) as Docket;

export function inboxItems(dockets: Docket[], messages: Msg[] = [], reviews: ReviewReady[] = []): Item[] {
  const items: Item[] = [];
  const byId = new Map(dockets.map((d) => [d.id, d]));
  for (const r of reviews) {
    const d = (r.docket && byId.get(r.docket)) || placeholder(r.key, `Review ${r.key}${r.title ? `: ${r.title}` : ""}`, r.updated_at || "");
    items.push({ d, kind: "review", r, line: `Review ready${r.round ? ` (round ${r.round})` : ""}: ${r.msg ?? ""}`, at: r.updated_at, key: `r:${r.item}` });
  }
  for (const m of messages) {
    const d = (m.docket && byId.get(m.docket)) || ({ id: m.docket || `#${m.id}`, title: msgTitle(m), title_source: "person", status: "in_progress", protocol: "none", prompts: 0, turns: 0, notes: 0, created_at: m.created_at, updated_at: m.created_at, entries: 0, head: "", short_head: "", live: null, quick: false } as Docket);
    items.push({ d, kind: "message", m, line: `${msgFrom(m)}: ${msgTitle(m)}`, at: m.created_at, key: `m${m.id}` });
  }
  for (const d of dockets) {
    const s = d.live?.state;
    if (s === "blocked") items.push({ d, kind: "blocked", line: d.live?.msg || `${appName(d.agent?.app)} needs you`, at: d.live?.since, key: d.id });
    else if (s === "error") items.push({ d, kind: "error", line: d.live?.msg || "Failed", at: d.live?.since, key: d.id });
    else if (s === "done") items.push({ d, kind: "done", line: firstLine(d.last_turn) || "Finished its turn", at: d.live?.since || d.updated_at, key: d.id });
  }
  const rank = { blocked: 0, review: 1, message: 2, error: 3, done: 4 };
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
  const items = useMemo(() => inboxItems(hub.state?.dockets ?? [], hub.state?.messages ?? [], hub.state?.reviews ?? []), [hub.state]);
  const [sel, setSel] = useState<string | null>(null);
  const menu = useMenu();
  const current = items.find((i) => i.key === sel) ?? items[0];

  useEffect(() => {
    lastList.path = "/inbox";
    lastList.title = "Inbox";
    lastList.ids = items.map((i) => i.d.id);
  }, [items]);

  const move = (delta: number) => {
    if (!items.length) return;
    const i = Math.max(0, items.findIndex((x) => x.key === current?.key));
    const next = items[Math.max(0, Math.min(items.length - 1, i + delta))];
    setSel(next.key);
    document.querySelector(`[data-inbox="${next.key}"]`)?.scrollIntoView({ block: "nearest" });
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
                key={it.key}
                data-inbox={it.key}
                className={`inbox-item${current?.key === it.key ? " sel" : ""} kind-${it.kind}`}
                href={`#/d/${it.d.id}`}
                onClick={(e) => {
                  e.preventDefault();
                  // A phone has no room for the detail beside the list: open the docket.
                  if (window.matchMedia("(max-width: 760px)").matches && it.d.entries) go(`/d/${it.d.id}`);
                  else setSel(it.key);
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
                    <LiveIcon state={it.kind === "message" || it.kind === "review" ? "blocked" : it.kind} size={11} />
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
                  {it.r && (
                    <span className="inbox-actions">
                      <ReviewButtons r={it.r} small />
                    </span>
                  )}
                  {it.m && it.m.options.length > 0 && (
                    <span className="inbox-actions">
                      <MessageCard m={it.m} compact />
                    </span>
                  )}
                  {it.kind !== "message" && it.d.live?.answerable && (
                    <span className="inbox-actions">
                      <AnswerButtons d={it.d} small />
                    </span>
                  )}
                </span>
              </a>
            ))}
          </div>
          <div className="split-detail">
            {current && current.d.entries > 0 ? (
              <DocketScreen key={current.d.id} id={current.d.id} embedded />
            ) : current?.r ? (
              <div className="docket embedded">
                <div className="docket-main">
                  <div className="docket-col">
                    <h1 className="title">{current.d.title}</h1>
                    <div className="callout callout-blocked">
                      <div>
                        <strong>Review ready to post</strong>
                        <div className="callout-msg">{current.r.msg}</div>
                      </div>
                      <span className="spacer" />
                      <ReviewButtons r={current.r} small />
                    </div>
                  </div>
                </div>
              </div>
            ) : current?.m ? (
              <div className="docket embedded">
                <div className="docket-main">
                  <div className="docket-col">
                    <MessageCard m={current.m} />
                  </div>
                </div>
              </div>
            ) : null}
          </div>
        </div>
      )}
      {menu.node}
    </div>
  );
}
