import { useState } from "react";
import { appName, type Msg } from "./api";
import { AppAvatar, HubAvatar, IconExternal } from "./icons";
import { onThisMac } from "./Pane";
import { agoLong, useHub } from "./live";
import { Kbd, Prose } from "./ui";

/** A message waiting for you: its words, its options as buttons, and a reply. */
export function MessageCard({ m, compact }: { m: Msg; compact?: boolean }) {
  const hub = useHub();
  const [text, setText] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const answer = (choice?: string) => {
    if (busy) return;
    if (!choice && !text.trim()) return;
    setBusy(choice ?? "reply");
    hub.replyTo(m.id, text, choice).finally(() => (setBusy(null), setText("")));
  };
  return (
    <div className="msg-card">
      <div className="msg-head">
        {m.from === "wheelhouse" ? <HubAvatar size={18} /> : <AppAvatar app={m.from_app === "Codex" ? "codex" : "claude-code"} size={18} />}
        <strong>{m.from === "wheelhouse" ? "Wheelhouse" : m.from_app || m.from}</strong>
        <span className="muted">{m.options.length ? "asks you" : "wrote to you"}</span>
        {m.subject && <span className="msg-subject">· {m.subject}</span>}
        <span className="spacer" />
        <span className="act-time">{agoLong(m.created_at)}</span>
        <span className="row-id">#{m.id}</span>
      </div>
      {!compact && (
        <div className="msg-body">
          <Prose text={m.body} />
        </div>
      )}
      {m.preview && (
        <div className="msg-preview">
          {m.preview.down ? (
            <span className="muted small">The preview is down: {m.preview.down}.</span>
          ) : (
            <a className="btn btn-small btn-primary" href={onThisMac() ? m.preview.local : m.preview.url} target="_blank" rel="noreferrer" onClick={(e) => e.stopPropagation()}>
              Open preview <IconExternal size={11} />
            </a>
          )}
          <span className="muted small mono-s">:{m.preview.app_port}</span>
        </div>
      )}
      {m.options.length > 0 && (
        <div className="msg-options">
          {m.options.map((o, i) => (
            <button key={o} className={`btn btn-small${i === 0 ? " btn-approve" : ""}`} disabled={!!busy} onClick={(e) => (e.preventDefault(), e.stopPropagation(), answer(o))}>
              {busy === o ? "Sending…" : o}
            </button>
          ))}
        </div>
      )}
      {!compact && (
        <div className="msg-reply">
          <textarea
            placeholder={m.options.length ? "Add words to your choice, or answer in your own…" : `Reply to ${m.from_app || m.from}…`}
            value={text}
            rows={1}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) (e.preventDefault(), answer());
            }}
          />
          <button className="btn btn-small" disabled={!text.trim() || !!busy} onClick={() => answer()}>
            Reply <Kbd>⌘↩</Kbd>
          </button>
          <button className="btn btn-small btn-ghost" onClick={() => hub.fileMessage(m.id)} title="File it without answering">
            Done
          </button>
        </div>
      )}
    </div>
  );
}

export const msgTitle = (m: Msg) => m.subject || m.body.split("\n").find((l) => l.trim())?.trim() || `Message #${m.id}`;
export const msgFrom = (m: Msg) => (m.from === "wheelhouse" ? "Wheelhouse" : m.from_app || appName(m.from));
