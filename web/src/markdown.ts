import DOMPurify from "dompurify";
import { Marked, type Token, type TokenizerAndRendererExtension } from "marked";
import markedFootnote from "marked-footnote";

/**
 * Markdown as GitHub renders a PR or issue: GFM (tables, task lists,
 * strikethrough, autolinks, footnotes), line breaks where you ended a
 * line, alerts (`> [!NOTE]`), and the HTML people put in PR and issue text
 * (`<details>`, `<summary>`, `<br>`, `<kbd>`, `<sub>`), sanitized. Also:
 * `D-12` links to the docket, and Linear's collapsible sections
 * (`+++ Title` … `+++`) open and close.
 */

const docketRef: TokenizerAndRendererExtension = {
  name: "docket",
  level: "inline",
  start(src) {
    const m = /(^|[^\w/-])D-\d+\b/.exec(src);
    return m ? m.index + m[1].length : undefined;
  },
  tokenizer(src) {
    const m = /^D-(\d+)\b/.exec(src);
    if (m) return { type: "docket", raw: m[0], id: m[0] };
    return undefined;
  },
  renderer(t) {
    return `<a href="#/d/${t.id}">${t.id}</a>`;
  },
};

/**
 * Prose an agent wrapped at a column reads as one paragraph, as it was
 * meant; a line someone ended on purpose keeps its break, as on GitHub.
 * Wrapped lines are 55 to 100 characters, of about one length, and break
 * mid-sentence; a line ended on purpose usually ends a sentence.
 */
export function hardWrapped(raw: string): boolean {
  const lines = raw
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
  if (lines.length < 2) return false;
  const broken = lines.slice(0, -1);
  const lens = broken.map((l) => l.length);
  if (!lens.every((n) => n >= 55 && n <= 100) || Math.max(...lens) - Math.min(...lens) > 30) return false;
  const mid = broken.filter((l) => !/[.!?:;]["')\]*]*$/.test(l)).length;
  return lines.length >= 3 ? mid * 2 >= broken.length : mid === broken.length;
}

/** Soft line breaks (not `  ` or `\` at a line's end) become spaces. */
function unbreak(tokens: Token[]) {
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i];
    if (t.type === "br" && t.raw === "\n") tokens[i] = { type: "text", raw: "\n", text: " " } as Token;
    else if ("tokens" in t && Array.isArray(t.tokens)) unbreak(t.tokens);
  }
}

const md = new Marked({ gfm: true, breaks: true, async: false });
md.use(markedFootnote({ description: "Footnotes" }));
md.use({
  extensions: [docketRef],
  walkTokens(token) {
    // A link inside a link's text would nest anchors: keep it as text.
    if (token.type === "link" && token.tokens) {
      token.tokens = token.tokens.map((t) => (t.type === "docket" ? ({ type: "text", raw: t.raw, text: t.raw } as Token) : t));
    }
    if ((token.type === "paragraph" || (token.type === "text" && "tokens" in token && token.tokens)) && hardWrapped(token.raw)) {
      unbreak((token as { tokens: Token[] }).tokens);
    }
  },
});

