// A docket's code changes: the files it touched, and their diff, drawn by
// Pierre's diffs (@pierre/diffs: Shiki-highlighted in a worker, virtualized,
// split or unified). From its base, or any point on its record, to its HEAD,
// a later point, or what isn't committed yet. The hub reads it with git
// (src/diffs.rs); this view only draws it. Its own chunk, loaded on first
// open, so the rest of the app never carries a highlighter.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CodeView, WorkerPoolContextProvider, type CodeViewHandle, type CodeViewItem } from "@pierre/diffs/react";
import { parsePatchFiles, type FileDiffMetadata } from "@pierre/diffs";
import DiffsWorker from "@pierre/diffs/worker/worker.js?worker";
import { get } from "./api";
import { countsLabel, diffPath, fileStatus, sortFiles, sourceLabel, splitPath, statKey, statusesOf, type DiffFile, type DiffPoint, type DiffView } from "./diffview";
import { IconBack, IconChevronDown, IconRefresh } from "./icons";
import { ago, go, store, useHub, useTheme } from "./live";
import { useKeys, useMenu, usePhone } from "./ui";

/** How often a live checkout's counts are read again while the view is open. */
const POLL_MS = 8000;

type Layout = "unified" | "split";

const POOL = {
  poolOptions: { workerFactory: () => new DiffsWorker(), poolSize: Math.min(3, Math.max(1, (navigator.hardwareConcurrency || 2) - 1)) },
  highlighterOptions: {},
};

export default function ChangesScreen({ id, embedded }: { id: string; embedded?: boolean }) {
  return (
    <WorkerPoolContextProvider poolOptions={POOL.poolOptions} highlighterOptions={POOL.highlighterOptions}>
      <Changes id={id} embedded={embedded} />
    </WorkerPoolContextProvider>
  );
}

