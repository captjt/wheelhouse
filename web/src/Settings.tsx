import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { post } from "./api";
import {
  AGENTS,
  PLAN_MODES,
  agentLabel,
  checkProblem,
  checkValue,
  CHECK_TIMEOUT,
  docGroups,
  guideId,
  mergePatches,
  patchAt,
  planValue,
  repoSlugs,
  rolePatch,
  setHere,
  timeoutWord,
  usedWord,
  varChanges,
  whenDraft,
  whenValue,
  type CheckSpec,
  type DocText,
  type Path,
  type Settings,
  type SettingsDoc,
  type WhenDraft,
} from "./config";
import { diffCounts, foldDiff, lineDiff } from "./diff";
import { MarkdownEditor, markdownKeys, type EditorHandle } from "./Editor";
import { IconBuild, IconFile, IconPR, IconPause, IconPlay, IconPlus, IconSettings, IconShield } from "./icons";
import { useData, useHub } from "./live";
import { LargeTitle } from "./mobile";
import { InlineMd, Kbd, Modal, useToast } from "./ui";

/**
 * Settings: what Wheelhouse does on its own (the standing orders), where a
 * Build stops to ask you, who builds and checks, each repo's rules and
 * checks, and the guides and briefs agents read. Values set here go in
 * settings.json, laid over build.lua and review.lua; each can go back to
 * the Lua's.
 */

const SECTIONS: [string, string][] = [
  ["orders", "Standing orders"],
  ["gates", "Gates"],
  ["agents", "Agents"],
  ["repos", "Repos"],
  ["guides", "Guides & briefs"],
  ["files", "Where these live"],
];

/** What each part of Settings needs: the view, and how to change it. */
type Ctl = {
  s: Settings;
  /** Send a patch; `where` names what it's for, so a refusal shows beside it. Resolves to the new view, or null. */
  change: (where: string, patch: Record<string, unknown>, done?: string | ((s: Settings) => string)) => Promise<Settings | null>;
  /** POST elsewhere (pauses, Review's post mode), with the same inline errors. */
  act: (where: string, url: string, body: unknown, done: string) => Promise<boolean>;
  errs: Record<string, string>;
  busy: string | null;
  openDoc: (id: string, fresh?: string) => void;
};

export function SettingsView({ section }: { section?: string }) {
  const hub = useHub();
  const toast = useToast();
  const { data, error, reload } = useData<Settings>("/api/settings");
  const [errs, setErrs] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [doc, setDoc] = useState<{ id: string; fresh?: string } | null>(null);
  const [here, setHere_] = useState(section || "orders");

  // Going to a section (from ⌘K or the list beside), once the page is there.
  useEffect(() => {
    if (!section || !data) return;
    document.getElementById(`set-${section}`)?.scrollIntoView({ block: "start" });
    setHere_(section);
  }, [section, !!data]);

  const settle = (where: string, err: string | null) =>
    setErrs((e) => {
      const n = { ...e };
      if (err) n[where] = err;
      else delete n[where];
      return n;
    });
  const ctl: Ctl | null = data && {
    s: data,
    errs,
    busy,
    openDoc: (id, fresh) => setDoc({ id, fresh }),
    change: (where, patch, done) => {
      setBusy(where);
      return post<Settings>("/api/settings", patch).then(
        (s) => {
          setBusy(null);
          settle(where, null);
          const msg = typeof done === "function" ? done(s) : done;
          if (msg) toast(msg);
          reload();
          hub.reload();
          return s;
        },
        (e: Error) => {
          setBusy(null);
          settle(where, e.message);
          return null;
        },
      );
    },
    act: (where, url, body, done) => {
      setBusy(where);
      return post(url, body).then(
        () => {
          setBusy(null);
          settle(where, null);
          toast(done);
          reload();
          hub.reload();
          return true;
        },
        (e: Error) => {
          setBusy(null);
          settle(where, e.message);
          return false;
        },
      );
    },
  };

  // The list beside follows the page as it scrolls.
  const scroller = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const root = scroller.current;
    if (!root || !data) return;
    const on = () => {
      const top = root.getBoundingClientRect().top + 60;
      let at = SECTIONS[0][0];
      for (const [id] of SECTIONS) {
        const el = document.getElementById(`set-${id}`);
        if (el && el.getBoundingClientRect().top <= top) at = id;
      }
      if (root.scrollTop + root.clientHeight >= root.scrollHeight - 4) at = SECTIONS[SECTIONS.length - 1][0];
      setHere_(at);
    };
    root.addEventListener("scroll", on, { passive: true });
    return () => root.removeEventListener("scroll", on);
  }, [!!data]);

  const jump = (id: string) => {
    setHere_(id);
    document.getElementById(`set-${id}`)?.scrollIntoView({ block: "start", behavior: "smooth" });
  };

  return (
    <div className="view settings">
      <header className="view-head">
        <span className="view-title">
          <IconSettings size={15} /> Settings
        </span>
        <span className="spacer" />
        <span className="muted small m-hide">What Wheelhouse does on its own, and how</span>
      </header>
      <LargeTitle title="Settings" />
      {!data ? (
        <div className="view-empty">{error ? `Can't read the settings: ${error}` : ""}</div>
      ) : (
        <div className="set-body">
          <nav className="set-toc" aria-label="Sections">
            {SECTIONS.map(([id, label]) => (
              <button key={id} className={`set-toc-item${here === id ? " on" : ""}`} onClick={() => jump(id)}>
                {label}
              </button>
            ))}
          </nav>
          <div className="set-scroll" ref={scroller}>
            <div className="set-col">
              {[data.set_error && `Your settings.json doesn't read, so nothing in it applies: ${data.set_error}`, data.build.error && `build.lua: ${data.build.error}`, data.review.error && `review.lua: ${data.review.error}`]
                .filter(Boolean)
                .map((m) => (
                  <div key={m as string} className="callout callout-error set-callout">
                    <span>{m}</span>
                  </div>
                ))}
              <Orders c={ctl!} />
              <Gates c={ctl!} />
              <Agents c={ctl!} />
              <Repos c={ctl!} />
              <Guides c={ctl!} />
              <Files s={data} />
            </div>
          </div>
        </div>
      )}
      {doc && ctl && (
        <DocEditor
          id={doc.id}
          fresh={doc.fresh}
          known={data?.docs.find((d) => d.id === doc.id)}
          onClose={() => setDoc(null)}
          onSaved={() => (reload(), hub.reload())}
        />
      )}
    </div>
  );
}

// ---- small parts ----

