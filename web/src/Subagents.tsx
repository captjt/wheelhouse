import { useEffect, useState } from "react";
import type { Docket } from "./api";
import { AppAvatar, LiveIcon } from "./icons";
import { ago, useData } from "./live";
import { Prose } from "./ui";

// What the docket's agents spawned (src/subagents.rs): read live from Claude
// Code's and Codex's own files, so it's polled while the page is open.

type Sub = {
  id: string;
  name: string;
  task?: string | null;
  title?: string | null;
  model?: string | null;
  effort?: string | null;
  state: "running" | "done" | "idle" | "stopped";
  tokens?: number;
  started_at?: string | null;
  last_at?: string | null;
  words?: string | null;
  depth?: number;
  background?: boolean;
};
type Agent = { app: string; label: string; round?: number | null; session: string; cwd: string; began_at?: string | null; subagents: Sub[] };
type Answer = { agents: Agent[]; running: number; total: number; tokens: number };

const POLL = 10_000;
const tokens = (n?: number | null) => (!n ? "" : n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));
const live = (s: Sub["state"]) => ({ running: "working", done: "done", idle: "idle", stopped: "error" })[s];
const word = (s: Sub["state"]) => ({ running: "Running", done: "Done", idle: "Quiet", stopped: "Stopped" })[s];

export function SubagentsSection({ d }: { d: Docket }) {
  const { data, reload } = useData<Answer>(`/api/dockets/${d.id}/subagents`);
  const [earlier, setEarlier] = useState(false);
  const [open, setOpen] = useState<string | null>(null);
  useEffect(() => {
    const t = window.setInterval(() => {
      if (document.visibilityState === "visible") reload();
    }, POLL);
    return () => window.clearInterval(t);
  }, [reload]);

  const agents = (data?.agents ?? []).filter((a) => a.subagents.length > 0);
  if (!data || agents.length === 0) return null;
  // Agents newest first, grouped by review round when they have one.
  const groups: { key: string; round: number | null; agents: Agent[] }[] = [];
  for (const a of agents) {
    const key = a.round != null ? `r${a.round}` : "now";
    const g = groups.find((x) => x.key === key);
    if (g) g.agents.push(a);
    else groups.push({ key, round: a.round ?? null, agents: [a] });
  }
  const shown = earlier ? groups : groups.slice(0, 1);
  const rest = groups.slice(1).reduce((n, g) => n + g.agents.reduce((m, a) => m + a.subagents.length, 0), 0);

  return (
    <section className="subagents">
      <div className="artifacts-head">
        <span className="artifacts-label">Subagents</span>
        <span className="muted small">
          {data.running > 0 ? `${data.running} running · ` : ""}
          {data.total} in all{data.tokens ? ` · ${tokens(data.tokens)} tokens` : ""}
        </span>
      </div>
      {shown.map((g) => (
        <div key={g.key} className="sa-group">
          {groups.length > 1 && g.round != null && <div className="art-group-label">Round {g.round}</div>}
          {g.agents.map((a) => (
            <div key={a.session} className="sa-agent">
              <div className="sa-agent-head" title={`${a.cwd}\nsession ${a.session}`}>
                <AppAvatar app={a.app === "claude" ? "claude-code" : a.app} size={16} />
                <span className="sa-agent-label">{a.label}</span>
                <span className="muted small">
                  {a.subagents.length} subagent{a.subagents.length === 1 ? "" : "s"}
                  {a.began_at ? ` · ${ago(a.began_at)}` : ""}
                </span>
              </div>
              <ul className="sa-list">
                {a.subagents.map((s) => {
                  const k = `${a.session}/${s.id}`;
                  const expanded = open === k;
                  return (
                    <li key={k} className={`sa-row sa-${s.state}${expanded ? " open" : ""}`} style={{ paddingLeft: 6 + ((s.depth ?? 1) - 1) * 16 }}>
                      <button className="sa-line" onClick={() => setOpen(expanded ? null : k)} aria-expanded={expanded} title={s.title || s.task || s.name}>
                        <span title={word(s.state)}>
                          <LiveIcon state={live(s.state)} size={12} />
                        </span>
                        <span className="sa-name">{s.name}</span>
                        {s.task && <span className="sa-task">{s.task}</span>}
                        <span className="spacer" />
                        <span className="sa-meta">
                          <span className="sa-model">{[s.model, s.effort].filter(Boolean).join(" · ")}</span>
                          {s.tokens ? <span className="sa-tok">{tokens(s.tokens)}</span> : null}
                          <span className="sa-when" title={s.last_at ?? ""}>
                            {s.state === "running" ? word(s.state) : s.last_at ? ago(s.last_at) : ""}
                          </span>
                        </span>
                      </button>
                      {expanded && (
                        <div className="sa-words">
                          {s.words ? <Prose text={s.words} /> : <span className="muted small">Nothing said yet.</span>}
                          <div className="muted small sa-ids">
                            {word(s.state)}
                            {s.started_at ? ` · started ${ago(s.started_at)}` : ""}
                            {s.background ? " · in the background" : ""} · <span className="mono-s">{s.id}</span>
                          </div>
                        </div>
                      )}
                    </li>
                  );
                })}
              </ul>
            </div>
          ))}
        </div>
      ))}
      {groups.length > 1 && (
        <button className="btn btn-small btn-ghost sa-more" onClick={() => setEarlier(!earlier)}>
          {earlier ? "Show the latest only" : `Show ${groups.length - 1} earlier round${groups.length - 1 === 1 ? "" : "s"} · ${rest} subagents`}
        </button>
      )}
    </section>
  );
}
