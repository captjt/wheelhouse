import { forwardRef, useEffect, useImperativeHandle, useRef, useState, type ReactNode } from "react";
import { appName, type Docket, type DocketPage, type EntryView } from "./api";
import { AppAvatar, HubAvatar, IconChevron, IconDown, IconDownload, IconShield, IconUp, LiveIcon, PersonAvatar, SessionIcon, StatusIcon, TitleText, liveWord, shortPath } from "./icons";
import { ago, agoLong, go, lastList, stamp, useData, useHub } from "./live";
import { Kbd, Prose, useKeys, useMenu } from "./ui";
import { AnswerButtons, PaneButton, onThisMac, usePane } from "./Pane";
import { IntentSection } from "./Intent";
import { LinearChip, LinearModal } from "./Linear";
import { post } from "./api";
import { IconIssue } from "./icons";
import { useToast } from "./ui";
import { STATUSES, statusItems } from "./views";

export function DocketScreen({ id, embedded }: { id: string; embedded?: boolean }) {
  const hub = useHub();
  const pane = usePane();
  const toast = useToast();
  const [linking, setLinking] = useState<string | false>(false);
  const { data, error, reload } = useData<DocketPage>(`/api/dockets/${id}`);
  const menu = useMenu();
  const statusRef = useRef<HTMLButtonElement>(null);
  const titleRef = useRef<{ edit: () => void }>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);

  const ids = lastList.ids;
  const pos = ids.indexOf(id);
  const step = (delta: number) => {
    const next = ids[pos + delta];
    if (pos >= 0 && next) go(`/d/${next}`);
  };
  const d = data?.docket;
  const openStatus = () => d && statusRef.current && menu.open(statusRef.current, statusItems(d, (s) => hub.setStatus(d.id, s).then(reload)), "Change status");

  useKeys(
    {
      j: () => step(1),
      k: () => step(-1),
      Escape: () => go(lastList.path),
      s: openStatus,
      o: () => d?.live && pane.act(d.id),
      r: () => titleRef.current?.edit(),
      m: () => composerRef.current?.focus(),
    },
    !embedded,
  );

  if (error && !data) return <div className="view-empty">{error}</div>;
  if (!data || !d) return <div className="view" />;
  const { entries, verify } = data;

  return (
    <div className={`view docket${embedded ? " embedded" : ""}`}>
      {!embedded && (
        <header className="view-head">
          <a className="crumb" href={`#${lastList.path}`}>
            {lastList.title}
          </a>
          <IconChevron size={12} />
          <span className="crumb-here">{d.id}</span>
          {pos >= 0 && (
            <span className="pager">
              <button className="icon-btn" title="Previous (K)" disabled={pos <= 0} onClick={() => step(-1)}>
                <IconUp size={14} />
              </button>
              <button className="icon-btn" title="Next (J)" disabled={pos >= ids.length - 1} onClick={() => step(1)}>
                <IconDown size={14} />
              </button>
              <span className="muted small">
                {pos + 1} / {ids.length}
              </span>
            </span>
          )}
          <span className="spacer" />
          {d.live && <PaneButton id={d.id} />}
        </header>
      )}

      <div className="docket-body">
        <div className="docket-main">
          <div className="docket-col">
          {embedded && (
            <div className="embedded-head">
              <a className="row-id" href={`#/d/${d.id}`}>
                {d.id}
              </a>
              <span className="spacer" />
              {d.live && <PaneButton id={d.id} small />}
              <a className="btn btn-small" href={`#/d/${d.id}`}>
                Open docket
              </a>
            </div>
          )}
          <Title ref={titleRef} d={d} onSave={(t) => hub.rename(d.id, t).then(reload)} />
          {d.live?.state === "blocked" && (
            <div className="callout callout-blocked">
              <LiveIcon state="blocked" />
              <div>
                <strong>{appName(d.agent?.app)} is waiting on you{d.live.kind ? ` · ${d.live.kind}` : ""}</strong>
                {d.live.msg && <div className="callout-msg">{d.live.msg}</div>}
              </div>
              <span className="spacer" />
              {d.live.answerable ? (
                <span className="callout-actions">
                  <AnswerButtons d={d} small />
                  <PaneButton id={d.id} small />
                </span>
              ) : (
                <PaneButton id={d.id} small label={onThisMac() ? "Answer in Rex" : "View pane"} />
              )}
            </div>
          )}
          {d.live?.state === "error" && (
            <div className="callout callout-error">
              <LiveIcon state="error" />
              <div>
                <strong>{appName(d.agent?.app)} failed</strong>
                {d.live.msg && <div className="callout-msg">{d.live.msg}</div>}
              </div>
            </div>
          )}

          <IntentSection d={d} entries={entries} onSaved={reload} />

          <div className="activity-head">
            <span>Activity</span>
            <span className="spacer" />
            <span className={`verify-chip ${verify.ok ? "ok" : "bad"}`} title={verify.ok ? `Every entry's id, chain link and signature checks. Head ${verify.head}` : verify.problems.join("\n")}>
              <IconShield size={13} /> {verify.ok ? `Verified · ${verify.entries} signed` : "Record doesn't check"}
            </span>
          </div>
          <div className="activity">
            {entries.map((e) => (
              <Activity key={e.seq} e={e} d={d} owner={hub.state?.owner || "you"} />
            ))}
          </div>
          <Composer ref={composerRef} onSend={(t) => hub.note(d.id, t).then(reload)} />
          </div>
        </div>

        <aside className="props">
          <Prop label="Status">
            <button ref={statusRef} className="prop-btn" onClick={openStatus} title="Change status (S)">
              <StatusIcon status={d.status} />
              {STATUSES.find((s) => s.key === d.status)?.label}
            </button>
          </Prop>
          <Prop label="Linear">
            {d.linear ? (
              <LinearChip l={d.linear} />
            ) : (
              <button className="prop-btn muted" onClick={() => setLinking("")} title="Link a Linear issue to this docket">
                <IconIssue size={14} /> Link issue
              </button>
            )}
          </Prop>
          {d.agent && (
            <>
              <Prop label="Agent">
                <span className="prop-val">
                  <AppAvatar app={d.agent.app} size={16} />
                  {appName(d.agent.app)}
                </span>
              </Prop>
              <Prop label="Now">
                <span className={`prop-val live-${d.live?.state || "gone"}`}>
                  <LiveIcon state={d.live?.state ?? "idle"} />
                  {d.live ? `${liveWord(d.live.state)}${d.live.since ? ` · ${ago(d.live.since)}` : ""}` : d.released ? `Ended · ${d.released}` : "Not running"}
                </span>
              </Prop>
              <Prop label="Rex">
                {d.live ? (
                  <button className="prop-btn" onClick={() => pane.act(d.id)} title={onThisMac() ? `Show ${d.agent.rex_session} / ${d.agent.pane} in Rex (O)` : "View the pane (O)"}>
                    <SessionIcon name={d.agent.rex_session} size={14} />
                    <span className="prop-text">
                      {d.live?.rex_session || d.agent.rex_session} <span className="muted">/ {d.live?.pane || d.agent.pane || "pane"}</span>
                    </span>
                  </button>
                ) : (
                  <span className="prop-val">
                    <SessionIcon name={d.agent.rex_session} size={14} />
                    <span className="prop-text muted">
                      {d.agent.rex_session} / {d.agent.pane || "pane"}
                    </span>
                  </span>
                )}
              </Prop>
              {d.agent.cwd && (
                <Prop label="Folder">
                  <button className="prop-btn" title={`${d.agent.cwd}\nClick to copy`} onClick={() => navigator.clipboard?.writeText(d.agent!.cwd!)}>
                    <span className="prop-text">{shortPath(d.agent.cwd)}</span>
                  </button>
                </Prop>
              )}
              {d.session && (
                <Prop label="Session">
                  <button className="prop-btn mono-s" title="Copy the agent's session id" onClick={() => navigator.clipboard?.writeText(d.session!)}>
                    {d.session.slice(0, 8)}
                  </button>
                </Prop>
              )}
            </>
          )}
          <Prop label="Protocol">
            <span className="prop-val">{d.protocol === "agent" ? "Agent" : "By hand"}</span>
          </Prop>
          <Prop label="Work">
            <span className="prop-val">
              {d.prompts} prompt{d.prompts === 1 ? "" : "s"} · {d.turns} turn{d.turns === 1 ? "" : "s"}
            </span>
          </Prop>
          <Prop label="Created">
            <span className="prop-val" title={d.created_at}>
              {stamp(d.created_at)}
            </span>
          </Prop>
          <Prop label="Updated">
            <span className="prop-val" title={d.updated_at}>
              {agoLong(d.updated_at)}
            </span>
          </Prop>

          <div className="props-section">Record</div>
          <Prop label="Check">
            <span className={`prop-val ${verify.ok ? "ok" : "bad"}`}>
              <IconShield size={14} />
              {verify.ok ? `Intact · ${verify.entries} signed` : verify.problems[0]}
            </span>
          </Prop>
          <Prop label="Head">
            <button className="prop-btn mono-s" title={`Copy: Docket: ${d.id} @ ${d.short_head}`} onClick={() => navigator.clipboard?.writeText(`Docket: ${d.id} @ ${d.short_head}`)}>
              {d.short_head}
            </button>
          </Prop>
          <Prop label="Export">
            <a className="prop-btn" href={`/api/dockets/${d.id}/export`} download={`${d.id}.jsonl`}>
              <IconDownload size={14} /> {d.id}.jsonl
            </a>
          </Prop>
        </aside>
      </div>
      {menu.node}
      {linking !== false && (
        <LinearModal
          title={`Link a Linear issue to ${d.id}`}
          busy={linking || null}
          onClose={() => setLinking(false)}
          onPick={(i) => {
            setLinking(i.identifier);
            post(`/api/dockets/${d.id}/linear`, { key: i.identifier }).then(
              () => {
                setLinking(false);
                toast(`${d.id} linked to ${i.identifier}${d.intent ? "" : ": its description is the intent"}`);
                reload();
              },
              (e: Error) => (setLinking(false), toast(e.message, "error")),
            );
          }}
        />
      )}
    </div>
  );
}

