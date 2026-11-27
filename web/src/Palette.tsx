import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { agentLabel, appName, get, liveAgents, quick, paneRuntime, post, spawnRuntime, startAppFor, startApps, type Docket, type StartApp } from "./api";
import { LinearPicker } from "./Linear";
import { IconActive, IconAgents, IconCall, IconDone, IconEye, IconIssue, IconInbox, IconLayers, IconMonitor, IconMoon, IconPause, IconPlay, IconPlus, IconSettings, IconShield, IconSun, IconTerminal, StatusIcon } from "./icons";
import { AttentionIcon } from "./Verify";
import { attentionShown } from "./evidence";
import { go, setTheme, useHub } from "./live";
import { Kbd, Modal, useToast } from "./ui";
import { onThisMac, usePane } from "./Pane";
import { STATUSES, canStartAgent, startLabel } from "./views";
import { inboxItems } from "./Inbox";

type Cmd = { id: string; group: string; label: string; icon?: ReactNode; hint?: ReactNode; keywords?: string; run: () => void };

/** How well `q` matches `text`: higher is better, -1 is no match. */
function score(q: string, text: string): number {
  if (!q) return 0;
  const t = text.toLowerCase();
  const i = t.indexOf(q);
  if (i >= 0) return 100 - i;
  let j = 0;
  let s = 0;
  for (const c of t) {
    if (c === q[j]) {
      j++;
      s++;
      if (j === q.length) return s;
    }
  }
  return -1;
}