function Section({ id, title, lead, action, children }: { id: string; title: string; lead?: ReactNode; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="set-section" id={`set-${id}`}>
      <div className="set-head">
        <h2>{title}</h2>
        <span className="spacer" />
        {action}
      </div>
      {lead && <p className="set-lead">{lead}</p>}
      {children}
    </section>
  );
}

function Err({ c, where }: { c: Ctl; where: string }) {
  const e = c.errs[where];
  return e ? <div className="set-err">{e}</div> : null;
}

/** "Set here", quietly, and the way back to the Lua's. */
function Here({ c, paths, part, where, what }: { c: Ctl; paths: Path[]; part: "build" | "review"; where: string; what: string }) {
  if (!paths.some((p) => setHere(c.s.set, p))) return null;
  const lua = c.s[part].lua_exists ? `${part}.lua's` : "the default";
  return (
    <span className="set-here">
      Set here
      <span className="act-dot">·</span>
      <button
        className="set-link"
        disabled={c.busy === where}
        onClick={() => c.change(where, mergePatches(...paths.filter((p) => setHere(c.s.set, p)).map((p) => patchAt(p, null))), `${what}: back to ${lua}`)}
        title={`Take this out of settings.json, so ${part}.lua (or the default) decides`}
      >
        Use {lua}
      </button>
    </span>
  );
}

function Seg<T extends string>({ value, options, onPick, disabled, label }: { value: T; options: { key: T; label: string; title?: string }[]; onPick: (v: T) => void; disabled?: boolean; label: string }) {
  return (
    <span className="seg seg-text set-seg" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button key={o.key} role="radio" aria-checked={value === o.key} className={value === o.key ? "on" : ""} disabled={disabled} title={o.title} onClick={() => value !== o.key && onPick(o.key)}>
          {o.label}
        </button>
      ))}
    </span>
  );
}

function Stepper({ value, min, max, onSet, disabled, label }: { value: number; min: number; max: number; onSet: (n: number) => void; disabled?: boolean; label: string }) {
  return (
    <span className="set-stepper" role="group" aria-label={label}>
      <button disabled={disabled || value <= min} onClick={() => onSet(value - 1)} aria-label={`Fewer ${label}`}>
        −
      </button>
      <span className="set-step-n">{value}</span>
      <button disabled={disabled || value >= max} onClick={() => onSet(value + 1)} aria-label={`More ${label}`}>
        +
      </button>
    </span>
  );
}

function Row({ label, children, note }: { label: string; children: ReactNode; note?: ReactNode }) {
  return (
    <div className="set-row">
      <span className="set-label">{label}</span>
      <div className="set-value">
        {children}
        {note}
      </div>
    </div>
  );
}

const agentOptions = AGENTS.map((a) => ({ key: a.key, label: a.label }));

// ---- standing orders ----

function Orders({ c }: { c: Ctl }) {
  const { s } = c;
  const groups = (["review", "build", "verify"] as const).map((p) => ({ p, orders: s.orders.filter((o) => o.protocol === p) })).filter((g) => g.orders.length);
  const rv = s.review.config;
  return (
    <Section
      id="orders"
      title="Standing orders"
      lead="What Wheelhouse does on its own, without asking first. They follow from the settings below; pause one to stop it starting anything new."
    >
      {groups.length === 0 && <div className="set-empty">No standing orders: Wheelhouse does nothing on its own until build.lua or review.lua sets some up.</div>}
      {groups.map(({ p, orders }) => {
        const on = orders.some((o) => o.on);
        const paused = orders.some((o) => o.paused);
        const state = !on ? "off" : paused ? "paused" : "on";
        const name = p === "review" ? "Review" : p === "build" ? "Build" : "Verify";
        const where = `orders.${p}`;
        return (
          <div key={p} className="set-order-group">
            <div className="set-order-head">
              <span className="set-order-icon">{p === "review" ? <IconPR size={14} /> : p === "build" ? <IconBuild size={14} /> : <IconShield size={14} />}</span>
              <span className="set-order-name">{name}</span>
              <span className={`rv-pill ${state === "on" ? "on" : state === "paused" ? "paused" : "off"}`}>{state === "on" ? "On" : state === "paused" ? "Paused" : "Off"}</span>
              <span className="spacer" />
              {p === "review" && !on && (
                <button className="btn btn-small" disabled={c.busy === where} onClick={() => c.act(where, "/api/review/settings", { enabled: true }, "Reviews are on, paused until you resume")}>
                  Turn on
                </button>
              )}
              {(p === "review" || p === "build") && on && (
                <button
                  className="btn btn-small"
                  disabled={c.busy === where}
                  onClick={() =>
                    c.act(
                      where,
                      p === "review" ? "/api/review/settings" : "/api/build/settings",
                      { paused: !paused },
                      paused ? (p === "review" ? "Reviews resumed" : "Builds resumed") : p === "review" ? "Reviews paused: no new rounds start" : "Builds paused: nothing new starts",
                    )
                  }
                >
                  {paused ? <IconPlay size={12} /> : <IconPause size={12} />} {paused ? "Resume" : "Pause"}
                </button>
              )}
            </div>
            {orders.map((o) => (
              <div key={o.text} className={`set-order${!o.on || o.paused ? " quiet" : ""}`}>
                <InlineMd text={o.text} />
              </div>
            ))}
            {p === "verify" && <div className="set-order-note">Change which checks run under Repos.</div>}
            {p === "review" && rv && (
              <div className="set-rows set-order-posting">
                <Row label="Posting" note={<span className="set-order-note">For one repo, under Repos.</span>}>
                  <Posting c={c} />
                </Row>
              </div>
            )}
            <Err c={c} where={where} />
          </div>
        );
      })}
    </Section>
  );
}

// ---- gates ----

function Gates({ c }: { c: Ctl }) {
  const b = c.s.build.config;
  if (!b) return null;
  return (
    <Section id="gates" title="Gates" lead="Where a Build stops to ask you, and what it does on its own when it doesn't.">
      <div className="set-rows">
        {b.gates.map((g) => {
          const where = `gates.${g.key}`;
          const path = ["build", "gates", g.key];
          return (
            <div key={g.key} className="set-row set-gate">
              <span className="set-label">{g.label}</span>
              <div className="set-value">
                <Seg
                  label={g.label}
                  value={g.value === "auto" ? "auto" : "ask"}
                  disabled={c.busy === where}
                  options={[
                    { key: "ask", label: "Ask", title: g.ask },
                    { key: "auto", label: "Auto", title: g.auto },
                  ]}
                  onPick={(v) => c.change(where, patchAt(path, v), `${g.label}: ${v === "ask" ? g.ask : g.auto}`)}
                />
                <span className="set-sentence">{g.value === "auto" ? g.auto : g.ask}</span>
                <Here c={c} paths={[path]} part="build" where={where} what={g.label} />
                <Err c={c} where={where} />
              </div>
            </div>
          );
        })}
      </div>
    </Section>
  );
}