function Prop({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="prop">
      <span className="prop-label">{label}</span>
      <span className="prop-value">{children}</span>
    </div>
  );
}

// ---- the activity feed ----

function Activity({ e, d, owner }: { e: EntryView; d: Docket; owner: string }) {
  const agent = appName(d.agent?.app);
  const time = (
    <span className="act-time" title={`${stamp(e.at)}\n#${e.seq} · ${e.mark}, signed by ${e.key || "?"}\n${e.id}`}>
      {agoLong(e.at)}
    </span>
  );
  const mark = <span className={`act-mark mark-${e.mark}`}>{e.mark}</span>;
  const sys = (avatar: ReactNode, text: ReactNode, tone = "") => (
    <div className={`act act-sys${tone ? ` tone-${tone}` : ""}`}>
      <span className="act-avatar">{avatar}</span>
      <span className="act-text">
        {text} <span className="act-dot">·</span> {time}
      </span>
      {mark}
    </div>
  );
  const comment = (avatar: ReactNode, name: ReactNode, verb: string, body: string, tone = "") => (
    <div className={`act act-comment${tone ? ` tone-${tone}` : ""}`}>
      <div className="act-card">
        <div className="act-card-head">
          {avatar}
          <strong>{name}</strong>
          <span className="muted">{verb}</span>
          <span className="act-dot">·</span>
          {time}
          <span className="spacer" />
          {mark}
        </div>
        <Clamp>
          <Prose text={body} />
        </Clamp>
      </div>
    </div>
  );
  const you = <PersonAvatar name={owner} size={18} />;
  const hubA = <HubAvatar size={18} />;
  const app = <AppAvatar app={d.agent?.app} size={18} />;
  const data = e.data || {};

  switch (e.kind) {
    case "open":
      return data.protocol === "agent"
        ? sys(hubA, <><b>Wheelhouse</b> picked up {agent}{d.agent?.cwd ? <> in <span className="act-path" title={d.agent.cwd}>{shortPath(d.agent.cwd)}</span></> : null}</>)
        : sys(you, <><b>You</b> opened this docket</>);
    case "claim": {
      const s = <span className="mono-s">{String(data.session || "").slice(0, 8)}</span>;
      const src = data.source;
      return sys(
        hubA,
        src === "resume" ? <><b>{agent}</b> resumed session {s}</> : src === "clear" ? <><b>{agent}</b> cleared into session {s}</> : <><b>{agent}</b> started session {s}</>,
      );
    }
    case "note":
      if (data.via === "prompt") return comment(you, "You", `asked ${agent}`, e.body || "", "you");
      if (data.via === "message") return comment(hubA, "rex-inbox", "delivered a message", e.body || "");
      if (e.actor.type === "agent") return comment(app, agent, "noted", e.body || "");
      return comment(you, "You", "noted", e.body || "", "you");
    case "turn":
      if (!e.body) return sys(app, <><b>{agent}</b> finished a turn{data.state && data.state !== "done" ? ` (${data.state})` : ""}</>);
      return comment(app, agent, "finished a turn", e.body, "agent");
    case "intent": {
      // The agent that wrote it, which may not be the docket's own.
      const writer = String(e.actor.id).split("@")[0];
      const by =
        data.source === "linear" ? (
          <><b>Linear</b> set the intent from {data.key}'s description{data.taken_by ? " (taken by you)" : ""}</>
        ) : (
          <><b>{e.actor.type === "person" ? "You" : e.actor.type === "agent" ? appName(writer) : "Wheelhouse"}</b> {e.actor.type === "person" ? "updated" : "proposed"} the intent</>
        );
      return sys(data.source === "linear" ? <IconIssue size={16} /> : e.actor.type === "person" ? you : <AppAvatar app={writer} size={18} />, by, "intent");
    }
    case "link":
      if (data.kind === "linear")
        return sys(
          you,
          <>
            <b>You</b> linked <a className="act-link" href={data.url} target="_blank" rel="noreferrer">{data.key}</a> {data.title ? <>· {data.title}</> : null}
          </>,
        );
      return sys(hubA, <>Linked {e.body}</>);
    case "observed":
      if (data.what === "linear-comment") return comment(<IconIssue size={18} />, data.author || "Someone", "commented in Linear", e.body || "", "linear");
      if (data.what === "linear") {
        const i = data.issue || {};
        const changed: string[] = data.changed || [];
        if (!changed.length)
          return sys(<IconIssue size={16} />, <><b>Linear</b>: {i.key} is {i.state ?? "open"}{i.assignee ? `, assigned to ${i.assignee}` : ""}</>);
        const parts = changed.map((c) =>
          c === "state" ? `${data.from?.state ?? "?"} → ${i.state}` : c === "title" ? `title “${i.title}”` : c === "assignee" ? `assigned to ${i.assignee ?? "no one"}` : c === "priority" ? `priority ${i.priority ?? "none"}` : "description changed",
        );
        return sys(<IconIssue size={16} />, <><b>Linear</b> · {i.key}: {parts.join(" · ")}</>);
      }
      return data.what === "error"
        ? sys(<LiveIcon state="error" size={16} />, <><b>{agent}</b> failed{e.body ? <>: {e.body}</> : null}</>, "error")
        : sys(<LiveIcon state="blocked" size={16} />, <><b>{agent}</b> waited on you{data.kind ? ` (${data.kind})` : ""}{e.body ? <>: {e.body}</> : null}</>, "blocked");
    case "set": {
      const who = e.actor.type === "person" ? "You" : "Wheelhouse";
      const parts: ReactNode[] = [];
      if (data.title)
        parts.push(data.title_source === "prompt" ? <span key="t">named it from your prompt</span> : <span key="t">renamed it to “{data.title}”</span>);
      if (data.status) {
        const st = STATUSES.find((s) => s.key === data.status);
        parts.push(
          <span key="s">
            moved it to <StatusIcon status={data.status} size={12} /> {st?.label ?? data.status}
          </span>,
        );
      }
      return sys(e.actor.type === "person" ? you : hubA, <><b>{who}</b> {parts.reduce<ReactNode[]>((a, p, i) => (i ? [...a, " and ", p] : [p]), [])}</>);
    }
    case "approval": {
      const ok = data.decision === "approve";
      return sys(
        you,
        <>
          <b>You</b> {ok ? "approved" : "denied"} {agent}'s {data.kind || "prompt"}
          {e.body ? <>: <span className="act-path">{e.body}</span></> : null}
        </>,
        ok ? "approved" : "denied",
      );
    }
    case "release":
      return sys(hubA, <><b>Wheelhouse</b> saw the session end: {data.reason}</>);
    default:
      return sys(hubA, <>{e.kind}{e.body ? `: ${e.body}` : ""}</>);
  }
}