export function Palette({
  onClose,
  onNew,
  onNewLinear,
  onNewAgent,
  onNewCall,
  context,
}: {
  onClose: () => void;
  onNew: () => void;
  onNewLinear: () => void;
  onNewAgent: () => void;
  onNewCall: () => void;
  context?: Docket;
}) {
  const hub = useHub();
  const pane = usePane();
  const toast = useToast();
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const cmds = useMemo<Cmd[]>(() => {
    const out: Cmd[] = [];
    // What needs you, answerable right here.
    const asks = inboxItems(hub.state?.dockets ?? [], hub.state?.messages ?? [], hub.state?.review?.prs ?? []);
    for (const it of asks) {
      const d = it.d;
      const l = it.live ?? d.live;
      if (it.kind === "blocked" && l && quick(l)) {
        const who = it.who ? ` (${it.who})` : "";
        out.push({ id: `ap-${it.key}`, group: "Needs you", label: `Approve ${d.id}${who}: ${l.msg ?? "the prompt"}`, keywords: `approve permission yes ${d.title}`, run: () => hub.answer(d, true, l) });
        out.push({ id: `dn-${it.key}`, group: "Needs you", label: `Deny ${d.id}${who}: ${l.msg ?? "the prompt"}`, keywords: `deny permission no ${d.title}`, run: () => hub.answer(d, false, l) });
      } else if (it.m && it.m.options.length) {
        const m = it.m;
        for (const o of m.options)
          out.push({ id: `op-${m.id}-${o}`, group: "Needs you", label: `${d.id} · ${m.subject || "a question"}: ${o}`, keywords: `answer reply ${d.title}`, run: () => hub.replyTo(m.id, "", o) });
      }
    }
    if (context) {
      const d = context;
      if (d.live && onThisMac()) out.push({ id: "rex", group: d.id, label: `Show pane in ${paneRuntime(d)}`, icon: <IconTerminal size={15} />, hint: <Kbd>O</Kbd>, run: () => hub.openInRex(d.id) });
      if (d.live && quick(d.live)) {
        out.push({ id: "approve", group: d.id, label: `Approve: ${d.live.msg ?? "the prompt"}`, keywords: "permission yes allow", run: () => hub.answer(d, true) });
        out.push({ id: "deny", group: d.id, label: "Deny the prompt", keywords: "permission no reject", run: () => hub.answer(d, false) });
      }
      if (d.live) out.push({ id: "peek", group: d.id, label: onThisMac() ? "Peek at pane" : "View pane", icon: <IconEye size={15} />, keywords: "screen terminal message send", run: () => pane.view(d.id) });
      for (const s of STATUSES)
        if (s.key !== d.status)
          out.push({ id: `st-${s.key}`, group: d.id, label: `Move to ${s.label}`, icon: <StatusIcon status={s.key} />, keywords: "status", run: () => hub.setStatus(d.id, s.key) });
      if (d.hold) out.push({ id: "resume", group: d.id, label: `Resume: it's on hold (${d.hold})`, icon: <IconPlay size={15} />, keywords: "hold unhold resume", run: () => void hub.resume(d.id).catch(() => {}) });
      else out.push({ id: "hold", group: d.id, label: "Hold…", icon: <IconPause size={15} />, keywords: "hold pause stop wait", run: () => hub.setDialog({ kind: "hold", id: d.id }) });
      out.push({ id: "checks", group: d.id, label: "Run checks", icon: <IconShield size={15} />, keywords: "verify test lint witnessed", run: () => void hub.runChecks(d.id).catch(() => {}) });
      if (canStartAgent(d)) out.push({ id: "start-agent", group: d.id, label: startLabel(d), icon: <IconAgents size={15} />, keywords: "agent claude codex agy antigravity pi start pane work", run: () => hub.setDialog({ kind: "agent", id: d.id }) });
    }
    // What else needs a look: held, failing, quiet, closed on your word.
    for (const a of attentionShown(hub.state?.attention, asks.map((i) => ({ docket: i.d.id, kind: i.kind }))))
      if (a.docket)
        out.push({ id: `at-${a.docket}-${a.kind}`, group: "Attention", label: `${a.docket} · ${a.text}`, icon: <AttentionIcon kind={a.kind} size={15} />, keywords: `${a.title} ${a.kind}`, run: () => go(`/d/${a.docket}`) });
    out.push(
      { id: "new", group: "Create", label: "New docket", icon: <IconPlus size={15} />, hint: <Kbd>C</Kbd>, run: onNew },
      { id: "new-agent", group: "Create", label: "Start an agent on a new docket", icon: <IconAgents size={15} />, keywords: "claude codex agy antigravity pi start run", run: () => onNewAgent() },
      { id: "new-call", group: "Create", label: "Add a call's transcript", icon: <IconCall size={15} />, keywords: "call meeting transcript listen listener zoom meet granola", run: () => onNewCall() },
      { id: "new-linear", group: "Create", label: "New docket from a Linear issue", icon: <IconIssue size={15} />, keywords: "linear issue import", run: () => onNewLinear() },
      { id: "inbox", group: "Go to", label: "Inbox", icon: <IconInbox size={15} />, hint: <><Kbd>G</Kbd><Kbd>I</Kbd></>, run: () => go("/inbox") },
      { id: "active", group: "Go to", label: "Active dockets", icon: <IconActive size={15} />, hint: <><Kbd>G</Kbd><Kbd>A</Kbd></>, run: () => go("/view/active") },
      { id: "all", group: "Go to", label: "All dockets", icon: <IconLayers size={15} />, hint: <><Kbd>G</Kbd><Kbd>D</Kbd></>, run: () => go("/view/all") },
      { id: "done", group: "Go to", label: "Done", icon: <IconDone size={15} />, run: () => go("/view/done") },
      { id: "agents", group: "Go to", label: "Agents", icon: <IconAgents size={15} />, hint: <><Kbd>G</Kbd><Kbd>N</Kbd></>, run: () => go("/agents") },
      { id: "settings", group: "Go to", label: "Settings", icon: <IconSettings size={15} />, hint: <Kbd>⌘,</Kbd>, keywords: "preferences config options", run: () => go("/settings") },
      { id: "set-orders", group: "Settings", label: "Standing orders", icon: <IconSettings size={15} />, keywords: "settings pause resume post auto", run: () => go("/settings/orders") },
      { id: "set-gates", group: "Settings", label: "Gates: where a Build asks you", icon: <IconSettings size={15} />, keywords: "settings ask auto plan open push", run: () => go("/settings/gates") },
      { id: "set-agents", group: "Settings", label: "Agents: builder, checker, reviewers", icon: <IconSettings size={15} />, keywords: "settings claude codex agy antigravity fix rounds", run: () => go("/settings/agents") },
      { id: "set-repos", group: "Settings", label: "Repos: notes, checks, review guide", icon: <IconSettings size={15} />, keywords: "settings rules notes checks base branch plan", run: () => go("/settings/repos") },
      { id: "set-guides", group: "Settings", label: "Guides & briefs", icon: <IconSettings size={15} />, keywords: "settings guide brief prompt style", run: () => go("/settings/guides") },
      {
        id: "verify",
        group: "Record",
        label: "Verify every docket's record",
        icon: <IconShield size={15} />,
        keywords: "check chain signatures",
        run: () =>
          get<{ ok: boolean; dockets: { ok: boolean }[] }>("/api/verify").then(
            (r) => toast(r.ok ? `All ${r.dockets.length} records check: chains intact, signatures good` : `${r.dockets.filter((d) => !d.ok).length} records don't check`, r.ok ? undefined : "error"),
            (e: Error) => toast(e.message, "error"),
          ),
      },
    );
    out.push(
      { id: "th-system", group: "Theme", label: "Theme: System", icon: <IconMonitor size={15} />, keywords: "appearance mode auto os", run: () => setTheme("system") },
      { id: "th-light", group: "Theme", label: "Theme: Light", icon: <IconSun size={15} />, keywords: "appearance mode", run: () => setTheme("light") },
      { id: "th-dark", group: "Theme", label: "Theme: Dark", icon: <IconMoon size={15} />, keywords: "appearance mode", run: () => setTheme("dark") },
    );
    for (const d of hub.state?.dockets ?? [])
      out.push({
        id: `d-${d.id}`,
        group: "Dockets",
        label: d.title,
        icon: <StatusIcon status={d.status} />,
        hint: <span className="row-id">{d.id}</span>,
        keywords: `${d.id} ${d.agent?.cwd ?? ""} ${d.agent?.rex_session ?? ""}`,
        run: () => go(`/d/${d.id}`),
      });
    return out;
  }, [hub, pane, context, onNew, onNewLinear, onNewAgent, onNewCall, toast]);

  const shown = useMemo(() => {
    const query = q.trim().toLowerCase();
    // Settings' sections come up when you type; Go to holds Settings itself.
    if (!query) return cmds.filter((c) => c.group !== "Dockets" && c.group !== "Settings").concat(cmds.filter((c) => c.group === "Dockets").slice(0, 6));
    return cmds
      .map((c) => ({ c, s: Math.max(score(query, c.label), score(query, `${c.keywords ?? ""}`) - 20) }))
      .filter((x) => x.s >= 0)
      .sort((a, b) => b.s - a.s)
      .slice(0, 30)
      .map((x) => x.c);
  }, [q, cmds]);

  useEffect(() => setSel(0), [q]);
  useEffect(() => {
    listRef.current?.querySelector(".pal-item.sel")?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  const run = (c?: Cmd) => {
    if (!c) return;
    onClose();
    c.run();
  };

  let lastGroup = "";
  return (
    <Modal onClose={onClose} className="palette">
      <div className="pal-top">
        {context && <span className="pal-context">{context.id}</span>}
        <input
          autoFocus
          className="pal-input"
          placeholder="Type a command or search dockets…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || (e.key === "n" && e.ctrlKey)) (e.preventDefault(), setSel((s) => Math.min(shown.length - 1, s + 1)));
            else if (e.key === "ArrowUp" || (e.key === "p" && e.ctrlKey)) (e.preventDefault(), setSel((s) => Math.max(0, s - 1)));
            else if (e.key === "Enter") (e.preventDefault(), run(shown[sel]));
          }}
        />
      </div>
      <div className="pal-list" ref={listRef}>
        {shown.length === 0 && <div className="pal-empty">No results</div>}
        {shown.map((c, i) => {
          const head = c.group !== lastGroup ? <div className="pal-group">{c.group}</div> : null;
          lastGroup = c.group;
          return (
            <div key={c.id}>
              {head}
              <button className={`pal-item${i === sel ? " sel" : ""}`} onMouseMove={() => setSel(i)} onClick={() => run(c)}>
                <span className="pal-icon">{c.icon}</span>
                <span className="pal-label">{c.label}</span>
                <span className="pal-hint">{c.hint}</span>
              </button>
            </div>
          );
        })}
      </div>
    </Modal>
  );
}