function Changes({ id, embedded }: { id: string; embedded?: boolean }) {
  const hub = useHub();
  const phone = usePhone();
  const [theme] = useTheme();
  const menu = useMenu();
  const [from, setFrom] = useState<string | null>(null);
  const [to, setTo] = useState<string | null>(null);
  const [view, setView] = useState<DiffView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [layout, setLayoutState] = useState<Layout>(() => store<Layout>("diffLayout", "split")[0]);
  const [active, setActive] = useState<string | null>(null);
  // Over the hub's cap: one file at a time, fetched on its own.
  const [single, setSingle] = useState<{ file: DiffFile; patch: string } | null>(null);
  const singleRef = useRef(single);
  singleRef.current = single;
  // The newest read wins: an older answer that comes back late is dropped.
  const ticket = useRef(0);
  const range = `${from ?? ""}|${to ?? ""}`;
  const rangeRef = useRef(range);
  rangeRef.current = range;
  const [filter, setFilter] = useState("");
  const codeRef = useRef<CodeViewHandle<undefined, undefined>>(null);
  const seen = useRef("");
  const fromRef = useRef<HTMLButtonElement>(null);
  const toRef = useRef<HTMLButtonElement>(null);
  const docket = hub.byId.get(id);
  const effective: Layout = phone ? "unified" : layout;
  // Embedded, the Mac app sets the mode (main.tsx); otherwise it's yours.
  const mode = document.documentElement.dataset.mode;
  const themeType = embedded ? (mode === "light" || mode === "dark" ? mode : "system") : theme;

  const setLayout = (l: Layout) => {
    setLayoutState(l);
    store<Layout>("diffLayout", "split")[1](l);
  };

  /** One file's own patch, for a diff too big to show whole. */
  const fetchOne = useCallback(
    async (f: DiffFile) => {
      const asked = rangeRef.current;
      try {
        const one = await get<DiffView>(diffPath(id, { from, to, path: f.path, old_path: f.old_path }));
        if (rangeRef.current !== asked) return;
        if (one.patch != null) setSingle({ file: f, patch: one.patch });
        else setError(`${f.path}'s diff is over the hub's limit on its own`);
      } catch (e) {
        if (rangeRef.current === asked) setError((e as Error).message);
      }
    },
    [id, from, to],
  );

  const load = useCallback(
    async (quiet = false) => {
      const n = ++ticket.current;
      if (!quiet) setLoading(true);
      try {
        const v = await get<DiffView>(diffPath(id, { from, to }));
        if (n !== ticket.current) return;
        seen.current = statKey(v);
        setView(v);
        setError(null);
        // A quiet read again keeps the file you had open, read again too.
        const open = singleRef.current;
        if (open && quiet && v.too_big) void fetchOne(open.file);
        else setSingle(null);
      } catch (e) {
        if (n === ticket.current) setError((e as Error).message);
      } finally {
        if (n === ticket.current) setLoading(false);
      }
    },
    [id, from, to, fetchOne],
  );

  useEffect(() => void load(), [load]);

  // The record moved (a commit, a round): read it again.
  const stamp = docket?.updated_at;
  const first = useRef(true);
  useEffect(() => {
    if (first.current) return void (first.current = false);
    void load(true);
  }, [stamp]);

  // The working tree changes without an entry: while it's what's shown,
  // read its counts now and then, and the patch only when they changed.
  const live = !!view && view.to.ref === "work";
  useEffect(() => {
    if (!live) return;
    const t = window.setInterval(async () => {
      if (document.hidden) return;
      try {
        const s = await get<DiffView>(diffPath(id, { from, to, stat: true }));
        if (statKey(s) !== seen.current) void load(true);
      } catch {
        /* the next tick tries again */
      }
    }, POLL_MS);
    return () => window.clearInterval(t);
  }, [live, id, from, to, load]);

  const files = useMemo(() => sortFiles(view?.files ?? []), [view]);
  const statuses = useMemo(() => statusesOf(view?.patch), [view]);
  const shown = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return q ? files.filter((f) => f.path.toLowerCase().includes(q)) : files;
  }, [files, filter]);

  const items = useMemo<CodeViewItem<undefined>[]>(() => {
    const patch = single?.patch ?? view?.patch;
    if (!patch) return [];
    let parsed: FileDiffMetadata[];
    try {
      parsed = parsePatchFiles(patch).flatMap((p) => p.files);
    } catch {
      return [];
    }
    const order = new Map(files.map((f, i) => [f.path, i]));
    return parsed
      .map((fd) => ({ id: fd.name, type: "diff" as const, fileDiff: fd }))
      .sort((a, b) => (order.get(a.id) ?? 1e9) - (order.get(b.id) ?? 1e9));
  }, [view, single, files]);

  const open = async (f: DiffFile) => {
    setActive(f.path);
    if (view?.patch) {
      codeRef.current?.scrollTo({ type: "item", id: f.path, align: "start" });
      return;
    }
    await fetchOne(f);
  };

  const step = (d: number) => {
    if (!shown.length) return;
    const i = Math.max(0, shown.findIndex((f) => f.path === active));
    const next = shown[Math.min(shown.length - 1, Math.max(0, active ? i + d : 0))];
    if (next) void open(next);
  };

  useKeys({
    j: () => step(1),
    k: () => step(-1),
    s: () => !phone && setLayout(layout === "split" ? "unified" : "split"),
    r: () => void load(),
    Escape: () => !embedded && go(`/d/${id}`),
  });

  const pick = (which: "from" | "to", anchor: HTMLElement | null) => {
    if (!view || !anchor) return;
    const pts = view.points.filter((p) => (which === "from" ? p.ref !== "work" : true));
    const current = which === "from" ? view.from.ref : view.to.ref;
    menu.open(
      anchor,
      pts.map((p) => ({
        key: p.ref,
        label: pointLabel(p),
        hint: p.sha ? p.sha.slice(0, 7) : undefined,
        checked: p.ref === current,
        onPick: () => (which === "from" ? setFrom(p.ref) : setTo(p.ref)),
      })),
      which === "from" ? "Compare from" : "Compare to",
    );
  };

  const title = docket?.title ?? id;
  return (
    <div className={`view changes${embedded ? " embedded" : ""}`}>
      <header className="view-head ch-head">
        {!embedded && (
          <button className="btn btn-ghost btn-small" onClick={() => go(`/d/${id}`)} title={`Back to ${id} (Esc)`}>
            <IconBack size={13} /> {id}
          </button>
        )}
        <span className="view-title ch-title" title={title}>
          Changes
          {view && (
            <span className="ch-counts">
              <span className="ch-add">+{view.added}</span> <span className="ch-del">−{view.removed}</span>
            </span>
          )}
        </span>
        {view && (
          <span className="ch-range">
            <button ref={fromRef} className="btn btn-small ch-point" onClick={() => pick("from", fromRef.current)} title="What to compare from">
              {view.from.label} <IconChevronDown size={11} />
            </button>
            <span className="ch-arrow">→</span>
            <button ref={toRef} className="btn btn-small ch-point" onClick={() => pick("to", toRef.current)} title="What to compare to">
              {view.to.label} <IconChevronDown size={11} />
            </button>
          </span>
        )}
        <span className="spacer" />
        {!phone && (
          <span className="seg seg-text" role="tablist" title="Layout (S)">
            <button className={effective === "split" ? "on" : ""} onClick={() => setLayout("split")} role="tab" aria-selected={effective === "split"}>
              Split
            </button>
            <button className={effective === "unified" ? "on" : ""} onClick={() => setLayout("unified")} role="tab" aria-selected={effective === "unified"}>
              Unified
            </button>
          </span>
        )}
        <button className={`btn btn-ghost btn-small${loading ? " spinning" : ""}`} onClick={() => void load()} title="Read it again (R)" aria-label="Refresh">
          <IconRefresh size={13} />
        </button>
      </header>
      {view && <div className="ch-source">{sourceLabel(view)}</div>}
      {error && !view ? (
        <div className="view-empty">{error}</div>
      ) : !view ? (
        <div className="view-empty">Reading the changes…</div>
      ) : !files.length ? (
        <div className="view-empty">
          No changes from {view.from.label.toLowerCase() === "base" ? "the base" : view.from.label} to {view.to.label === "Uncommitted" ? "the working tree" : view.to.label}.
        </div>
      ) : (
        <div className={`ch-body${phone ? " ch-phone" : ""}`}>
          <FileList files={shown} all={files.length} view={view} statuses={statuses} active={active} filter={filter} setFilter={setFilter} onOpen={(f) => void open(f)} phone={phone} />
          <div className="ch-diff">
            {error && <div className="ch-error">{error}</div>}
            {view.too_big && !single ? (
              <div className="view-empty">This diff is too big to show at once. Pick a file to see its changes.</div>
            ) : (
              <CodeView
                ref={codeRef}
                className="ch-code"
                items={items}
                options={{
                  diffStyle: effective,
                  themeType,
                  theme: { dark: "pierre-dark", light: "pierre-light" },
                  lineDiffType: "word-alt",
                  diffIndicators: "bars",
                  hunkSeparators: "line-info",
                  overflow: phone ? "wrap" : "scroll",
                  stickyHeaders: true,
                }}
              />
            )}
          </div>
        </div>
      )}
      {menu.node}
    </div>
  );
}

