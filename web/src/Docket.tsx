import { forwardRef, useEffect, useImperativeHandle, useRef, useState, type ReactNode } from "react";
import { Person, PersonDot } from "./Team";
import { agentLabel, agentOfEntry, agentsOn, appName, entryBlock, liveAgents, mateOf, paneRuntime, runtimeOf, runtimeTitle, spawnRuntime, type CheckRun, type Docket, type DocketAgent, type DocketPage, type EntryView } from "./api";
import { AppAvatar, HubAvatar, IconChevron, IconDown, IconDownload, IconShield, IconUp, LiveIcon, PersonAvatar, SessionIcon, StatusIcon, TitleText, liveWord, shortPath } from "./icons";
import { ago, agoLong, go, lastList, stamp, useData, useHub } from "./live";
import { Kbd, Prose, Sheet, useKeys, useMenu, usePhone } from "./ui";
import { nav } from "./mobile";
import { IconBack, IconBuild, IconInfo } from "./icons";
import { AnswerButtons, PaneButton, onThisMac, usePane } from "./Pane";
import { DecisionsSection, IntentSection } from "./Intent";
import { ArtifactViewer, ArtifactsSection, RecordViewer } from "./Artifacts";
import { ChatView } from "./Chat";
import { IconTerminal } from "./icons";
import { markdownKeys } from "./Editor";
import { MessageCard } from "./Messages";
import { ReviewPanel } from "./Reviews";
import { BuildIt, BuildPanel } from "./Builds";
import { IconExternal } from "./icons";
import { LinearChip, LinearModal } from "./Linear";
import { post } from "./api";
import { IconIssue, IconPR } from "./icons";
import { useToast } from "./ui";
import { STATUSES, canStartAgent, startLabel, statusItems } from "./views";
import { CheckOutput, VerifySection } from "./Verify";
import { checkState, checkWord, independence, shortSha, verifyShown } from "./evidence";
import { IconAgents, IconPause, IconPlay } from "./icons";
import { SubagentsSection } from "./Subagents";

