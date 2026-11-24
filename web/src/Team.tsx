import { useState } from "react";
import { post, type TeamStatus } from "./api";
import { IconChevronDown, IconTeam, LiveIcon } from "./icons";
import { ago, useHub } from "./live";
import { LargeTitle } from "./mobile";
import { useToast } from "./ui";

/** A handle as a small round mark: the same color for a person everywhere. */
export function Person({ handle, you }: { handle: string; you?: boolean }) {
  let h = 0;
  for (const c of handle) h = (h * 31 + c.charCodeAt(0)) % 360;
  return (
    <span className="tm-person" title={you ? `${handle} (you)` : handle}>
      <span className="tm-dot" style={{ ["--hue" as string]: h }}>
        {handle.slice(0, 1).toUpperCase()}
      </span>
      {handle}
    </span>
  );
}

/** Your workspace: its shared dockets, who's on each, its members, and what's waiting to go. */
export function TeamView() {
  const hub = useHub();
  const toast = useToast();
  const t: TeamStatus | undefined = hub.state?.team;
  const [shut, setShut] = useState<Set<string>>(new Set());
  const w = t?.workspace;
  const toggle = (k: string) =>
    setShut((s) => {
      const n = new Set(s);
      if (n.has(k)) n.delete(k);
      else n.add(k);
      return n;
    });
  const outbox = (n: number, action: "send" | "drop") =>
    post(`/api/team/outbox/${n}`, { action }).then(
      () => (toast(action === "send" ? `#${n} goes to the workspace` : `#${n} dropped`), hub.reload()),
      (e: Error) => toast(e.message, "error"),
    );
  const sync = () =>
    post("/api/team/sync", {}).then(
      () => (toast("Up to date with the workspace"), hub.reload()),
      (e: Error) => toast(e.message, "error"),
    );
  const dockets = t?.dockets ?? [];
  const members = (t?.members ?? []).filter((m) => m.active);
  const waiting = (t?.outbox ?? []).filter((o) => o.status !== "queued");
  const queued = (t?.outbox ?? []).filter((o) => o.status === "queued").length;
  const head = (key: string, label: string, n: number) => (
    <button className="group-head" onClick={() => toggle(key)}>
      <span className={`caret${shut.has(key) ? " closed" : ""}`}>
        <IconChevronDown size={12} />
      </span>
      <span className="group-label">{label}</span>
      <span className="view-count">{n}</span>
    </button>
  );
  return (
    <div className="view">
      <header className="view-head">
        <span className="view-title">
          <IconTeam size={15} /> {w ? w.name : "Team"} {w && <span className="view-count">{dockets.length}</span>}
        </span>
        <span className="spacer" />
        {w && (
          <button className="btn btn-ghost" onClick={sync} title="Send what's waiting and take in what's new, now">
            Sync
          </button>
        )}
      </header>
      <LargeTitle title={w ? w.name : "Team"} count={dockets.length} />
      {!w ? (
        <div className="view-empty rv-off">
          <IconTeam size={22} />
          <p>
            <strong>This Wheelhouse isn't in a workspace.</strong> A workspace joins your Wheelhouse with your teammates': you share dockets you choose, see who's on each, and your agents work with theirs across Macs. Your panes, credentials and approvals stay on your Mac.
          </p>
          <p className="muted small">
            Ask a workspace admin for an invite, then in a terminal: <code>wh team join CODE</code>. To make one: <code>wh team create</code> (docs/teams.md).
          </p>
        </div>
      ) : (
        <>
          <div className="rv-bar">
            <span className={`rv-pill ${t?.online ? "on" : "paused"}`}>{t?.online ? "Connected" : "Not connected"}</span>
            <span className="rv-cfg">
              <span>
                {w.prefix} · you're <b>{w.you}</b>
              </span>
              <span className="act-dot">·</span>
              <span className="mono-s" title="The workspace hub">
                {w.url.replace(/^https?:\/\//, "")}
              </span>
              {queued > 0 && (
                <>
                  <span className="act-dot">·</span>
                  <span>{queued} waiting to send</span>
                </>
              )}
            </span>
            {!t?.online && t?.error && <span className="rv-err">{t.error}</span>}
          </div>
          <div className="list" style={{ ["--idch" as string]: Math.max(4, ...dockets.map((d) => d.id.length)) }}>
            {waiting.length > 0 && (
              <div>
                {head("outbox", "Held back", waiting.length)}
                {!shut.has("outbox") &&
                  waiting.map((o) => (
                    <div key={o.n} className="row tm-row">
                      <span className="row-live">
                        <LiveIcon state={o.status === "held" ? "blocked" : "error"} />
                      </span>
                      <span className="row-id">{o.docket}</span>
                      <span className="row-title">{o.status === "held" ? "Held: it looks like it holds a secret" : "Refused by the workspace"}</span>
                      {o.why && <span className="row-note">{o.why}</span>}
                      <span className="row-end">
                        {o.status === "held" && (
                          <button className="btn btn-small btn-ghost" onClick={() => outbox(o.n, "send")} title="Send it to the workspace after all">
                            Send anyway
                          </button>
                        )}
                        <button className="btn btn-small btn-ghost" onClick={() => outbox(o.n, "drop")}>
                          Drop
                        </button>
                        <span className="row-age">{ago(o.at)}</span>
                      </span>
                    </div>
                  ))}
              </div>
            )}
            <div>
              {head("dockets", "Shared dockets", dockets.length)}
              {!shut.has("dockets") &&
                (dockets.length === 0 ? (
                  <div className="tm-empty muted small">
                    Nothing shared yet. Share one of yours with <code>wh team share D-12</code>, or open one shared with <code>wh new --team "Title"</code>.
                  </div>
                ) : (
                  dockets.map((d) => {
                    const agents = d.agents ?? [];
                    const whose = [...new Set(agents.map((a) => a.member ?? w.you))];
                    return (
                      <a key={d.id} className="row tm-row" href={`#/d/${d.id}`}>
                        <span className="row-live">
                          <LiveIcon state={d.halted ? "error" : agents.length > 0 ? "working" : "idle"} />
                        </span>
                        <span className="row-id">{d.id}</span>
                        <span className="row-title">{d.title ?? d.id}</span>
                        {d.halted && <span className="row-note">Not followed: {d.halted}</span>}
                        <span className="row-end tm-people">
                          {d.owners.map((o) => (
                            <Person key={`o-${o}`} handle={o} you={o === w.you} />
                          ))}
                          {d.members.length > 0 && <span className="muted small">+{d.members.length}</span>}
                          {agents.length > 0 && (
                            <span className="chip-mini" title={agents.map((a) => `${a.member ?? w.you}: ${a.role ?? a.app}`).join("\n")}>
                              {agents.length} agent{agents.length === 1 ? "" : "s"} · {whose.join(", ")}
                            </span>
                          )}
                          <span className="row-status-text muted small">{(d.status ?? "").replace("_", " ")}</span>
                        </span>
                      </a>
                    );
                  })
                ))}
            </div>
            <div>
              {head("members", "Members", members.length)}
              {!shut.has("members") &&
                members.map((m) => (
                  <div key={m.handle} className="row tm-row">
                    <Person handle={m.handle} you={m.you} />
                    <span className="row-title muted">{m.name && m.name !== m.handle ? m.name : ""}</span>
                    <span className="row-end">
                      {m.role === "admin" && <span className="chip-mini">admin</span>}
                      {m.you && <span className="chip-mini">you</span>}
                      <span className="muted small mono-s m-hide">{m.login}</span>
                      <span className="row-age" title={`Joined ${m.joined_at}`}>
                        {ago(m.joined_at)}
                      </span>
                    </span>
                  </div>
                ))}
            </div>
          </div>
        </>
      )}
    </div>
  );
}
