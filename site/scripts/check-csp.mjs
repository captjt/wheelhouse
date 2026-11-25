// After a build: every page in dist has exactly one CSP <meta>, it says what
// the page's own inline scripts and styles need (recomputed here, as a
// browser would hash them), and <meta charset> is still in the first 1024
// bytes. `just check-site` runs it.
//
//   node scripts/check-csp.mjs [dist]
import { readFile } from "node:fs/promises";
import { pages, policy, read } from "../src/integrations/csp.mjs";

const dir = process.argv[2] ?? "dist";
let n = 0;
const bad = [];
for await (const file of pages(dir)) {
  n++;
  const html = await readFile(file, "utf8");
  const { csp, csps } = read(html);
  if (csp === undefined) bad.push(`${file}: no CSP <meta>`);
  else if (csps !== 1) bad.push(`${file}: ${csps} CSP <meta>s`);
  else if (csp !== policy(html)) bad.push(`${file}: its CSP doesn't match its inline scripts and styles`);
  const at = Buffer.from(html).indexOf("<meta charset");
  if (at < 0 || at > 1024) bad.push(`${file}: <meta charset> isn't in the first 1024 bytes (at ${at})`);
}
if (n === 0) bad.push(`${dir}: no pages`);
if (bad.length) {
  console.error(bad.join("\n"));
  process.exit(1);
}
console.log(`csp: ${n} pages checked`);
