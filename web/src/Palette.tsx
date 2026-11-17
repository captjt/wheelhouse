import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { get, post, type Docket } from "./api";
import { LinearPicker } from "./Linear";
import { IconActive, IconAgents, IconDone, IconEye, IconIssue, IconInbox, IconLayers, IconMonitor, IconMoon, IconPlus, IconShield, IconSun, IconTerminal, StatusIcon } from "./icons";
import { go, setTheme, useHub } from "./live";
import { Kbd, Modal, useToast } from "./ui";
import { onThisMac, usePane } from "./Pane";
import { STATUSES } from "./views";

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

export function Palette({ onClose, onNew, onNewLinear, context }: { onClose: () => void; onNew: () => void; onNewLinear: () => void; context?: Docket }) {
  const hub = useHub();
  const pane = usePane();
  const toast = useToast();
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const cmds = useMemo<Cmd[]>(() => {
    const out: Cmd[] = [];
    if (context) {
      const d = context;
      if (d.live && onThisMac()) out.push({ id: "rex", group: d.id, label: "Show pane in Rex", icon: <IconTerminal size={15} />, hint: <Kbd>O</Kbd>, run: () => hub.openInRex(d.id) });
      if (d.live?.answerable) {
        out.push({ id: "approve", group: d.id, label: `Approve: ${d.live.msg ?? "the prompt"}`, keywords: "permission yes allow", run: () => hub.answer(d, true) });
        out.push({ id: "deny", group: d.id, label: "Deny the prompt", keywords: "permission no reject", run: () => hub.answer(d, false) });
      }
      if (d.live) out.push({ id: "peek", group: d.id, label: onThisMac() ? "Peek at pane" : "View pane", icon: <IconEye size={15} />, keywords: "screen terminal message send", run: () => pane.view(d.id) });
      for (const s of STATUSES)
        if (s.key !== d.status)
          out.push({ id: `st-${s.key}`, group: d.id, label: `Move to ${s.label}`, icon: <StatusIcon status={s.key} />, keywords: "status", run: () => hub.setStatus(d.id, s.key) });
    }
    out.push(
      { id: "new", group: "Create", label: "New docket", icon: <IconPlus size={15} />, hint: <Kbd>C</Kbd>, run: onNew },
      { id: "new-linear", group: "Create", label: "New docket from a Linear issue", icon: <IconIssue size={15} />, keywords: "linear issue import", run: () => onNewLinear() },
      { id: "inbox", group: "Go to", label: "Inbox", icon: <IconInbox size={15} />, hint: <><Kbd>G</Kbd><Kbd>I</Kbd></>, run: () => go("/inbox") },
      { id: "active", group: "Go to", label: "Active dockets", icon: <IconActive size={15} />, hint: <><Kbd>G</Kbd><Kbd>A</Kbd></>, run: () => go("/view/active") },
      { id: "all", group: "Go to", label: "All dockets", icon: <IconLayers size={15} />, hint: <><Kbd>G</Kbd><Kbd>D</Kbd></>, run: () => go("/view/all") },
      { id: "done", group: "Go to", label: "Done", icon: <IconDone size={15} />, run: () => go("/view/done") },
      { id: "agents", group: "Go to", label: "Agents", icon: <IconAgents size={15} />, hint: <><Kbd>G</Kbd><Kbd>N</Kbd></>, run: () => go("/agents") },
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
  }, [hub, pane, context, onNew, onNewLinear, toast]);

  const shown = useMemo(() => {
    const query = q.trim().toLowerCase();
    if (!query) return cmds.filter((c) => c.group !== "Dockets").concat(cmds.filter((c) => c.group === "Dockets").slice(0, 6));
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

export function NewDocket({ onClose, linear }: { onClose: () => void; linear?: boolean }) {
  const hub = useHub();
  const toast = useToast();
  const [mode, setMode] = useState<"blank" | "linear">(linear ? "linear" : "blank");
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
      </div>
    </div>
  );
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
