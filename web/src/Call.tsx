// A docket's calls: each call's transcript, and what the Listener proposes
// from it, each to apply, edit then apply, or ignore (src/calls.rs).
// Nothing reaches a docket until you apply it.
import { useEffect, useMemo, useState } from "react";
import { post, type CallView, type Docket, type EntryView, type Proposal } from "./api";
import { blobUrl } from "./Artifacts";
import { cantApply, editable, kindWord, listeningWords, parseOptions, tally, whatItDoes } from "./calls";
import { foldDiff, lineDiff } from "./diff";
import { agoLong, stamp, useHub } from "./live";
import { Prose, useToast } from "./ui";

/** The Calls section of a docket's page: its calls, newest first. */
export function CallsSection({ d, calls, entries, reload, onAdd }: { d: Docket; calls: CallView[]; entries: EntryView[]; reload: () => void; onAdd?: () => void }) {
  const reading = calls.some((c) => c.listening.state === "started");
  // While the Listener reads, look again now and then: it can take minutes.
  useEffect(() => {
    if (!reading) return;
    const t = window.setInterval(reload, 3000);
    return () => window.clearInterval(t);
  }, [reading, reload]);
  const own = d.protocol === "call";
  return (
    <section className="calls">
      <div className="intent-head">
        <span className="intent-label">{own ? "Proposals" : "Calls"}</span>
        <span className="intent-meta">{own ? "from the call's transcript" : calls.length ? `${calls.length} call${calls.length === 1 ? "" : "s"}` : "what was said on calls about this docket"}</span>
        <span className="spacer" />
        {onAdd && (
          <button className="btn btn-ghost btn-small" onClick={onAdd} title="Paste or drop a call's transcript: the Listener reads it against this docket">
            Add a call
          </button>
        )}
      </div>
      {calls.map((c) => (
        <CallBlock key={c.call} d={d} c={c} entry={entries.find((e) => e.seq === c.call)} own={own} reload={reload} />
      ))}
    </section>
  );
}

function CallBlock({ d, c, entry, own, reload }: { d: Docket; c: CallView; entry?: EntryView; own: boolean; reload: () => void }) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const busyReading = c.listening.state === "started";
  const listen = () => {
    setBusy(true);
    post(`/api/dockets/${encodeURIComponent(d.id)}/calls/${c.call}/listen`, {}).then(
      () => (setBusy(false), reload()),
      (e: Error) => (setBusy(false), toast(e.message, "error")),
    );
  };
  const words = listeningWords(c);
  return (
    <div className="call">
      {!own && (
        <div className="call-head">
          <span className="call-title">{c.title}</span>
          <span className="muted small" title={stamp(c.at)}>
            {agoLong(c.at)} · {tally(c)}
          </span>
        </div>
      )}
      <div className="call-tools small">
        {entry && (
          <a href={blobUrl(entry)} target="_blank" rel="noreferrer">
            Transcript
          </a>
        )}
        {!busyReading && (
          <button className="btn btn-ghost btn-small" disabled={busy} onClick={listen} title={own ? "Read the call again, against your open dockets as they are now" : `Read the call again, against ${d.id} as it is now`}>
            Listen again
          </button>
        )}
      </div>
      {words && <div className={`call-state ${words.tone}`}>{words.text}</div>}
      <ul className="call-list">
        {c.proposals.map((p) => (
          <ProposalCard key={p.n} d={d} p={p} reload={reload} />
        ))}
      </ul>
    </div>
  );
}