function Clamp({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [long, setLong] = useState(false);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (el) setLong(el.scrollHeight > 260);
  }, [children]);
  return (
    <div className={`clamp${long && !open ? " clamped" : ""}`}>
      <div ref={ref} className="clamp-body">
        {children}
      </div>
      {long && (
        <button className="link" onClick={() => setOpen((o) => !o)}>
          {open ? "Show less" : "Show more"}
        </button>
      )}
    </div>
  );
}

// ---- title and composer ----


const Title = forwardRef<{ edit: () => void }, { d: Docket; onSave: (t: string) => Promise<unknown> }>(function Title({ d, onSave }, ref) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(d.title);
  useImperativeHandle(ref, () => ({ edit: () => (setValue(d.title), setEditing(true)) }), [d.title]);
  const save = () => {
    const t = value.trim();
    setEditing(false);
    if (t && t !== d.title) onSave(t);
  };
  if (editing)
    return (
      <textarea
        className="title-input"
        autoFocus
        rows={1}
        value={value}
        onChange={(e) => setValue(e.target.value.replace(/\n/g, ""))}
        onBlur={save}
        onFocus={(e) => e.currentTarget.select()}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.preventDefault(), save());
          if (e.key === "Escape") (e.preventDefault(), e.stopPropagation(), setEditing(false));
        }}
      />
    );
  return (
    <h1 className="title" onClick={() => (setValue(d.title), setEditing(true))} title="Rename (R)">
      <TitleText d={d} />
    </h1>
  );
});

const Composer = forwardRef<HTMLTextAreaElement, { onSend: (t: string) => Promise<unknown> }>(function Composer({ onSend }, ref) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const send = () => {
    const t = text.trim();
    if (!t || busy) return;
    setBusy(true);
    onSend(t).then(
      () => (setText(""), setBusy(false)),
      () => setBusy(false),
    );
  };
  return (
    <div className="composer">
      <textarea
        ref={ref}
        placeholder="Leave a note on the record…"
        value={text}
        rows={text.split("\n").length > 2 ? 5 : 2}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) (e.preventDefault(), send());
          if (e.key === "Escape") (e.currentTarget.blur(), e.stopPropagation());
        }}
      />
      <div className="composer-bar">
        <span className="muted small">Signed as you</span>
        <span className="spacer" />
        <button className="btn btn-primary btn-small" disabled={!text.trim() || busy} onClick={send}>
          Note <Kbd>⌘↩</Kbd>
        </button>
      </div>
    </div>
  );
});
