// The CSP integration against what a regex reading of HTML gets wrong.
//   npm test
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import { policy, read, withPolicy } from "../src/integrations/csp.mjs";

const sha = (t) => `'sha256-${createHash("sha256").update(t, "utf8").digest("base64")}'`;
const page = (head, body = "") => `<!doctype html><html><head><meta charset="utf-8">${head}</head><body>${body}</body></html>`;

test("an inline script with a data-src attribute is hashed, an external one isn't", () => {
  const p = policy(page(`<script data-src="x">a()</script><script src="/b.js"></script>`));
  assert.ok(p.includes(sha("a()")));
  assert.equal(read(page(`<script src="/b.js">ignored()</script>`)).scripts.length, 0);
});

test("a > in an attribute and a spaced </script > close where a browser closes them", () => {
  const { scripts } = read(page(`<script data-x="a>b">one()</script ><script>two()</script>`));
  assert.deepEqual(scripts, ["one()", "two()"]);
});

test("CRLF is hashed as the LF a browser sees", () => {
  assert.ok(policy(page("<style>a{}\r\nb{}</style>")).includes(sha("a{}\nb{}")));
});

test("an SVG <style> is hashed after its entities are decoded", () => {
  const { styles } = read(page("", `<svg><style>a::after{content:"&amp;"}</style></svg>`));
  assert.deepEqual(styles, [`a::after{content:"&"}`]);
});

test("a page quoting the CSP's name in its text is fine; a second CSP <meta> isn't", () => {
  const prose = page("", `<p>http-equiv="content-security-policy"</p>`);
  assert.doesNotThrow(() => withPolicy(prose, "prose.html"));
  assert.throws(() => withPolicy(page(`<meta http-equiv="Content-Security-Policy" content="x">`), "twice.html"), /already has/);
});

test("the policy goes after <meta charset>, in <head> and not in a <header>", () => {
  const out = withPolicy(`<!doctype html><html><head><meta charset="utf-8"><title>t</title></head><body><header>h</header></body></html>`, "p.html");
  assert.ok(out.indexOf("content-security-policy") > out.indexOf("<meta charset"));
  assert.ok(out.indexOf("content-security-policy") < out.indexOf("<title>"));
  assert.throws(() => withPolicy(`<header>no head</header>`, "nohead.html"), /no <head>/);
});