export function DocketScreen({ id, embedded }: { id: string; embedded?: boolean }) {
  const hub = useHub();
  const pane = usePane();
  const toast = useToast();
  const [linking, setLinking] = useState<string | false>(false);
  const { data, error, reload } = useData<DocketPage>(`/api/dockets/${id}`);
  const menu = useMenu();
  const statusRef = useRef<HTMLButtonElement>(null);
  const titleRef = useRef<{ edit: () => void }>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const phone = usePhone();
  const [details, setDetails] = useState(false);
  const [viewing, setViewing] = useState<EntryView | null>(null);
  const [record, setRecord] = useState(false);
  const [checkOut, setCheckOut] = useState<CheckRun | null>(null);
  // An agent's docket opens to its conversation on a phone; the record is a tap away.
  const [tab, setTab] = useState<"chat" | "docket" | null>(null);
  // A long record shows its latest entries first; the rest are a tap away.
  const [earlier, setEarlier] = useState(false);
  // On a docket with several agents: only one agent's entries, by its pane.
  const [only, setOnly] = useState<string | null>(null);

  const ids = lastList.ids;
  const pos = ids.indexOf(id);
  const step = (delta: number) => {
    const next = ids[pos + delta];
    if (pos >= 0 && next) go(`/d/${next}`);
  };
  const d = data?.docket;
  const building = !!d && !!hub.state?.build?.tasks.some((t) => t.docket === d.id);
  const openStatus = () => d && statusRef.current && menu.open(statusRef.current, statusItems(d, (s) => hub.setStatus(d.id, s).then(reload)), "Change status");

  useKeys(
    {
      j: () => step(1),
      k: () => step(-1),
      Escape: () => go(lastList.path),
      s: openStatus,
      o: () => d?.live && pane.act(d.id),
      r: () => titleRef.current?.edit(),
      m: () => composerRef.current?.focus(),
    },
    !embedded,
  );

  if (error && !data) return <div className="view-empty">{error}</div>;
  if (!data || !d) return <div className="view" />;
  const { entries, verify } = data;
  const ev = data.evidence;
  const verifying = verifyShown(ev);
  const open = d.status === "todo" || d.status === "in_progress";
  const talks = !embedded && d.protocol === "agent" && !!d.agent;
  const view = tab ?? (talks && phone ? "chat" : "docket");
  const chatting = talks && view === "chat";
  const seg = talks && (
    <span className="seg seg-text docket-seg" role="tablist">
      <button className={chatting ? "on" : ""} onClick={() => setTab("chat")} role="tab" aria-selected={chatting}>
        Chat
      </button>
      <button className={!chatting ? "on" : ""} onClick={() => setTab("docket")} role="tab" aria-selected={!chatting}>
        Docket
      </button>
    </span>
  );
  const aside = (
        <aside className="props">
          <Prop label="Status">
            <button ref={statusRef} className="prop-btn" onClick={openStatus} title="Change status (S)">
              <StatusIcon status={d.status} />
              {STATUSES.find((s) => s.key === d.status)?.label}
            </button>
          </Prop>
          {(open || d.hold) && (
            <Prop label="Hold">
              {d.hold ? (
                <button className="prop-btn held" onClick={() => void hub.resume(d.id).catch(() => {})} title={`On hold: ${d.hold}\nClick to resume: Build and Review take steps again`}>
                  <IconPause size={14} />
                  <span className="prop-text">On hold</span>
                </button>
              ) : (
                <button className="prop-btn muted" onClick={() => hub.setDialog({ kind: "hold", id: d.id })} title="Hold it: Build and Review take no steps on it until you resume">
                  <IconPause size={14} /> Hold…
                </button>
              )}
            </Prop>
          )}
          {d.review && (
            <Prop label="Review">
              <a className="prop-btn" href={d.review.url || "#"} target="_blank" rel="noreferrer" title={d.review.title ?? ""}>
                <span className="prop-text">
                  {d.review.key}
                  <span className="muted">
                    {d.review.round ? ` · round ${d.review.round}` : ""}
                    {d.review.verdict ? ` · ${d.review.verdict.replace("_", " ").toLowerCase()}` : d.review.phase ? ` · ${d.review.phase}` : ""}
                    {d.review.merge_risk ? ` · risk ${d.review.merge_risk}` : ""}
                  </span>
                  <IconExternal size={11} />
                </span>
              </a>
            </Prop>
          )}
          {d.protocol !== "review" && (
            <Prop label="Build">
              {hub.state?.build?.tasks.some((t) => t.docket === d.id) ? (
                <span className="prop-val">{(() => { const t = hub.state!.build!.tasks.find((x) => x.docket === d.id)!; return `${t.repo} · ${t.state === "active" ? t.phase?.replace("_", " ") : t.state}`; })()}</span>
              ) : d.intent?.body?.trim() ? (
                <BuildIt d={d} />
              ) : (
                <span className="prop-val muted" title="Write the intent first: what this is for, and how you'll know it's done">Needs an intent</span>
              )}
            </Prop>
          )}
          <Prop label="Linear">
            {d.linear ? (
              <span className="prop-issues">
                <LinearChip l={d.linear} />
                {(d.linear_also ?? []).map((l) => (
                  <a key={l.key} className="prop-btn" href={l.url || "#"} target="_blank" rel="noreferrer" title={`${l.key}${l.title ? `: ${l.title}` : ""} (open in Linear)`}>
                    <IconIssue size={14} />
                    <span className="prop-text">
                      {l.key}
                      <IconExternal size={11} />
                    </span>
                  </a>
                ))}
              </span>
            ) : (
              <button className="prop-btn muted" onClick={() => setLinking("")} title="Link a Linear issue to this docket">
                <IconIssue size={14} /> Link issue
              </button>
            )}
          </Prop>
          {!d.agent && !building && canStartAgent(d) && (
            <Prop label="Agent">
              <button className="prop-btn muted" onClick={() => hub.setDialog({ kind: "agent", id: d.id })} title={`Start Claude Code, Codex, Antigravity or Pi on this docket, in a new ${spawnRuntime(hub.state)} pane`}>
                <IconAgents size={14} /> Start an agent…
              </button>
            </Prop>
          )}
          {agentsOn(d).length > 1 && d.protocol !== "review" && !building && <AgentsProp d={d} />}
          {d.agent && agentsOn(d).length <= 1 && d.protocol !== "review" && !building && (
            <>
              <Prop label="Agent">
                <span className="prop-val">
                  <AppAvatar app={d.agent.app} size={16} />
                  {appName(d.agent.app)}
                </span>
              </Prop>
              <Prop label="Now">
                {!d.live && canStartAgent(d) ? (
                  <button className="prop-btn muted" onClick={() => hub.setDialog({ kind: "agent", id: d.id })} title={`${d.released ? `Ended: ${d.released}. ` : ""}Start an agent on this docket again, in a new ${spawnRuntime(hub.state)} pane`}>
                    <LiveIcon state="idle" />
                    <span className="prop-text">Not running · Start one…</span>
                  </button>
                ) : (
                  <span className={`prop-val live-${d.live?.state || "gone"}`}>
                    <LiveIcon state={d.live?.state ?? "idle"} />
                    {d.live ? `${liveWord(d.live.state)}${d.live.since ? ` · ${ago(d.live.since)}` : ""}` : d.released ? `Ended · ${d.released}` : "Not running"}
                  </span>
                )}
              </Prop>
              <Prop label={paneRuntime(d)}>
                {d.live ? (
                  <button className="prop-btn" onClick={() => pane.act(d.id)} title={onThisMac() ? `Show ${d.agent.rex_session} / ${d.agent.pane} in ${paneRuntime(d)} (O)` : "View the pane (O)"}>
                    <SessionIcon name={d.agent.rex_session} size={14} />
                    <span className="prop-text">
                      {d.live?.rex_session || d.agent.rex_session} <span className="muted">/ {d.live?.pane || d.agent.pane || "pane"}</span>
                    </span>
                  </button>
                ) : (
                  <span className="prop-val">
                    <SessionIcon name={d.agent.rex_session} size={14} />
                    <span className="prop-text muted">
                      {d.agent.rex_session} / {d.agent.pane || "pane"}
                    </span>
                  </span>
                )}
              </Prop>
              {d.agent.cwd && (
                <Prop label="Folder">
                  <button className="prop-btn" title={`${d.agent.cwd}\nClick to copy`} onClick={() => navigator.clipboard?.writeText(d.agent!.cwd!)}>
                    <span className="prop-text">{shortPath(d.agent.cwd)}</span>
                  </button>
                </Prop>
              )}
              {d.session && (
                <Prop label="Session">
                  <button className="prop-btn mono-s" title="Copy the agent's session id" onClick={() => navigator.clipboard?.writeText(d.session!)}>
                    {d.session.slice(0, 8)}
                  </button>
                </Prop>
              )}
              {d.live && canStartAgent(d) && (
                <Prop label="">
                  <button className="prop-btn muted" onClick={() => hub.setDialog({ kind: "agent", id: d.id })} title={`Start another agent on this docket, beside this one, in a new ${spawnRuntime(hub.state)} pane`}>
                    <IconAgents size={14} /> Add an agent…
                  </button>
                </Prop>
              )}
            </>
          )}
          <Prop label="Protocol">
            <span className="prop-val">{building ? "Build" : d.protocol === "agent" ? "Agent" : d.protocol === "review" ? "Review" : "By hand"}</span>
          </Prop>
          {d.protocol !== "review" && !building && <Prop label="Work">
            <span className="prop-val">
              {d.prompts} prompt{d.prompts === 1 ? "" : "s"} · {d.turns} turn{d.turns === 1 ? "" : "s"}
            </span>
          </Prop>}
          <Prop label="Created">
            <span className="prop-val" title={d.created_at}>
              {stamp(d.created_at)}
            </span>
          </Prop>
          <Prop label="Updated">
            <span className="prop-val" title={d.updated_at}>
              {agoLong(d.updated_at)}
            </span>
          </Prop>
          <SharedProps d={d} />

          <div className="props-section">Record</div>
          {ev && !verifying && ev.configured.length > 0 && open && (
            <Prop label="Checks">
              <button className="prop-btn" onClick={() => void hub.runChecks(d.id).catch(() => {})} title={`Run ${ev.repo}'s checks now: Wheelhouse runs them itself and writes down what it saw`}>
                <IconPlay size={14} />
                <span className="prop-text">
                  Run {ev.configured.map((c) => c.name).join(", ")}
                </span>
              </button>
            </Prop>
          )}
          <Prop label="Check">
            <button className={`prop-btn ${verify.ok ? "ok" : "bad"}`} onClick={() => setRecord(true)} title="Open the record: every signed entry, and what the check means">
              <IconShield size={14} />
              <span className="prop-text">{verify.ok ? `Intact · ${verify.entries} signed` : verify.problems[0]}</span>
            </button>
          </Prop>
          <Prop label="Head">
            <button className="prop-btn mono-s" title={`Copy: Docket: ${d.id} @ ${d.short_head}`} onClick={() => navigator.clipboard?.writeText(`Docket: ${d.id} @ ${d.short_head}`)}>
              {d.short_head}
            </button>
          </Prop>
          <Prop label="Record">
            <span className="prop-pair">
              <button className="prop-btn" onClick={() => setRecord(true)} title="Read the record here">
                Open
              </button>
              <a className="prop-btn" href={`/api/dockets/${d.id}/export`} download={`${d.id}.jsonl`} title="Download it: it checks without the hub (wh verify --file)">
                <IconDownload size={14} /> {d.id}.jsonl
              </a>
            </span>
          </Prop>
        </aside>
  );

  return (
    <div className={`view docket${embedded ? " embedded" : ""}`}>
      {!embedded && phone && (
        <header className="view-head m-nav">
          <button className="m-back" onClick={() => nav.back()} aria-label={`Back to ${lastList.title}`}>
            <IconBack size={20} />
            <span>{lastList.title}</span>
          </button>
          {seg || <span className="m-nav-title">{d.id}</span>}
          <span className="m-nav-end">
            {d.live && (
              <button className="icon-btn m-icon" onClick={() => pane.view(d.id)} aria-label="Terminal">
                <IconTerminal size={20} />
              </button>
            )}
            <button className="icon-btn m-icon" onClick={() => setDetails(true)} aria-label="Details">
              <IconInfo size={20} />
            </button>
          </span>
        </header>
      )}
      {!embedded && !phone && (
        <header className="view-head">
          <a className="crumb" href={`#${lastList.path}`}>
            {lastList.title}
          </a>
          <IconChevron size={12} />
          <span className="crumb-here">{d.id}</span>
          {pos >= 0 && (
            <span className="pager">
              <button className="icon-btn" title="Previous (K)" disabled={pos <= 0} onClick={() => step(-1)}>
                <IconUp size={14} />
              </button>
              <button className="icon-btn" title="Next (J)" disabled={pos >= ids.length - 1} onClick={() => step(1)}>
                <IconDown size={14} />
              </button>
              <span className="muted small">
                {pos + 1} / {ids.length}
              </span>
            </span>
          )}
          {seg}
          <span className="spacer" />
          {d.live && <PaneButton id={d.id} />}
        </header>
      )}

      {chatting ? (
        <ChatView d={d} entries={entries} queued={data.queued ?? []} reload={reload} onArtifact={setViewing} />
      ) : (
      <div className="docket-body">
        <div className="docket-main">
          <div className="docket-col">
          {embedded && (
            <div className="embedded-head">
              <a className="row-id" href={`#/d/${d.id}`}>
                {d.id}
              </a>
              <span className="spacer" />
              {d.live && <PaneButton id={d.id} small />}
              <a className="btn btn-small" href={`#/d/${d.id}`}>
                Open docket
              </a>
            </div>
          )}
          <Title ref={titleRef} d={d} onSave={(t) => hub.rename(d.id, t).then(reload)} />
          {phone && (
            <div className="m-chips">
              <button className="m-chip" onClick={() => menu.open(new DOMRect(), statusItems(d, (s) => hub.setStatus(d.id, s).then(reload)), "Change status")}>
                <StatusIcon status={d.status} />
                {STATUSES.find((s) => s.key === d.status)?.label}
              </button>
              {d.live && (
                <span className={`m-chip live-${d.live.state}`}>
                  <LiveIcon state={d.live.state} />
                  {liveWord(d.live.state)}
                </span>
              )}
              {d.review && (
                <a className="m-chip" href={d.review.url || "#"} target="_blank" rel="noreferrer">
                  <IconPR size={14} />
                  {d.review.key}
                  {d.review.verdict ? <span className="muted"> · {d.review.verdict.replace("_", " ").toLowerCase()}</span> : null}
                </a>
              )}
              {[...(d.linear ? [d.linear] : []), ...(d.linear_also ?? [])].map((l) => (
                <a key={l.key} className="m-chip" href={l.url || "#"} target="_blank" rel="noreferrer">
                  <IconIssue size={14} />
                  {l.key}
                </a>
              ))}
              {d.hold && (
                <button className="m-chip held" onClick={() => document.getElementById("verify")?.scrollIntoView({ block: "start", behavior: "smooth" })}>
                  <IconPause size={14} />
                  On hold
                </button>
              )}
              {building && (
                <span className="m-chip">
                  <IconBuild size={14} />
                  Building
                </span>
              )}
            </div>
          )}
          {/* Each agent that's asking, or failed: on a docket with several, by name. */}
          {liveAgents(d)
            .filter((a) => a.live?.state === "blocked" || a.live?.state === "error")
            .map((a) => {
              const l = a.live!;
              const who = agentsOn(d).length > 1 ? `${appName(a.app)} (${agentLabel(a)})` : appName(a.app);
              const rt = runtimeTitle(runtimeOf(a.block));
              return l.state === "blocked" ? (
                <div key={a.block} className="callout callout-blocked">
                  <LiveIcon state="blocked" />
                  <div>
                    <strong>
                      {who} is waiting on you{l.kind ? ` · ${l.kind}` : ""}
                    </strong>
                    {l.msg && <div className="callout-msg">{l.msg}</div>}
                  </div>
                  <span className="spacer" />
                  {l.answerable ? (
                    <span className="callout-actions">
                      <AnswerButtons d={d} live={l} small />
                      <PaneButton id={d.id} block={a.block} small />
                    </span>
                  ) : (
                    <PaneButton id={d.id} block={a.block} small label={onThisMac() ? `Answer in ${rt}` : "View pane"} />
                  )}
                </div>
              ) : (
                <div key={a.block} className="callout callout-error">
                  <LiveIcon state="error" />
                  <div>
                    <strong>{who} failed</strong>
                    {l.msg && <div className="callout-msg">{l.msg}</div>}
                  </div>
                </div>
              );
            })}

          <ReviewPanel d={d} />
          <BuildPanel d={d} />
          <SubagentsSection d={d} />
          {(hub.state?.messages ?? [])
            .filter((m) => m.docket === d.id)
            .map((m) => (
              <MessageCard key={m.id} m={m} />
            ))}

          <IntentSection d={d} entries={entries} onSaved={reload} />
          <DecisionsSection decisions={data.decisions ?? []} docket={d.id} repo={ev?.repo ?? null} />
          {ev && verifying && <VerifySection d={d} ev={ev} onCheck={setCheckOut} />}
          <ArtifactsSection entries={entries} onOpen={setViewing} />

          <div className="activity-head">
            <span>Activity</span>
            {(d.agents?.length ?? 0) > 1 && (
              <span className="act-agents" role="group" aria-label="Show one agent's activity">
                <button className={`act-agent-chip${only ? "" : " on"}`} onClick={() => setOnly(null)}>
                  All
                </button>
                {d.agents!.map((a) => (
                  <button key={a.block} className={`act-agent-chip${only === a.block ? " on" : ""}`} onClick={() => setOnly(only === a.block ? null : a.block)} title={`${appName(a.app)} in ${runtimeTitle(runtimeOf(a.block))}${a.on ? "" : `, left${a.left ? `: ${a.left}` : ""}`}`}>
                    <AppAvatar app={a.app} size={13} /> {agentLabel(a)}
                  </button>
                ))}
              </span>
            )}
            <span className="spacer" />
            <button className={`verify-chip ${verify.ok ? "ok" : "bad"}`} onClick={() => setRecord(true)} title={verify.ok ? `Every entry's id, chain link and signature checks. Head ${verify.head}. Open the record.` : verify.problems.join("\n")}>
              <IconShield size={13} /> {verify.ok ? `Verified · ${verify.entries} signed` : "Record doesn't check"}
            </button>
          </div>
          <div className="activity">
            {!earlier && entries.length > 200 && (
              <button className="btn btn-small btn-ghost act-earlier" onClick={() => setEarlier(true)}>
                Show {entries.length - 150} earlier entries
              </button>
            )}
            {(earlier || entries.length <= 200 ? entries : entries.slice(-150))
              .filter((e) => !only || entryBlock(e) === only)
              .map((e) => (
              <Activity key={e.seq} e={e} d={d} owner={hub.state?.owner || "you"} me={hub.state?.team?.workspace?.you} onOpen={setViewing} onCheck={setCheckOut} />
            ))}
          </div>
          <Composer ref={composerRef} onSend={(t) => hub.note(d.id, t).then(reload)} />
          </div>
        </div>

        {phone ? (
          details && (
            <Sheet title={`${d.id} · Details`} onClose={() => setDetails(false)} className="details-sheet">
              {aside}
            </Sheet>
          )
        ) : (
          aside
        )}
      </div>
      )}
      {menu.node}
      {viewing && <ArtifactViewer e={viewing} docket={d.id} onClose={() => setViewing(null)} />}
      {checkOut && <CheckOutput c={checkOut} onClose={() => setCheckOut(null)} />}
      {record && <RecordViewer d={d} onClose={() => setRecord(false)} />}
      {linking !== false && (
        <LinearModal
          title={`Link a Linear issue to ${d.id}`}
          busy={linking || null}
          onClose={() => setLinking(false)}
          onPick={(i) => {
            setLinking(i.identifier);
            post(`/api/dockets/${d.id}/linear`, { key: i.identifier }).then(
              () => {
                setLinking(false);
                toast(`${d.id} linked to ${i.identifier}${d.intent ? "" : ": its description is the intent"}`);
                reload();
              },
              (e: Error) => (setLinking(false), toast(e.message, "error")),
            );
          }}
        />
      )}
    </div>
  );
}

