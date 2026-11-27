import { forwardRef, useImperativeHandle, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { IconBold, IconCode, IconCodeBlock, IconHeading, IconImage, IconItalic, IconLink, IconListBullet, IconListNumber, IconListTask, IconQuote, IconStrike, IconTable } from "./icons";
import { imageFiles, markdownImage, pastedName, sendable, stage } from "./images";
import { Prose, useToast } from "./ui";

let adding = 0;

/** Put images in Markdown: a placeholder at the cursor at once, then the
 * image's link when the hub has it (src/images.rs keeps it with the docket). */
function insertImages(ta: HTMLTextAreaElement, files: File[], value: { get: () => string; set: (v: string) => void }, onError: (msg: string) => void) {
  for (const f of files) {
    // No brackets in it, so the editor can always tell it's still going up.
    const tag = `![Adding ${(f.name || "image").replace(/[[\]()\n]/g, "")} (${++adding})…]()`;
    const at = ta.selectionStart;
    put(ta, at, ta.selectionEnd, tag + "\n", [at + tag.length + 1, at + tag.length + 1]);
    const name = f.name && f.name !== "image.png" ? f.name : pastedName();
    const alt = name.replace(/\.[a-z0-9]+$/i, "").replace(/[-_]+/g, " ");
    const swap = (text: string) => {
      // Still the textarea on screen: through its own editing, so ⌘Z undoes it.
      if (ta.isConnected && ta.value.includes(tag)) {
        const i = ta.value.indexOf(tag);
        const caret = ta.selectionStart;
        put(ta, i, i + tag.length, text, caret > i ? [caret + text.length - tag.length, caret + text.length - tag.length] : [caret, caret]);
        return;
      }
      // Gone (Preview, say): in the editor's value, never through what has focus now.
      const v = value.get();
      const i = v.indexOf(tag);
      if (i >= 0) value.set(v.slice(0, i) + text + v.slice(i + tag.length));
    };
    sendable(f)
      .then((b) => stage(b, name))
      .then(
        (s) => swap(markdownImage(s, alt)),
        (e: Error) => (swap(""), onError(e.message)),
      );
  }
}

/**
 * Writing Markdown: a toolbar, Write and Preview, the usual shortcuts
 * (⌘B, ⌘I, ⌘E, ⌘K, ⌘⇧X), lists that continue on Enter, and Tab to indent
 * a list item. Every change goes through the browser's own editing, so ⌘Z
 * undoes it.
 */

const mac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
const mod = mac ? "⌘" : "Ctrl+";

/** Replace `from..to` with `text`, keeping undo, then select `sel`. */
function put(ta: HTMLTextAreaElement, from: number, to: number, text: string, sel: [number, number]) {
  ta.focus();
  ta.setSelectionRange(from, to);
  let done = false;
  try {
    done = document.execCommand("insertText", false, text);
  } catch {
    done = false;
  }
  if (!done) {
    ta.setRangeText(text, from, to, "end");
    ta.dispatchEvent(new Event("input", { bubbles: true }));
  }
  ta.setSelectionRange(sel[0], sel[1]);
}

/** Wrap the selection in a mark (or unwrap it), or insert a placeholder. */
function wrap(ta: HTMLTextAreaElement, open: string, close = open, placeholder = "text") {
  const { selectionStart: s, selectionEnd: e, value: v } = ta;
  const sel = v.slice(s, e);
  if (sel && v.slice(s - open.length, s) === open && v.slice(e, e + close.length) === close) {
    put(ta, s - open.length, e + close.length, sel, [s - open.length, e - open.length]);
    return;
  }
  if (sel.startsWith(open) && sel.endsWith(close) && sel.length >= open.length + close.length) {
    const inner = sel.slice(open.length, sel.length - close.length);
    put(ta, s, e, inner, [s, s + inner.length]);
    return;
  }
  const inner = sel || placeholder;
  put(ta, s, e, open + inner + close, [s + open.length, s + open.length + inner.length]);
}

function link(ta: HTMLTextAreaElement) {
  const { selectionStart: s, selectionEnd: e, value: v } = ta;
  const sel = v.slice(s, e);
  if (/^https?:\/\/\S+$/.test(sel)) {
    put(ta, s, e, `[](${sel})`, [s + 1, s + 1]);
  } else {
    const text = sel || "text";
    const at = s + text.length + 3;
    put(ta, s, e, `[${text}](url)`, sel ? [at, at + 3] : [s + 1, s + 1 + text.length]);
  }
}

/** The whole lines the selection touches. */
function lineSpan(v: string, s: number, e: number): [number, number] {
  const a = v.lastIndexOf("\n", s - 1) + 1;
  const end = e > s && v[e - 1] === "\n" ? e - 1 : e;
  const b = v.indexOf("\n", end);
  return [a, b === -1 ? v.length : b];
}

const MARK = /^(\s*)(?:[-*+] \[[ xX]\] |[-*+] |\d+[.)] |> |#{1,6} )?/;

/** Give each selected line a prefix, or take it away if they all have it. */
function prefix(ta: HTMLTextAreaElement, kind: "ul" | "ol" | "task" | "quote" | "h") {
  const { selectionStart: s, selectionEnd: e, value: v } = ta;
  const [a, b] = lineSpan(v, s, e);
  const lines = v.slice(a, b).split("\n");
  const want = (i: number) => (kind === "ul" ? "- " : kind === "ol" ? `${i + 1}. ` : kind === "task" ? "- [ ] " : kind === "quote" ? "> " : "## ");
  const has = (l: string) =>
    kind === "ul" ? /^\s*[-*+] (?!\[[ xX]\] )/.test(l) : kind === "ol" ? /^\s*\d+[.)] /.test(l) : kind === "task" ? /^\s*[-*+] \[[ xX]\] /.test(l) : kind === "quote" ? /^\s*> /.test(l) : /^\s*#{1,6} /.test(l);
  const all = lines.filter((l) => l.trim()).every(has);
  const out = lines.map((l, i) => {
    if (!l.trim() && lines.length > 1) return l;
    const [, indent] = MARK.exec(l) ?? ["", ""];
    const bare = l.replace(MARK, "");
    return all ? indent + bare : indent + want(i) + bare;
  });
  const text = out.join("\n");
  put(ta, a, b, text, lines.length === 1 ? [a + text.length, a + text.length] : [a, a + text.length]);
}

