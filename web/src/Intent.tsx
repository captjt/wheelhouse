import { useCallback, useMemo, useRef, useState } from "react";
import { appName, post, type Docket, type EntryView } from "./api";
import { IconEdit, IconHistory, IconIssue } from "./icons";
import { agoLong, stamp } from "./live";
import { Kbd, Modal, Prose, useToast } from "./ui";
import { MarkdownEditor, type EditorHandle } from "./Editor";
import { renderMarkdown, toggleTask } from "./markdown";

/** Who wrote an intent version, in words. */
function author(e: { actor: EntryView["actor"]; data: EntryView["data"] }, d: Docket) {
  if (e.data?.source === "linear") return e.data?.taken_by ? `Linear (${e.data.key}), taken by you` : `Linear (${e.data?.key ?? d.linear?.key ?? "issue"})`;
  if (e.data?.source === "review") return "Wheelhouse, from the PR";
  if (e.actor.type === "person") return "You";
  if (e.actor.type === "agent") return appName(String(e.actor.id).split("@")[0]);
  return "Wheelhouse";
}

/**
 * The docket's living spec: what it's for, how you'll know it's done, what's
 * out of scope. Each edit is a new version on the record, by whoever made it.
 */
export function IntentSection({ d, entries, onSaved }: { d: Docket; entries: EntryView[]; onSaved: () => void }) {
  const toast = useToast();
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [history, setHistory] = useState(false);
  const area = useRef<EditorHandle>(null);
  const versions = useMemo(() => entries.filter((e) => e.kind === "intent"), [entries]);
  const current = versions[versions.length - 1];
  const intent = d.intent;
  // Linear's description moved on while the intent was edited here.
  // A Review's intent comes from its PR, not the issue's description.
  const linearAhead = d.protocol !== "review" && !!d.linear?.description && intent?.source !== "linear" && d.linear.description.trim() !== (intent?.body ?? "").trim() && !!intent;

  const edit = () => {
    setText(intent?.body ?? "");
    setEditing(true);
    window.setTimeout(() => area.current?.focus(), 0);
  };
  // Ticking a box in the intent saves a new version, as an edit would.
  const tick = useCallback(
    (index: number, checked: boolean): Promise<boolean> => {
      const body = d.intent?.body ?? "";
      const count = (renderMarkdown(body).match(/<input /g) ?? []).length;
      const next = toggleTask(body, index, checked, count);
      if (next === null) {
        toast("That box isn't plain in the text: edit the intent to change it", "error");
        return Promise.resolve(false);
      }
      return post(`/api/dockets/${d.id}/intent`, { body: next }).then(
        () => (toast(`${d.id}: ${checked ? "ticked off" : "unticked"}, saved as version ${(d.intent_versions ?? 0) + 1}`), onSaved(), true),
        (e: Error) => (toast(e.message, "error"), false),
      );
    },
    [d.id, d.intent?.body, d.intent_versions, onSaved, toast],
  );
  const save = () => {
    if (busy) return;
    setBusy(true);
    post(`/api/dockets/${d.id}/intent`, { body: text }).then(
      () => {
        setBusy(false);
        setEditing(false);
        toast(`${d.id}: intent saved as version ${(d.intent_versions ?? 0) + 1}`);
        onSaved();
      },
      (e: Error) => {
        setBusy(false);
        toast(e.message, "error");
      },
    );
  };
  const takeLinear = () =>
    post(`/api/dockets/${d.id}/intent`, { take_linear: true }).then(
      () => (toast(`${d.id} follows ${d.linear?.key}'s description again`), onSaved()),
      (e: Error) => toast(e.message, "error"),
    );

  return (
    <section className={`intent${editing ? " editing" : ""}`}>
      <div className="intent-head">
        <span className="intent-label">Intent</span>
        {intent && current && (
          <span className="intent-meta" title={stamp(intent.at)}>
            v{d.intent_versions} · {author(current, d)} · {agoLong(intent.at)}
          </span>
        )}
        <span className="spacer" />
        {versions.length > 1 && !editing && (
          <button className="btn btn-ghost btn-small" onClick={() => setHistory(true)}>
            <IconHistory size={13} /> History
          </button>
        )}
        {!editing && intent && (
          <button className="btn btn-ghost btn-small" onClick={edit} title="Edit the intent">
            <IconEdit size={13} /> Edit
          </button>
        )}
      </div>
      {linearAhead && !editing && (
        <div className="intent-notice">
          <IconIssue size={14} />
          <span>
            {d.linear?.key}'s description in Linear has changed since this was edited here.
          </span>
          <span className="spacer" />
          <button className="btn btn-small" onClick={takeLinear}>
            Use Linear's version
          </button>
        </div>
      )}
      {editing ? (
        <div className="intent-editor">
          <MarkdownEditor
            ref={area}
            value={text}
            onChange={setText}
            onSubmit={save}
            onCancel={() => setEditing(false)}
            placeholder={"What must be true when this is done?\n\n## Acceptance\n- [ ] …\n\n## Out of scope\n- …"}
            footer={
              <div className="intent-bar">
                <span className="muted small">Each save is a new version on the record, signed as you.</span>
                <span className="spacer" />
                <button className="btn btn-small" onClick={() => setEditing(false)}>
                  Cancel
                </button>
                <button className="btn btn-primary btn-small" disabled={busy || text.trim() === (intent?.body ?? "").trim()} onClick={save}>
                  Save <Kbd>⌘↩</Kbd>
                </button>
              </div>
            }
          />
        </div>
      ) : intent?.body.trim() ? (
        <div className="intent-body" onDoubleClick={edit}>
          <Prose text={intent.body} onTask={tick} />
        </div>
      ) : (
        <button className="intent-empty" onClick={edit}>
          Write the intent: what this docket is for, how you'll know it's done, and what's out of scope. Agents read it with <code>wh brief</code>.
        </button>
      )}
      {history && <IntentHistory d={d} versions={versions} onClose={() => setHistory(false)} />}
    </section>
  );
}

