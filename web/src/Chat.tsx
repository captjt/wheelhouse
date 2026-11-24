import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { agentLabel, agentOfEntry, appName, entryBlock, get, liveAgents, paneRuntime, post, runtimeOf, runtimeTitle, type Docket, type DocketAgent, type EntryView, type Live } from "./api";
import { AppAvatar, IconDown, IconFile, IconSend, IconTerminal, LiveIcon, liveWord } from "./icons";
import { agoLong, stamp, useHub } from "./live";
import { MessageCard } from "./Messages";
import { AnswerButtons, usePane } from "./Pane";
import { Prose, isPhone, useToast } from "./ui";

/**
 * Talking to a docket's agent, as a conversation: what you asked, what it
 * answered (as it rendered), what it's doing now, and a box to send it the
 * next thing. While it works, what you send waits and goes in when the turn
 * ends; when it replies to something you sent from here, the reply comes to
 * your phone.
 */

type Item =
  | { t: "you"; seq: number; at: string; text: string; note?: boolean }
  | { t: "agent"; seq: number; at: string; text: string; app: string; who?: string }
  | { t: "ask"; seq: number; at: string; text: string; subject?: string; options: string[]; app: string; who?: string }
  | { t: "chose"; seq: number; at: string; text: string }
  | { t: "sys"; seq: number; at: string; text: string; tone?: string }
  | { t: "file"; seq: number; at: string; e: EntryView }
  | { t: "break"; seq: number; at: string; text: string };

function itemsOf(entries: EntryView[], dk: Docket): Item[] {
  const out: Item[] = [];
  // Each agent's session, by its pane: on a docket with several, one's new session isn't another's.
  const sessions = new Map<string, string>();
  const several = (dk.agents?.length ?? 0) > 1;
  for (const e of entries) {
    const d = e.data || {};
    const a = agentOfEntry(dk, e);
    const agentApp = e.actor.type === "agent" ? String(e.actor.id).split("@")[0] : a?.app || "agent";
    const who = several && a ? agentLabel(a) : undefined;
    switch (e.kind) {
      case "claim": {
        const s = String(d.session || "");
        const b = entryBlock(e) ?? "";
        const was = sessions.get(b);
        if (d.source === "join") out.push({ t: "break", seq: e.seq, at: e.at, text: `${appName(a?.app)}${who ? ` (${who})` : ""} joined` });
        else if (s && s !== was && was) out.push({ t: "break", seq: e.seq, at: e.at, text: `${who ? `${who}: ` : ""}${d.source === "resume" ? "Resumed an earlier session" : d.source === "clear" ? "A new session (/clear)" : "A new session"}` });
        if (s) sessions.set(b, s);
        break;
      }
      case "note":
        if (d.via === "prompt") out.push({ t: "you", seq: e.seq, at: e.at, text: e.body || "" });
        else if (d.message && e.actor.type === "agent") out.push({ t: "agent", seq: e.seq, at: e.at, text: `${d.subject ? `**${d.subject}**\n\n` : ""}${e.body || ""}`, app: agentApp, who });
        else if (e.actor.type === "person" && !d.message) out.push({ t: "you", seq: e.seq, at: e.at, text: e.body || "", note: true });
        break;
      case "question":
        out.push({ t: "ask", seq: e.seq, at: e.at, text: e.body || "", subject: d.subject, options: d.options || [], app: agentApp, who });
        break;
      case "decision":
        out.push({ t: "chose", seq: e.seq, at: e.at, text: e.body && e.body !== d.choice ? `${d.choice}: ${e.body}` : String(d.choice || "") });
        break;
      case "turn":
        if (e.body?.trim()) out.push({ t: "agent", seq: e.seq, at: e.at, text: e.body, app: a?.app || "agent", who });
        break;
      case "observed":
        if (d.what === "blocked") out.push({ t: "sys", seq: e.seq, at: e.at, text: `${who ? `${who} w` : "W"}aited on you${d.kind ? ` (${d.kind})` : ""}${e.body ? `: ${e.body}` : ""}`, tone: "blocked" });
        else if (d.what === "error") out.push({ t: "sys", seq: e.seq, at: e.at, text: `${who ? `${who} f` : "F"}ailed${e.body ? `: ${e.body}` : ""}`, tone: "error" });
        break;
      case "approval":
        if (!d.what && !d.kind) break;
        out.push({ t: "sys", seq: e.seq, at: e.at, text: `You ${d.decision === "approve" ? "approved" : "denied"} ${d.kind || "a prompt"}${e.body ? `: ${e.body}` : ""}` });
        break;
      case "artifact":
        out.push({ t: "file", seq: e.seq, at: e.at, e });
        break;
      case "release":
        out.push({ t: "break", seq: e.seq, at: e.at, text: who ? `${who} left: ${d.reason || "released"}` : `The session ended: ${d.reason || "released"}` });
        break;
    }
  }
  return out;
}