/** Start an agent on a docket you already have, in a new pane: beside any agents already on it. */
export function StartAgent({ id, prompt, onClose }: { id: string; prompt?: string; onClose: () => void }) {
  const hub = useHub();
  const toast = useToast();
  const d = hub.byId.get(id);
  const [folders, setFolders] = useState<string[]>([]);
  const [cwd, setCwd] = useState(d?.agent?.cwd ?? "");
  const [app, setApp] = useState<StartApp>(startAppFor(d?.agent?.app));
  // From the composer's /task: what to do, after reading the docket's brief.
  const [note, setNote] = useState(prompt ? `Read wh brief ${id}, then: ${prompt}` : `Read wh brief ${id}, then work on its intent.`);
  const [busy, setBusy] = useState(false);
  const others = liveAgents(d);
  useEffect(() => {
    get<{ folders: string[]; suggested: string | null }>(`/api/folders?docket=${encodeURIComponent(id)}`).then(
      (r) => {
        setFolders(r.folders);
        if (r.suggested) setCwd((c) => c || r.suggested!);
      },
      () => {},
    );
  }, [id]);
  const startIt = () => {
    if (busy || !cwd.trim()) return;
    setBusy(true);
    post<{ docket: string; pane?: string; others?: number }>("/api/agents/start", { app, cwd: cwd.trim(), prompt: note.trim() || undefined, docket: id }).then(
      (r) => {
        const n = r.others ?? 0;
        toast(`${appName(app)} is on ${id}, in ${r.pane ?? `wheelhouse/${id}`}${n ? `, beside ${n} other agent${n > 1 ? "s" : ""}` : ""}`);
        // The composer's /task that asked for it is done with.
        if (prompt) window.dispatchEvent(new CustomEvent("wh-task-started", { detail: id }));
        hub.reload();
        onClose();
      },
      (e: Error) => (setBusy(false), toast(e.message, "error")),
    );
  };
  const keys = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) (e.preventDefault(), startIt());
  };
  return (
    <Modal onClose={onClose} className="new-docket">
      <div className="nd-head">
        <span className="chip">{id}</span>
        <span className="muted">›</span>
        <span>{others.length ? "Add an agent" : "Start an agent"}</span>
        <span className="spacer" />
        <span className="muted small nd-for" title={d?.title}>
          {d?.title}
        </span>
      </div>
      {others.length > 0 && (
        <div className="nd-beside muted small">
          It joins {others.length === 1 ? "the agent" : `the ${others.length} agents`} on {id}: {others.map((a) => `${appName(a.app)} (${agentLabel(a)})`).join(", ")}. They can reach each other with <code>wh send --to @{id}</code>.
        </div>
      )}
      <textarea
        autoFocus
        className="nd-note nd-prompt"
        placeholder="What should the agent do first? Leave it empty to start it idle."
        value={note}
        rows={4}
        onChange={(e) => setNote(e.target.value)}
        onKeyDown={keys}
      />
      <div className="nd-row">
        <label className="nd-label">Folder</label>
        <input className="nd-input" list="wh-start-folders" placeholder="~/git/project" value={cwd} onChange={(e) => setCwd(e.target.value)} onKeyDown={keys} />
        <datalist id="wh-start-folders">
          {folders.map((f) => (
            <option key={f} value={f} />
          ))}
        </datalist>
      </div>
      <div className="nd-row">
        <label className="nd-label">Agent</label>
        <AppPicker app={app} onPick={setApp} />
      </div>
      <div className="nd-foot">
        <span className="muted small">A new tab in {spawnRuntime(hub.state)}'s wheelhouse session, named {id}. The agent works on this docket and reads its intent.</span>
        <span className="spacer" />
        <button className="btn btn-primary" disabled={!cwd.trim() || busy} onClick={startIt}>
          {busy ? "Starting…" : "Start agent"} <Kbd>⌘↩</Kbd>
        </button>
      </div>
    </Modal>
  );
}

