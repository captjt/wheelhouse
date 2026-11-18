import { useEffect, useMemo, useState } from "react";
import { appName, type Attention, type Docket, type Msg, type ReviewPr } from "./api";
import { AttentionIcon } from "./Verify";
import { attentionShown } from "./evidence";
import { ReviewActions, draftLine } from "./Reviews";
import { MessageCard, msgFrom, msgTitle } from "./Messages";
import { DocketScreen } from "./Docket";
import { AppAvatar, IconInbox, IconPR, LiveIcon, TitleText } from "./icons";
import { ago, go, lastList, useHub } from "./live";
import { InlineMd, useKeys, useMenu, usePhone } from "./ui";
import { rowMenu, swipeEnd } from "./views";
import { LargeTitle, SwipeRow, type SwipeAction } from "./mobile";
import { IconCheck, IconX } from "./icons";
import { AnswerButtons, usePane } from "./Pane";

type Item = { d: Docket; kind: "blocked" | "error" | "done" | "message" | "review"; line: string; at?: string | null; m?: Msg; p?: ReviewPr; key: string };

const ATTENTION: Record<string, string> = {
  ready: "Ready to post",
  blocked: "Blocked",
  "start-failed": "Didn't start",
  stalled: "Stalled",
  "pane-closed": "Pane closed",
  error: "Failed",
};

/** What needs you, newest worry first: waiting on you, failed, then agents
 * that finished a turn and wait for your next word. */
const placeholder = (id: string, title: string, at: string): Docket =>
  ({ id, title, title_source: "person", status: "in_progress", protocol: "none", prompts: 0, turns: 0, notes: 0, created_at: at, updated_at: at, entries: 0, head: "", short_head: "", live: null, quick: false }) as Docket;

export function inboxItems(dockets: Docket[], messages: Msg[] = [], prs: ReviewPr[] = []): Item[] {
  const items: Item[] = [];
  const byId = new Map(dockets.map((d) => [d.id, d]));
  for (const p of prs) {
    if (!p.attention) continue;
    const d = (p.docket && byId.get(p.docket)) || placeholder(p.key, `Review ${p.key}${p.title ? `: ${p.title}` : ""}`, p.updated_at || "");
    const what = p.attention.kind === "ready" ? draftLine(p) ?? p.attention.text : p.attention.text;
    items.push({ d, kind: "review", p, line: `${ATTENTION[p.attention.kind] ?? "Needs you"} (round ${p.attention.round}): ${what}`, at: p.updated_at, key: `rv:${p.full_key}` });
  }
  for (const m of messages) {
    const d = (m.docket && byId.get(m.docket)) || ({ id: m.docket || `#${m.id}`, title: msgTitle(m), title_source: "person", status: "in_progress", protocol: "none", prompts: 0, turns: 0, notes: 0, created_at: m.created_at, updated_at: m.created_at, entries: 0, head: "", short_head: "", live: null, quick: false } as Docket);
    items.push({ d, kind: "message", m, line: `${msgFrom(m)}: ${msgTitle(m)}`, at: m.created_at, key: `m${m.id}` });
  }
  for (const d of dockets) {
    const s = d.live?.state;
    // A turn finished, or a failure, on a docket you've closed isn't your
    // move; an agent waiting on a prompt still is.
    const open = d.status === "todo" || d.status === "in_progress";
    if (s === "blocked") items.push({ d, kind: "blocked", line: d.live?.msg || `${appName(d.agent?.app)} needs you`, at: d.live?.since, key: d.id });
    else if (s === "error" && open) items.push({ d, kind: "error", line: d.live?.msg || "Failed", at: d.live?.since, key: d.id });
    else if (s === "done" && open)
      items.push({ d, kind: "done", line: firstLine(d.last_turn) || "Finished its turn", at: d.live?.since || d.updated_at, key: d.id });
  }
  const rank = { blocked: 0, review: 1, message: 2, error: 3, done: 4 };
  return items.sort((a, b) => rank[a.kind] - rank[b.kind] || (b.at || "").localeCompare(a.at || ""));
}

