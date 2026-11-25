// A call's docket: what the Listener proposes from its transcript, each to
// apply, edit then apply, or ignore (src/calls.rs). Nothing reaches a
// docket until you apply it.
import { useEffect, useState } from "react";
import { post, type CallView, type Proposal } from "./api";
import { cantApply, editable, kindWord, listeningWords, parseOptions, whatItDoes } from "./calls";
import { agoLong, useHub } from "./live";
import { Prose, useToast } from "./ui";

export function CallSection({ id, call, reload }: { id: string; call: CallView; reload: () => void }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const busyReading = call.listening.state === "started";
  // While the Listener reads, look again now and then: it can take minutes.
  useEffect(() => {
    if (!busyReading) return;
    const t = window.setInterval(reload, 3000);
    return () => window.clearInterval(t);
  }, [busyReading, reload]);
  const listen = () => {
    setBusy(true);
    post(`/api/calls/${encodeURIComponent(id)}/listen`, {}).then(
      () => (setBusy(false), reload()),
      (e: Error) => (setBusy(false), toast(e.message, "error")),
    );
  };
  const words = listeningWords(call);
  const open = call.proposals.filter((p) => p.status === "open").length;
  const applied = call.proposals.filter((p) => p.status === "applied").length;
  return (
    <section className="call">
      <div className="intent-head">
        <span className="intent-label">Proposals</span>
        <span className="intent-meta">
          {call.proposals.length ? `${open} open · ${applied} applied` : "from the call's transcript"}
        </span>
        <span className="spacer" />
        {!busyReading && (
          <button className="btn btn-ghost btn-small" disabled={busy} onClick={listen} title="Read the call again, against your open dockets as they are now">
            Listen again
          </button>
        )}
      </div>
      {words && <div className={`call-state ${words.tone}`}>{words.text}</div>}
      <ul className="call-list">
        {call.proposals.map((p) => (
          <ProposalCard key={p.n} id={id} p={p} reload={reload} />
        ))}
      </ul>
    </section>
  );
}

function ProposalCard({ id, p, reload }: { id: string; p: Proposal; reload: () => void }) {
  const toast = useToast();
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const act = (action: "apply" | "ignore", edit?: Record<string, unknown>) => {
    setBusy(true);
    post<{ result?: { docket?: string; did?: string; told?: string[] } }>(`/api/calls/${encodeURIComponent(id)}/proposals/${p.n}`, { action, edit }).then(
      (r) => {
        setBusy(false);
        setEditing(false);
        const d = r.result?.docket;
        if (action === "apply") toast(r.result?.did === "opened" ? `${d} opened from the call` : `Applied to ${d}${r.result?.told?.length ? `, and told ${r.result.told.length === 1 ? r.result.told[0] : `${r.result.told.length} agents`}` : ""}`);
        reload();
      },
      (e: Error) => (setBusy(false), toast(e.message, "error")),
    );
  };
  const blocked = cantApply(p);
  return (
    <li className={`call-card ${p.status}`}>
      <div className="call-top">
        <span className={`tag call-kind ${p.kind}`}>{kindWord(p.kind)}</span>
        {p.kind === "task" ? (
          <span className="muted small">a new docket</span>
        ) : p.docket ? (
          <a className="chip" href={`#/d/${p.docket}`}>
            {p.docket}
          </a>
        ) : (
          <span className="call-warn small">no docket yet</span>
        )}
        <span className="call-n muted small">#{p.n}</span>
        <span className="spacer" />
        {p.status === "applied" && (
          <span className="call-done small">
            Applied{p.outcome?.applied?.docket ? <> to <a href={`#/d/${p.outcome.applied.docket}`}>{p.outcome.applied.docket}</a></> : null}
            {p.outcome?.edited ? ", edited" : ""}
            {p.outcome?.at ? ` · ${agoLong(p.outcome.at)}` : ""}
          </span>
        )}
        {p.status === "ignored" && <span className="muted small">Ignored{p.outcome?.at ? ` · ${agoLong(p.outcome.at)}` : ""}</span>}
      </div>
      {editing ? (
        <EditProposal p={p} busy={busy} onCancel={() => setEditing(false)} onApply={(edit) => act("apply", edit)} />
      ) : (
        <>
          <div className="call-what">
            {p.kind === "decide" && (
              <>
                {p.subject && <span className="call-subject">{p.subject}</span>}
                {p.subject && <span className="muted"> → </span>}
                <b>{p.choice}</b>
              </>
            )}
            {p.kind === "ask" && (
              <>
                <span>{p.question}</span>
                {p.options.length > 0 && <span className="call-options">{p.options.map((o) => <span key={o} className="chip">{o}</span>)}</span>}
              </>
            )}
            {p.kind === "task" && (
              <>
                <b>{p.title}</b>
                {p.intent && (
                  <div className="call-intent">
                    <Prose text={p.intent} />
                  </div>
                )}
              </>
            )}
          </div>
          <blockquote className={`call-quote${p.quote_found ? "" : " unfound"}`} title={p.quote_found ? "From the transcript" : "These words aren't in the transcript"}>
            “{p.quote}”{!p.quote_found && <span className="call-warn small"> Not in the transcript</span>}
          </blockquote>
          {p.why && <div className="muted small">{p.why}</div>}
          {p.status === "open" && (
            <div className="call-bar">
              <button className="btn btn-primary btn-small" disabled={busy || !!blocked} onClick={() => act("apply")} title={blocked ?? whatItDoes(p)}>
                Apply
              </button>
              <button className="btn btn-small" disabled={busy} onClick={() => setEditing(true)}>
                Edit
              </button>
              <button className="btn btn-ghost btn-small" disabled={busy} onClick={() => act("ignore")}>
                Ignore
              </button>
              {blocked && <span className="call-warn small">{blocked}</span>}
            </div>
          )}
        </>
      )}
    </li>
  );
}

