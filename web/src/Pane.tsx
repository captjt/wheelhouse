import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { agentLabel, agentsOn, appName, get, paneRuntime, post, runtimeOf, runtimeTitle, type Live } from "./api";
import { AppAvatar, IconEye, IconTerminal, IconX, LiveIcon, TitleText, liveWord } from "./icons";
import { useHub } from "./live";
import { QuestionPicker } from "./Picker";
import { Kbd, Modal, useToast } from "./ui";

/** Whether this page is open on the Mac the hub (and Rex or Herdr) runs on.
 * From a phone, "Open in Rex" would only move a window on a Mac across the room. */
export const onThisMac = () => ["localhost", "127.0.0.1", "[::1]"].includes(window.location.hostname);

/** `block`: which of the docket's agents; its lead when left out. */
type PaneCtx = { view: (id: string, block?: string) => void; act: (id: string, block?: string) => void };
const Ctx = createContext<PaneCtx>({ view: () => {}, act: () => {} });

/** Opens the pane sheet from anywhere: rows, menus, the palette, keys. */
export function PaneProvider({ children }: { children: ReactNode }) {
  const hub = useHub();
  const [open, setOpen] = useState<{ id: string; block?: string } | null>(null);
  const view = useCallback((id: string, block?: string) => setOpen({ id, block }), []);
  // The one action for "show me this agent": its runtime's app on the Mac, the sheet elsewhere.
  const act = useCallback((id: string, block?: string) => (onThisMac() ? hub.openInRex(id, block) : setOpen({ id, block })), [hub]);
  return (
    <Ctx.Provider value={{ view, act }}>
      {children}
      {open && <PaneSheet key={`${open.id} ${open.block ?? ""}`} id={open.id} block={open.block} onClose={() => setOpen(null)} />}
    </Ctx.Provider>
  );
}

export const usePane = () => useContext(Ctx);

/** The pane button: Open in Rex or Herdr (and a peek) on the Mac, View pane anywhere else. */
export function PaneButton({ id, block, small, label }: { id: string; block?: string; small?: boolean; label?: string }) {
  const hub = useHub();
  const pane = usePane();
  const cls = small ? "btn btn-small" : "btn";
  const rt = block ? runtimeTitle(runtimeOf(block)) : paneRuntime(hub.byId.get(id));
  if (!onThisMac())
    return (
      <button className={cls} onClick={() => pane.view(id, block)} title="See the agent's terminal, and message it">
        <IconEye size={small ? 13 : 14} /> {label ? label.replace(/in (Rex|Herdr)$/, "").trim() || "View pane" : "View pane"}
      </button>
    );
  return (
    <span className="btn-pair">
      <button className={cls} onClick={() => hub.openInRex(id, block)} title={`Bring the pane to the front in ${rt} (O)`}>
        <IconTerminal size={small ? 13 : 14} /> {label || `Open in ${rt}`}
      </button>
      <button className={`${cls} btn-icon`} onClick={() => pane.view(id, block)} title="Peek at the pane here">
        <IconEye size={small ? 13 : 14} />
      </button>
    </span>
  );
}

/** Approve and Deny for a permission prompt Wheelhouse can answer. The hub
 * re-reads the prompt from the pane's runtime first and refuses if it isn't the one shown. */
export function AnswerButtons({ d, live, small }: { d: import("./api").Docket; live?: Live | null; small?: boolean }) {
  const hub = useHub();
  const [busy, setBusy] = useState<"approve" | "deny" | null>(null);
  const l = live ?? d.live;
  if (!l?.answerable) return null;
  const go = (approve: boolean) => (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (busy) return;
    setBusy(approve ? "approve" : "deny");
    hub.answer(d, approve, l).finally(() => setBusy(null));
  };
  const cls = small ? "btn btn-small" : "btn";
  return (
    <span className="btn-pair answer">
      <button className={`${cls} btn-approve`} disabled={!!busy} onClick={go(true)} title={`Approve: ${l.msg ?? ""}`}>
        {busy === "approve" ? "Approving…" : "Approve"}
      </button>
      <button className={cls} disabled={!!busy} onClick={go(false)} title="Deny (Esc in the pane)">
        {busy === "deny" ? "Denying…" : "Deny"}
      </button>
    </span>
  );
}

type Screen = { text: string; live: Live | null; at: string };

