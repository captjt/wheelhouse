import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { appName, get, post, type Live } from "./api";
import { AppAvatar, IconEye, IconTerminal, IconX, LiveIcon, TitleText, liveWord } from "./icons";
import { useHub } from "./live";
import { Kbd, Modal, useToast } from "./ui";

/** Whether this page is open on the Mac the hub (and Rex) runs on. From a
 * phone, "Open in Rex" would only move a window on a Mac across the room. */
export const onThisMac = () => ["localhost", "127.0.0.1", "[::1]"].includes(window.location.hostname);

type PaneCtx = { view: (id: string) => void; act: (id: string) => void };
const Ctx = createContext<PaneCtx>({ view: () => {}, act: () => {} });

/** Opens the pane sheet from anywhere: rows, menus, the palette, keys. */
export function PaneProvider({ children }: { children: ReactNode }) {
  const hub = useHub();
  const [open, setOpen] = useState<string | null>(null);
  const view = useCallback((id: string) => setOpen(id), []);
  // The one action for "show me this agent": Rex on the Mac, the sheet elsewhere.
  const act = useCallback((id: string) => (onThisMac() ? hub.openInRex(id) : setOpen(id)), [hub]);
  return (
    <Ctx.Provider value={{ view, act }}>
      {children}
      {open && <PaneSheet id={open} onClose={() => setOpen(null)} />}
    </Ctx.Provider>
  );
}

export const usePane = () => useContext(Ctx);

/** The pane button: Open in Rex (and a peek) on the Mac, View pane anywhere else. */
export function PaneButton({ id, small, label }: { id: string; small?: boolean; label?: string }) {
  const hub = useHub();
  const pane = usePane();
  const cls = small ? "btn btn-small" : "btn";
  if (!onThisMac())
    return (
      <button className={cls} onClick={() => pane.view(id)} title="See the agent's terminal, and message it">
        <IconEye size={small ? 13 : 14} /> {label ? label.replace(/in Rex$/, "").trim() || "View pane" : "View pane"}
      </button>
    );
  return (
    <span className="btn-pair">
      <button className={cls} onClick={() => hub.openInRex(id)} title="Bring the pane to the front in Rex (O)">
        <IconTerminal size={small ? 13 : 14} /> {label || "Open in Rex"}
      </button>
      <button className={`${cls} btn-icon`} onClick={() => pane.view(id)} title="Peek at the pane here">
        <IconEye size={small ? 13 : 14} />
      </button>
    </span>
  );
}

/** Approve and Deny for a permission prompt Wheelhouse can answer. The hub
 * re-reads the prompt from Rex first and refuses if it isn't the one shown. */
export function AnswerButtons({ d, small }: { d: import("./api").Docket; small?: boolean }) {
  const hub = useHub();
  const [busy, setBusy] = useState<"approve" | "deny" | null>(null);
  if (!d.live?.answerable) return null;
  const go = (approve: boolean) => (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (busy) return;
    setBusy(approve ? "approve" : "deny");
    hub.answer(d, approve).finally(() => setBusy(null));
  };
  const cls = small ? "btn btn-small" : "btn";
  return (
    <span className="btn-pair answer">
      <button className={`${cls} btn-approve`} disabled={!!busy} onClick={go(true)} title={`Approve: ${d.live.msg ?? ""}`}>
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
function PaneSheet({ id, onClose }: { id: string; onClose: () => void }) {
  const hub = useHub();
  const toast = useToast();
  const d = hub.byId.get(id);
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
      get<Screen>(`/api/dockets/${id}/screen`)
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
  }, [id]);

  // Follow the bottom, the way a terminal does, unless you've scrolled up.
  useLayoutEffect(() => {
    const el = pre.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [screen?.text]);

  const state = screen?.live?.state ?? d?.live?.state;
  const app = appName(d?.agent?.app);
  const why =
    state === "working"
      ? `${app} is working. You can send when the turn ends.`
      : state === "blocked"
        ? `${app} is waiting on a prompt${screen?.live?.kind ? ` (${screen.live.kind})` : ""}. Answer it in Rex.`
        : state === "error"
          ? `${app} stopped with an error.`
          : !state
            ? "The pane isn't open in Rex."
            : null;
  const send = () => {
    const t = text.trim();
    if (!t || busy || why) return;
    setBusy(true);
    post(`/api/dockets/${id}/send`, { text: t }).then(
      () => {
        setText("");
        setBusy(false);
        stick.current = true;
        toast(`Sent to ${app}`);
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
        <AppAvatar app={d?.agent?.app} size={20} />
        <div className="ps-title">
          <div className="ps-name">{d ? <TitleText d={d} /> : id}</div>
          <div className="ps-sub">
            <span className="row-id">{id}</span>
            {state && (
              <span className={`ps-state state-${state}`}>
                <LiveIcon state={state} size={11} /> {liveWord(state)}
              </span>
            )}
            {d?.agent && (
              <span className="muted">
                {screen?.live?.rex_session || d.agent.rex_session} / {screen?.live?.pane || d.agent.pane || "pane"}
              </span>
            )}
          </div>
        </div>
        <span className="spacer" />
        <button className="btn btn-small" onClick={() => hub.openInRex(id)} title={onThisMac() ? "Bring the pane to the front in Rex" : "Bring the pane to the front in Rex, on the Mac"}>
          <IconTerminal size={13} /> {onThisMac() ? "Open in Rex" : "Show on Mac"}
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
      {d?.live?.answerable && state === "blocked" ? (
        <div className="ps-answer">
          <LiveIcon state="blocked" size={14} />
          <div className="ps-answer-text">
            <div className="ps-answer-title">{app} is asking{d.live.kind ? ` (${d.live.kind})` : ""}</div>
            {d.live.msg && <div className="ps-answer-msg">{d.live.msg}</div>}
          </div>
          <AnswerButtons d={d} />
        </div>
      ) : (
      <div className="ps-compose">
        <textarea
          placeholder={why || `Message ${app}…`}
          value={text}
          disabled={!!why}
          rows={text.split("\n").length > 1 ? 3 : 1}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) (e.preventDefault(), send());
          }}
        />
        <button className="btn btn-primary" disabled={!text.trim() || busy || !!why} onClick={send}>
          Send <Kbd>⌘↩</Kbd>
        </button>
      </div>
      )}
    </Modal>
  );
}