/** Change a proposal before applying it: its docket and its words. */
function EditProposal({ p, busy, onCancel, onApply }: { p: Proposal; busy: boolean; onCancel: () => void; onApply: (edit: Record<string, unknown>) => void }) {
  const hub = useHub();
  const open = (hub.state?.dockets ?? []).filter((d) => (d.status === "todo" || d.status === "in_progress") && d.protocol !== "call");
  const [f, setF] = useState<Record<string, string>>({
    docket: p.docket ?? "",
    subject: p.subject ?? "",
    choice: p.choice ?? "",
    question: p.question ?? "",
    options: p.options.join("\n"),
    title: p.title ?? "",
    intent: p.intent ?? "",
  });
  const set = (k: string) => (e: { target: { value: string } }) => setF((x) => ({ ...x, [k]: e.target.value }));
  const edit = () => {
    const out: Record<string, unknown> = {};
    for (const k of editable(p.kind)) out[k] = k === "options" ? parseOptions(f.options) : f[k as string].trim() || null;
    onApply(out);
  };
  const fields = editable(p.kind);
  return (
    <div className="call-edit">
      {fields.includes("docket") && (
        <label className="call-field">
          <span>Docket</span>
          <select value={f.docket} onChange={set("docket")}>
            <option value="">Pick one…</option>
            {open.map((d) => (
              <option key={d.id} value={d.id}>
                {d.id} · {d.title}
              </option>
            ))}
          </select>
        </label>
      )}
      {fields.includes("subject") && (
        <label className="call-field">
          <span>Subject</span>
          <input value={f.subject} onChange={set("subject")} placeholder="What was decided about (Retries)" />
        </label>
      )}
      {fields.includes("choice") && (
        <label className="call-field">
          <span>Decided</span>
          <input value={f.choice} onChange={set("choice")} />
        </label>
      )}
      {fields.includes("question") && (
        <label className="call-field">
          <span>Question</span>
          <textarea rows={2} value={f.question} onChange={set("question")} />
        </label>
      )}
      {fields.includes("options") && (
        <label className="call-field">
          <span>Options</span>
          <textarea rows={2} value={f.options} onChange={set("options")} placeholder="One per line, or none" />
        </label>
      )}
      {fields.includes("title") && (
        <label className="call-field">
          <span>Title</span>
          <input value={f.title} onChange={set("title")} />
        </label>
      )}
      {fields.includes("intent") && (
        <label className="call-field">
          <span>Intent</span>
          <textarea rows={5} value={f.intent} onChange={set("intent")} />
        </label>
      )}
      <div className="call-bar">
        <button className="btn btn-primary btn-small" disabled={busy} onClick={edit}>
          Apply
        </button>
        <button className="btn btn-ghost btn-small" disabled={busy} onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}

