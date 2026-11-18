import { useEffect, useState, type ReactNode } from "react";
import { appName, type CheckRun, type Docket, type DocketPage, type Evidence, type VerdictView } from "./api";
import { IconBuild, IconCheck, IconHistory, IconPR, IconPause, IconPlay, IconShield, IconX, LiveIcon, StatusIcon } from "./icons";
import { ago, agoLong, go, stamp, useData, useHub } from "./live";
import { InlineMd, Kbd, Modal } from "./ui";
import { cantRun, checkState, checkWord, duration, independence, notRun, shortSha, whyWord } from "./evidence";

/**
 * Verify, on a docket: what its done rests on, the checks Wheelhouse ran
 * itself, the checker's verdicts, and whether it's held or was closed on
 * your word. Hidden when there's nothing to say (see `verifyShown`).
 */
export function VerifySection({ d, ev, onCheck }: { d: Docket; ev: Evidence; onCheck: (c: CheckRun) => void }) {
  const hub = useHub();
  const [busy, setBusy] = useState(false);
  const why = cantRun(ev);
  const run = () => {
    if (busy || why) return;
    setBusy(true);
    hub.runChecks(d.id).catch(() => {}).finally(() => setBusy(false));
  };
  const pending = ev.needs || ev.checks.length > 0 || ev.running ? notRun(ev) : [];
  const showChecks = ev.checks.length > 0 || pending.length > 0 || ev.running;
  // Closed with nothing witnessed behind it: your word is what it rests on.
  const word = !!ev.closed_on && d.status === "done";
  const required = ev.checks.filter((c) => c.required);
  const meta = [
    ev.repo,
    required.length ? `${required.filter((c) => c.ok).length} of ${required.length} required passing` : null,
    ev.verdicts.length ? `${ev.verdicts.length} verdict${ev.verdicts.length === 1 ? "" : "s"}` : null,
  ].filter(Boolean);
  return (
    <section className="verify" id="verify">
      <div className="intent-head">
        <span className="intent-label">Verify</span>
        {meta.length > 0 && <span className="intent-meta">{meta.join(" · ")}</span>}
        <span className="spacer" />
        {(ev.configured.length > 0 || ev.running || ev.needs) && (
          <button className="btn btn-small" onClick={run} disabled={!!why || busy} title={why ?? `Run ${ev.configured.map((c) => c.name).join(", ")} now, at the docket's commit. Wheelhouse runs them itself and writes down what it saw.`}>
            {ev.running ? (
              <>
                <LiveIcon state="working" size={12} /> Running…
              </>
            ) : (
              <>
                <IconPlay size={12} /> Run checks
              </>
            )}
          </button>
        )}
      </div>

      {ev.hold && (
        <div className="callout callout-blocked vf-hold">
          <span className="vf-hold-icon">
            <IconPause size={14} />
          </span>
          <div>
            <strong>On hold</strong>
            <div className="vf-hold-why">{ev.hold}</div>
            <div className="muted small">Build and Review take no steps on it until you resume.</div>
          </div>
          <span className="spacer" />
          <button className="btn btn-small" onClick={() => void hub.resume(d.id).catch(() => {})}>
            <IconPlay size={12} /> Resume
          </button>
        </div>
      )}

      {(ev.needs || ev.evidence.length > 0 || word) && (
        <div className="vf-block">
          <div className="vf-sub">Done rests on</div>
          {ev.evidence.length > 0 || word ? (
            <ul className="vf-list">
              {word && (
                <li className="vf-ev">
                  <span className="vf-mark mark-said" title="You said so: nothing witnessed or checkable was behind it">
                    said
                  </span>
                  <span className="vf-ev-what">
                    Your word: <span className="vf-word-why">“{ev.closed_on}”</span>
                  </span>
                </li>
              )}
              {ev.evidence.map((e) => (
                <li key={`${e.how}-${e.what}`} className="vf-ev">
                  <span className={`vf-mark mark-${e.how}`} title={e.how === "witnessed" ? "Wheelhouse saw it happen and wrote it down" : "Read from a source anyone can read again"}>
                    {e.how}
                  </span>
                  <span className="vf-ev-what">
                    <InlineMd text={e.what} />
                  </span>
                  <span className="vf-at" title={e.at ? `${stamp(e.at)}${e.seq ? ` · entry #${e.seq}` : ""}` : undefined}>
                    {agoLong(e.at)}
                  </span>
                </li>
              ))}
            </ul>
          ) : (
            <div className="vf-none">
              Nothing yet but what was said. Its intent has acceptance items, so it closes on a passing check or a merged PR, or on your word.
            </div>
          )}
        </div>
      )}

      {showChecks && (
        <div className="vf-block">
          <div className="vf-sub">Checks</div>
          <div className="vf-checks">
            {ev.checks.map((c) => (
              <CheckRow key={c.name} c={c} onOpen={() => onCheck(c)} />
            ))}
            {pending.map((c) => (
              <div key={c.name} className="vf-check st-none">
                <span className="vf-icon">
                  <span className="vf-dot" />
                </span>
                <span className="vf-name">{c.name}</span>
                <span className="vf-result">{ev.running ? "running" : "not run yet"}</span>
                {!c.required && <span className="vf-opt">optional</span>}
                <span className="vf-cmd" title={c.run}>
                  {c.run}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
      {ev.configured.length === 0 && ev.needs && (
        <div className="vf-none">
          {why}.{" "}
          <a className="act-link" href="#/settings/repos">
            Open Settings
          </a>
        </div>
      )}

      {ev.verdicts.length > 0 && (
        <div className="vf-block">
          <div className="vf-sub">Verdicts</div>
          {ev.verdicts.map((v) => (
            <VerdictRow key={v.seq} v={v} />
          ))}
        </div>
      )}
    </section>
  );
}

function CheckIcon({ state }: { state: ReturnType<typeof checkState> }) {
  if (state === "pass") return <IconCheck size={13} />;
  if (state === "timeout") return <IconHistory size={13} />;
  return <IconX size={13} />;
}

function CheckRow({ c, onOpen }: { c: CheckRun; onOpen: () => void }) {
  const st = checkState(c);
  return (
    <div className={`vf-check st-${st}`} title={`${c.name}: ${checkWord(c)} at ${shortSha(c.head)}${c.dirty ? ", with uncommitted changes" : ""}\nRan because ${whyWord(c.why)} · ${stamp(c.at)} · entry #${c.seq}, witnessed`}>
      <span className="vf-icon">
        <CheckIcon state={st} />
      </span>
      <span className="vf-name">{c.name}</span>
      <span className="vf-result">{checkWord(c)}</span>
      {!c.required && (
        <span className="vf-opt" title="An optional check only informs: it doesn't hold a PR, a push, or a close">
          optional
        </span>
      )}
      <span className="vf-cmd">{c.command}</span>
      <span className="vf-meta">
        <span className="mono-s">
          {shortSha(c.head)}
          {c.dirty ? "+" : ""}
        </span>
        <span>{duration(c.ms)}</span>
        <span>{ago(c.at)}</span>
      </span>
      {c.output ? (
        <button className="btn btn-ghost btn-small vf-out" onClick={onOpen}>
          Output
        </button>
      ) : (
        <span className="vf-out" />
      )}
    </div>
  );
}

function VerdictRow({ v }: { v: VerdictView }) {
  const app = String(v.by || "").split("@")[0];
  const ind = independence(v);
  const tone = v.verdict === "pass" ? "ok" : v.verdict === "fail" ? "changes" : "muted";
  return (
    <div className="vf-verdict">
      <div className="vf-verdict-line">
        <span className={`rv-verdict v-${tone}`}>{(v.verdict ?? "no verdict").replace(/_/g, " ")}</span>
        <span className="vf-verdict-text">
          {v.n ? `Check ${v.n}` : "Check"}
          {typeof v.findings === "number" ? ` · ${v.findings} finding${v.findings === 1 ? "" : "s"}` : ""}
        </span>
        <span className="act-dot">·</span>
        <span className="muted">{appName(app)}</span>
        {ind && (
          <>
            <span className="act-dot">·</span>
            <span className={`vf-indep${v.independent ? " ok" : " warn"}`} title={v.independent ? "A session and pane other than the builder's judged it" : "The builder's own session judged its work"}>
              {ind}
            </span>
          </>
        )}
        <span className="spacer" />
        {v.head && <span className="mono-s muted">{shortSha(v.head)}</span>}
        <span className="vf-at" title={stamp(v.at)}>
          {ago(v.at)}
        </span>
        <span className="vf-mark mark-said" title="The checker said so: true if you trust it">
          said
        </span>
      </div>
      {v.summary && (
        <div className="vf-summary">
          <InlineMd text={v.summary} />
        </div>
      )}
    </div>
  );
}

/** SHA-256 of bytes, as hex, in the browser. */
async function sha256(buf: ArrayBuffer): Promise<string | null> {
  if (!crypto?.subtle) return null;
  const h = await crypto.subtle.digest("SHA-256", buf);
  return [...new Uint8Array(h)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** What a check printed, as the record pins it. */
export function CheckOutput({ c, onClose }: { c: CheckRun; onClose: () => void }) {
  const hex = (c.output || "").replace(/^sha256:/, "");
  const [got, setGot] = useState<{ text?: string; match: boolean | null; error?: string } | null>(null);
  useEffect(() => {
    let live = true;
    fetch(`/api/blobs/${hex}?name=${encodeURIComponent(`${c.name}.txt`)}`)
      .then(async (r) => {
        if (!r.ok) throw new Error(r.status === 404 ? "The output is missing from the hub" : `The hub answered ${r.status}`);
        const buf = await r.arrayBuffer();
        const h = await sha256(buf);
        if (live) setGot({ text: new TextDecoder().decode(buf), match: h === null ? null : h === hex });
      })
      .catch((e: Error) => live && setGot({ match: false, error: e.message }));
    return () => {
      live = false;
    };
  }, [hex, c.name]);
  const st = checkState(c);
  return (
    <Modal onClose={onClose} className="artifact-modal check-modal">
      <div className="art-head">
        <span className={`vf-icon vf-big st-${st}`}>
          <CheckIcon state={st} />
        </span>
        <div className="art-head-main">
          <div className="art-head-title">
            {c.name} · {checkWord(c)}
            {!c.required && <span className="muted"> · optional</span>}
          </div>
          <div className="art-head-meta">
            <span className="mono-s">{c.command}</span> · at {shortSha(c.head)}
            {c.dirty ? " with uncommitted changes" : ""} · {duration(c.ms)} · <span title={stamp(c.at)}>{agoLong(c.at)}</span>
          </div>
        </div>
      </div>
      <div className={`art-pin${got?.match === false ? " bad" : got?.match ? " ok" : ""}`}>
        <IconShield size={13} />
        <span>
          Witnessed by Wheelhouse in entry #{c.seq}: <span className="mono-s">{(c.output || "").slice(0, 19)}…</span>
          {got?.match === true ? " · these bytes match it" : got?.match === false ? " · these bytes don't match it" : got ? "" : " · checking…"}
        </span>
      </div>
      <div className="art-body">
        {got?.error ? <div className="art-error">{got.error}.</div> : !got ? <div className="art-loading muted">Loading…</div> : got.text?.trim() ? <pre className="art-pre">{got.text}</pre> : <div className="art-loading muted">It printed nothing.</div>}
      </div>
    </Modal>
  );
}

/**
 * Done, refused: the docket's acceptance items have nothing witnessed or
 * checkable behind them. Run its checks, or close it on your word, with why.
 */
export function RefusedDialog({ id, onClose }: { id: string; onClose: () => void }) {
  const hub = useHub();
  const { data } = useData<DocketPage>(`/api/dockets/${id}`);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState<"run" | "close" | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const ev = data?.evidence;
  const why = ev ? cantRun(ev) : "Loading…";
  const close = () => {
    if (!reason.trim() || busy) return;
    setBusy("close");
    hub.closeOnWord(id, reason.trim()).then(
      () => onClose(),
      (e: Error) => (setBusy(null), setErr(e.message)),
    );
  };
  const run = () => {
    if (why || busy) return;
    setBusy("run");
    hub.runChecks(id).then(
      () => (onClose(), go(`/d/${id}`)),
      (e: Error) => (setBusy(null), setErr(e.message)),
    );
  };
  return (
    <Modal onClose={onClose} className="ask-modal">
      <div className="nd-head">
        <StatusIcon status="done" size={14} />
        <span>Close {id}?</span>
      </div>
      <div className="ask-body">
        <p>
          <strong>{data?.docket.title ?? id}</strong> has acceptance items, and nothing witnessed or checkable says they're met: no passing checks, and no merged PR.
        </p>
        <p className="muted">
          {ev?.configured.length
            ? `Run its checks (${ev.configured.map((c) => c.name).join(", ")}) and close it once the required ones pass, or close it now on your word. The record will say so, with your reason.`
            : "Close it on your word: the record will say so, with your reason."}
        </p>
        <textarea
          autoFocus
          className="nd-note ask-reason"
          placeholder="Why it's done anyway, such as “Checked by hand on staging”"
          value={reason}
          rows={3}
          onChange={(e) => (setReason(e.target.value), setErr(null))}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) (e.preventDefault(), close());
          }}
        />
        {err && <div className="set-err">{err}</div>}
      </div>
      <div className="nd-foot">
        <button className="btn" onClick={run} disabled={!!why || !!busy} title={why ?? "Run them now; the results arrive on the docket"}>
          <IconPlay size={12} /> {busy === "run" ? "Starting…" : "Run checks"}
        </button>
        <span className="spacer" />
        <button className="btn" onClick={onClose}>
          Cancel
        </button>
        <button className="btn btn-primary" disabled={!reason.trim() || !!busy} onClick={close} title={reason.trim() ? undefined : "Say why first"}>
          {busy === "close" ? "Closing…" : "Close on your word"} <Kbd>⌘↩</Kbd>
        </button>
      </div>
    </Modal>
  );
}

/** Hold a docket, with why. */
export function HoldDialog({ id, onClose }: { id: string; onClose: () => void }) {
  const hub = useHub();
  const d = hub.byId.get(id);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const hold = () => {
    if (busy) return;
    setBusy(true);
    hub.hold(id, reason).then(
      () => onClose(),
      (e: Error) => (setBusy(false), setErr(e.message)),
    );
  };
  return (
    <Modal onClose={onClose} className="ask-modal">
      <div className="nd-head">
        <IconPause size={14} />
        <span>
          Hold {id}
          {d ? <span className="muted"> · {d.title}</span> : null}
        </span>
      </div>
      <input
        autoFocus
        className="nd-title ask-input"
        placeholder="Why? Such as “Waiting on the DBA's review”"
        value={reason}
        onChange={(e) => (setReason(e.target.value), setErr(null))}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.preventDefault(), hold());
        }}
      />
      {err && <div className="set-err ask-pad">{err}</div>}
      <div className="nd-foot">
        <span className="muted small">Build and Review take no steps on it until you resume. Your Inbox lists it under Attention.</span>
        <span className="spacer" />
        <button className="btn btn-primary" disabled={busy} onClick={hold}>
          {busy ? "Holding…" : "Hold"} <Kbd>↩</Kbd>
        </button>
      </div>
    </Modal>
  );
}

/** An attention item's icon, by its kind. */
export function AttentionIcon({ kind, size = 14 }: { kind: string; size?: number }): ReactNode {
  const wrap = (cls: string, icon: ReactNode) => <span className={`att-icon att-${cls}`}>{icon}</span>;
  switch (kind) {
    case "held":
      return wrap("held", <IconPause size={size} />);
    case "failing":
      return wrap("failing", <IconX size={size} />);
    case "build":
      return wrap("build", <IconBuild size={size} />);
    case "review":
      return wrap("review", <IconPR size={size} />);
    case "long":
      return <LiveIcon state="working" size={size} />;
    case "quiet":
      return <LiveIcon state="idle" size={size} />;
    case "word":
      return wrap("word", <IconCheck size={size} />);
    default:
      return <LiveIcon state="idle" size={size} />;
  }
}