/** A docket's agents, when it has several: each on its own line, to open, with the ones that left folded away. */
function AgentsProp({ d }: { d: Docket }) {
  const hub = useHub();
  const pane = usePane();
  // A shared docket's teammates' agents are on their Macs: the Team section lists them.
  const me = hub.state?.team?.workspace?.you;
  const mine = (a: DocketAgent) => !a.member || a.member === me;
  const on = liveAgents(d).concat(agentsOn(d).filter((a) => !a.live)).filter(mine);
  const left = (d.agents ?? []).filter((a) => !a.on && mine(a));
  const [earlier, setEarlier] = useState(false);
  const row = (a: DocketAgent) => {
    const state = a.live?.state;
    const rt = runtimeTitle(runtimeOf(a.block));
    return (
      <button
        key={a.block}
        className="prop-btn da-row"
        disabled={!a.live}
        onClick={() => pane.act(d.id, a.block)}
        title={a.live ? `${onThisMac() ? `Show ${a.rex_session} / ${a.pane} in ${rt}` : "View the pane"}${a.addr ? `\nMessage it: wh send --to ${a.addr}` : ""}` : `Not running${a.left ? `: ${a.left}` : ""}`}
      >
        <AppAvatar app={a.app} size={16} />
        <span className="prop-text">
          {agentLabel(a)}
          <span className="muted">
            {" "}
            · {appName(a.app)} · {rt}
          </span>
          <span className={`da-state live-${state || "gone"}`}>
            <LiveIcon state={state ?? "idle"} size={11} /> {state ? `${liveWord(state)}${a.live?.since ? ` · ${ago(a.live.since)}` : ""}` : a.left ? `Left · ${a.left}` : "Not running"}
          </span>
        </span>
      </button>
    );
  };
  return (
    <>
      <Prop label="Agents">
        <span className="agent-list">
          {on.map(row)}
          {canStartAgent(d) && (
            <button className="prop-btn muted" onClick={() => hub.setDialog({ kind: "agent", id: d.id })} title={`Start another agent on this docket, in a new ${spawnRuntime(hub.state)} pane`}>
              <IconAgents size={14} /> {startLabel(d)}
            </button>
          )}
          {left.length > 0 && (
            <button className="prop-btn muted small" onClick={() => setEarlier((e) => !e)} aria-expanded={earlier}>
              {earlier ? "Hide" : "Earlier"} · {left.length} left
            </button>
          )}
          {earlier && left.map(row)}
        </span>
      </Prop>
    </>
  );
}

