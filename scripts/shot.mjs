// Screenshot the web app for scripts/screenshots.sh: headless Chrome over its
// DevTools protocol, no packages. Needs Node 22 or later (for its WebSocket)
// and Google Chrome ($CHROME to use another).
//
//   node scripts/shot.mjs URL OUT.png light|dark desktop|phone [SCRIPT]
//
// desktop is 1440x900 at 2x; phone is 390x844 at 3x, like an iPhone. With
// SCRIPT, it runs that in the page once it's loaded (to open a sheet, say),
// then waits SHOT_WAIT ms (1200) before the shot.
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [url, out, scheme = "light", size = "desktop", script] = process.argv.slice(2);
if (!url || !out) {
  console.error("usage: node scripts/shot.mjs URL OUT.png light|dark desktop|phone [SCRIPT]");
  process.exit(2);
}
const chrome = process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const port = 9200 + Math.floor(Math.random() * 600);
const profile = mkdtempSync(join(tmpdir(), "wheelhouse-shot-"));
// A mock keychain and a plain password store: a fresh profile otherwise asks
// macOS for "Chrome Safe Storage", and you get a Keychain prompt per shot.
const proc = spawn(
  chrome,
  ["--headless=new", "--disable-gpu", "--hide-scrollbars", "--use-mock-keychain", "--password-store=basic", "--no-first-run", `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`, "about:blank"],
  { stdio: "ignore" },
);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const phone = size === "phone";

try {
  let targets;
  for (let i = 0; i < 50 && !targets; i++) {
    await sleep(200);
    targets = await fetch(`http://127.0.0.1:${port}/json`).then((r) => r.json()).catch(() => undefined);
  }
  const page = targets?.find((t) => t.type === "page");
  if (!page) throw new Error("Chrome didn't start");
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r, j) => {
    ws.addEventListener("open", r);
    ws.addEventListener("error", j);
  });
  let n = 0;
  const waiting = new Map();
  ws.addEventListener("message", (e) => {
    const m = JSON.parse(e.data);
    if (m.id && waiting.has(m.id)) {
      waiting.get(m.id)(m);
      waiting.delete(m.id);
    }
  });
  const call = (method, params = {}) =>
    new Promise((r, j) => {
      const id = ++n;
      waiting.set(id, (m) => (m.error ? j(new Error(`${method}: ${m.error.message}`)) : r(m.result)));
      ws.send(JSON.stringify({ id, method, params }));
    });
  await call("Emulation.setDeviceMetricsOverride", phone ? { width: 390, height: 844, deviceScaleFactor: 3, mobile: true } : { width: 1440, height: 900, deviceScaleFactor: 2, mobile: false });
  if (phone) await call("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 });
  await call("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
  await call("Page.enable");
  await call("Page.navigate", { url });
  await sleep(1800);
  if (script) {
    const r = await call("Runtime.evaluate", { expression: script, awaitPromise: true });
    if (r.exceptionDetails) throw new Error(`the script failed: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`);
  }
  await sleep(Number(process.env.SHOT_WAIT || 1200));
  const shot = await call("Page.captureScreenshot", { format: "png" });
  writeFileSync(out, Buffer.from(shot.data, "base64"));
  console.log(`wrote ${out}`);
  ws.close();
} finally {
  proc.kill();
  await sleep(200);
  rmSync(profile, { recursive: true, force: true });
}