// ---- agents ----

function Agents({ c }: { c: Ctl }) {
  const b = c.s.build.config;
  const r = c.s.review.config;
  if (!b && !r) return null;
  const pre = b?.prereview ?? [];
  const togglePre = (a: string) => {
    const next = AGENTS.map((x) => x.key).filter((k) => (k === a ? !pre.includes(k) : pre.includes(k)));
    c.change("agents.prereview", patchAt(["build", "prereview", "reviewers"], next), next.length ? `Pre-review: ${next.map(agentLabel).join(" and ")}` : "No pre-review: the PR opens without a blind review first");
  };
  return (
    <Section id="agents" title="Agents" lead="Who does the work, and who checks it. Each runs in its own Rex pane.">
      {b && (
        <div className="set-group">
          <div className="set-sub">
            <IconBuild size={13} /> Build
          </div>
          <div className="set-rows">
            <Row label="Builder">
              <Seg label="Builder" value={b.builder} options={agentOptions} disabled={c.busy === "agents.builder"} onPick={(v) => c.change("agents.builder", rolePatch("build", "builder", v), `${agentLabel(v)} builds from now on`)} />
              <span className="set-sentence">Plans the work, builds it, and addresses review.</span>
              <Here c={c} paths={[["build", "builder"]]} part="build" where="agents.builder" what="Builder" />
              <Err c={c} where="agents.builder" />
            </Row>
            <Row label="Checker">
              <Seg label="Checker" value={b.checker} options={agentOptions} disabled={c.busy === "agents.checker"} onPick={(v) => c.change("agents.checker", rolePatch("build", "checker", v), `${agentLabel(v)} checks Builds from now on`)} />
              <span className="set-sentence">A fresh session that holds the work to the intent and the plan.</span>
              <Here c={c} paths={[["build", "checker"]]} part="build" where="agents.checker" what="Checker" />
              <Err c={c} where="agents.checker" />
            </Row>
            <Row label="Pre-review">
              <span className="set-chips" role="group" aria-label="Pre-review">
                {AGENTS.map((a) => (
                  <button key={a.key} className={`set-chip${pre.includes(a.key) ? " on" : ""}`} aria-pressed={pre.includes(a.key)} disabled={c.busy === "agents.prereview"} onClick={() => togglePre(a.key)}>
                    {a.label}
                  </button>
                ))}
              </span>
              <span className="set-sentence">{pre.length ? `${pre.map(agentLabel).join(" and ")} review${pre.length === 1 ? "s" : ""} the branch blind before the PR opens. Nothing is posted.` : "None: the PR opens without a blind review first."}</span>
              <Here c={c} paths={[["build", "prereview", "reviewers"]]} part="build" where="agents.prereview" what="Pre-review" />
              <Err c={c} where="agents.prereview" />
            </Row>
            {(pre.length > 0 || setHere(c.s.set, ["build", "prereview", "loops"])) && (
              <Row label="Pre-review rounds">
                <Stepper label="pre-review rounds" value={b.prereview_loops} min={0} max={5} disabled={c.busy === "agents.prereview_loops"} onSet={(n) => c.change("agents.prereview_loops", patchAt(["build", "prereview", "loops"], n), `Pre-review: up to ${n} round${n === 1 ? "" : "s"}`)} />
                <span className="set-sentence">Rounds of blind review, and fixes, before the PR opens.</span>
                <Here c={c} paths={[["build", "prereview", "loops"]]} part="build" where="agents.prereview_loops" what="Pre-review rounds" />
                <Err c={c} where="agents.prereview_loops" />
              </Row>
            )}
            <Row label="Fix rounds">
              <Stepper label="fix rounds" value={b.fix_loops} min={0} max={10} disabled={c.busy === "agents.fix_loops"} onSet={(n) => c.change("agents.fix_loops", patchAt(["build", "fix_loops"], n), `Up to ${n} fix round${n === 1 ? "" : "s"}`)} />
              <span className="set-sentence">
                {b.fix_loops ? `Up to ${b.fix_loops} time${b.fix_loops === 1 ? "" : "s"}, the checker's findings go back to the builder before you're asked.` : "The checker's findings come to you, not back to the builder."}
              </span>
              <Here c={c} paths={[["build", "fix_loops"]]} part="build" where="agents.fix_loops" what="Fix rounds" />
              <Err c={c} where="agents.fix_loops" />
            </Row>
            <Row label="At once">
              <Stepper label="Builds at once" value={b.max_tasks} min={1} max={10} disabled={c.busy === "agents.max_tasks"} onSet={(n) => c.change("agents.max_tasks", patchAt(["build", "max_tasks"], n), `Up to ${n} Build${n === 1 ? "" : "s"} at once`)} />
              <span className="set-sentence">Builds that run at the same time; the rest wait their turn.</span>
              <Here c={c} paths={[["build", "max_tasks"]]} part="build" where="agents.max_tasks" what="Builds at once" />
              <Err c={c} where="agents.max_tasks" />
            </Row>
            <Row label="The PR">
              <Seg
                label="The PR"
                value={b.pr.draft ? "draft" : "ready"}
                disabled={c.busy === "agents.draft"}
                options={[
                  { key: "draft", label: "Draft" },
                  { key: "ready", label: "Ready" },
                ]}
                onPick={(v) => c.change("agents.draft", patchAt(["build", "pr", "draft"], v === "draft"), v === "draft" ? "PRs open as drafts" : "PRs open ready for review")}
              />
              <span className="set-sentence">{b.pr.draft ? "Opens as a draft, for you to mark ready." : "Opens ready for review."}</span>
              <Here c={c} paths={[["build", "pr", "draft"]]} part="build" where="agents.draft" what="The PR" />
              <Err c={c} where="agents.draft" />
            </Row>
          </div>
        </div>
      )}
      {r && (
        <div className="set-group">
          <div className="set-sub">
            <IconPR size={13} /> Review
          </div>
          <div className="set-rows">
            <Row label="Reviewers">
              <span className="set-chips">
                {r.reviewers.map((x) => (
                  <span key={x.name} className="rv-agent" title={`${x.name}: ${agentLabel(x.agent)}`}>
                    {x.name === x.agent ? agentLabel(x.agent) : <>{x.name}<span className="muted"> · {agentLabel(x.agent)}</span></>}
                  </span>
                ))}
              </span>
              <span className="set-sentence">Each reviews blind, in its own worktree. Change them in review.lua, where each can carry its own effort and guides.</span>
            </Row>
            <Row label="Checker">
              <Seg label="Review's checker" value={r.checker} options={agentOptions} disabled={c.busy === "agents.rchecker"} onPick={(v) => c.change("agents.rchecker", rolePatch("review", "checker", v), `${agentLabel(v)} checks reviews from now on`)} />
              <span className="set-sentence">Merges the drafts into one review, checks every finding, and rates the merge risk.</span>
              <Here c={c} paths={[["review", "checker"]]} part="review" where="agents.rchecker" what="Review's checker" />
              <Err c={c} where="agents.rchecker" />
            </Row>
            <Row label="At once">
              <Stepper label="review rounds at once" value={r.max_rounds} min={1} max={10} disabled={c.busy === "agents.max_rounds"} onSet={(n) => c.change("agents.max_rounds", patchAt(["review", "max_rounds"], n), `Up to ${n} review round${n === 1 ? "" : "s"} at once`)} />
              <span className="set-sentence">Review rounds that run at the same time.</span>
              <Here c={c} paths={[["review", "max_rounds"]]} part="review" where="agents.max_rounds" what="Review rounds at once" />
              <Err c={c} where="agents.max_rounds" />
            </Row>
          </div>
        </div>
      )}
    </Section>
  );
}