/** Which agent to start: one button each. */
function AppPicker({ app, onPick }: { app: StartApp; onPick: (a: StartApp) => void }) {
  return (
    <div className="seg">
      {startApps.map((a) => (
        <button key={a} className={app === a ? "on" : ""} onClick={() => onPick(a)}>
          {appName(a)}
        </button>
      ))}
    </div>
  );
}

export function NewDocket({ onClose, linear, agent, call, docket }: { onClose: () => void; linear?: boolean; agent?: boolean; call?: boolean; docket?: string }) {
  const hub = useHub();
  const toast = useToast();
  const [mode, setMode] = useState<"blank" | "linear" | "agent" | "call">(call ? "call" : agent ? "agent" : linear ? "linear" : "blank");
  const [cwd, setCwd] = useState("");
  const [app, setApp] = useState<StartApp>("claude-code");
  const folders = Array.from(new Set((hub.state?.dockets ?? []).map((d) => d.agent?.cwd).filter((c): c is string => !!c && !c.includes("/.local/share/")))).sort();
  const [opening, setOpening] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const create = () => {
    if (!title.trim() || busy) return;
    setBusy(true);
    hub.create(title.trim(), note).then(
      (id) => {
        onClose();
        go(`/d/${id}`);
      },
      () => setBusy(false),
    );
  };
  const keys = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) (e.preventDefault(), create());
  };
  const head = (
    <div className="nd-head">
      <span className="chip">Wheelhouse</span>
      <span className="muted">›</span>
      <span>New docket</span>
      <span className="spacer" />
      <div className="seg">
        <button className={mode === "blank" ? "on" : ""} onClick={() => setMode("blank")} title="A blank docket">
          Blank
        </button>
        <button className={mode === "linear" ? "on" : ""} onClick={() => setMode("linear")} title="From a Linear issue">
          From Linear
        </button>
        <button className={mode === "agent" ? "on" : ""} onClick={() => setMode("agent")} title="Start an agent on a new docket">
          Agent
        </button>
        <button className={mode === "call" ? "on" : ""} onClick={() => setMode("call")} title="A call's transcript: the Listener proposes what it settled for your dockets">
          Call
        </button>
      </div>
    </div>
  );
  if (mode === "call") return <NewCall head={head} onClose={onClose} docket={docket} />;
  if (mode === "agent") {
    const startIt = () => {
      if (busy || !cwd.trim()) return;
      setBusy(true);
      post<{ docket: string }>("/api/agents/start", { app, cwd: cwd.trim(), prompt: note, title: title || undefined }).then(
        (r) => {
          toast(`${r.docket}: ${appName(app)} started in wheelhouse/${r.docket}`);
          hub.reload();
          onClose();
          go(`/d/${r.docket}`);
        },
        (e: Error) => (setBusy(false), toast(e.message, "error")),
      );
    };
    const keys = (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) (e.preventDefault(), startIt());
    };
    return (
      <Modal onClose={onClose} className="new-docket">
        {head}
        <textarea
          autoFocus
          className="nd-note nd-prompt"
          placeholder="What should the agent do? (its first prompt; the docket is named from it)"
          value={note}
          rows={5}
          onChange={(e) => setNote(e.target.value)}
          onKeyDown={keys}
        />
        <div className="nd-row">
          <label className="nd-label">Folder</label>
          <input className="nd-input" list="wh-folders" placeholder="~/git/project" value={cwd} onChange={(e) => setCwd(e.target.value)} onKeyDown={keys} />
          <datalist id="wh-folders">
            {folders.map((f) => (
              <option key={f} value={f} />
            ))}
          </datalist>
        </div>
        <div className="nd-row">
          <label className="nd-label">Agent</label>
          <AppPicker app={app} onPick={setApp} />
        </div>
        <div className="nd-foot">
          <span className="muted small">A new tab in {spawnRuntime(hub.state)}'s wheelhouse session, named after the docket.</span>
          <span className="spacer" />
          <button className="btn btn-primary" disabled={!cwd.trim() || busy} onClick={startIt}>
            {busy ? "Starting…" : "Start agent"} <Kbd>⌘↩</Kbd>
          </button>
        </div>
      </Modal>
    );
  }
  if (mode === "linear")
    return (
      <Modal onClose={onClose} className="new-docket">
        {head}
        <LinearPicker
          busy={opening}
          onPick={(i) => {
            setOpening(i.identifier);
            post<{ docket: string; existing?: boolean }>("/api/dockets/from-linear", { key: i.identifier }).then(
              (r) => {
                toast(r.existing ? `${i.identifier} is already ${r.docket}` : `${r.docket} created from ${i.identifier}`);
                hub.reload();
                onClose();
                go(`/d/${r.docket}`);
              },
              (e: Error) => (setOpening(null), toast(e.message, "error")),
            );
          }}
        />
      </Modal>
    );
  return (
    <Modal onClose={onClose} className="new-docket">
      {head}
      <input
        autoFocus
        className="nd-title"
        placeholder="Docket title"
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onKeyDown={(e) => {
          keys(e);
          if (e.key === "Enter" && !e.metaKey && !e.ctrlKey) (e.preventDefault(), document.querySelector<HTMLTextAreaElement>(".nd-note")?.focus());
        }}
      />
      <textarea className="nd-note" placeholder="Add a note… (it goes on the record, signed as you)" value={note} rows={4} onChange={(e) => setNote(e.target.value)} onKeyDown={keys} />
      <div className="nd-foot">
        <span className="chip">
          <StatusIcon status="todo" size={12} /> To do
        </span>
        <span className="spacer" />
        <button className="btn btn-primary" disabled={!title.trim() || busy} onClick={create}>
          Create docket <Kbd>⌘↩</Kbd>
        </button>
      </div>
    </Modal>
  );
}

