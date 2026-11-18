import { useEffect, useMemo, useState, type ReactNode } from "react";
import { appName, get, getText, type Docket, type EntryView, type RecordEntry, type RecordView } from "./api";
import { AppAvatar, HubAvatar, IconBraces, IconDownload, IconExternal, IconFile, IconGlobe, IconImage, IconLinkOut, IconShield, PersonAvatar } from "./icons";
import { agoLong, stamp, useHub } from "./live";
import { InlineMd, Modal, Prose } from "./ui";

/**
 * Artifacts: what a docket's work made, kept with it (the record pins each
 * file's bytes by hash). And the record itself, readable: every signed
 * entry, what the check covers, and what it doesn't.
 */

// ---- artifacts ----

const sizeOf = (n?: number | null) => (n == null ? "" : n < 1024 ? `${n} B` : n < 1 << 20 ? `${Math.round(n / 1024)} KB` : `${(n / (1 << 20)).toFixed(1)} MB`);
const hexOf = (digest?: string | null) => (digest || "").replace(/^sha256:/, "");
export const blobUrl = (e: EntryView, download = false) => `/api/blobs/${hexOf(e.data?.digest)}?name=${encodeURIComponent(e.data?.name || "file")}${download ? "&download=1" : ""}`;

function kindOf(e: EntryView): "link" | "markdown" | "json" | "html" | "image" | "pdf" | "text" | "file" {
  if (e.data?.url) return "link";
  const t = String(e.data?.media_type || "");
  if (t === "text/markdown") return "markdown";
  if (t === "application/json") return "json";
  if (t === "text/html") return "html";
  if (t.startsWith("image/")) return "image";
  if (t === "application/pdf") return "pdf";
  if (t.startsWith("text/")) return "text";
  return "file";
}

function ArtifactIcon({ e, size = 16 }: { e: EntryView; size?: number }) {
  const k = kindOf(e);
  return k === "link" ? <IconLinkOut size={size} /> : k === "json" ? <IconBraces size={size} /> : k === "html" ? <IconGlobe size={size} /> : k === "image" ? <IconImage size={size} /> : <IconFile size={size} />;
}

/** Who kept it, in words. */
function keptBy(e: EntryView): string {
  if (e.actor.type === "person") return "You";
  if (e.actor.type === "agent") return appName(String(e.actor.id).split("@")[0]);
  return "Wheelhouse";
}

/** The docket's artifacts, grouped (Round 3, Plan, Attached), newest group first. */
export function ArtifactsSection({ entries, onOpen }: { entries: EntryView[]; onOpen: (e: EntryView) => void }) {
  const [all, setAll] = useState(false);
  const groups = useMemo(() => {
    const list = entries.filter((e) => e.kind === "artifact");
    const m = new Map<string, EntryView[]>();
    for (const e of list) {
      const g = e.data?.group || "Kept";
      m.set(g, [...(m.get(g) ?? []), e]);
    }
    return [...m.entries()].sort((a, b) => Math.max(...b[1].map((x) => x.seq)) - Math.max(...a[1].map((x) => x.seq)));
  }, [entries]);
  const total = groups.reduce((n, [, l]) => n + l.length, 0);
  if (!total) return null;
  const shown = all ? groups : groups.slice(0, 2);
  return (
    <section className="artifacts">
      <div className="artifacts-head">
        <span className="artifacts-label">Artifacts</span>
        <span className="muted small">{total} kept · each pinned in the record by its hash</span>
      </div>
      {shown.map(([g, list]) => (
        <div key={g} className="art-group">
          {groups.length > 1 && <div className="art-group-label">{g}</div>}
          <div className="art-grid">
            {list.map((e) => (
              <button key={e.seq} className="art-card" onClick={() => onOpen(e)} title={`${e.body || e.data?.name}\n#${e.seq} · ${e.data?.digest || e.data?.url || ""}`}>
                <span className={`art-icon k-${kindOf(e)}`}>
                  <ArtifactIcon e={e} />
                </span>
                <span className="art-main">
                  <span className="art-title">{e.body || e.data?.name}</span>
                  <span className="art-meta">
                    {e.data?.url ? (e.data?.provider === "claude" ? "Claude artifact" : new URL(e.data.url).host) : `${e.data?.name}${e.data?.size != null ? ` · ${sizeOf(e.data.size)}` : ""}`}
                  </span>
                </span>
              </button>
            ))}
          </div>
        </div>
      ))}
      {groups.length > 2 && (
        <button className="link art-more" onClick={() => setAll((a) => !a)}>
          {all ? "Show fewer" : `Show all ${groups.length} groups`}
        </button>
      )}
    </section>
  );
}