// ---- posting ----

type PostKey = "same" | "ask" | "auto" | "rule";

/**
 * Who posts a checked review, for every repo or (with `slug`) one: Ask,
 * Auto, or When… (a rule: it posts itself when every condition holds, and
 * otherwise waits for you, saying why). docs/review.md#who-posts.
 */
function Posting({ c, slug }: { c: Ctl; slug?: string }) {
  const r = c.s.review.config;
  const [draft, setDraft] = useState<WhenDraft | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  if (!r) return null;
  const own = slug ? (r.repos[slug]?.post ?? null) : r.post_rule;
  const path: Path = slug ? ["review", "repos", slug, "post"] : ["review", "post"];
  const where = slug ? `repo.${slug}.post` : "review.post";
  const value: PostKey = draft ? "rule" : !own ? "same" : own.mode;
  const inRepo = (s: string) => (slug ? `In ${slug}, ${s.charAt(0).toLowerCase()}${s.slice(1)}` : s);
  const save = (v: unknown, done: string | ((s: Settings) => string)) => c.change(where, patchAt(path, v), done).then((s) => s && (setDraft(null), setProblem(null)));
  const pick = (k: PostKey) => {
    setProblem(null);
    if (k === "rule") return setDraft(whenDraft(own?.mode === "rule" ? own.auto : null));
    setDraft(null);
    if (k === "same") return save(null, (s) => (s.review.config?.repos[slug!]?.post ? `review.lua sets ${slug}'s posting: change it there` : `${slug} posts as every repo does`));
    save(k, inRepo(k === "auto" ? "The checker posts each review" : "You post each review"));
  };
  const saveRule = () => {
    if (!draft) return;
    const v = whenValue(draft);
    if ("error" in v) return setProblem(v.error);
    save(v.value, inRepo("A checked review posts itself when your rule holds"));
  };
  const options: { key: PostKey; label: string; title: string }[] = [
    ...(slug ? [{ key: "same" as const, label: "Same as all", title: "As Agents → Review → Posting says, for every repo" }] : []),
    { key: "ask", label: "Ask", title: "You post each checked review, or dismiss it" },
    { key: "auto", label: "Auto", title: "The checker posts each review" },
    { key: "rule", label: "When…", title: "It posts itself when every condition holds; otherwise it waits for you" },
  ];
  const sentence = own?.fn
    ? `\`${own.fn}\` in review.lua decides, for each checked review. Pick Ask, Auto or When… to set it here instead.`
    : own
      ? `A checked review ${own.text}.`
      : `As every repo: a checked review ${r.post_rule.text}.`;
  return (
    <>
      <Seg label={slug ? `${slug}'s posting` : "Review's posting"} value={value} options={options} disabled={c.busy === where} onPick={pick} />
      {!draft && <span className="set-sentence">{sentence}</span>}
      {!draft && own?.mode === "rule" && own.auto && (
        <button className="set-link" onClick={() => pick("rule")}>
          Edit the rule
        </button>
      )}
      {!slug && r.post_override && (
        <div className="set-note">
          <code>wh review post-mode {r.post_override}</code> is on: every repo {r.post_override === "auto" ? "posts itself" : "waits for you"} until you change posting here.{" "}
          <button className="set-link" disabled={c.busy === where} onClick={() => c.act(where, "/api/review/settings", { post: "config" }, "Posting follows these settings again")}>
            Follow these settings
          </button>
        </div>
      )}
      {draft && (
        <div className="set-check-edit">
          <div className="set-check-fields">
            <div className="set-field">
              <span>Verdict</span>
              <Seg
                label="Verdict"
                value={draft.approve ? "approve" : "any"}
                options={[
                  { key: "approve", label: "Approves", title: "A review that requests changes waits for you" },
                  { key: "any", label: "Any" },
                ]}
                onPick={(v) => (setDraft({ ...draft, approve: v === "approve" }), setProblem(null))}
              />
            </div>
            <div className="set-field">
              <span>Merge risk</span>
              <Seg
                label="Merge risk"
                value={draft.risk || "any"}
                options={[
                  { key: "low", label: "Low" },
                  { key: "medium", label: "Medium or lower" },
                  { key: "any", label: "Any" },
                ]}
                onPick={(v) => (setDraft({ ...draft, risk: v === "any" ? "" : v }), setProblem(null))}
              />
            </div>
            <label className="set-field">
              <span>At most, lines</span>
              <input className="set-input set-num" inputMode="numeric" value={draft.maxLines} placeholder="any" onChange={(e) => (setDraft({ ...draft, maxLines: e.target.value.replace(/[^0-9]/g, "") }), setProblem(null))} onKeyDown={(e) => e.key === "Enter" && saveRule()} />
            </label>
          </div>
          <div className="set-check-fields">
            <label className="set-field set-field-grow">
              <span>By one of</span>
              <input className="set-input" value={draft.authors} placeholder="anyone" onChange={(e) => (setDraft({ ...draft, authors: e.target.value }), setProblem(null))} onKeyDown={(e) => e.key === "Enter" && saveRule()} />
            </label>
            <label className="set-field set-field-grow">
              <span>Labeled one of</span>
              <input className="set-input" value={draft.labels} placeholder="any labels" onChange={(e) => (setDraft({ ...draft, labels: e.target.value }), setProblem(null))} onKeyDown={(e) => e.key === "Enter" && saveRule()} />
            </label>
            <label className="set-field set-field-grow">
              <span>Not touching</span>
              <input className="set-input mono-s" value={draft.notTouching} placeholder="db/migrations/, auth/" onChange={(e) => (setDraft({ ...draft, notTouching: e.target.value }), setProblem(null))} onKeyDown={(e) => e.key === "Enter" && saveRule()} />
            </label>
          </div>
          <div className="set-actions">
            <span className="set-sentence">It posts itself only when every condition holds. Otherwise it waits for you, and says why.</span>
            <span className="spacer" />
            <button className="btn btn-small btn-ghost" onClick={() => (setDraft(null), setProblem(null))}>
              Cancel
            </button>
            <button className="btn btn-small btn-primary" disabled={c.busy === where} onClick={saveRule}>
              Save
            </button>
          </div>
          {problem && <div className="set-err">{problem}</div>}
        </div>
      )}
      <Here c={c} paths={[path]} part="review" where={where} what={slug ? `${slug}'s posting` : "Posting"} />
      <Err c={c} where={where} />
    </>
  );
}