/** Linear's `+++ Title` … `+++` collapsibles, outside code blocks. */
function linearCollapsibles(text: string): string {
  if (!text.includes("+++")) return text;
  const out: string[] = [];
  let fence: string | null = null;
  let open = 0;
  for (const line of text.split("\n")) {
    const f = /^\s*(```+|~~~+)/.exec(line);
    if (f) fence = fence ? (line.trim().startsWith(fence) ? null : fence) : f[1];
    if (!fence) {
      const start = /^\+\+\+\s+(\S.*)$/.exec(line);
      if (start) {
        out.push(`<details><summary>${escape(start[1].trim())}</summary>`, "");
        open++;
        continue;
      }
      if (/^\+\+\+\s*$/.test(line) && open > 0) {
        out.push("", "</details>", "");
        open--;
        continue;
      }
    }
    out.push(line);
  }
  while (open-- > 0) out.push("", "</details>");
  return out.join("\n");
}

function escape(s: string) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

let hooked = false;
function purify(html: string, forbid: string[] = []): string {
  if (!hooked) {
    hooked = true;
    DOMPurify.addHook("afterSanitizeAttributes", (node) => {
      if (node.tagName === "A") {
        const href = node.getAttribute("href") || "";
        if (href && !href.startsWith("#")) {
          node.setAttribute("target", "_blank");
          node.setAttribute("rel", "noreferrer noopener");
        }
      }
      if (node.tagName === "IMG") node.setAttribute("loading", "lazy");
      if (node.tagName === "INPUT") node.setAttribute("disabled", "");
    });
  }
  return DOMPurify.sanitize(html, {
    ADD_ATTR: ["target"],
    FORBID_TAGS: ["style", "form", "button", "textarea", "select", "option", "iframe", "video", "audio", "source", ...forbid],
    FORBID_ATTR: ["style", "class"],
    // Ids (footnotes) get a `user-content-` prefix, as on GitHub, so text
    // can't clobber the page's own names.
    SANITIZE_NAMED_PROPS: true,
  });
}

const ALERTS: Record<string, string> = { NOTE: "Note", TIP: "Tip", IMPORTANT: "Important", WARNING: "Warning", CAUTION: "Caution" };

/** GitHub's alerts: a quote whose first line is `[!NOTE]`, `[!TIP]`, … */
function alerts(html: string): string {
  return html.replace(/<blockquote>\s*<p>\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]\s*(<br>\s*|<\/p>\s*)?/g, (_m, kind: string, after?: string) => {
    const head = `<blockquote data-alert="${kind.toLowerCase()}"><p data-alert-title="">${ALERTS[kind]}</p>`;
    return after?.startsWith("</p>") ? head : `${head}<p>`;
  });
}

const cache = new Map<string, string>();
const LIMIT = 400_000;

/** Sanitized HTML for Markdown text, cached by the text. */
export function renderMarkdown(text: string): string {
  const hit = cache.get(text);
  if (hit !== undefined) return hit;
  let html: string;
  if (text.length > LIMIT) {
    html = `<pre>${escape(text.slice(0, LIMIT))}\n…</pre>`;
  } else {
    try {
      html = md.parse(linearCollapsibles(text.replace(/\r\n?/g, "\n"))) as string;
    } catch {
      html = `<pre>${escape(text)}</pre>`;
    }
    html = alerts(purify(html))
      // Wide tables scroll on their own, not the page.
      .replace(/<table>/g, '<div class="prose-table"><table>')
      .replace(/<\/table>/g, "</table></div>")
      // FORBID_ATTR drops class, so the docket links get theirs back here.
      .replace(/<a href="#\/d\/(D-\d+)">/g, '<a href="#/d/$1" class="docket-ref">');
  }
  if (cache.size > 400) cache.delete(cache.keys().next().value as string);
  cache.set(text, html);
  return html;
}

const inlineCache = new Map<string, string>();

/**
 * One line of Markdown, inline only (`code`, **bold**, _em_, ~~del~~,
 * links), for summaries and list rows. A leading heading mark or bullet is
 * dropped. With `links: false` (inside a row that's a link itself), links
 * keep their text but aren't links.
 */
export function renderInline(text: string, links = true): string {
  const k = `${links ? 1 : 0}${text}`;
  const hit = inlineCache.get(k);
  if (hit !== undefined) return hit;
  const line = text.slice(0, 4000).replace(/\s*\n\s*/g, " ").replace(/^\s*(#{1,6}\s+|[-*+]\s+|>\s*)/, "");
  let html: string;
  try {
    html = md.parseInline(line) as string;
  } catch {
    html = escape(line);
  }
  html = purify(html, links ? [] : ["a"]);
  if (inlineCache.size > 600) inlineCache.delete(inlineCache.keys().next().value as string);
  inlineCache.set(k, html);
  return html;
}

// ---- task lists ----

const TASK = /^((?:\s*>)*\s*(?:[-*+]|\d+[.)])\s+)\[( |x|X)\](?=\s|$)/;

/** Where each task box is in the text: its line, outside code blocks. */
function taskLines(lines: string[]): number[] {
  const out: number[] = [];
  let fence: string | null = null;
  lines.forEach((line, i) => {
    const f = /^\s*(```+|~~~+)/.exec(line);
    if (f) {
      fence = fence ? (line.trim().startsWith(fence) ? null : fence) : f[1];
      return;
    }
    if (!fence && TASK.test(line)) out.push(i);
  });
  return out;
}

/**
 * The text with its `index`th task box ticked or cleared, or null when the
 * boxes in the text don't match the `count` rendered (then it's safer to
 * edit by hand).
 */
export function toggleTask(text: string, index: number, checked: boolean, count: number): string | null {
  const lines = text.split("\n");
  const at = taskLines(lines);
  if (at.length !== count || index < 0 || index >= at.length) return null;
  const n = at[index];
  lines[n] = lines[n].replace(TASK, (_m, pre: string) => `${pre}[${checked ? "x" : " "}]`);
  return lines.join("\n");
}