/** SHA-256 of bytes, as hex, in the browser. */
async function sha256(buf: ArrayBuffer): Promise<string | null> {
  if (!crypto?.subtle) return null;
  const h = await crypto.subtle.digest("SHA-256", buf);
  return [...new Uint8Array(h)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

type Loaded = { text?: string; json?: unknown; match: boolean | null; error?: string };

/** One artifact: shown as what it is, with its place in the record. */
export function ArtifactViewer({ e, onClose }: { e: EntryView; onClose: () => void }) {
  const k = kindOf(e);
  const [got, setGot] = useState<Loaded | null>(null);
  useEffect(() => {
    if (k === "link") return;
    let live = true;
    fetch(blobUrl(e))
      .then(async (r) => {
        if (!r.ok) throw new Error(r.status === 404 ? "The file is missing from the hub" : `The hub answered ${r.status}`);
        const buf = await r.arrayBuffer();
        const hex = await sha256(buf);
        const match = hex === null ? null : hex === hexOf(e.data?.digest);
        const text = k === "markdown" || k === "json" || k === "text" ? new TextDecoder().decode(buf) : undefined;
        let json: unknown;
        if (k === "json" && text) {
          try {
            json = JSON.parse(text);
          } catch {
            json = undefined;
          }
        }
        if (live) setGot({ text, json, match });
      })
      .catch((err: Error) => live && setGot({ match: false, error: err.message }));
    return () => {
      live = false;
    };
  }, [e, k]);

  let body: ReactNode;
  if (k === "link")
    body = (
      <div className="art-link">
        <p>
          {e.data?.provider === "claude" ? "A Claude artifact, on claude.ai. It opens there; the record keeps the link, not what's at it." : "A link. The record keeps the link, not what's at it."}
        </p>
        <a className="btn btn-primary" href={e.data.url} target="_blank" rel="noreferrer">
          <IconExternal size={13} /> Open {e.data?.provider === "claude" ? "on claude.ai" : "it"}
        </a>
        <div className="mono-s muted art-url">{e.data.url}</div>
      </div>
    );
  else if (got?.error) body = <div className="art-error">{got.error}.</div>;
  else if (!got) body = <div className="art-loading muted">Loading…</div>;
  else if (k === "markdown") body = <Prose text={got.text ?? ""} />;
  else if (k === "json") body = <JsonView value={got.json} text={got.text ?? ""} role={e.data?.role} />;
  else if (k === "text") body = <pre className="art-pre">{got.text}</pre>;
  else if (k === "image") body = <img className="art-img" src={blobUrl(e)} alt={e.body || e.data?.name} />;
  else if (k === "html" || k === "pdf") body = <iframe className="art-frame" src={blobUrl(e)} title={e.body || e.data?.name} sandbox={k === "html" ? "allow-scripts allow-popups" : undefined} />;
  else body = <div className="art-loading muted">No preview for this kind of file. Download it to open it.</div>;

  return (
    <Modal onClose={onClose} className="artifact-modal">
      <div className="art-head">
        <span className={`art-icon k-${k}`}>
          <ArtifactIcon e={e} size={18} />
        </span>
        <div className="art-head-main">
          <div className="art-head-title">{e.body || e.data?.name}</div>
          <div className="art-head-meta">
            {e.data?.name}
            {e.data?.size != null && ` · ${sizeOf(e.data.size)}`} · kept by {keptBy(e)} · <span title={stamp(e.at)}>{agoLong(e.at)}</span>
          </div>
        </div>
        {k !== "link" && (
          <span className="art-actions">
            <a className="btn btn-small btn-ghost" href={blobUrl(e)} target="_blank" rel="noreferrer" title="Open it in a new tab">
              <IconExternal size={13} />
            </a>
            <a className="btn btn-small" href={blobUrl(e, true)} title="Download it">
              <IconDownload size={13} /> <span className="m-hide">Download</span>
            </a>
          </span>
        )}
      </div>
      <div className={`art-pin${got?.match === false ? " bad" : got?.match ? " ok" : ""}`}>
        <IconShield size={13} />
        {k === "link" ? (
          <span>
            Linked in entry #{e.seq}, signed by {keptBy(e)}.
          </span>
        ) : (
          <span>
            Pinned by entry #{e.seq}: <span className="mono-s">{String(e.data?.digest || "").slice(0, 19)}…</span>
            {got?.match === true ? " · these bytes match it" : got?.match === false ? " · these bytes don't match it" : got ? "" : " · checking…"}
          </span>
        )}
      </div>
      <div className={`art-body k-${k}`}>{body}</div>
    </Modal>
  );
}

/** JSON, readably: a review draft as a review, merge risk as a table, the rest pretty. */
function JsonView({ value, text, role }: { value: unknown; text: string; role?: string }) {
  const [raw, setRaw] = useState(false);
  const v = value as Record<string, unknown> | undefined;
  const findings = v && Array.isArray(v.findings) ? (v.findings as Record<string, unknown>[]) : null;
  const risk = v && ["alignment", "impact", "verification", "rollback_cost", "error_detection"].every((c) => typeof v[c] === "object") ? v : null;
  const pretty = v === undefined ? text : JSON.stringify(v, null, 2);
  const shaped = !raw && (findings || risk);
  return (
    <div className="json-view">
      {(findings || risk) && (
        <div className="seg seg-text json-seg">
          <button className={!raw ? "on" : ""} onClick={() => setRaw(false)}>
            {findings ? "Review" : "Merge risk"}
          </button>
          <button className={raw ? "on" : ""} onClick={() => setRaw(true)}>
            JSON
          </button>
        </div>
      )}
      {shaped && findings && v ? (
        <div className="rv-draft-view">
          <div className="rdv-event">
            <span className={`rv-verdict v-${String(v.event).toLowerCase()}`}>{String(v.event ?? "").replace("_", " ").toLowerCase()}</span>
            {role === "checked" && <span className="muted small">checked by the checker</span>}
          </div>
          {typeof v.verdict === "string" && <Prose text={v.verdict} />}
          {typeof v.implementation === "string" && (
            <p className="muted small">
              <b>What it changes:</b> <InlineMd text={v.implementation} />
            </p>
          )}
          {findings.map((f, i) => (
            <div key={i} className={`rdv-finding sev-${String(f.severity)}`}>
              <div className="rdv-f-head">
                <span className="rdv-sev">{String(f.id ?? "")} · {String(f.severity ?? "")}</span>
                {typeof f.path === "string" && (
                  <span className="mono-s muted">
                    {f.path}
                    {f.line ? `:${f.line}` : ""}
                  </span>
                )}
                {Array.isArray(f.found_by) && <span className="muted small">found by {(f.found_by as string[]).join(", ")}</span>}
              </div>
              <div className="rdv-f-title">{String(f.bullet ?? f.title ?? f.plus_one_label ?? "")}</div>
              {typeof f.body === "string" && <Prose text={f.body} />}
              {typeof f.suggestion === "string" && <pre className="art-pre">{f.suggestion}</pre>}
              {typeof f.evidence === "string" && (
                <p className="muted small">
                  <b>Evidence:</b> <InlineMd text={f.evidence} />
                </p>
              )}
            </div>
          ))}
          {typeof v.checked === "string" && (
            <p className="muted small">
              Checked: <InlineMd text={v.checked} />
            </p>
          )}
        </div>
      ) : shaped && risk ? (
        <table className="rdv-risk">
          <tbody>
            {["alignment", "impact", "verification", "rollback_cost", "error_detection"].map((c) => {
              const r = risk[c] as { verdict?: string; gaps?: string[] };
              return (
                <tr key={c}>
                  <th>{c.replace("_", " ")}</th>
                  <td className={`risk-v v-${r.verdict}`}>{r.verdict}</td>
                  <td>{(r.gaps ?? []).length ? <Prose text={(r.gaps ?? []).map((g) => `- ${g}`).join("\n")} /> : <span className="muted">No gaps</span>}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      ) : (
        <pre className="art-pre">{pretty}</pre>
      )}
    </div>
  );
}

// ---- the record ----

const MARKS: Record<string, string> = {
  witnessed: "Wheelhouse saw it happen and wrote it down.",
  checkable: "Read from a source anyone can read again (GitHub, Linear).",
  said: "A person or an agent said it: true if you trust them.",
};

function who(e: { actor: EntryView["actor"] }, owner: string) {
  if (e.actor.type === "person") return { name: "You", avatar: <PersonAvatar name={owner} size={16} /> };
  if (e.actor.type === "agent") {
    const app = String(e.actor.id).split("@")[0];
    return { name: appName(app), avatar: <AppAvatar app={app} size={16} /> };
  }
  return { name: "Wheelhouse", avatar: <HubAvatar size={16} /> };
}

const keyName = (k?: string | null) => (k === "owner" ? "your key" : k === "hub" ? "Wheelhouse's key" : k?.startsWith("agent-") ? `${appName(k.slice(6).split("@")[0])}'s key` : k || "an unknown key");

/** The docket's record, readable: what the check means, who signed, and every entry. */
export function RecordViewer({ d, onClose }: { d: Docket; onClose: () => void }) {
  const hub = useHub();
  const owner = hub.state?.owner || "you";
  const [rec, setRec] = useState<RecordView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [mark, setMark] = useState<string>("all");
  const [q, setQ] = useState("");
  const [open, setOpen] = useState<number | null>(null);
  useEffect(() => {
    get<RecordView>(`/api/dockets/${d.id}/record`).then(setRec, (e: Error) => setError(e.message));
  }, [d.id]);
  const shown = useMemo(() => {
    const t = q.trim().toLowerCase();
    return (rec?.entries ?? [])
      .filter((e) => mark === "all" || e.mark === mark || (mark === "problems" && e.problems.length > 0))
      .filter((e) => !t || `${e.kind} ${e.body ?? ""} ${JSON.stringify(e.data ?? "")} #${e.seq}`.toLowerCase().includes(t))
      .slice()
      .reverse();
  }, [rec, mark, q]);
  const v = rec?.verify;
  return (
    <Modal onClose={onClose} className="record-modal">
      <div className="rec-head">
        <div className="rec-title">
          {d.id}'s record
          {rec && <span className="muted"> · {rec.entries.length} entries · head <span className="mono-s">{rec.short_head}</span></span>}
        </div>
        <a className="btn btn-small" href={`/api/dockets/${d.id}/export`} download={`${d.id}.jsonl`}>
          <IconDownload size={13} /> <span className="m-hide">{d.id}.jsonl</span>
        </a>
      </div>
      {error && <div className="art-error">{error}</div>}
      {v && (
        <div className={`rec-verdict ${v.ok ? "ok" : "bad"}`}>
          <IconShield size={16} />
          <div>
            <strong>{v.ok ? "Verified: the record is intact." : "The record doesn't check."}</strong>
            {v.ok ? (
              <span> All {v.entries} entries are as they were written: none changed, removed, added in between or reordered, each signed by the key it names, and every file they keep matches its hash.</span>
            ) : (
              <ul>
                {v.problems.slice(0, 6).map((p) => (
                  <li key={p}>{p}</li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}
      <details className="rec-explain">
        <summary>What this means</summary>
        <ul>
          <li>
            <b>Each entry is signed</b> by whoever wrote it, with their own key on this Mac: you, Wheelhouse, or an agent (each app has its own). Nobody can sign as someone else through Wheelhouse.
          </li>
          <li>
            <b>Each entry is chained</b> to the one before by hash. Its id is the hash of its exact bytes. The head ({rec?.short_head ?? "…"}) names this exact record: a PR footer that says <span className="mono-s">{d.id} @ {rec?.short_head ?? "…"}</span> points here.
          </li>
          <li>
            <b>Verified</b> means nothing was changed, removed, slipped in or reordered since it was written, and every kept file is byte for byte what the record pinned.
          </li>
          <li>
            <b>It doesn't mean an entry is true.</b> Each one's mark says how it's known. <i>Witnessed</i>: {MARKS.witnessed} <i>Checkable</i>: {MARKS.checkable} <i>Said</i>: {MARKS.said}
          </li>
          <li>
            <b>The export</b> ({d.id}.jsonl) checks without the hub: <span className="mono-s">wh verify --file {d.id}.jsonl</span>.
          </li>
        </ul>
      </details>
      {rec && (
        <div className="rec-keys">
          <span className="muted small">Signed by</span>
          {rec.keys.map((k) => (
            <span key={k.keyid} className="rec-key" title={k.keyid}>
              {keyName(k.name)} <span className="muted">· {k.entries}</span>
              <span className="mono-s muted"> {k.keyid.replace("ed25519:", "").slice(0, 8)}</span>
            </span>
          ))}
        </div>
      )}
      <div className="rec-bar">
        <div className="seg seg-text">
          {["all", "witnessed", "checkable", "said", ...(v && !v.ok ? ["problems"] : [])].map((m) => (
            <button key={m} className={mark === m ? "on" : ""} onClick={() => setMark(m)} title={MARKS[m] ?? ""}>
              {m === "all" ? "All" : m[0].toUpperCase() + m.slice(1)}
            </button>
          ))}
        </div>
        <input className="rec-search" placeholder="Find in the record…" value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
      <div className="rec-list">
        {!rec && !error && <div className="art-loading muted">Loading…</div>}
        {shown.map((e) => (
          <RecordRow key={e.seq} e={e} d={d} owner={owner} open={open === e.seq} onToggle={() => setOpen((o) => (o === e.seq ? null : e.seq))} />
        ))}
      </div>
    </Modal>
  );
}

function RecordRow({ e, d, owner, open, onToggle }: { e: RecordEntry; d: Docket; owner: string; open: boolean; onToggle: () => void }) {
  const w = who(e, owner);
  const [env, setEnv] = useState<{ envelope: string; payload: string } | null>(null);
  const summary = e.body ? e.body.replace(/\s+/g, " ").slice(0, 140) : e.data && Object.keys(e.data).length ? Object.entries(e.data).filter(([, x]) => typeof x !== "object").slice(0, 3).map(([k, x]) => `${k}: ${x}`).join(" · ") : "";
  return (
    <div className={`rec-row${open ? " open" : ""}${e.problems.length ? " bad" : ""}`}>
      <button className="rec-line" onClick={onToggle}>
        <span className="rec-seq mono-s">#{e.seq}</span>
        <span className="rec-who">{w.avatar}</span>
        <span className="rec-kind">{e.kind}</span>
        <span className="rec-sum">{summary}</span>
        <span className={`act-mark mark-${e.mark}`} title={MARKS[e.mark]}>
          {e.mark}
        </span>
        <span className="rec-at" title={stamp(e.at)}>
          {agoLong(e.at)}
        </span>
      </button>
      {open && (
        <div className="rec-detail">
          {e.problems.length > 0 && (
            <ul className="rec-problems">
              {e.problems.map((p) => (
                <li key={p}>{p}</li>
              ))}
            </ul>
          )}
          <dl className="rec-facts">
            <dt>Written by</dt>
            <dd>
              {w.name} <span className="muted">({e.actor.type}: {e.actor.id})</span>
            </dd>
            <dt>Signed with</dt>
            <dd>
              {keyName(e.key)} <span className="mono-s muted">{e.keyid}</span>
            </dd>
            <dt>When</dt>
            <dd>{stamp(e.at)}</dd>
            <dt>How it's known</dt>
            <dd>
              <i>{e.mark}</i>: {MARKS[e.mark]}
            </dd>
            <dt>Id</dt>
            <dd className="mono-s">{e.id}</dd>
            <dt>Chain</dt>
            <dd className="mono-s">{e.chain}</dd>
            {e.blobs.length > 0 && (
              <>
                <dt>Files</dt>
                <dd>
                  {e.blobs.map((b) => (
                    <a key={b} className="mono-s act-link" href={`/api/blobs/${b.replace("sha256:", "")}?name=${encodeURIComponent(e.data?.name || "file")}`} target="_blank" rel="noreferrer">
                      {b}
                    </a>
                  ))}
                </dd>
              </>
            )}
          </dl>
          {e.body && (
            <div className="rec-body">
              <Prose text={e.body} />
            </div>
          )}
          {e.data && Object.keys(e.data).length > 0 && <pre className="art-pre rec-data">{JSON.stringify(e.data, null, 2)}</pre>}
          {env ? (
            <div className="rec-env">
              <div className="muted small">The envelope, exactly as stored and signed (DSSE, Ed25519):</div>
              <pre className="art-pre">{JSON.stringify(JSON.parse(env.envelope), null, 2)}</pre>
              <div className="muted small">Its payload, the bytes the signature covers:</div>
              <pre className="art-pre">{env.payload}</pre>
            </div>
          ) : (
            <button className="btn btn-small btn-ghost" onClick={() => getText(`/api/dockets/${d.id}/record/${e.seq}`).then((t) => setEnv(JSON.parse(t)))}>
              Show the signed envelope
            </button>
          )}
        </div>
      )}
    </div>
  );
}