// ---- repos ----

function Repos({ c }: { c: Ctl }) {
  const slugs = repoSlugs(c.s);
  return (
    <Section id="repos" title="Repos" lead="Each repo's rules for agents, its branch, its plan, the checks Wheelhouse runs itself, and its review guide.">
      {slugs.length === 0 && <div className="set-empty">No repos yet. Add one to build.lua or review.lua, with its checkout.</div>}
      {slugs.map((slug) => (
        <RepoCard key={slug} c={c} slug={slug} />
      ))}
    </Section>
  );
}

function RepoCard({ c, slug }: { c: Ctl; slug: string }) {
  const b = c.s.build.config?.repos[slug];
  const r = c.s.review.config?.repos[slug];
  const checkout = b?.checkout ?? r?.checkout ?? "";
  const notesNow = b?.notes ?? r?.notes ?? "";
  const [notes, setNotes] = useState<string | null>(null);
  const [base, setBase] = useState<string | null>(null);
  const notesPaths: Path[] = [...(b ? [["build", "repos", slug, "notes"]] : []), ...(r ? [["review", "repos", slug, "notes"]] : [])];
  const w = (k: string) => `repo.${slug}.${k}`;
  const saveNotes = () => {
    if (notes === null) return;
    c.change(w("notes"), mergePatches(...notesPaths.map((p) => patchAt(p, notes))), `Saved ${slug}'s notes: agents there read them from now on`).then((s) => s && setNotes(null));
  };
  const saveBase = () => {
    if (base === null) return;
    const v = base.trim();
    c.change(w("base"), patchAt(["build", "repos", slug, "base"], v || null), v ? `${slug}'s Builds branch from ${v}` : `${slug}'s Builds branch from its default branch`).then((s) => s && setBase(null));
  };
  const plan = b?.plan ?? "full";
  return (
    <div className="set-repo">
      <div className="set-repo-head">
        <span className="set-repo-name">{slug}</span>
        <span className="mono-s muted set-repo-path" title="Its checkout, where worktrees come from">
          {checkout}
        </span>
      </div>
      <div className="set-rows">
        <Row label="Notes for agents">
          <textarea
            className="set-textarea"
            value={notes ?? notesNow}
            rows={Math.min(14, Math.max(3, (notes ?? notesNow).split("\n").length + 1))}
            placeholder={"Rules agents in this repo follow, such as:\n- Run `just check` before you commit.\n- Don't touch the generated client in src/gen."}
            onChange={(e) => setNotes(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) return (e.preventDefault(), saveNotes());
              if (e.key === "Escape") return (e.preventDefault(), setNotes(null));
              markdownKeys(e);
            }}
          />
          <div className="set-actions">
            <span className="set-sentence">{r && b ? "Builders, checkers and reviewers here read these first." : b ? "Builders and checkers here read these first." : "Reviewers here read these first."}</span>
            <span className="spacer" />
            {notes !== null && notes !== notesNow && (
              <>
                <button className="btn btn-small btn-ghost" onClick={() => setNotes(null)}>
                  Cancel
                </button>
                <button className="btn btn-small btn-primary" disabled={c.busy === w("notes")} onClick={saveNotes}>
                  Save <Kbd>⌘↩</Kbd>
                </button>
              </>
            )}
          </div>
          <Here c={c} paths={notesPaths} part={b ? "build" : "review"} where={w("notes")} what={`${slug}'s notes`} />
          <Err c={c} where={w("notes")} />
        </Row>
        {b && (
          <Row label="Base branch">
            <span className="set-inline">
              <input
                className="set-input mono-s"
                value={base ?? b.base ?? ""}
                placeholder="The default branch"
                onChange={(e) => setBase(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") (e.preventDefault(), saveBase());
                  if (e.key === "Escape") (e.preventDefault(), setBase(null));
                }}
              />
              {base !== null && base.trim() !== (b.base ?? "") && (
                <button className="btn btn-small btn-primary" disabled={c.busy === w("base")} onClick={saveBase}>
                  Save
                </button>
              )}
            </span>
            <span className="set-sentence">Builds branch from it, and their PRs go into it.</span>
            <Here c={c} paths={[["build", "repos", slug, "base"]]} part="build" where={w("base")} what={`${slug}'s base branch`} />
            <Err c={c} where={w("base")} />
          </Row>
        )}
        {b && (
          <Row label="Plan on the PR">
            <Seg
              label="Plan on the PR"
              value={plan as "full" | "summary" | "off"}
              disabled={c.busy === w("plan")}
              options={PLAN_MODES.map((m) => ({ key: m.key, label: m.label, title: m.text }))}
              onPick={(v) => c.change(w("plan"), patchAt(["build", "repos", slug, "plan"], planValue(v)), `${slug}: ${PLAN_MODES.find((m) => m.key === v)!.text}`)}
            />
            <span className="set-sentence">{PLAN_MODES.find((m) => m.key === plan)?.text}</span>
            <Here c={c} paths={[["build", "repos", slug, "plan"]]} part="build" where={w("plan")} what={`${slug}'s plan`} />
            <Err c={c} where={w("plan")} />
          </Row>
        )}
        {b && <Checks c={c} slug={slug} checks={b.checks} />}
        {r && (
          <Row label="Posting">
            <Posting c={c} slug={slug} />
          </Row>
        )}
        {r && <ReviewGuide c={c} slug={slug} />}
      </div>
    </div>
  );
}