function codeBlock(ta: HTMLTextAreaElement) {
  const { selectionStart: s, selectionEnd: e, value: v } = ta;
  const [a, b] = s === e ? [s, e] : lineSpan(v, s, e);
  const sel = v.slice(a, b);
  const before = a > 0 && v[a - 1] !== "\n" ? "\n" : "";
  const text = `${before}\`\`\`\n${sel}\n\`\`\`\n`;
  const at = a + before.length + 4;
  put(ta, a, b, text, [at, at + sel.length]);
}

function table(ta: HTMLTextAreaElement) {
  const { selectionStart: s, value: v } = ta;
  const lineStart = v.lastIndexOf("\n", s - 1) + 1;
  const atLineStart = s === lineStart && (s === v.length || v[s] === "\n");
  const lead = s === 0 ? "" : atLineStart ? (v[s - 2] === "\n" || s < 2 ? "" : "\n") : "\n\n";
  const text = `${lead}| Column | Column |\n| --- | --- |\n| Cell | Cell |\n`;
  const at = s + lead.length + 2;
  put(ta, s, s, text, [at, at + 6]);
}

/** Enter in a list item continues the list; on an empty item, ends it. */
function continueList(ta: HTMLTextAreaElement): boolean {
  const { selectionStart: s, selectionEnd: e, value: v } = ta;
  if (s !== e) return false;
  const a = v.lastIndexOf("\n", s - 1) + 1;
  const line = v.slice(a, s);
  const m = /^(\s*)(?:([-*+]) \[[ xX]\] |([-*+]) |(\d+)([.)]) |> )/.exec(line);
  if (!m) return false;
  if (line.trim() === m[0].trim()) {
    put(ta, a, s, "", [a, a]);
    return true;
  }
  const next = m[2] ? `${m[1]}${m[2]} [ ] ` : m[3] ? `${m[1]}${m[3]} ` : m[4] ? `${m[1]}${Number(m[4]) + 1}${m[5]} ` : `${m[1]}> `;
  put(ta, s, s, `\n${next}`, [s + 1 + next.length, s + 1 + next.length]);
  return true;
}

