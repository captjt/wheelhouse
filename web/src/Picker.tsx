import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { get, post, type Docket, type Live } from "./api";
import { useHub } from "./live";
import { useToast } from "./ui";

/** Claude Code's question picker, as the hub read it off the pane's screen (src/picker.rs). */
export type PickerOpt = { n: number; label: string; description?: string; checked?: boolean; kind: "choice" | "other" | "chat" | "submit" | "cancel" };
export type Picker = {
  tabs: { label: string; answered: boolean }[];
  submit_tab: boolean;
  review: boolean;
  question: string;
  multi: boolean;
  options: PickerOpt[];
  next?: string;
  answers?: { question: string; answer: string }[];
  notes?: string[];
  /** The box for your own words is open in the pane. */
  typing?: boolean;
  /** What's typed in that box already: replaced by yours. */
  draft?: string;
};
/** `since`: when the question was asked, sent back with an answer so the hub refuses one asked again since. */
type Read = { picker: Picker | null; since?: string | null; typed?: boolean };
type Done = Read & { did: string; cleared: boolean; recorded: boolean; now?: string };

/** The agent's question, with its answers as buttons: picked here, the hub
 * presses the same keys you would in the pane, once it has checked the pane
 * still shows this question. `fallback` when the agent isn't asking one, or
 * the hub can't read it (then the pane's button is the way). */