/** An attention item's key in the list. */
const attKey = (a: Attention) => `at:${a.docket ?? a.title}:${a.kind}`;

function firstLine(s?: string) {
  return s
    ?.split("\n")
    .map((l) => l.replace(/^[#>*\-\s]+/, "").replace(/\*\*/g, "").trim())
    .find(Boolean);
}

export function InboxView() {
  const hub = useHub();
  const pane = usePane();
  const items = useMemo(() => inboxItems(hub.state?.dockets ?? [], hub.state?.messages ?? [], hub.state?.review?.prs ?? []), [hub.state]);
  // Not asks, but worth a look: held, failing, quiet, closed on your word.
  const att = useMemo(() => attentionShown(hub.state?.attention, items.map((i) => ({ docket: i.d.id, kind: i.kind }))), [hub.state, items]);
  const [sel, setSel] = useState<string | null>(null);
  const menu = useMenu();
  const phone = usePhone();
  const selAtt = att.find((a) => attKey(a) === sel);
  const current = selAtt ? undefined : (items.find((i) => i.key === sel) ?? items[0]);
  const shownAtt = selAtt ?? (current ? undefined : att[0]);
  const currentKey = current?.key ?? (shownAtt ? attKey(shownAtt) : undefined);
  const currentId = current?.d.id ?? shownAtt?.docket ?? undefined;
  const keys = [...items.map((i) => i.key), ...att.map(attKey)];
  // On a phone each kind swipes to what it's waiting for: a message files,
  // a prompt approves or denies (a tap, never a full swipe), and a docket's
  // turn or failure closes.
  const swipes = (it: Item): { leading: SwipeAction[]; trailing: SwipeAction[]; full: boolean } => {
    if (it.kind === "message" && it.m) {
      const id = it.m.id;
      return { leading: [], trailing: [{ key: "file", label: "File", icon: <IconCheck size={18} />, tone: "done", run: () => void hub.fileMessage(id) }], full: true };
    }
    if (it.kind === "review") return { leading: [], trailing: [], full: false };
    const leading: SwipeAction[] =
      it.kind === "blocked" && it.d.live?.answerable
        ? [
            { key: "approve", label: "Approve", icon: <IconCheck size={18} />, tone: "approve", run: () => void hub.answer(it.d, true) },
            { key: "deny", label: "Deny", icon: <IconX size={18} />, tone: "deny", run: () => void hub.answer(it.d, false) },
          ]
        : [];
    return { leading, trailing: it.d.entries ? swipeEnd(it.d, hub).slice(0, 1) : [], full: it.kind !== "blocked" };
  };

  useEffect(() => {
    lastList.path = "/inbox";
    lastList.title = "Inbox";
    lastList.ids = [...new Set([...items.map((i) => i.d.id), ...att.flatMap((a) => (a.docket ? [a.docket] : []))])];
  }, [items, att]);

  const move = (delta: number) => {
    if (!keys.length) return;
    const i = Math.max(0, keys.indexOf(currentKey ?? ""));
    const next = keys[Math.max(0, Math.min(keys.length - 1, i + delta))];
    setSel(next);
    document.querySelector(`[data-inbox="${next}"]`)?.scrollIntoView({ block: "nearest" });
  };
  useKeys({
    j: () => move(1),
    ArrowDown: () => move(1),
    k: () => move(-1),
    ArrowUp: () => move(-1),
    Enter: () => currentId && go(`/d/${currentId}`),
    o: () => currentId && pane.act(currentId),
    e: () => currentId && hub.setStatus(currentId, "done"),
    // A prompt: approve or deny it. A message with options: 1 to 6 picks one.
    a: () => current && current.kind !== "message" && current.d.live?.answerable && void hub.answer(current.d, true),
    d: () => current && current.kind !== "message" && current.d.live?.answerable && void hub.answer(current.d, false),
    ...Object.fromEntries(
      [1, 2, 3, 4, 5, 6].map((n) => [String(n), () => current?.m && current.m.options[n - 1] !== undefined && void hub.replyTo(current.m.id, "", current.m.options[n - 1])]),
    ),
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
      <LargeTitle title="Inbox" count={items.length} />
      {items.length === 0 && att.length === 0 ? (
        <div className="view-empty inbox-zero">
          <IconInbox size={28} />
          <div className="inbox-zero-title">You're all caught up</div>
          <div className="muted">Nothing is waiting on you. Agents that block, fail, or finish a turn show up here.</div>
        </div>
      ) : (
        <div className="split">
          <div className="split-list">
            {items.length === 0 && <div className="inbox-none muted small">Nothing is waiting on your answer.</div>}
            {items.map((it) => {
              const el = (
              <a
                key={it.key}
                data-inbox={it.key}
                className={`inbox-item${currentKey === it.key ? " sel" : ""} kind-${it.kind}`}
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
                  {it.p ? (
                    <span className="inbox-pr">
                      <IconPR size={14} />
                    </span>
                  ) : (
                    <AppAvatar app={it.d.agent?.app} size={22} />
                  )}
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
                    <InlineMd text={it.line} links={false} />
                  </span>
                  {it.p?.attention?.kind === "ready" && (
                    <span className="inbox-actions">
                      <ReviewActions p={it.p} small />
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
              );
              if (!phone) return el;
              const sw = swipes(it);
              return (
                <SwipeRow key={it.key} leading={sw.leading} trailing={sw.trailing} fullTrailing={sw.full} onLongPress={it.d.entries ? () => menu.open(new DOMRect(), rowMenu(it.d, hub, pane), it.d.id) : undefined}>
                  {el}
                </SwipeRow>
              );
            })}
            {att.length > 0 && (
              <>
                <div className="group-head static inbox-group">
                  <span className="group-label">Attention</span>
                  <span className="view-count">{att.length}</span>
                  <span className="spacer" />
                  <span className="muted small">Not asks, but worth a look</span>
                </div>
                {att.map((a) => {
                  const k = attKey(a);
                  const d = a.docket ? hub.byId.get(a.docket) : undefined;
                  return (
                    <a
                      key={k}
                      data-inbox={k}
                      className={`inbox-item att-item att-kind-${a.kind}${currentKey === k ? " sel" : ""}`}
                      href={a.docket ? `#/d/${a.docket}` : "#/reviews"}
                      onClick={(e) => {
                        e.preventDefault();
                        if (!a.docket) return go("/reviews");
                        if (window.matchMedia("(max-width: 760px)").matches || !d) go(`/d/${a.docket}`);
                        else setSel(k);
                      }}
                      onDoubleClick={() => a.docket && go(`/d/${a.docket}`)}
                      onContextMenu={(e) => {
                        if (!d) return;
                        e.preventDefault();
                        menu.open({ x: e.clientX, y: e.clientY }, rowMenu(d, hub, pane));
                      }}
                    >
                      <span className="inbox-avatar att-avatar">
                        <AttentionIcon kind={a.kind} size={14} />
                      </span>
                      <span className="inbox-main">
                        <span className="inbox-top">
                          <span className="inbox-title">{d ? <TitleText d={d} /> : a.title}</span>
                          <span className="inbox-age">{ago(a.at)}</span>
                        </span>
                        <span className="inbox-line">
                          {a.docket && <span className="row-id">{a.docket}</span>}
                          <InlineMd text={a.text} links={false} />
                        </span>
                      </span>
                    </a>
                  );
                })}
              </>
            )}
          </div>
          <div className="split-detail">
            {shownAtt?.docket && hub.byId.get(shownAtt.docket) ? (
              <DocketScreen key={shownAtt.docket} id={shownAtt.docket} embedded />
            ) : current && current.d.entries > 0 ? (
              <DocketScreen key={current.d.id} id={current.d.id} embedded />
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
