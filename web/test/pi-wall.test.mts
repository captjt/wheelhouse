// Pi's wall (src/pi-wall.ts), as Pi runs it in Review's and Build's panes:
// each tool call that runs a command is checked against WH_PI_DENY.
import assert from "node:assert/strict";
import { copyFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { after, test } from "node:test";

const dir = mkdtempSync(join(tmpdir(), "wh-pi-wall-"));
after(() => rmSync(dir, { recursive: true, force: true }));
const source = new URL("../../src/pi-wall.ts", import.meta.url);

/** The wall's tool_call handler, loaded fresh with `WH_PI_DENY` as given. */
async function wall(deny: string | undefined) {
  const file = join(dir, `wall-${Math.random().toString(36).slice(2)}.ts`);
  copyFileSync(source, file);
  const saved = process.env.WH_PI_DENY;
  if (deny === undefined) delete process.env.WH_PI_DENY;
  else process.env.WH_PI_DENY = deny;
  try {
    const mod = await import(pathToFileURL(file).href);
    let handler: ((event: any) => any) | undefined;
    mod.default({ on: (name: string, fn: (event: any) => any) => name === "tool_call" && (handler = fn) });
    assert.ok(handler);
    return (toolName: string, input: any) => handler!({ toolName, toolCallId: "c1", input });
  } finally {
    if (saved === undefined) delete process.env.WH_PI_DENY;
    else process.env.WH_PI_DENY = saved;
  }
}

const BUILD = JSON.stringify(["git push", "gh", "wh send", "wh share", "wh reply"]);

test("a builder's denied commands are refused wherever they are in the line", async () => {
  const call = await wall(BUILD);
  const blocked = [
    "git push",
    "git push origin HEAD",
    "cargo test && git push -f",
    "echo done; gh pr create --fill",
    "ls | gh api /user",
    "FOO=1 GIT_TRACE=1 git push",
    "env -i PATH=/bin git push",
    "/usr/bin/git push",
    'bash -c "git push"',
    "sh -lc 'gh auth status'",
    "nohup git push &",
    "sudo -u me gh repo view",
    "echo $(gh auth token)",
    "x=`gh auth token`",
    "(cd sub && git push)",
    "{ git push; }",
    "eval gh pr list",
    "git push\ncargo test",
    "wh send --to me hi",
    "wh share 5173",
    "wh reply 3 ok",
  ];
  for (const c of blocked) {
    const r = call("bash", { command: c });
    assert.equal(r?.block, true, c);
    assert.match(r.reason, /Wheelhouse's wall: `(git push|gh|wh send|wh share|wh reply)`/, c);
  }
  const fine = [
    "git status",
    "git pushd",
    "git log --grep push",
    "echo git push",
    "grep -r gh src/",
    "ghq list",
    "cargo test -- push",
    "wh brief",
    "wh note D-1 'pushed nothing'",
    "wh attach report.md",
  ];
  for (const c of fine) assert.equal(call("bash", { command: c }), undefined, c);
  // Tools that run no command, and other tools' commands, alike.
  assert.equal(call("read", { path: "/etc/hosts" }), undefined);
  assert.equal(call("powershell", { command: "gh pr list" })?.block, true);
});

test("a checker previews, never dismisses, and posts only when it may", async () => {
  const ask = await wall(JSON.stringify(["wh review dismiss", "wh review post"]));
  assert.equal(ask("bash", { command: "wh review preview .tmp/draft.md" }), undefined);
  assert.equal(ask("bash", { command: "wh review post .tmp/draft.md" })?.block, true);
  assert.equal(ask("bash", { command: "wh review dismiss 12" })?.block, true);
  const auto = await wall(JSON.stringify(["wh review dismiss"]));
  assert.equal(auto("bash", { command: "wh review post .tmp/draft.md" }), undefined);
});

test("a reviewer with nothing denied runs anything, but codemode never", async () => {
  for (const deny of [undefined, "", "[]"]) {
    const call = await wall(deny);
    assert.equal(call("bash", { command: "git push" }), undefined);
    assert.equal(call("codemode", { code: "await tools.bash({ command: 'git push' })" })?.block, true);
  }
});

test("a deny list it can't read stops every command, not none", async () => {
  for (const bad of ["git push", "{", '{"deny":["gh"]}', "[1, 2]"]) {
    const call = await wall(bad);
    const r = call("bash", { command: "ls" });
    assert.equal(r?.block, true, bad);
    assert.match(r.reason, /couldn't read/);
    assert.equal(call("read", { path: "x" }), undefined, "reading still works");
  }
});
