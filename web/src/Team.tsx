import { useState } from "react";
import { post, type TeamStatus } from "./api";
import { IconChevronDown, IconRefresh, IconTeam, IconTerminal, LiveIcon, StatusIcon } from "./icons";
import { ago, useHub } from "./live";
import { LargeTitle } from "./mobile";
import { useToast } from "./ui";

/** A steady hue for a handle: the same person is the same color everywhere. */
function hue(handle: string): number {
  let h = 0;
  for (const c of handle) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

/** A person's round mark: their initial on their color. */
export function PersonDot({ handle, size = 18 }: { handle: string; size?: number }) {
  return (
    <span className="tm-dot" style={{ ["--hue" as string]: hue(handle), width: size, height: size, fontSize: size * 0.52 }}>
      {handle.slice(0, 1).toUpperCase()}
    </span>
  );
}

/** A handle with its mark, for a properties rail. */
export function Person({ handle, you }: { handle: string; you?: boolean }) {
  return (
    <span className="tm-person" title={you ? `${handle} (you)` : handle}>
      <PersonDot handle={handle} size={16} />
      {handle}
    </span>
  );
}

/** Several people as overlapping marks, the way Linear shows a team. */
function Stack({ handles, you, label }: { handles: string[]; you?: string; label: string }) {
  if (handles.length === 0) return null;
  const shown = handles.slice(0, 4);
  return (
    <span className="tm-stack" title={`${label}: ${handles.map((h) => (h === you ? `${h} (you)` : h)).join(", ")}`}>
      {shown.map((h) => (
        <PersonDot key={h} handle={h} />
      ))}
      {handles.length > shown.length && <span className="tm-more">+{handles.length - shown.length}</span>}
    </span>
  );
}

/** What an outbox entry was held for, in a few words. */
function heldWhat(o: { status: string; why?: string | null }) {
  if (o.status === "refused") return "Refused by the workspace";
  const m = o.why?.match(/holds (.+)$/);
  return m ? `Looks like ${m[1]}` : "Looks like a secret";
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
      () => (toast(action === "send" ? "Sent to the workspace" : "Dropped"), hub.reload()),
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
  const conn = t?.online ? "Connected" : "Not connected";
  return (
    <div className="view">
      <header className="view-head">
        <span className="view-title">
          <IconTeam size={15} /> {w ? w.name : "Team"}
        </span>
        {w && <span className="tm-prefix">{w.prefix}</span>}
        <span className="spacer" />
        {w && (
          <>
            <span className={`tm-conn ${t?.online ? "on" : "off"}`} title={`${w.url}${t?.error ? `\n${t.error}` : ""}`}>
              <span className="tm-conn-dot" />
              <span className="m-hide">{queued > 0 ? `${conn} · ${queued} to send` : conn}</span>
            </span>
            <button className="icon-btn" onClick={sync} title="Sync now: send what's waiting and take in what's new">
              <IconRefresh size={14} />
            </button>
          </>
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
          {!t?.online && t?.error && <div className="tm-banner">{t.error}</div>}
          <div className="list" style={{ ["--idch" as string]: Math.max(4, ...dockets.map((d) => d.id.length), ...waiting.map((o) => o.docket.length)) }}>
            {waiting.length > 0 && (
              <div>
                {head("outbox", "Held back", waiting.length)}
                {!shut.has("outbox") &&
                  waiting.map((o) => (
                    <div key={o.n} className="row tm-row" title={o.why ?? ""}>
                      <span className="row-live">
                        <LiveIcon state={o.status === "held" ? "blocked" : "error"} />
                      </span>
                      <span className="row-id">{o.docket}</span>
                      <span className="row-title">{heldWhat(o)}</span>
                      <span className="row-note m-hide">{o.status === "held" ? "Kept on this Mac until you say" : o.why}</span>
                      <span className="spacer" />
                      <span className="row-end">
                        {o.status === "held" && (
                          <button className="btn btn-small" onClick={() => outbox(o.n, "send")} title="Send it to the workspace after all">
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
                  dockets.map((sd) => {
                    const d = hub.byId.get(sd.id);
                    const agents = sd.agents ?? [];
                    const whose = [...new Set(agents.map((a) => a.member ?? w.you))];
                    return (
                      <a key={sd.id} className="row" href={`#/d/${sd.id}`}>
                        <span className="row-live" title={d?.live?.state ?? ""}>
                          <LiveIcon state={sd.halted ? "error" : d?.live?.state} />
                        </span>
                        <span className="row-id">{sd.id}</span>
                        <span className="row-status">
                          <StatusIcon status={sd.status ?? "todo"} />
                        </span>
                        <span className="row-title">{sd.title ?? sd.id}</span>
                        {sd.halted && <span className="row-note tm-bad">Not followed</span>}
                        <span className="spacer" />
                        <span className="row-end">
                          {agents.length > 0 && (
                            <span className="chip-mini tm-agents" title={agents.map((a) => `${a.member ?? w.you}'s ${a.app}${a.role ? ` · ${a.role}` : ""}`).join("\n")}>
                              <IconTerminal size={12} /> {agents.length}
                              {whose.some((m) => m !== w.you) && <span className="m-hide"> · {whose.filter((m) => m !== w.you).join(", ")}</span>}
                            </span>
                          )}
                          <Stack handles={[...sd.owners, ...sd.members]} you={w.you} label={`Owners ${sd.owners.join(", ")}${sd.members.length ? `; members ${sd.members.join(", ")}` : ""}`} />
                          <span className="row-age" title={d?.updated_at}>
                            {d ? ago(d.updated_at) : ""}
                          </span>
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
                  <div key={m.handle} className="row tm-member">
                    <PersonDot handle={m.handle} size={20} />
                    <span className="row-title">
                      {m.handle}
                      {m.you && <span className="muted"> (you)</span>}
                    </span>
                    {m.name && m.name !== m.handle && <span className="row-note">{m.name}</span>}
                    <span className="spacer" />
                    <span className="row-end">
                      {m.role === "admin" && <span className="tm-role">Admin</span>}
                      <span className="tm-login m-hide">{m.login}</span>
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