function Prop({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="prop">
      <span className="prop-label">{label}</span>
      <span className="prop-value">{children}</span>
    </div>
  );
}

// ---- the activity feed ----

function Activity({ e, d, owner, me, onOpen, onCheck }: { e: EntryView; d: Docket; owner: string; me?: string; onOpen: (e: EntryView) => void; onCheck: (c: CheckRun) => void }) {
  // On a shared docket, whose it is: a teammate's (`sam`, `hub@sam`,
  // `codex@sam`) is named for them, never "You".
  const mate = mateOf(e.actor, d.id, me);
  const youName = mate && e.actor.type === "person" ? mate : "You";
  const actorApp = (mate ? `${mate}'s ` : "") + appName(String(e.actor.id).split("@")[0]);
  // The entry's own agent, on a docket that's had several; named by its role or pane.
  const who = agentOfEntry(d, e);
  const several = (d.agents?.length ?? 0) > 1;
  const agent = (mate ? `${mate}'s ` : "") + (several && who ? `${appName(who.app)} (${agentLabel(who)})` : appName(who?.app));
  const time = (
    <span className="act-time" title={`${stamp(e.at)}\n#${e.seq} · ${e.mark}, signed by ${e.key || "?"}\n${e.id}`}>
      {agoLong(e.at)}
    </span>
  );
  const mark = <span className={`act-mark mark-${e.mark}`}>{e.mark}</span>;
  const sys = (avatar: ReactNode, text: ReactNode, tone = "") => (
    <div className={`act act-sys${tone ? ` tone-${tone}` : ""}`}>
      <span className="act-avatar">{avatar}</span>
      <span className="act-text">
        {text} <span className="act-dot">·</span> {time}
      </span>
      {mark}
    </div>
  );
  const comment = (avatar: ReactNode, name: ReactNode, verb: string, body: string, tone = "") => (
    <div className={`act act-comment${tone ? ` tone-${tone}` : ""}`}>
      <div className="act-card">
        <div className="act-card-head">
          {avatar}
          <strong>{name}</strong>
          <span className="muted">{verb}</span>
          <span className="act-dot">·</span>
          {time}
          <span className="spacer" />
          {mark}
        </div>
        <Clamp>
          <Prose text={body} />
        </Clamp>
      </div>
    </div>
  );
  const you = mate && e.actor.type === "person" ? <PersonDot handle={mate} /> : <PersonAvatar name={owner} size={18} />;
  const hubA = <HubAvatar size={18} />;
  const app = <AppAvatar app={who?.app} size={18} />;
  const data = e.data || {};

  switch (e.kind) {
    case "share":
      return sys(you, <><b>{youName}</b> shared this docket with the team{data.local ? <> from <span className="mono-s">{data.local}</span></> : null}</>);
    case "people": {
      const h = data.handle as string | undefined;
      const self = h && h === e.actor.id;
      const them = h === me ? "you" : h;
      const text = !h
        ? <>let {data.answer === "members" ? "anyone on it" : "only owners"} answer its agents' questions</>
        : data.action === "remove"
          ? self ? <>left this docket</> : <>took {them} off this docket</>
          : self ? <>joined as {data.role === "owner" ? "an owner" : "a member"}</> : <>made {them} {data.role === "owner" ? "an owner" : "a member"}</>;
      return sys(you, <><b>{youName}</b> {text}</>);
    }
    case "trust": {
      const t = data.teammate === me ? "your" : data.teammate ? `${data.teammate}'s` : null;
      const parts: ReactNode[] = [];
      if (t) parts.push(<span key="t">{data.on === false ? <>asks before letting {t} agents' messages in</> : <>lets {t} agents' messages in</>}</span>);
      if (typeof data.private_prompts === "boolean") parts.push(<span key="p">{data.private_prompts ? "keeps their prompts here private" : "shares their prompts here"}</span>);
      return sys(you, <><b>{youName}</b> {parts.reduce<ReactNode[]>((a, p, i) => (i ? [...a, " and ", p] : [p]), [])}</>);
    }
    case "open":
      if (data.protocol === "review")
        return sys(<IconPR size={16} />, <><b>Wheelhouse</b> opened this Review docket{data.review?.key ? <> for {data.review.key}</> : null}</>);
      return data.protocol === "agent"
        ? sys(hubA, <><b>Wheelhouse</b> picked up {agent}{who?.cwd ? <> in <span className="act-path" title={who.cwd}>{shortPath(who.cwd)}</span></> : null}</>)
        : e.actor.type === "person"
          ? sys(you, <><b>{youName}</b> opened this docket</>)
          : sys(hubA, <><b>Wheelhouse</b> opened this docket</>);
    case "claim": {
      if (data.source === "rex-reviewer")
        return sys(hubA, <><b>rex-reviewer</b> started its {data.role}{data.round ? ` for round ${data.round}` : ""}</>);
      if (data.source === "wheelhouse" && data.role)
        return sys(hubA, <><b>Wheelhouse</b> {data.round ? "started" : "found"} the {data.role}{data.round ? ` for round ${data.round}` : ""}</>);
      if (data.source === "wheelhouse") return sys(hubA, <><b>Wheelhouse</b> started {agent} in {runtimeTitle(runtimeOf((data.agent as { block?: string } | undefined)?.block))}'s wheelhouse session</>);
      if (data.source === "join") return sys(app, <><b>{agent}</b> joined this docket{data.role ? <> as {data.role}</> : null}, from {runtimeTitle(runtimeOf((data.agent as { block?: string } | undefined)?.block))}</>);
      if (data.source === "leave") return null;
      const s = <span className="mono-s">{String(data.session || "").slice(0, 8)}</span>;
      const src = data.source;
      return sys(
        hubA,
        src === "resume" ? <><b>{agent}</b> resumed session {s}</> : src === "clear" ? <><b>{agent}</b> cleared into session {s}</> : <><b>{agent}</b> started session {s}</>,
      );
    }
    case "question":
      return comment(e.actor.type === "agent" ? <AppAvatar app={String(e.actor.id).split("@")[0]} size={18} /> : you, e.actor.type === "agent" ? actorApp : youName, `asked ${data.to === "me" ? "you" : data.to}${data.subject ? ` · ${data.subject}` : ""} (#${data.message})`, `${e.body || ""}\n\nOptions: ${(data.options || []).join(" · ")}`, "ask");
    case "decision":
      return comment(e.actor.type === "person" ? you : <AppAvatar app={String(e.actor.id).split("@")[0]} size={18} />, e.actor.type === "person" ? youName : actorApp, `chose “${data.choice}” (#${data.replies_to})`, e.body && e.body !== data.choice ? e.body : `**${data.choice}**`, e.actor.type === "person" ? "you" : "");
    case "note":
      if (data.message) {
        const writer = e.actor.type === "agent" ? actorApp : youName;
        const verb = data.replies_to ? `replied to #${data.replies_to}` : data.to === "me" ? `wrote to you${data.subject ? ` · ${data.subject}` : ""}` : `wrote to ${data.to}`;
        return comment(e.actor.type === "agent" ? <AppAvatar app={String(e.actor.id).split("@")[0]} size={18} /> : you, writer, verb, e.body || "", e.actor.type === "person" ? "you" : "");
      }
      if (data.via === "prompt") return comment(you, youName, `asked ${agent}`, e.body || "", "you");
      if (data.via === "message") return comment(hubA, "rex-inbox", "delivered a message", e.body || "");
      if (e.actor.type === "agent") return comment(app, agent, "noted", e.body || "");
      return comment(you, youName, "noted", e.body || "", "you");
    case "turn":
      if (!e.body) return sys(app, <><b>{agent}</b> finished a turn{data.state && data.state !== "done" ? ` (${data.state})` : ""}</>);
      return comment(app, agent, "finished a turn", e.body, "agent");
    case "intent": {
      // The agent that wrote it, which may not be the docket's own.
      const writer = String(e.actor.id).split("@")[0];
      const by =
        data.source === "linear" ? (
          <><b>Linear</b> set the intent from {data.key}'s description{data.taken_by ? " (taken by you)" : ""}</>
        ) : data.source === "review" ? (
          <><b>Wheelhouse</b> wrote the intent from the PR{data.linear?.length ? <> and {data.linear.join(", ")}</> : null}</>
        ) : (
          <><b>{e.actor.type === "person" ? youName : e.actor.type === "agent" ? appName(writer) : "Wheelhouse"}</b> {e.actor.type === "person" ? "updated" : "proposed"} the intent</>
        );
      return sys(data.source === "linear" ? <IconIssue size={16} /> : e.actor.type === "person" ? you : data.source === "review" ? hubA : <AppAvatar app={writer} size={18} />, by, "intent");
    }
    case "link":
      if (data.kind === "linear" && e.actor.type !== "person") {
        const how: Record<string, string> = { linkback: "Linear links it to the PR", closes: "the PR closes it", refs: "the PR is part of it", branch: "named in the branch", title: "named in the PR's title" };
        return sys(
          <IconIssue size={16} />,
          <>
            <b>Wheelhouse</b> linked {data.url ? <a className="act-link" href={data.url} target="_blank" rel="noreferrer">{data.key}</a> : data.key}
            {data.title ? <> · {data.title}</> : null}
            {how[data.how] ? <span className="muted">: {how[data.how]}</span> : null}
          </>,
        );
      }
      if (data.kind === "linear")
        return sys(
          you,
          <>
            <b>{youName}</b> linked <a className="act-link" href={data.url} target="_blank" rel="noreferrer">{data.key}</a> {data.title ? <>· {data.title}</> : null}
          </>,
        );
      if (data.kind === "github-pr")
        return sys(<IconPR size={16} />, <>Following <a className="act-link" href={data.url} target="_blank" rel="noreferrer">{data.key}</a> on GitHub</>);
      if (data.kind === "github-review")
        return sys(<IconPR size={16} />, <>Round {data.round}'s review is <a className="act-link" href={data.url} target="_blank" rel="noreferrer">on GitHub</a></>, "approved");
      return sys(hubA, <>Linked {e.body}</>);
    case "observed":
      if (data.what === "review" && data.source === "wheelhouse") return null;
      if (data.what === "review-event") {
        const tone = data.event === "posted" ? "approved" : data.event === "dismissed" || data.event === "post-refused" ? "denied" : data.event === "merged" ? "approved" : undefined;
        return sys(<IconPR size={16} />, <>{e.body}</>, tone);
      }
      if (data.what === "review-attention")
        return sys(<LiveIcon state={data.kind === "ready" || data.kind === "blocked" ? "blocked" : "error"} size={16} />, <>{e.body}</>, data.kind === "ready" || data.kind === "blocked" ? "blocked" : "error");
      if (data.what === "review") {
        const r = data.review || {};
        const bits = [r.round && `round ${r.round}`, r.phase && r.state === "reviewing" ? r.phase : r.state, r.verdict && r.verdict.replace("_", " ").toLowerCase(), r.merge_risk && `risk ${r.merge_risk}`].filter(Boolean);
        return sys(hubA, <><b>rex-reviewer</b> · {r.key}: {bits.join(" · ")}{r.review_url ? <> · <a className="act-link" href={r.review_url} target="_blank" rel="noreferrer">the review</a></> : null}</>);
      }
      if (data.what === "linear-comment") return comment(<IconIssue size={18} />, data.author || "Someone", "commented in Linear", e.body || "", "linear");
      if (data.what === "linear") {
        const i = data.issue || {};
        const changed: string[] = data.changed || [];
        if (!changed.length)
          return sys(<IconIssue size={16} />, <><b>Linear</b>: {i.key} is {i.state ?? "open"}{i.assignee ? `, assigned to ${i.assignee}` : ""}</>);
        const parts = changed.map((c) =>
          c === "state" ? `${data.from?.state ?? "?"} → ${i.state}` : c === "title" ? `title “${i.title}”` : c === "assignee" ? `assigned to ${i.assignee ?? "no one"}` : c === "priority" ? `priority ${i.priority ?? "none"}` : "description changed",
        );
        return sys(<IconIssue size={16} />, <><b>Linear</b> · {i.key}: {parts.join(" · ")}</>);
      }
      return data.what === "error"
        ? sys(<LiveIcon state="error" size={16} />, <><b>{agent}</b> failed{e.body ? <>: {e.body}</> : null}</>, "error")
        : sys(<LiveIcon state="blocked" size={16} />, <><b>{agent}</b> waited on you{data.kind ? ` (${data.kind})` : ""}{e.body ? <>: {e.body}</> : null}</>, "blocked");
    case "set": {
      const who = e.actor.type === "person" ? youName : "Wheelhouse";
      const parts: ReactNode[] = [];
      if (data.title)
        parts.push(data.title_source === "prompt" ? <span key="t">named it from your prompt</span> : <span key="t">renamed it to “{data.title}”</span>);
      if (data.status) {
        const st = STATUSES.find((s) => s.key === data.status);
        parts.push(
          <span key="s">
            moved it to <StatusIcon status={data.status} size={12} /> {st?.label ?? data.status}
            {data.on === "word" ? <> on your word{data.reason || e.body ? <>: “{data.reason || e.body}”</> : null}</> : null}
          </span>,
        );
      }
      if ("hold" in data) parts.push(<span key="h">{data.hold ? <>put it on hold: “{data.hold}”</> : "resumed it"}</span>);
      return sys(e.actor.type === "person" ? you : hubA, <><b>{who}</b> {parts.reduce<ReactNode[]>((a, p, i) => (i ? [...a, " and ", p] : [p]), [])}</>);
    }
    case "approval": {
      if (data.what === "post-review") return sys(you, <><b>{youName}</b> approved posting round {data.round}'s review</>, "approved");
      if (data.kind === "review")
        return sys(you, <><b>{youName}</b> {data.decision === "post" ? "posted" : "dismissed"} the review{e.body ? <>: {e.body}</> : null}{data.url ? <> · <a className="act-link" href={data.url} target="_blank" rel="noreferrer">on GitHub</a></> : null}</>, data.decision === "post" ? "approved" : "denied");
      const ok = data.decision === "approve";
      return sys(
        you,
        <>
          <b>{youName}</b> {ok ? "approved" : "denied"} {agent}'s {data.kind || "prompt"}
          {e.body ? <>: <span className="act-path">{e.body}</span></> : null}
        </>,
        ok ? "approved" : "denied",
      );
    }
    case "artifact": {
      const by = e.actor.type === "person" ? you : e.actor.type === "agent" ? <AppAvatar app={String(e.actor.id).split("@")[0]} size={18} /> : hubA;
      const name = e.actor.type === "person" ? youName : e.actor.type === "agent" ? actorApp : "Wheelhouse";
      return sys(
        by,
        <>
          <b>{name}</b> {data.url ? "linked" : "kept"}{" "}
          <button className="act-link link-btn" onClick={() => onOpen(e)}>
            {e.body || data.name}
          </button>
          {data.group ? <span className="muted"> · {data.group}</span> : null}
        </>,
        "artifact",
      );
    }
    case "release":
      if (several) return sys(app, <><b>{agent}</b> left: {data.reason}</>);
      return sys(hubA, <><b>Wheelhouse</b> saw the session end: {data.reason}</>);
    case "check": {
      const st = checkState(data);
      const c: CheckRun = { ...data, seq: e.seq, at: e.at };
      return sys(
        hubA,
        <>
          <b>Wheelhouse</b> ran <span className="act-path">{data.name}</span>
          {data.required === false ? <span className="muted"> (optional)</span> : null}: {checkWord(data)} at <span className="mono-s">{shortSha(data.head)}</span>
          {data.output ? (
            <>
              {" "}
              <span className="act-dot">·</span>{" "}
              <button className="act-link link-btn" onClick={() => onCheck(c)}>
                output
              </button>
            </>
          ) : null}
        </>,
        st === "pass" ? "approved" : st === "inform" ? "" : "error",
      );
    }
    case "verdict": {
      const who = String(e.actor.id).split("@")[0];
      const ind = independence(data);
      const line = `judged ${data.n ? `check ${data.n}` : "the work"}: ${String(data.verdict ?? "no verdict").replace(/_/g, " ")}${typeof data.findings === "number" ? ` · ${data.findings} finding${data.findings === 1 ? "" : "s"}` : ""}${ind ? ` · ${ind}` : ""}`;
      if (e.body) return comment(<AppAvatar app={who} size={18} />, appName(who), line, e.body, data.verdict === "pass" ? "" : "ask");
      return sys(<AppAvatar app={who} size={18} />, <><b>{appName(who)}</b> {line}</>, data.verdict === "pass" ? "approved" : "error");
    }
    default:
      return sys(hubA, <>{e.kind}{e.body ? `: ${e.body}` : ""}</>);
  }
}

function Clamp({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [long, setLong] = useState(false);
  const [open, setOpen] = useState(false);
  // Measured when its size changes, not on every render.
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => setLong(el.scrollHeight > 260);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return (
    <div className={`clamp${long && !open ? " clamped" : ""}`}>
      <div ref={ref} className="clamp-body">
        {children}
      </div>
      {long && (
        <button className="link" onClick={() => setOpen((o) => !o)}>
          {open ? "Show less" : "Show more"}
        </button>
      )}
    </div>
  );
}

// ---- title and composer ----


const Title = forwardRef<{ edit: () => void }, { d: Docket; onSave: (t: string) => Promise<unknown> }>(function Title({ d, onSave }, ref) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(d.title);
  useImperativeHandle(ref, () => ({ edit: () => (setValue(d.title), setEditing(true)) }), [d.title]);
  const save = () => {
    const t = value.trim();
    setEditing(false);
    if (t && t !== d.title) onSave(t);
  };
  if (editing)
    return (
      <textarea
        className="title-input"
        autoFocus
        rows={1}
        value={value}
        onChange={(e) => setValue(e.target.value.replace(/\n/g, ""))}
        onBlur={save}
        onFocus={(e) => e.currentTarget.select()}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.preventDefault(), save());
          if (e.key === "Escape") (e.preventDefault(), e.stopPropagation(), setEditing(false));
        }}
      />
    );
  return (
    <h1 className="title" onClick={() => (setValue(d.title), setEditing(true))} title="Rename (R)">
      <TitleText d={d} />
    </h1>
  );
});

