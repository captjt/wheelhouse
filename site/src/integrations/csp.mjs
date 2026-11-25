// A Content Security Policy for every built page, from the page itself.
//
// Astro's own security.csp misses Starlight's inline scripts (the theme and
// the sidebar's restore) and the component styles MDX pages pull in, and a
// browser then refuses them. So after the build, this reads each page, hashes
// every inline <script> and <style> exactly as written, and puts a <meta> CSP
// first in its <head> allowing those and nothing else inline.
//
// Starlight sizes icons and indents the sidebar with style="" attributes,
// which no hash can cover, so style attributes (and only those) are allowed
// inline: an attribute can't run script. Pagefind's search runs WebAssembly,
// hence 'wasm-unsafe-eval'. public/_headers adds what a <meta> can't carry
// (frame-ancestors).

import { createHash } from "node:crypto";
import { readdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const sha = (text) => `'sha256-${createHash("sha256").update(text, "utf8").digest("base64")}'`;

// An inline element's exact body: no src (scripts), not empty.
const inline = (html, tag) => {
  const found = [];
  const re = new RegExp(`<${tag}(\\s[^>]*)?>([\\s\\S]*?)</${tag}>`, "gi");
  for (const [, attrs = "", body] of html.matchAll(re)) {
    if (/\bsrc\s*=/.test(attrs) || body.length === 0) continue;
    found.push(body);
  }
  return found;
};

export function policy(html) {
  const scripts = [...new Set(inline(html, "script").map(sha))];
  const styles = [...new Set(inline(html, "style").map(sha))];
  return [
    "default-src 'self'",
    `script-src 'self' 'wasm-unsafe-eval' ${scripts.join(" ")}`.trim(),
    `style-src 'self' ${styles.join(" ")}`.trim(),
    "style-src-attr 'unsafe-inline'",
    "img-src 'self' data:",
    "font-src 'self'",
    "connect-src 'self'",
    "base-uri 'self'",
    "form-action 'self'",
    "object-src 'none'",
  ].join("; ");
}

async function* pages(dir) {
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
          const html = await readFile(file, "utf8");
          if (/http-equiv="content-security-policy"/i.test(html)) {
            throw new Error(`${file} already has a CSP <meta>`);
          }
          const head = html.match(/<head[^>]*>/i);
          if (!head) throw new Error(`${file} has no <head>`);
          const meta = `<meta http-equiv="content-security-policy" content="${policy(html)}">`;
          const at = head.index + head[0].length;
          await writeFile(file, html.slice(0, at) + meta + html.slice(at));
          count++;
        }
        logger.info(`a CSP on ${count} pages`);
      },
    },
  };
}