function IntentHistory({ d, versions, onClose }: { d: Docket; versions: EntryView[]; onClose: () => void }) {
  const [i, setI] = useState(versions.length - 1);
  const v = versions[i];
  const prev = versions[i - 1];
  const diff = useMemo(() => lineDiff(prev?.body ?? "", v?.body ?? ""), [prev, v]);
  const [view, setView] = useState<"rendered" | "changes">("rendered");
  const showing = prev ? view : "rendered";
  return (
    <Modal onClose={onClose} className="history-modal">
      <div className="hist">
        <div className="hist-list">
          <div className="pal-group">Versions</div>
          {versions
            .map((x, n) => ({ x, n }))
            .reverse()
            .map(({ x, n }) => (
              <button key={x.seq} className={`hist-item${n === i ? " sel" : ""}`} onClick={() => setI(n)}>
                <span className="hist-v">v{n + 1}</span>
                <span className="hist-who">{author(x, d)}</span>
                <span className="hist-at">{agoLong(x.at)}</span>
              </button>
            ))}
        </div>
        <div className="hist-body">
          <div className="hist-title">
            v{i + 1} · {author(v, d)} · {stamp(v.at)}
            <span className="spacer" />
            {prev ? (
              <span className="seg seg-text hist-seg">
                <button className={showing === "rendered" ? "on" : ""} onClick={() => setView("rendered")}>
                  Rendered
                </button>
                <button className={showing === "changes" ? "on" : ""} onClick={() => setView("changes")} title={`What changed from v${i}`}>
                  Changes
                </button>
              </span>
            ) : (
              <span className="muted small">first version</span>
            )}
          </div>
          {showing === "rendered" ? (
            <div className="hist-rendered">
              <Prose text={v?.body ?? ""} />
            </div>
          ) : (
            <pre className="diff">
              {diff.map((l, n) => (
                <div key={n} className={`dl dl-${l.op}`}>
                  <span className="dl-op">{l.op === "add" ? "+" : l.op === "del" ? "−" : " "}</span>
                  {l.text || " "}
                </div>
              ))}
            </pre>
          )}
        </div>
      </div>
    </Modal>
  );
}

type DiffLine = { op: "same" | "add" | "del"; text: string };

/** A line diff (longest common subsequence): enough for a spec's versions. */
export function lineDiff(a: string, b: string): DiffLine[] {
  const x = a ? a.split("\n") : [];
  const y = b ? b.split("\n") : [];
  const n = x.length;
  const m = y.length;
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) lcs[i][j] = x[i] === y[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (x[i] === y[j]) (out.push({ op: "same", text: x[i] }), i++, j++);
    else if (lcs[i + 1][j] >= lcs[i][j + 1]) out.push({ op: "del", text: x[i++] });
    else out.push({ op: "add", text: y[j++] });
  }
  while (i < n) out.push({ op: "del", text: x[i++] });
  while (j < m) out.push({ op: "add", text: y[j++] });
  return out;
}