function ProposalCard({ d, p, reload }: { d: Docket; p: Proposal; reload: () => void }) {
  const toast = useToast();
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const act = (action: "apply" | "ignore", edit?: Record<string, unknown>) => {
    setBusy(true);
    post<{ result?: { docket?: string; did?: string; told?: string[]; sent?: unknown[] } }>(`/api/dockets/${encodeURIComponent(d.id)}/proposals/${p.n}`, { action, edit }).then(
      (r) => {
        setBusy(false);
        setEditing(false);
        const x = r.result;
        if (action === "apply") toast(appliedToast(x));
        reload();
      },
      (e: Error) => (setBusy(false), toast(e.message, "error")),
    );
  };
  const blocked = cantApply(p, p.kind === "intent" ? (d.intent_versions ?? 0) : undefined);
  const elsewhere = p.docket && p.docket !== d.id;
  return (
    <li className={`call-card ${p.status}`}>
      <div className="call-top">
        <span className={`tag call-kind kind-${p.kind}`}>{kindWord(p.kind)}</span>
        {p.kind === "task" ? (
          <span className="muted small">a new docket</span>
        ) : !p.docket ? (
          <span className="call-warn small">no docket yet</span>
        ) : elsewhere ? (
          <a className="chip" href={`#/d/${p.docket}`}>
            {p.docket}
          </a>
        ) : null}
        <span className="call-n muted small">#{p.n}</span>
        <span className="spacer" />
        {p.status === "applied" && (
          <span className="call-done small">
            Applied
            {p.outcome?.applied?.docket && p.outcome.applied.docket !== d.id ? (
              <>
                {" "}
                to <a href={`#/d/${p.outcome.applied.docket}`}>{p.outcome.applied.docket}</a>
              </>
            ) : null}
            {p.outcome?.edited ? ", edited" : ""}
            {p.outcome?.at ? ` · ${agoLong(p.outcome.at)}` : ""}
          </span>
        )}
        {p.status === "ignored" && <span className="muted small">Ignored{p.outcome?.at ? ` · ${agoLong(p.outcome.at)}` : ""}</span>}
      </div>
      {editing ? (
        <EditProposal p={p} d={d} busy={busy} onCancel={() => setEditing(false)} onApply={(edit) => act("apply", edit)} />
      ) : (
        <>
          <div className="call-what">
            {p.kind === "intent" && (
              <>
                {p.subject && <b>{p.subject}</b>}
                {p.status === "open" ? <IntentChange from={d.intent?.body ?? ""} to={p.intent ?? ""} /> : null}
              </>
            )}
            {p.kind === "decide" && (
              <>
                {p.subject && <span className="call-subject">{p.subject}</span>}
                {p.subject && <span className="muted"> → </span>}
                <b>{p.choice}</b>
              </>
            )}
            {p.kind === "instruct" && <span className="call-text">{p.text}</span>}
            {p.kind === "ask" && (
              <>
                <span>{p.question}</span>
                {p.options.length > 0 && (
                  <span className="call-options">
                    {p.options.map((o) => (
                      <span key={o} className="chip">
                        {o}
                      </span>
                    ))}
                  </span>
                )}
              </>
            )}
            {p.kind === "task" && (
              <>
                <b>{p.title}</b>
                {p.intent && (
                  <div className="call-intent">
                    <Prose text={p.intent} images={false} />
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

/** What applying did, for a toast. */
export function appliedToast(r?: { docket?: string; did?: string; told?: string[]; sent?: unknown[] }): string {
  const d = r?.docket ?? "";
  if (r?.did === "opened") return `${d} opened from the call`;
  if (r?.did === "intent") return `${d}'s intent revised: a new version`;
  const told = r?.told ?? [];
  if (told.length) return `Applied to ${d}, and told ${told.length === 1 ? told[0] : `${told.length} agents`}`;
  if (r?.did === "instructed") return `Sent to the agents on ${d}`;
  return `Applied to ${d}`;
}

/** The intent as the call revised it, against the intent now. */
function IntentChange({ from, to }: { from: string; to: string }) {
  const diff = useMemo(() => foldDiff(lineDiff(from, to), 2), [from, to]);
  return (
    <pre className="diff call-diff">
      {diff.map((l, n) =>
        l.op === "skip" ? (
          <div key={n} className="dl dl-skip">
            <span className="dl-op">⋯</span>
            {l.count} unchanged line{l.count === 1 ? "" : "s"}
          </div>
        ) : (
          <div key={n} className={`dl dl-${l.op}`}>
            <span className="dl-op">{l.op === "add" ? "+" : l.op === "del" ? "−" : " "}</span>
            {l.text || " "}
          </div>
        ),
      )}
    </pre>
  );
}

/** Change a proposal before applying it: its docket and its words. */
function EditProposal({ p, d, busy, onCancel, onApply }: { p: Proposal; d: Docket; busy: boolean; onCancel: () => void; onApply: (edit: Record<string, unknown>) => void }) {
  const hub = useHub();
  const open = (hub.state?.dockets ?? []).filter((d) => (d.status === "todo" || d.status === "in_progress") && d.protocol !== "call");
  // The intent you edit against, as Edit opened: the hub refuses it if
  // the intent moves meanwhile, rather than write over what changed.
  const [against] = useState(() => ({ version: d.intent_versions ?? 0, body: d.intent?.body ?? "" }));
  const [f, setF] = useState<Record<string, string>>({
    docket: p.docket ?? "",
    subject: p.subject ?? "",
    choice: p.choice ?? "",
    question: p.question ?? "",
    options: p.options.join("\n"),
    text: p.text ?? "",
    title: p.title ?? "",
    intent: p.intent ?? "",
  });
  const set = (k: string) => (e: { target: { value: string } }) => setF((x) => ({ ...x, [k]: e.target.value }));
  const fields = editable(p.kind);
  const edit = () => {
    const out: Record<string, unknown> = {};
    for (const k of fields) out[k] = k === "options" ? parseOptions(f.options) : f[k as string].trim() || null;
    if (p.kind === "intent") out.version = against.version;
    onApply(out);
  };
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
          <span>{p.kind === "intent" ? "What changed" : "Subject"}</span>
          <input value={f.subject} onChange={set("subject")} placeholder={p.kind === "intent" ? "In a line" : "What was decided about (Retries)"} />
        </label>
      )}
      {fields.includes("choice") && (
        <label className="call-field">
          <span>Decided</span>
          <input value={f.choice} onChange={set("choice")} />
        </label>
      )}
      {fields.includes("text") && (
        <label className="call-field">
          <span>To the agents</span>
          <textarea rows={3} value={f.text} onChange={set("text")} />
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
          <textarea rows={p.kind === "intent" ? 14 : 5} value={f.intent} onChange={set("intent")} />
        </label>
      )}
      {p.kind === "intent" && (
        <div className="call-field">
          <span>Against the intent now (v{against.version})</span>
          <IntentChange from={against.body} to={f.intent} />
        </div>
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