/** Long replies fold, so the conversation scrolls. */
function Fold({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [long, setLong] = useState(false);
  const [open, setOpen] = useState(false);
  useLayoutEffect(() => {
    if (ref.current) setLong(ref.current.scrollHeight > 520);
  }, [children]);
  return (
    <div className={`chat-fold${long && !open ? " folded" : ""}`}>
      <div ref={ref}>{children}</div>
      {long && (
        <button className="link chat-more" onClick={() => setOpen((o) => !o)}>
          {open ? "Show less" : "Show all"}
        </button>
      )}
    </div>
  );
}

const elapsed = (since?: string | null) => {
  if (!since) return "";
  const s = Math.max(0, (Date.now() - Date.parse(since)) / 1000);
  return s < 60 ? `${Math.floor(s)}s` : s < 3600 ? `${Math.floor(s / 60)}m` : `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
};

/** What an agent is doing now, at the end of the conversation: one line per live agent. */
function NowLine({ d, a, several, onTerminal }: { d: Docket; a?: DocketAgent; several?: boolean; onTerminal: () => void }) {
  const app = several && a ? `${appName(a.app)} (${agentLabel(a)})` : appName(a?.app ?? d.agent?.app);
  const l = a ? (a.live ?? null) : d.live;
  const q = a ? `?block=${encodeURIComponent(a.block)}` : "";
  const [peek, setPeek] = useState(false);
  const [tail, setTail] = useState<string>("");
  const [, tick] = useState(0);
  useEffect(() => {
    if (l?.state !== "working") return;
    const t = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(t);
  }, [l?.state]);
  useEffect(() => {
    if (!peek || !l) return;
    let live = true;
    let timer: number | undefined;
    const load = () =>
      get<{ text: string; live: Live | null }>(`/api/dockets/${d.id}/screen${q}`)
        .then((s) => live && setTail(s.text.replace(/\s+$/, "").split("\n").slice(-14).join("\n")))
        .catch(() => {})
        .finally(() => {
          if (live) timer = window.setTimeout(load, 2000);
        });
    load();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [peek, d.id, q, !!l]);
  if (!l)
    return (
      <div className="chat-now off">
        {d.released ? `The session ended (${d.released}).` : `${app} isn't running in ${paneRuntime(d)}.`} Start it again on the Mac to keep going.
      </div>
    );
  if (l.state === "blocked")
    return (
      <div className="chat-now blocked">
        <div className="chat-now-row">
          <LiveIcon state="blocked" size={16} />
          <div className="chat-now-text">
            <strong>
              {app} is asking{l.kind ? ` (${l.kind})` : ""}
            </strong>
            {l.msg && <div className="chat-now-msg">{l.msg}</div>}
          </div>
        </div>
        <div className="chat-now-actions">
          <AnswerButtons d={d} live={l} />
          <button className="btn" onClick={onTerminal}>
            <IconTerminal size={14} /> Terminal
          </button>
        </div>
      </div>
    );
  if (l.state === "error")
    return (
      <div className="chat-now error">
        <LiveIcon state="error" size={16} /> {app} stopped with an error{l.msg ? `: ${l.msg}` : "."}
        <button className="link" onClick={onTerminal}>
          Look at the terminal
        </button>
      </div>
    );
  if (l.state === "working" || l.state === "starting")
    return (
      <div className="chat-now working">
        <div className="chat-now-row">
          <span className="chat-dots" aria-hidden="true">
            <span />
            <span />
            <span />
          </span>
          <span>
            {app} is {l.state === "starting" ? "starting" : "working"}
            {l.since ? ` · ${elapsed(l.since)}` : ""}
          </span>
          <span className="spacer" />
          <button className="link" onClick={() => setPeek((p) => !p)}>
            {peek ? "Hide" : "Peek"}
          </button>
        </div>
        {peek && <pre className="chat-peek">{tail || "…"}</pre>}
      </div>
    );
  return null;
}