const pointLabel = (p: DiffPoint) => (p.at ? `${p.label} · ${ago(p.at)}` : p.label);

function FileList(p: {
  files: DiffFile[];
  all: number;
  view: DiffView;
  statuses: Map<string, "A" | "D">;
  active: string | null;
  filter: string;
  setFilter: (s: string) => void;
  onOpen: (f: DiffFile) => void;
  phone: boolean;
}) {
  const [shut, setShut] = useState(p.phone);
  return (
    <nav className={`ch-files${shut ? " shut" : ""}`} aria-label="Changed files">
      <button className="ch-files-head" onClick={() => setShut(!shut)} aria-expanded={!shut}>
        {p.all} file{p.all === 1 ? "" : "s"} <span className="muted">{countsLabel(p.view.added, p.view.removed)}</span>
        {p.phone && <IconChevronDown size={12} />}
      </button>
      {!shut && (
        <>
          {p.all > 8 && <input className="ch-filter" placeholder="Filter files" value={p.filter} onChange={(e) => p.setFilter(e.target.value)} spellCheck={false} />}
          <ul>
            {p.files.map((f) => {
              const { dir, name } = splitPath(f.path);
              const st = fileStatus(f, p.statuses);
              return (
                <li key={f.path}>
                  <button className={`ch-file${p.active === f.path ? " on" : ""}`} onClick={() => p.onOpen(f)} title={f.old_path ? `${f.old_path} → ${f.path}` : f.path}>
                    <span className={`ch-st st-${st}`}>{st}</span>
                    <span className="ch-name">
                      {name}
                      {dir && <span className="ch-dir">{dir}</span>}
                    </span>
                    {f.binary ? (
                      <span className="ch-n muted">bin</span>
                    ) : (
                      <span className="ch-n">
                        {f.added > 0 && <span className="ch-add">+{f.added}</span>}
                        {f.removed > 0 && <span className="ch-del">−{f.removed}</span>}
                      </span>
                    )}
                  </button>
                </li>
              );
            })}
          </ul>
          {p.view.untracked_more > 0 && <div className="ch-more">and {p.view.untracked_more} more new files not shown</div>}
        </>
      )}
    </nav>
  );
}