type Draft = { name: string; run: string; required: boolean; timeout: string; old?: string };

function Checks({ c, slug, checks }: { c: Ctl; slug: string; checks: CheckSpec[] }) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const where = `repo.${slug}.checks`;
  const path = (name: string): Path => ["build", "repos", slug, "checks", name];
  const save = () => {
    if (!draft) return;
    const spec = { name: draft.name.trim(), run: draft.run, required: draft.required, timeout: Number(draft.timeout) };
    const p = checkProblem(spec, checks.map((x) => x.name).filter((n) => n !== draft.old));
    setProblem(p);
    if (p) return;
    c.change(where, patchAt(path(spec.name), checkValue(spec)), draft.old ? `Saved ${spec.name}` : `Added ${spec.name}: Wheelhouse runs it in ${slug} from now on`).then((s) => s && setDraft(null));
  };
  const remove = (name: string) =>
    c.change(where, patchAt(path(name), null), (s) => (s.build.config?.repos[slug]?.checks.some((x) => x.name === name) ? `${name} is back to build.lua's` : `Removed ${name}`));
  // A check build.lua lists: `false` here takes it away (settings.json wins).
  const removeLua = (name: string) => c.change(where, patchAt(path(name), false), `Removed ${name}. build.lua still lists it; Settings keeps it off`);
  const editor = draft && (
    <div className="set-check-edit">
      <div className="set-check-fields">
        <label className="set-field">
          <span>Name</span>
          {draft.old ? (
            <span className="set-check-name">{draft.name}</span>
          ) : (
            <input className="set-input mono-s" autoFocus value={draft.name} placeholder="test" onChange={(e) => (setDraft({ ...draft, name: e.target.value }), setProblem(null))} onKeyDown={(e) => e.key === "Enter" && save()} />
          )}
        </label>
        <label className="set-field set-field-grow">
          <span>Command</span>
          <input className="set-input mono-s" autoFocus={!!draft.old} value={draft.run} placeholder="cargo test" onChange={(e) => (setDraft({ ...draft, run: e.target.value }), setProblem(null))} onKeyDown={(e) => e.key === "Enter" && save()} />
        </label>
        <label className="set-field">
          <span>Timeout</span>
          <span className="set-inline">
            <input className="set-input set-num" inputMode="numeric" value={draft.timeout} onChange={(e) => (setDraft({ ...draft, timeout: e.target.value.replace(/[^0-9]/g, "") }), setProblem(null))} onKeyDown={(e) => e.key === "Enter" && save()} />
            <span className="muted small">s</span>
          </span>
        </label>
      </div>
      <div className="set-actions">
        <Seg
          label="Required"
          value={draft.required ? "required" : "optional"}
          options={[
            { key: "required", label: "Required", title: "A failure holds the PR, the push, and closing the docket" },
            { key: "optional", label: "Optional", title: "It only informs" },
          ]}
          onPick={(v) => setDraft({ ...draft, required: v === "required" })}
        />
        <span className="set-sentence">{draft.required ? "A failure holds the PR or the push until it passes or you approve over it." : "It only informs: a failure holds nothing."}</span>
        <span className="spacer" />
        <button className="btn btn-small btn-ghost" onClick={() => (setDraft(null), setProblem(null))}>
          Cancel
        </button>
        <button className="btn btn-small btn-primary" disabled={c.busy === where} onClick={save}>
          {draft.old ? "Save" : "Add check"}
        </button>
      </div>
      {problem && <div className="set-err">{problem}</div>}
    </div>
  );
  return (
    <Row label="Checks">
      <div className="set-checks">
        {checks.length === 0 && !draft && <div className="set-empty-line">No checks. Add the commands that show this repo works, like its tests.</div>}
        {checks.map((x) =>
          draft?.old === x.name ? (
            <div key={x.name}>{editor}</div>
          ) : (
            <div key={x.name} className="set-check">
              <span className="set-check-name">{x.name}</span>
              <span className="set-check-cmd mono-s" title={x.run}>
                {x.run}
              </span>
              <span className={`set-check-req${x.required ? "" : " opt"}`}>{x.required ? "required" : "optional"}</span>
              <span className="set-check-to muted" title={`Stops it after ${x.timeout} seconds`}>
                {timeoutWord(x.timeout)}
              </span>
              <span className="set-check-src muted">{setHere(c.s.set, path(x.name)) ? "set here" : "build.lua"}</span>
              <span className="set-check-acts">
                <button className="set-link" onClick={() => (setDraft({ name: x.name, run: x.run, required: x.required, timeout: String(x.timeout), old: x.name }), setProblem(null))}>
                  Edit
                </button>
                {setHere(c.s.set, path(x.name)) ? (
                  <button className="set-link" disabled={c.busy === where} onClick={() => remove(x.name)} title="Take it out of settings.json">
                    Remove
                  </button>
                ) : (
                  <button className="set-link" disabled={c.busy === where} onClick={() => removeLua(x.name)} title="build.lua lists it; Settings turns it off">
                    Remove
                  </button>
                )}
              </span>
            </div>
          ),
        )}
        {draft && !draft.old && editor}
        {!draft && (
          <button className="btn btn-small btn-ghost set-add" onClick={() => (setDraft({ name: "", run: "", required: true, timeout: String(CHECK_TIMEOUT) }), setProblem(null))}>
            <IconPlus size={12} /> Add a check
          </button>
        )}
      </div>
      {problem && !draft && <div className="set-err">{problem}</div>}
      <span className="set-sentence">Wheelhouse runs these itself, at the commit, before a PR opens or a round pushes, and writes down what it saw.</span>
      <Err c={c} where={where} />
    </Row>
  );
}

function ReviewGuide({ c, slug }: { c: Ctl; slug: string }) {
  const r = c.s.review.config?.repos[slug];
  const own = c.s.docs.filter((d) => d.kind === "guide" && !d.builtin);
  const cur = (r?.guides ?? {})["review"];
  const where = `repo.${slug}.guide`;
  const path: Path = ["review", "repos", slug, "guides", "review"];
  const value = typeof cur === "string" && cur ? cur : "";
  const known = own.some((d) => d.path === value);
  const curDoc = own.find((d) => d.path === value);
  return (
    <Row label="Review guide">
      <span className="set-inline">
        <select
          className="set-select"
          value={value}
          disabled={c.busy === where}
          onChange={(e) => {
            const v = e.target.value;
            const d = own.find((x) => x.path === v);
            c.change(where, patchAt(path, v || false), v ? `Reviews in ${slug} follow ${d?.title ?? v}` : `Reviews in ${slug} follow no guide of their own`);
          }}
        >
          <option value="">None</option>
          {own.map((d) => (
            <option key={d.id} value={d.path}>
              {d.title}
            </option>
          ))}
          {value && !known && <option value={value}>{value.includes("\n") ? "Instructions in review.lua" : `From review.lua: ${value}`}</option>}
        </select>
        {curDoc && (
          <button className="set-link" onClick={() => c.openDoc(curDoc.id)}>
            Open
          </button>
        )}
      </span>
      <span className="set-sentence">{own.length ? "What reviewers here look for, beyond the usual: one of your guides." : "Write a guide of your own under Guides & briefs, then pick it here."}</span>
      <Here c={c} paths={[path]} part="review" where={where} what={`${slug}'s review guide`} />
      <Err c={c} where={where} />
    </Row>
  );
}