export function ChatView({
  d,
  entries,
  queued,
  reload,
  onArtifact,
}: {
  d: Docket;
  entries: EntryView[];
  queued: { id: number; body: string; at: string }[];
  reload: () => void;
  onArtifact: (e: EntryView) => void;
}) {
  const hub = useHub();
  const pane = usePane();
  const toast = useToast();
  const items = useMemo(() => itemsOf(entries, d), [entries, d]);
  // Who the box sends to: on a docket with several agents, the one you pick (its lead at first).
  const live = liveAgents(d);
  const several = live.length > 1;
  const [toBlock, setTo] = useState<string | null>(null);
  const target = live.find((a) => a.block === toBlock) ?? live[0];
  const app = several && target ? `${appName(target.app)} (${agentLabel(target)})` : appName(target?.app ?? d.agent?.app);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState<string[]>([]);
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const [below, setBelow] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const asks = (hub.state?.messages ?? []).filter((m) => m.docket === d.id);
  const tl = target?.live ?? (live.length ? null : d.live);
  const state = tl?.state;
  const canSend = !!tl && state !== "error";
  const willQueue = state === "working" || state === "starting" || state === "blocked";

  // What you sent shows at once, until the record has it.
  const pending = sent.filter((t) => !items.some((i) => i.t === "you" && i.text.trim() === t.trim()) && !queued.some((q) => q.body.trim() === t.trim()));

  // Open at the latest; follow new lines unless you've scrolled up.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    if (stick.current) el.scrollTop = el.scrollHeight;
    else setBelow(true);
  }, [items.length, queued.length, pending.length, state, asks.length]);
  const toBottom = () => {
    const el = scroller.current;
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
    stick.current = true;
    setBelow(false);
  };

  // While it works: Enter queues it for the turn's end; Send now (⌘⇧↩)
  // steers the running turn, for an agent that reads it at its next step.
  const canSteer = willQueue && state !== "blocked" && !!tl?.steer;
  const send = (steer = false) => {
    const t = text.trim();
    if (!t || busy || !canSend || (steer && !canSteer)) return;
    setBusy(true);
    post<{ ok: boolean; queued?: number; steered?: boolean }>(`/api/dockets/${d.id}/send`, { text: t, queue: true, steer, block: target?.block }).then(
      (r) => {
        setBusy(false);
        setText("");
        setSent((s) => [...s, t]);
        stick.current = true;
        if (r.steered) toast(`Sent: ${app} reads it at its next step`);
        else if (r.queued) toast(queued.length ? `Queued with ${queued.length} more: they go in together when ${app}'s turn ends` : `Queued: it goes in when ${app}'s turn ends`);
        reload();
        hub.reload();
      },
      (e: Error) => {
        setBusy(false);
        toast(e.message, "error");
      },
    );
  };
  const unqueue = (id: number) =>
    post(`/api/dockets/${d.id}/queue/${id}/cancel`).then(
      () => (toast("Taken back"), reload()),
      (e: Error) => toast(e.message, "error"),
    );

  let lastAt = 0;
  const rows: ReactNode[] = [];
  for (const it of items) {
    const at = Date.parse(it.at);
    if (lastAt && at - lastAt > 25 * 60 * 1000) rows.push(<div key={`t${it.seq}`} className="chat-time">{stamp(it.at)}</div>);
    lastAt = at;
    if (it.t === "you")
      rows.push(
        <div key={it.seq} className={`chat-you${it.note ? " note" : ""}`} title={stamp(it.at)}>
          {it.note && <div className="chat-label">Your note on the record (not sent)</div>}
          <Fold>
            <Prose text={it.text} />
          </Fold>
        </div>,
      );
    else if (it.t === "agent" || it.t === "ask")
      rows.push(
        <div key={it.seq} className="chat-agent">
          <div className="chat-who">
            <AppAvatar app={it.app} size={18} />
            <strong>{appName(it.app)}</strong>
            {it.who && <span className="chat-role">{it.who}</span>}
            {it.t === "ask" && <span className="muted">asked you{it.subject ? ` · ${it.subject}` : ""}</span>}
            <span className="chat-at" title={stamp(it.at)}>
              {agoLong(it.at)}
            </span>
          </div>
          <Fold>
            <Prose text={it.t === "ask" && it.options.length ? `${it.text}\n\nOptions: ${it.options.join(" · ")}` : it.text} />
          </Fold>
        </div>,
      );
    else if (it.t === "chose") rows.push(<div key={it.seq} className="chat-you chose">Chose “{it.text}”</div>);
    else if (it.t === "file")
      rows.push(
        <button key={it.seq} className="chat-file" onClick={() => onArtifact(it.e)}>
          <IconFile size={15} />
          <span>
            {it.e.body || it.e.data?.name}
            <span className="muted"> · {it.e.data?.url ? "a link" : it.e.data?.name}</span>
          </span>
        </button>,
      );
    else if (it.t === "break")
      rows.push(
        <div key={it.seq} className="chat-break">
          <span>{it.text}</span>
        </div>,
      );
    else rows.push(<div key={it.seq} className={`chat-sys tone-${it.tone ?? ""}`}>{it.text}</div>);
  }

  return (
    <div className="chat">
      <div
        ref={scroller}
        className="chat-scroll"
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
          if (stick.current) setBelow(false);
        }}
      >
        {rows.length === 0 && <div className="chat-empty">Nothing said yet. Send {app} something to start.</div>}
        {rows}
        {asks.map((m) => (
          <div key={m.id} className="chat-ask-card">
            <MessageCard m={m} />
          </div>
        ))}
        {queued.map((q) => (
          <div key={`q${q.id}`} className="chat-you queued">
            <Prose text={q.body} />
            <div className="chat-queued">
              Queued · {queued.length > 1 ? "they go in together when the turn ends" : "goes in when the turn ends"} ·{" "}
              <button className="link" onClick={() => unqueue(q.id)}>
                Take back
              </button>
            </div>
          </div>
        ))}
        {pending.map((t, i) => (
          <div key={`p${i}`} className="chat-you sending">
            <Prose text={t} />
          </div>
        ))}
        {live.length ? live.map((a) => <NowLine key={a.block} d={d} a={a} several={several} onTerminal={() => pane.view(d.id, a.block)} />) : <NowLine d={d} onTerminal={() => pane.view(d.id)} />}
      </div>
      {below && (
        <button className="chat-jump" onClick={toBottom} aria-label="Go to the latest">
          <IconDown size={16} />
        </button>
      )}
      <div className="chat-compose">
        {several && (
          <select className="chat-to" value={target?.block} onChange={(e) => setTo(e.target.value)} aria-label="Send to" title="Which agent this goes to">
            {live.map((a) => (
              <option key={a.block} value={a.block}>
                {agentLabel(a)} · {appName(a.app)} · {runtimeTitle(runtimeOf(a.block))}
              </option>
            ))}
          </select>
        )}
        <textarea
          ref={box}
          value={text}
          rows={1}
          placeholder={!canSend ? (tl ? `${app} stopped with an error` : `${app} isn't running`) : willQueue ? `Queue a message for ${app}…` : `Message ${app}…`}
          disabled={!canSend}
          onChange={(e) => {
            setText(e.target.value);
            const el = e.target;
            el.style.height = "auto";
            el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
          }}
          onKeyDown={(e) => {
            // A keyboard: Enter sends (queues, while it works), Shift+Enter is a
            // new line, ⌘⇧↩ sends now (steers). A phone: the buttons send.
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && e.shiftKey) {
              e.preventDefault();
              send(true);
            } else if (e.key === "Enter" && ((e.metaKey || e.ctrlKey) || (!e.shiftKey && !isPhone() && !e.nativeEvent.isComposing))) {
              e.preventDefault();
              send();
            }
          }}
        />
        {canSteer && (
          <button className="chat-steer" disabled={!text.trim() || busy} onClick={() => send(true)} title={`Send now: ${app} reads it at its next step, without waiting for the turn to end (⌘⇧↩)`}>
            Send now
          </button>
        )}
        <button className="chat-send" disabled={!text.trim() || busy || !canSend} onClick={() => send()} aria-label={willQueue ? "Queue it" : "Send"} title={willQueue ? `Queue it: it goes in when ${app}'s turn ends (↩)` : `Send to ${app}`}>
          <IconSend size={18} />
        </button>
      </div>
      {state && <span className="chat-state-sr">{liveWord(state)}</span>}
    </div>
  );
}