/** Tab and Shift+Tab indent and outdent list items. */
function indent(ta: HTMLTextAreaElement, out: boolean): boolean {
  const { selectionStart: s, selectionEnd: e, value: v } = ta;
  const [a, b] = lineSpan(v, s, e);
  const lines = v.slice(a, b).split("\n");
  if (!lines.every((l) => !l.trim() || /^\s*([-*+]|\d+[.)]) /.test(l))) return false;
  const moved = lines.map((l) => (out ? l.replace(/^ {1,2}/, "") : l.trim() ? `  ${l}` : l));
  const text = moved.join("\n");
  const shift = text.length - (b - a);
  put(ta, a, b, text, lines.length === 1 ? [Math.max(a, s + shift), Math.max(a, e + shift)] : [a, a + text.length]);
  return true;
}

/**
 * The Markdown keys for any textarea: shortcuts, list continuation and
 * indenting. True when it handled the key.
 */
export function markdownKeys(e: KeyboardEvent<HTMLTextAreaElement>): boolean {
  const ta = e.currentTarget;
  const cmd = e.metaKey || e.ctrlKey;
  const k = e.key.toLowerCase();
  let done = false;
  if (cmd && !e.altKey && !e.shiftKey && k === "b") (wrap(ta, "**"), (done = true));
  else if (cmd && !e.altKey && !e.shiftKey && k === "i") (wrap(ta, "_"), (done = true));
  else if (cmd && !e.altKey && !e.shiftKey && k === "e") (wrap(ta, "`", "`", "code"), (done = true));
  else if (cmd && !e.altKey && !e.shiftKey && k === "k") (link(ta), (done = true));
  else if (cmd && e.shiftKey && k === "x") (wrap(ta, "~~"), (done = true));
  else if (e.key === "Enter" && !cmd && !e.shiftKey && !e.altKey && !e.nativeEvent.isComposing) done = continueList(ta);
  else if (e.key === "Tab" && !cmd && !e.altKey) done = indent(ta, e.shiftKey);
  if (done) e.preventDefault();
  return done;
}

/** A pasted link over selected text makes it a link. */
function pasteLink(e: React.ClipboardEvent<HTMLTextAreaElement>) {
  const ta = e.currentTarget;
  const url = e.clipboardData.getData("text/plain").trim();
  const { selectionStart: s, selectionEnd: en, value: v } = ta;
  const sel = v.slice(s, en);
  if (sel && !sel.includes("\n") && /^https?:\/\/\S+$/.test(url) && !/^https?:\/\//.test(sel)) {
    e.preventDefault();
    const text = `[${sel}](${url})`;
    put(ta, s, en, text, [s + text.length, s + text.length]);
  }
}

type Tool = { key: string; icon: ReactNode; label: string; run: (ta: HTMLTextAreaElement) => void; gap?: boolean };

const TOOLS: Tool[] = [
  { key: "h", icon: <IconHeading size={15} />, label: "Heading", run: (ta) => prefix(ta, "h") },
  { key: "b", icon: <IconBold size={15} />, label: `Bold (${mod}B)`, run: (ta) => wrap(ta, "**") },
  { key: "i", icon: <IconItalic size={15} />, label: `Italic (${mod}I)`, run: (ta) => wrap(ta, "_") },
  { key: "s", icon: <IconStrike size={15} />, label: `Strikethrough (${mod}⇧X)`, run: (ta) => wrap(ta, "~~") },
  { key: "code", icon: <IconCode size={15} />, label: `Code (${mod}E)`, run: (ta) => wrap(ta, "`", "`", "code") },
  { key: "link", icon: <IconLink size={15} />, label: `Link (${mod}K)`, run: link },
  { key: "quote", icon: <IconQuote size={15} />, label: "Quote", run: (ta) => prefix(ta, "quote"), gap: true },
  { key: "ul", icon: <IconListBullet size={15} />, label: "Bulleted list", run: (ta) => prefix(ta, "ul") },
  { key: "ol", icon: <IconListNumber size={15} />, label: "Numbered list", run: (ta) => prefix(ta, "ol") },
  { key: "task", icon: <IconListTask size={15} />, label: "Task list", run: (ta) => prefix(ta, "task") },
  { key: "pre", icon: <IconCodeBlock size={15} />, label: "Code block", run: codeBlock, gap: true },
  { key: "table", icon: <IconTable size={15} />, label: "Table", run: table },
];