const Composer = forwardRef<HTMLTextAreaElement, { onSend: (t: string) => Promise<unknown> }>(function Composer({ onSend }, ref) {
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const send = () => {
    const t = text.trim();
    if (!t || busy) return;
    setBusy(true);
    onSend(t).then(
      () => (setText(""), setBusy(false)),
      () => setBusy(false),
    );
  };
  return (
    <div className="composer">
      <textarea
        ref={ref}
        placeholder="Leave a note… (Markdown)"
        value={text}
        rows={text.split("\n").length > 2 ? 5 : 2}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) return (e.preventDefault(), send());
          if (e.key === "Escape") return (e.currentTarget.blur(), e.stopPropagation());
          markdownKeys(e);
        }}
      />
      <div className="composer-bar">
        <span className="muted small">Signed as you</span>
        <span className="spacer" />
        <button className="btn btn-primary btn-small" disabled={!text.trim() || busy} onClick={send}>
          Note <Kbd>⌘↩</Kbd>
        </button>
      </div>
    </div>
  );
});

/** A shared docket's people and teammates' agents, or where a local docket went when it was shared (docs/teams.md). */
function SharedProps({ d }: { d: Docket }) {
  const hub = useHub();
  const toast = useToast();
  const me = hub.state?.team?.workspace?.you;
  if (d.shared_as)
    return (
      <Prop label="Shared as">
        <a className="prop-btn" href={`#/d/${d.shared_as}`} title="This docket went on the workspace; it continues there">
          <span className="prop-text">{d.shared_as}</span>
        </a>
      </Prop>
    );
  if (!d.owners?.length || !me) return null;
  const on = d.owners.includes(me) || (d.members ?? []).includes(me);
  const theirs = (d.agents ?? []).filter((a) => a.on && a.member && a.member !== me);
  const join = () =>
    post(`/api/team/dockets/${encodeURIComponent(d.id)}/people`, { handle: me, role: "member" }).then(
      () => (toast(`You're on ${d.id}`), hub.reload()),
      (e: Error) => toast(e.message, "error"),
    );
  return (
    <>
      <div className="props-section">Team</div>
      <Prop label="Owners">
        <span className="prop-val tm-people">
          {d.owners.map((o) => (
            <Person key={o} handle={o} you={o === me} />
          ))}
        </span>
      </Prop>
      <Prop label="Members">
        {(d.members ?? []).length > 0 ? (
          <span className="prop-val tm-people">
            {(d.members ?? []).map((m) => (
              <Person key={m} handle={m} you={m === me} />
            ))}
          </span>
        ) : (
          <span className="prop-val muted">None yet</span>
        )}
      </Prop>
      {!on && (
        <Prop label="">
          <button className="prop-btn muted" onClick={join} title="Put yourself on this docket as a member">
            <span className="prop-text">Join as a member</span>
          </button>
        </Prop>
      )}
      {theirs.length > 0 && (
        <Prop label="Agents">
          <span className="prop-val">
            {theirs.map((a) => (
              <span key={`${a.member}/${a.block}`} className="prop-text" title={`On ${a.member}'s Mac. Reach them with wh send --to @${d.id}`}>
                {a.member}'s {appName(a.app)}
                {a.role ? ` · ${a.role}` : ""}
              </span>
            ))}
          </span>
        </Prop>
      )}
      {d.shared_from && (
        <Prop label="From">
          <span className="prop-val muted mono-s">{d.shared_from}</span>
        </Prop>
      )}
    </>
  );
}