/** A call's transcript, pasted or from a file: a call docket that keeps it, and the Listener reading it (src/calls.rs). */
/** A docket a call's transcript can be kept on: one of your own, open, not itself a call. */
export const callable = (d: { id: string; status: string; protocol?: string | null; shared_as?: string | null }) =>
  d.id.startsWith("D-") && !d.shared_as && (d.status === "todo" || d.status === "in_progress") && d.protocol !== "call";

/** Add a call's transcript from a docket's page: on that docket. */
export function NewCallModal({ docket, onClose, onAdded }: { docket: string; onClose: () => void; onAdded?: () => void }) {
  const head = (
    <div className="nd-head">
      <span className="chip">{docket}</span>
      <span className="muted">›</span>
      <span className="nd-mode">Add a call</span>
    </div>
  );
  return <NewCall head={head} onClose={onClose} docket={docket} onAdded={onAdded} />;
}

function NewCall({ head, onClose, docket, onAdded }: { head: ReactNode; onClose: () => void; docket?: string; onAdded?: () => void }) {
  const hub = useHub();
  const toast = useToast();
  const dockets = (hub.state?.dockets ?? []).filter(callable);
  const [on, setOn] = useState(docket && dockets.some((d) => d.id === docket) ? docket : "");
  const [title, setTitle] = useState("");
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const read = (f: File | undefined) => {
    if (!f) return;
    if (f.size > CALL_MAX) return toast(`${f.name} is over ${Math.round(CALL_MAX / 1000)} KB: a transcript is at most that`, "error");
    f.text().then(
      (t) => (setText(t), setTitle((x) => x || f.name.replace(/\.[a-z0-9]+$/i, ""))),
      () => toast(`${f.name} couldn't be read`, "error"),
    );
  };
  const add = () => {
    if (!text.trim() || busy) return;
    setBusy(true);
    post<{ docket: string; call: number }>("/api/calls", { docket: on || undefined, title: title.trim() || undefined, transcript: text }).then(
      (r) => {
        toast(`${r.docket}: the Listener is reading the call`);
        hub.reload();
        onAdded?.();
        onClose();
        go(`/d/${r.docket}`);
      },
      (e: Error) => (setBusy(false), toast(e.message, "error")),
    );
  };
  const keys = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) (e.preventDefault(), add());
  };
  return (
    <Modal onClose={onClose} className="new-docket">
      {head}
      <input className="nd-title" placeholder="What the call was (Webhooks sync)" value={title} onChange={(e) => setTitle(e.target.value)} onKeyDown={keys} />
      <label className="nd-on small">
        <span className="muted">About</span>
        <select value={on} onChange={(e) => setOn(e.target.value)}>
          <option value="">No one docket: a docket of its own</option>
          {dockets.map((d) => (
            <option key={d.id} value={d.id}>
              {d.id} · {d.title}
            </option>
          ))}
        </select>
      </label>
      <textarea
        autoFocus
        className="nd-note nd-transcript"
        placeholder="Paste the call's transcript, or drop its file here (.txt, .vtt, .srt, .md)"
        value={text}
        rows={10}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={keys}
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => (e.preventDefault(), read(e.dataTransfer.files[0]))}
      />
      <div className="nd-foot">
        <label className="btn btn-small btn-ghost">
          Choose file…
          <input type="file" accept=".txt,.vtt,.srt,.md,text/plain" hidden onChange={(e) => read(e.target.files?.[0])} />
        </label>
        <span className="muted small">
          {on ? `The Listener reads it against ${on}: its intent, decisions and agents, and proposes.` : "The Listener reads it against your open dockets and proposes."} Nothing changes until you apply it.
        </span>
        <span className="spacer" />
        <button className="btn btn-primary" disabled={!text.trim() || busy || text.length > CALL_MAX} onClick={add}>
          {busy ? "Adding…" : "Add call"} <Kbd>⌘↩</Kbd>
        </button>
      </div>
    </Modal>
  );
}

/** A transcript is at most this long (src/calls.rs MAX_TRANSCRIPT). */
const CALL_MAX = 400_000;
