// A Content Security Policy for every built page, from the page itself.
//
// Astro's own security.csp misses Starlight's inline scripts (the theme and
// the sidebar's restore) and the component styles MDX pages pull in, and a
// browser then refuses them. So after the build, this parses each page as a
// browser does (parse5: the same tokenizer rules, CRLF and entities already
// resolved), hashes every inline <script> and <style> body, and puts a <meta>
// CSP in its <head> allowing those and nothing else inline. It goes right
// after <meta charset>, so the charset stays in the first 1024 bytes, and
// before any script or style.
//
// Starlight sizes icons and indents the sidebar with style="" attributes,
// which no hash can cover, so style attributes (and only those) are allowed
// inline: an attribute can't run script. Pagefind's search runs WebAssembly,
// hence 'wasm-unsafe-eval'. public/_headers adds what a <meta> can't carry
// (frame-ancestors). scripts/check-csp.mjs checks the result after a build.

import { createHash } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { parse } from "parse5";

const sha = (text) => `'sha256-${createHash("sha256").update(text, "utf8").digest("base64")}'`;

function walk(node, visit) {
  visit(node);
  for (const child of node.childNodes ?? []) walk(child, visit);
  // a <template>'s contents live apart from its children
  if (node.content) walk(node.content, visit);
}

const attrs = (el) => Object.fromEntries((el.attrs ?? []).map((a) => [a.name, a.value]));
const textOf = (el) =>
  (el.childNodes ?? [])
    .filter((n) => n.nodeName === "#text")
    .map((n) => n.value)
    .join("");

/** What a page has inline, where its <head> and <meta charset> are, and its CSP <meta>s. */
export function read(html) {
  const doc = parse(html, { sourceCodeLocationInfo: true });
  const scripts = [];
  const styles = [];
  let head;
  let charset;
  let csp;
  let csps = 0;
  walk(doc, (n) => {
    if (n.nodeName === "head") head = n;
    else if (n.nodeName === "script" && !("src" in attrs(n))) {
      const body = textOf(n);
      if (body) scripts.push(body);
    } else if (n.nodeName === "style") {
      const body = textOf(n);
      if (body) styles.push(body);
    } else if (n.nodeName === "meta") {
      const a = attrs(n);
      if ("charset" in a && !charset) charset = n;
      if ((a["http-equiv"] ?? "").toLowerCase() === "content-security-policy") {
        csp = a.content ?? "";
        csps++;
      }
    }
  });
  return { scripts, styles, head, charset, csp, csps };
}

/** The policy a page needs, from what it has inline. */
export function policy(html) {
  const { scripts, styles } = read(html);
  const hashes = (bodies) => [...new Set(bodies.map(sha))].join(" ");
  return [
    "default-src 'self'",
    `script-src 'self' 'wasm-unsafe-eval' ${hashes(scripts)}`.trim(),
    `style-src 'self' ${hashes(styles)}`.trim(),
    "style-src-attr 'unsafe-inline'",
    "img-src 'self' data:",
    "font-src 'self'",
    "connect-src 'self'",
    "base-uri 'self'",
    "form-action 'self'",
    "object-src 'none'",
  ].join("; ");
}

/** The page with its policy in its <head>. */
export function withPolicy(html, file) {
  const { head, charset, csp } = read(html);
  if (csp !== undefined) throw new Error(`${file} already has a CSP <meta>`);
  // An implied <head> (none in the file) has no place to put it.
  if (!head?.sourceCodeLocation?.startTag) throw new Error(`${file} has no <head>`);
  const at = charset?.sourceCodeLocation?.endOffset ?? head.sourceCodeLocation.startTag.endOffset;
  const meta = `<meta http-equiv="content-security-policy" content="${policy(html)}">`;
  return html.slice(0, at) + meta + html.slice(at);
}

export async function* pages(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) yield* pages(path);
    else if (entry.name.endsWith(".html")) yield path;
  }
}

export default function csp() {
  return {
    name: "wheelhouse:csp",
    hooks: {
      "astro:build:done": async ({ dir, logger }) => {
        let count = 0;
        for await (const file of pages(fileURLToPath(dir))) {
          await writeFile(file, withPolicy(await readFile(file, "utf8"), file));
          count++;
        }
        logger.info(`a CSP on ${count} pages`);
      },
    },
  };
}