export type EditorHandle = { focus: () => void };

/** A Markdown editor: Write and Preview, a toolbar, and the keys above. */
export const MarkdownEditor = forwardRef<
  EditorHandle,
  { value: string; onChange: (v: string) => void; onSubmit?: () => void; onCancel?: () => void; placeholder?: string; minRows?: number; maxRows?: number; footer?: ReactNode; images?: boolean }
>(function MarkdownEditor({ value, onChange, onSubmit, onCancel, placeholder, minRows = 6, maxRows = 24, footer, images = false }, ref) {
  const ta = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);
  const toast = useToast();
  // The value as it is now, for an image that finishes going up after a re-render.
  const latest = useRef({ value, onChange });
  latest.current = { value, onChange };
  const addImages = (files: File[]) =>
    ta.current && files.length && insertImages(ta.current, files, { get: () => latest.current.value, set: (v) => latest.current.onChange(v) }, (m) => toast(m, "error"));
  const [tab, setTab] = useState<"write" | "preview">("write");
  useImperativeHandle(ref, () => ({ focus: () => (setTab("write"), window.setTimeout(() => ta.current?.focus(), 0)) }), []);
  const run = (t: Tool) => {
    if (tab !== "write") setTab("write");
    window.setTimeout(() => ta.current && t.run(ta.current), 0);
  };
  return (
    <div
      className="md-editor"
      onKeyDown={(e) => {
        // Escape anywhere in the editor (the Preview tab too) cancels the
        // edit, not the page behind it.
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          onCancel?.();
        }
      }}
    >
      <div className="md-bar">
        <div className="md-tabs" role="tablist">
          <button role="tab" aria-selected={tab === "write"} className={tab === "write" ? "on" : ""} onClick={() => (setTab("write"), window.setTimeout(() => ta.current?.focus(), 0))}>
            Write
          </button>
          <button role="tab" aria-selected={tab === "preview"} className={tab === "preview" ? "on" : ""} onClick={() => setTab("preview")}>
            Preview
          </button>
        </div>
        <div className="md-tools" aria-label="Formatting">
          {TOOLS.map((t) => (
            <span key={t.key} className={`md-tool-wrap${t.gap ? " gap" : ""}`}>
              <button className="md-tool" title={t.label} aria-label={t.label} onMouseDown={(e) => e.preventDefault()} onClick={() => run(t)} disabled={tab !== "write"}>
                {t.icon}
              </button>
            </span>
          ))}
          {images && (
            <span className="md-tool-wrap gap">
              <button className="md-tool" title="Image: or paste or drop one" aria-label="Image" onMouseDown={(e) => e.preventDefault()} onClick={() => picker.current?.click()} disabled={tab !== "write"}>
                <IconImage size={15} />
              </button>
              <input
                ref={picker}
                type="file"
                accept="image/*"
                multiple
                hidden
                onChange={(e) => {
                  addImages(Array.from(e.target.files ?? []));
                  e.target.value = "";
                }}
              />
            </span>
          )}
        </div>
      </div>
      {tab === "write" ? (
        <textarea
          ref={ta}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onPaste={(e) => {
            const files = images ? imageFiles(e.clipboardData) : [];
            if (files.length) return (e.preventDefault(), addImages(files));
            pasteLink(e);
          }}
          onDragOver={(e) => images && Array.from(e.dataTransfer.items).some((i) => i.kind === "file") && e.preventDefault()}
          onDrop={(e) => {
            const files = images ? imageFiles(e.dataTransfer) : [];
            if (!files.length) return;
            e.preventDefault();
            addImages(files);
          }}
          rows={Math.min(maxRows, Math.max(minRows, value.split("\n").length + 1))}
          placeholder={placeholder}
          spellCheck
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              onSubmit?.();
              return;
            }
            markdownKeys(e);
          }}
        />
      ) : (
        <div className="md-preview" style={{ minHeight: `${minRows * 1.6}em` }}>
          {value.trim() ? <Prose text={value} /> : <span className="muted">Nothing to preview.</span>}
        </div>
      )}
      {footer}
    </div>
  );
});