// ---- guides and briefs ----

function Guides({ c }: { c: Ctl }) {
  const groups = useMemo(() => docGroups(c.s.docs), [c.s.docs]);
  const [naming, setNaming] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const create = () => {
    const g = guideId(naming ?? "");
    if ("error" in g) return setProblem(g.error);
    if (c.s.docs.some((d) => d.id === g.id)) return setProblem(`There's already a guide named ${g.id.slice(6)}: open it below.`);
    const name = g.id.slice(6, -3);
    setNaming(null);
    setProblem(null);
    c.openDoc(g.id, `# ${name.charAt(0).toUpperCase()}${name.slice(1).replace(/-/g, " ")}\n\n`);
  };
  return (
    <Section
      id="guides"
      title="Guides & briefs"
      lead="Guides say how work should look: a review's style, a plan's shape. Briefs are what each agent is told at each step. Yours replace the built-in ones, and whatever a save replaces is kept."
      action={
        naming === null && (
          <button className="btn btn-small" onClick={() => (setNaming(""), setProblem(null))}>
            <IconPlus size={12} /> New guide
          </button>
        )
      }
    >
      {naming !== null && (
        <div className="set-new">
          <span className="set-inline">
            <input
              className="set-input mono-s"
              autoFocus
              value={naming}
              placeholder="api-review"
              onChange={(e) => (setNaming(e.target.value), setProblem(null))}
              onKeyDown={(e) => {
                if (e.key === "Enter") (e.preventDefault(), create());
                if (e.key === "Escape") (e.preventDefault(), setNaming(null));
              }}
            />
            <span className="muted mono-s">.md</span>
          </span>
          <span className="spacer" />
          <button className="btn btn-small btn-ghost" onClick={() => (setNaming(null), setProblem(null))}>
            Cancel
          </button>
          <button className="btn btn-small btn-primary" onClick={create}>
            Write it
          </button>
          {problem && <div className="set-err">{problem}</div>}
        </div>
      )}
      {groups.map((g) => (
        <div key={g.key} className="set-docs">
          <div className="set-sub">
            {g.label} <span className="view-count">{g.docs.length}</span>
          </div>
          {g.key === "build" && (
            <div className="set-note">
              Briefs use <code>{"{{VARIABLES}}"}</code>, which Wheelhouse fills in before an agent reads them. What you change here is what agents are told from then on, so a broken brief changes their work.
            </div>
          )}
          {g.key === "own" && g.docs.length === 0 && <div className="set-empty-line">None yet. A guide of your own can be a repo's review guide.</div>}
          {g.docs.map((d) => (
            <DocRow key={d.id} d={d} onOpen={() => c.openDoc(d.id)} />
          ))}
        </div>
      ))}
    </Section>
  );
}

function DocRow({ d, onOpen }: { d: SettingsDoc; onOpen: () => void }) {
  return (
    <button className="set-doc" onClick={onOpen}>
      <span className="set-doc-icon">
        <IconFile size={14} />
      </span>
      <span className="set-doc-main">
        <span className="set-doc-top">
          <span className="set-doc-title">{d.title}</span>
          <DocBadge d={d} />
        </span>
        {d.about && (
          <span className="set-doc-about">
            <InlineMd text={d.about} links={false} />
          </span>
        )}
        {d.used_by.length > 0 && <span className="set-doc-used">Used as {d.used_by.map(usedWord).join(", ")}</span>}
      </span>
    </button>
  );
}

function DocBadge({ d }: { d: SettingsDoc }) {
  if (!d.builtin) return <span className="set-badge yours">Yours</span>;
  if (d.overridden) return <span className="set-badge changed">Changed</span>;
  return <span className="set-badge">Built in</span>;
}

/** A guide or brief: edit it, compare it with the built-in, save, or reset. */
function DocEditor({ id, fresh, known, onClose, onSaved }: { id: string; fresh?: string; known?: SettingsDoc; onClose: () => void; onSaved: () => void }) {
  const toast = useToast();
  const isNew = fresh !== undefined && !known;
  const { data, error } = useData<DocText>(isNew ? null : `/api/settings/doc?id=${encodeURIComponent(id)}`);
  const [text, setText] = useState<string | null>(isNew ? fresh! : null);
  const [view, setView] = useState<"edit" | "compare">("edit");
  const [busy, setBusy] = useState<"save" | "reset" | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [leaving, setLeaving] = useState(false);
  const area = useRef<EditorHandle>(null);
  const doc = data ?? known;
  const saved = data?.text ?? "";
  const value = text ?? saved;
  const dirty = isNew ? !!value.trim() : text !== null && text !== saved;
  const original = data?.original ?? null;
  const diff = useMemo(() => (original !== null ? lineDiff(original, value) : []), [original, value]);
  const counts = diffCounts(diff);
  const vars = doc?.kind === "brief" ? varChanges(original ?? saved, value) : { missing: [], unknown: [] };
  const title = doc?.title ?? id.replace(/^guide\//, "");
  const where = known?.path ?? doc?.path ?? `~/.config/wheelhouse/guides/${id.slice(6)}`;

  useEffect(() => {
    if (isNew) window.setTimeout(() => area.current?.focus(), 0);
  }, [isNew]);

  const save = () => {
    if (busy || !dirty) return;
    setBusy("save");
    post<DocText>("/api/settings/doc", { id, text: value }).then(
      (d) => {
        setBusy(null);
        toast(isNew ? `Wrote ${d.title}: pick it as a repo's review guide under Repos` : `Saved ${d.title}: agents get it from now on`);
        onSaved();
        onClose();
      },
      (e: Error) => (setBusy(null), setErr(e.message)),
    );
  };
  const reset = () => {
    if (busy || !doc) return;
    setBusy("reset");
    post("/api/settings/doc/reset", { id }).then(
      () => {
        setBusy(null);
        toast(doc.builtin ? `${doc.title} is back to the built-in one; yours is kept in .history` : `Set ${doc.title} aside; it's kept in .history`);
        onSaved();
        onClose();
      },
      (e: Error) => (setBusy(null), setErr(e.message)),
    );
  };
  const tryClose = () => (dirty ? setLeaving(true) : onClose());
  const kind = !doc ? "Guide" : doc.kind === "guide" ? "Guide" : doc.protocol === "build" ? "Build's brief" : "Review's brief";
  const state = isNew ? "New, yours" : !doc ? "" : !doc.builtin ? "Yours" : doc.overridden ? "Yours, in place of the built-in one" : "Built in";

  return (
    <Modal onClose={tryClose} className="doc-modal">
      <div className="art-head doc-head">
        <span className="art-icon k-markdown">
          <IconFile size={18} />
        </span>
        <div className="art-head-main">
          <div className="art-head-title">{title}</div>
          <div className="art-head-meta">
            {kind}
            {state && ` · ${state}`} · <span className="mono-s">{where}</span>
          </div>
        </div>
        {original !== null && (
          <span className="seg seg-text">
            <button className={view === "edit" ? "on" : ""} onClick={() => setView("edit")}>
              Edit
            </button>
            <button className={view === "compare" ? "on" : ""} onClick={() => setView("compare")} title="What's different from the built-in one">
              Compare
            </button>
          </span>
        )}
      </div>
      {(doc?.about || (doc?.used_by.length ?? 0) > 0) && (
        <div className="doc-about">
          {doc?.about && <InlineMd text={doc.about} />}
          {doc && doc.used_by.length > 0 && (
            <span className="muted">
              {doc.about ? " " : ""}Used as {doc.used_by.map(usedWord).join(", ")}
              {!doc.builtin ? ": point that elsewhere to set it aside." : "."}
            </span>
          )}
        </div>
      )}
      {doc?.kind === "brief" && (
        <div className="doc-warn">
          This is what agents are told. Its <code>{"{{VARIABLES}}"}</code> are filled in by Wheelhouse
          {vars.missing.length || vars.unknown.length ? ":" : "; keep them where they are."}
          {vars.missing.length > 0 && (
            <span className="doc-warn-bad">
              {" "}
              it no longer has {vars.missing.map((v) => <code key={v}>{v}</code>).reduce<ReactNode[]>((a, x, i) => (i ? [...a, ", ", x] : [x]), [])}, so what goes there won't reach the agent.
            </span>
          )}
          {vars.unknown.length > 0 && (
            <span className="doc-warn-bad">
              {" "}
              {vars.unknown.map((v) => <code key={v}>{v}</code>).reduce<ReactNode[]>((a, x, i) => (i ? [...a, ", ", x] : [x]), [])} {vars.unknown.length === 1 ? "isn't one" : "aren't ones"} Wheelhouse fills in: agents will read {vars.unknown.length === 1 ? "it" : "them"} as written.
            </span>
          )}
        </div>
      )}
      <div className="doc-body">
        {error && !data ? (
          <div className="art-error">{error}</div>
        ) : !isNew && !data ? (
          <div className="art-loading muted">Loading…</div>
        ) : view === "compare" && original !== null ? (
          <div className="doc-diff">
            <div className="doc-diff-sum muted small">{counts.added || counts.removed ? `Against the built-in one: ${counts.added} line${counts.added === 1 ? "" : "s"} added, ${counts.removed} taken out.` : "The same as the built-in one."}</div>
            <pre className="diff">
              {foldDiff(diff).map((l, n) =>
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
          </div>
        ) : (
          <MarkdownEditor ref={area} value={value} onChange={(v) => (setText(v), setErr(null), setLeaving(false))} onSubmit={save} onCancel={tryClose} minRows={16} maxRows={60} placeholder="Write it in Markdown." />
        )}
      </div>
      {err && <div className="set-err doc-err">{err}</div>}
      <div className="nd-foot doc-foot">
        {leaving ? (
          <>
            <span className="small">You have changes that aren't saved.</span>
            <span className="spacer" />
            <button className="btn btn-small" onClick={() => setLeaving(false)}>
              Keep editing
            </button>
            <button className="btn btn-small" onClick={onClose}>
              Discard them
            </button>
          </>
        ) : (
          <>
            {doc?.overridden && (
              <button className="btn btn-small btn-ghost" disabled={!!busy} onClick={reset} title="Go back to the built-in text; yours is kept in .history">
                Reset to built-in
              </button>
            )}
            {doc && !doc.builtin && !isNew && (
              <button className="btn btn-small btn-ghost" disabled={!!busy || doc.used_by.length > 0} onClick={reset} title={doc.used_by.length ? `It's used as ${doc.used_by.map(usedWord).join(", ")}: point that elsewhere first` : "Move it out of your guides; it's kept in .history"}>
                Set aside
              </button>
            )}
            <span className="spacer" />
            <span className="muted small m-hide">{doc?.builtin && !doc.overridden ? "Saving puts yours in place of the built-in one." : ""}</span>
            <button className="btn btn-small" onClick={tryClose}>
              {dirty ? "Cancel" : "Close"}
            </button>
            <button className="btn btn-small btn-primary" disabled={!dirty || !!busy} onClick={save}>
              {busy === "save" ? "Saving…" : "Save"} <Kbd>⌘↩</Kbd>
            </button>
          </>
        )}
      </div>
    </Modal>
  );
}

// ---- where these live ----

function Files({ s }: { s: Settings }) {
  return (
    <section className="set-section set-files" id="set-files">
      <div className="set-head">
        <h2>Where these live</h2>
      </div>
      <dl className="set-dl">
        <dt>Your settings</dt>
        <dd className="mono-s">{s.file}</dd>
        <dt>build.lua</dt>
        <dd className="mono-s">
          {s.build.lua}
          {!s.build.lua_exists && <span className="muted"> · not there: Build uses its defaults</span>}
        </dd>
        <dt>review.lua</dt>
        <dd className="mono-s">
          {s.review.lua}
          {!s.review.lua_exists && <span className="muted"> · not there: Review isn't set up</span>}
        </dd>
        <dt>What you replaced</dt>
        <dd className="mono-s">{s.history}</dd>
      </dl>
      <p className="set-lead">
        What you set here goes in settings.json, over build.lua and review.lua: it wins, and “Use build.lua's” takes it out again. Some things stay in the Lua, because they're code or they guard the wall: agents' programs and their arguments, PATH, caches, the wall itself, and effort functions.
      </p>
    </section>
  );
}