export function QuestionPicker({ d, live, fallback = null }: { d: Docket; live?: Live | null; fallback?: ReactNode }) {
  const hub = useHub();
  const toast = useToast();
  const l = live ?? d.live;
  const on = !!l?.pickable && l.state === "blocked";
  const [read, setRead] = useState<Read | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [typingHere, setTypingHere] = useState(false);
  const q = l?.block ? `?block=${encodeURIComponent(l.block)}` : "";
  const busyRef = useRef(false);
  // Bumped by each answer: a read that started before it is older than its result, and dropped.
  const gen = useRef(0);
  // Reads in a row that found no picker: one is a redraw, two is gone.
  const misses = useRef(0);

  const load = useCallback(() => {
    const at = gen.current;
    return get<Read>(`/api/dockets/${d.id}/picker${q}`).then(
      (r) => {
        if (busyRef.current || at !== gen.current) return;
        if (r.picker) {
          misses.current = 0;
          setRead(r);
        } else if (++misses.current >= 2) setRead(r);
      },
      // The hub out of reach for a moment: keep what's shown; an answer is checked anyway.
      () => {},
    );
  }, [d.id, q]);

  // Read it now, when the question changes, and every few seconds: it may be
  // answered in the pane, or move on to the next question. Not while you type
  // your own answer: a redraw would take the box from under you.
  const hold = typingHere || !!text.trim();
  useEffect(() => {
    if (!on) {
      misses.current = 0;
      return setRead(null);
    }
    if (hold) return;
    let live = true;
    let timer: number | undefined;
    const tick = () => {
      load().finally(() => {
        if (live) timer = window.setTimeout(tick, document.hidden ? 8000 : 2500);
      });
    };
    tick();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [on, load, l?.since, hold]);

  const p = read?.picker ?? null;
  // Your words were for that question: not the next one.
  const asked = p ? `${p.review} ${p.question}` : "";
  useEffect(() => setText(""), [asked]);
  if (!on || !p) return <>{fallback}</>;

  const act = (action: "pick" | "next" | "back", o?: PickerOpt) => (e?: React.SyntheticEvent) => {
    e?.preventDefault();
    e?.stopPropagation();
    if (busyRef.current) return;
    const key = o ? `${action}-${o.n}` : action;
    busyRef.current = true;
    gen.current++;
    setBusy(key);
    post<Done>(`/api/dockets/${d.id}/pick`, {
      block: l?.block,
      since: read?.since ?? null,
      checked: o?.checked,
      question: p.question,
      review: p.review,
      action,
      n: o?.n,
      label: o?.label,
      text: o?.kind === "other" ? text : undefined,
    })
      .then(
        (r) => {
          if (o?.kind === "other") setText("");
          if (r.cleared) toast(o?.kind === "submit" || !p.submit_tab ? "Answered" : r.did);
          else if (!r.picker) toast(`Sent, but ${r.now ?? "the question didn't change"}. Look at the pane.`, "error");
          if (r.picker) {
            misses.current = 0;
            setRead({ picker: r.picker, since: r.since, typed: r.typed });
          }
          hub.reload();
        },
        (err: Error) => toast(err.message, "error"),
      )
      .finally(() => {
        busyRef.current = false;
        gen.current++;
        setBusy(null);
        if (!document.hidden) void load();
      });
  };

  const choices = p.options.filter((o) => o.kind === "choice");
  const other = p.options.find((o) => o.kind === "other");
  const chat = p.options.find((o) => o.kind === "chat");
  const submit = p.options.find((o) => o.kind === "submit");
  const cancel = p.options.find((o) => o.kind === "cancel");
  const tabbed = p.tabs.length > 1 || p.submit_tab;

  return (
    <div className="qp" onClick={(e) => e.stopPropagation()}>
      {tabbed && (
        <div className="qp-tabs" aria-label="Questions">
          {p.tabs.map((t, i) => (
            <span key={i} className={`qp-tab${t.answered ? " done" : ""}`}>
              {t.answered ? "✓ " : ""}
              {t.label}
            </span>
          ))}
          {p.submit_tab && <span className={`qp-tab${p.review ? " on" : ""}`}>Submit</span>}
        </div>
      )}
      {p.review ? (
        <>
          <div className="qp-q">Review your answers</div>
          <dl className="qp-review">
            {(p.answers ?? []).map((a, i) => (
              <div key={i}>
                <dt>{a.question}</dt>
                <dd>{a.answer || <span className="muted">Not answered</span>}</dd>
              </div>
            ))}
          </dl>
          {(p.notes ?? []).map((n, i) => (
            <div key={i} className="qp-note">
              {n}
            </div>
          ))}
          <div className="qp-foot">
            {submit && (
              <button className="btn btn-primary" disabled={!!busy} onClick={act("pick", submit)}>
                {busy === `pick-${submit.n}` ? "Submitting…" : submit.label}
              </button>
            )}
            <button className="btn" disabled={!!busy} onClick={act("back")} title="Back to the last question (← in the pane)">
              Back
            </button>
            {cancel && (
              <button className="btn" disabled={!!busy} onClick={act("pick", cancel)}>
                {cancel.label}
              </button>
            )}
          </div>
        </>
      ) : (
        <>
          <div className="qp-q">{p.question}</div>
          {p.multi && <div className="qp-hint">Pick any that apply, then {p.next === "Submit" ? "review them" : "go on"}.</div>}
          <div className="qp-opts" role={p.multi ? "group" : "radiogroup"} aria-label={p.question}>
            {choices.map((o) => (
              <button
                key={o.n}
                className={`qp-opt${o.checked ? " on" : ""}`}
                disabled={!!busy}
                onClick={act("pick", o)}
                role={p.multi ? "checkbox" : "radio"}
                aria-checked={p.multi ? !!o.checked : false}
              >
                <span className="qp-n">{p.multi ? (o.checked ? "✓" : "") : o.n}</span>
                <span className="qp-opt-text">
                  <span className="qp-opt-label">{busy === `pick-${o.n}` ? `${o.label}…` : o.label}</span>
                  {o.description && o.description !== o.label && <span className="qp-opt-desc">{o.description}</span>}
                </span>
              </button>
            ))}
          </div>
          {other && read?.typed && (
            <form className="qp-other" onSubmit={act("pick", other)}>
              <input
                value={text}
                maxLength={2000}
                placeholder={p.draft ? `Or type your own (replaces “${p.draft}” in the pane)…` : "Or type your own answer…"}
                aria-label="Your own answer"
                disabled={!!busy}
                onFocus={() => setTypingHere(true)}
                onBlur={() => setTypingHere(false)}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => e.stopPropagation()}
              />
              <button className="btn" type="submit" disabled={!text.trim() || !!busy}>
                {busy === `pick-${other.n}` ? "Sending…" : "Answer"}
              </button>
            </form>
          )}
          <div className="qp-foot">
            {p.multi && p.next && (
              <button className="btn btn-primary" disabled={!!busy} onClick={act("next")}>
                {busy === "next" ? "…" : p.next === "Submit" ? "Review" : "Next"}
              </button>
            )}
            {chat && (
              <button className="btn btn-ghost" disabled={!!busy} onClick={act("pick", chat)} title="No answer: Claude Code waits for you to tell it instead">
                {chat.label}
              </button>
            )}
            {other && !read?.typed && <span className="qp-hint">Type your own answer in the pane.</span>}
          </div>
        </>
      )}
    </div>
  );
}