/** A live look at an agent's pane, and a box to message it between turns. */
function PaneSheet({ id, block, onClose }: { id: string; block?: string; onClose: () => void }) {
  const hub = useHub();
  const toast = useToast();
  const d = hub.byId.get(id);
  // The agent shown: the one asked for, or the docket's lead.
  const on = agentsOn(d);
  // Asked for one agent: that one, even after it leaves (then the sheet says
  // so and sends nothing), never another that happens to be on the docket.
  const agent = block ? (on.find((a) => a.block === block) ?? d?.agents?.find((a) => a.block === block)) : (on.find((a) => a.block === d?.lead) ?? d?.agent);
  const gone = !!block && !!d?.agents && !on.some((a) => a.block === block);
  const dLive = (block ? on.find((a) => a.block === block)?.live : d?.live) ?? null;
  const q = block ? `?block=${encodeURIComponent(block)}` : "";
  const [screen, setScreen] = useState<Screen | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const pre = useRef<HTMLPreElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    let live = true;
    let timer: number | undefined;
    const load = () =>
      get<Screen>(`/api/dockets/${id}/screen${q}`)
        .then((s) => {
          if (!live) return;
          setScreen(s);
          setError(null);
        })
        .catch((e: Error) => live && setError(e.message))
        .finally(() => {
          if (live) timer = window.setTimeout(load, document.hidden ? 5000 : 1500);
        });
    load();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [id, q]);

  // Follow the bottom, the way a terminal does, unless you've scrolled up.
  useLayoutEffect(() => {
    const el = pre.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [screen?.text]);

  const state = screen?.live?.state ?? dLive?.state;
  const app = on.length > 1 ? `${appName(agent?.app)} (${agentLabel(agent)})` : appName(agent?.app);
  const rt = agent?.block ? runtimeTitle(runtimeOf(agent.block)) : paneRuntime(d);
  const why = gone ? `${app} isn't on ${id} any more.` : state === "error" ? `${app} stopped with an error.` : !state ? `The pane isn't open in ${rt}.` : null;
  const queues = state === "working" || state === "starting" || state === "blocked";
  // While it works: Queue for the turn's end, or Send now to steer it.
  const canSteer = (state === "working" || state === "starting") && !!(screen?.live ?? dLive)?.steer;
  const send = (steer = false) => {
    const t = text.trim();
    if (!t || busy || why || (steer && !canSteer)) return;
    setBusy(true);
    post<{ queued?: number; steered?: boolean }>(`/api/dockets/${id}/send`, { text: t, queue: true, steer, block: agent?.block }).then(
      (r) => {
        setText("");
        setBusy(false);
        stick.current = true;
        toast(r.steered ? `Sent: ${app} reads it at its next step` : r.queued ? `Queued: it goes in when ${app}'s turn ends` : `Sent to ${app}`);
        hub.reload();
      },
      (e: Error) => {
        setBusy(false);
        toast(e.message, "error");
      },
    );
  };

  return (
    <Modal onClose={onClose} className="pane-sheet">
      <div className="ps-head">
        <AppAvatar app={agent?.app} size={20} />
        <div className="ps-title">
          <div className="ps-name">{d ? <TitleText d={d} /> : id}</div>
          <div className="ps-sub">
            <span className="row-id">{id}</span>
            {state && (
              <span className={`ps-state state-${state}`}>
                <LiveIcon state={state} size={11} /> {liveWord(state)}
              </span>
            )}
            {agent && (
              <span className="muted">
                {screen?.live?.rex_session || agent.rex_session} / {screen?.live?.pane || agent.pane || "pane"}
              </span>
            )}
          </div>
        </div>
        <span className="spacer" />
        <button className="btn btn-small" onClick={() => hub.openInRex(id, agent?.block)} title={onThisMac() ? `Bring the pane to the front in ${rt}` : `Bring the pane to the front in ${rt}, on the Mac`}>
          <IconTerminal size={13} /> {onThisMac() ? `Open in ${rt}` : "Show on Mac"}
        </button>
        <button className="icon-btn" onClick={onClose} aria-label="Close">
          <IconX size={15} />
        </button>
      </div>
      <pre
        ref={pre}
        className="ps-screen"
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
        }}
      >
        {screen ? screen.text || " " : error ? error : "Loading the pane…"}
      </pre>
      {d && dLive?.pickable && state === "blocked" ? (
        <div className="ps-answer ps-question">
          <div className="ps-answer-text">
            <div className="ps-answer-title">{app} is asking</div>
            <QuestionPicker d={d} live={dLive} fallback={dLive.msg && <div className="ps-answer-msg">{dLive.msg}</div>} />
          </div>
        </div>
      ) : d && dLive?.answerable && state === "blocked" ? (
        <div className="ps-answer">
          <LiveIcon state="blocked" size={14} />
          <div className="ps-answer-text">
            <div className="ps-answer-title">{app} is asking{dLive.kind ? ` (${dLive.kind})` : ""}</div>
            {dLive.msg && <div className="ps-answer-msg">{dLive.msg}</div>}
          </div>
          <AnswerButtons d={d} live={dLive} />
        </div>
      ) : (
      <div className="ps-compose">
        <textarea
          placeholder={why || (queues ? `Queue a message for ${app}…` : `Message ${app}…`)}
          value={text}
          disabled={!!why}
          rows={text.split("\n").length > 1 ? 3 : 1}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && e.shiftKey) (e.preventDefault(), send(true));
            else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) (e.preventDefault(), send());
          }}
        />
        {canSteer && (
          <button className="btn" disabled={!text.trim() || busy || !!why} onClick={() => send(true)} title={`${app} reads it at its next step, without waiting for the turn to end (⌘⇧↩)`}>
            Send now
          </button>
        )}
        <button className="btn btn-primary" disabled={!text.trim() || busy || !!why} onClick={() => send()}>
          {queues ? "Queue" : "Send"} <Kbd>⌘↩</Kbd>
        </button>
      </div>
      )}
    </Modal>
  );
}
