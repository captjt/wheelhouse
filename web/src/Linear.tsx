import { useEffect, useRef, useState } from "react";
import { get, linearStatus, type LinearIssue, type LinearRef } from "./api";
import { IconExternal, IconIssue, StatusIcon } from "./icons";
import { Modal, useToast } from "./ui";

type Found = { issues: LinearIssue[] };

/** Search Linear: your open assigned issues, or by words, or a key. */
export function LinearPicker({ onPick, busy }: { onPick: (i: LinearIssue) => void; busy?: string | null }) {
  const [q, setQ] = useState("");
  const [list, setList] = useState<LinearIssue[] | null>(null);
  const [err, setErr] = useState<{ msg: string; setup: boolean } | null>(null);
  const [sel, setSel] = useState(0);
  const toast = useToast();
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let live = true;
    const t = window.setTimeout(() => {
      get<Found>(`/api/linear/search?q=${encodeURIComponent(q)}`).then(
        (r) => {
          if (!live) return;
          setList(r.issues);
          setErr(null);
          setSel(0);
        },
        (e: Error) => live && setErr({ msg: e.message, setup: e.message.includes("wh setup-linear") }),
      );
    }, 220);
    return () => {
      live = false;
      window.clearTimeout(t);
    };
  }, [q]);
  useEffect(() => {
    box.current?.querySelector(".lp-item.sel")?.scrollIntoView({ block: "nearest" });
  }, [sel]);
  if (err?.setup)
    return (
      <div className="lp-setup">
        <IconIssue size={20} />
        <div>
          <div className="lp-setup-title">Connect Linear</div>
          <div className="muted">
            Run <code>wh setup-linear</code> in a terminal and paste a personal API key (read access is enough). Then restart the hub.
          </div>
        </div>
        <button
          className="btn btn-small"
          onClick={() => {
            navigator.clipboard?.writeText("wh setup-linear");
            toast("Copied “wh setup-linear”");
          }}
        >
          Copy command
        </button>
      </div>
    );
  return (
    <div className="lp">
      <input
        autoFocus
        className="lp-input"
        placeholder="Search Linear, or type a key like ENG-142"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        onKeyDown={(e) => {
          if (!list?.length) return;
          if (e.key === "ArrowDown") (e.preventDefault(), setSel((s) => Math.min(list.length - 1, s + 1)));
          else if (e.key === "ArrowUp") (e.preventDefault(), setSel((s) => Math.max(0, s - 1)));
          else if (e.key === "Enter") (e.preventDefault(), onPick(list[sel]));
        }}
      />
      <div className="lp-list" ref={box}>
        {err && <div className="lp-empty">{err.msg}</div>}
        {!err && list === null && <div className="lp-empty">Asking Linear…</div>}
        {!err && list?.length === 0 && <div className="lp-empty">No issues match.</div>}
        {!q && list && list.length > 0 && <div className="pal-group">Assigned to you</div>}
        {list?.map((i, n) => (
          <button key={i.id} className={`lp-item${n === sel ? " sel" : ""}`} onMouseMove={() => setSel(n)} onClick={() => onPick(i)} disabled={!!busy}>
            <StatusIcon status={linearStatus(i.state_type)} />
            <span className="lp-key">{i.identifier}</span>
            <span className="lp-title">{busy === i.identifier ? "Opening…" : i.title}</span>
            {i.state && <span className="lp-state">{i.state}</span>}
          </button>
        ))}
      </div>
    </div>
  );
}

export function LinearModal({ title, onPick, onClose, busy }: { title: string; onPick: (i: LinearIssue) => void; onClose: () => void; busy?: string | null }) {
  return (
    <Modal onClose={onClose} className="linear-modal">
      <div className="nd-head">
        <IconIssue size={14} />
        <span>{title}</span>
      </div>
      <LinearPicker onPick={onPick} busy={busy} />
    </Modal>
  );
}

/** The docket's Linear issue, as a property: key, state, and a link out. */
export function LinearChip({ l }: { l: LinearRef }) {
  return (
    <a className="prop-btn" href={l.url || "#"} target="_blank" rel="noreferrer" title={`${l.key}: ${l.title ?? ""} (open in Linear)`}>
      <StatusIcon status={linearStatus(l.state_type)} />
      <span className="prop-text linear-chip">
        {l.key}
        {l.state && <span className="muted"> · {l.state}</span>}
        <IconExternal size={11} />
      </span>
    </a>
  );
}
