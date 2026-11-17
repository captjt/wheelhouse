import DOMPurify from "dompurify";
import { Marked, type TokenizerAndRendererExtension } from "marked";

/**
 * Markdown as GitHub renders it (GFM: tables, task lists, strikethrough,
 * autolinks, fenced code), plus the HTML people put in PR and issue text
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
    return `<a href="#/d/${t.id}" class="docket-ref">${t.id}</a>`;
  },
};

const md = new Marked({ gfm: true, breaks: false, async: false });
md.use({
  extensions: [docketRef],
  // A link inside a link's text would nest anchors: keep it as text.
  walkTokens(token) {
    if (token.type === "link" && token.tokens) {
      token.tokens = token.tokens.map((t) => (t.type === "docket" ? { type: "text", raw: t.raw, text: t.raw } : t));
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
    FORBID_ATTR: ["style", "class", "id"],
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
    html = purify(html)
      // Wide tables scroll on their own, not the page.
      .replace(/<table>/g, '<div class="prose-table"><table>')
      .replace(/<\/table>/g, "</table></div>");
    // FORBID_ATTR drops class, so the docket links get theirs back here.
    html = html.replace(/<a href="#\/d\/(D-\d+)">/g, '<a href="#/d/$1" class="docket-ref">');
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
