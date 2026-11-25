// Settings' patches and words, as the hub reads them (src/settings.rs): a
// patch is shaped like the Lua configs and laid over them, objects merge key
// by key, and `null` takes a value set here away.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  AGENTS,
  agentLabel,
  briefVars,
  checkProblem,
  checkValue,
  docGroups,
  guideId,
  mergePatches,
  patchAt,
  planValue,
  repoSlugs,
  rolePatch,
  setHere,
  timeoutWord,
  usedWord,
  valueAt,
  varChanges,
  whenDraft,
  whenValue,
  type SettingsDoc,
} from "../src/config.ts";

test("a patch sets one value at its path, and a slug with a slash stays one key", () => {
  assert.deepEqual(patchAt(["build", "gates", "open"], "auto"), { build: { gates: { open: "auto" } } });
  assert.deepEqual(patchAt(["build", "repos", "acme/api", "checks", "lint"], null), { build: { repos: { "acme/api": { checks: { lint: null } } } } });
  assert.throws(() => patchAt([], 1));
});

test("patches merge where they meet, later ones winning, without touching the inputs", () => {
  const a = patchAt(["build", "repos", "acme/api", "notes"], "x");
  const b = patchAt(["review", "repos", "acme/api", "notes"], "x");
  const c = patchAt(["build", "repos", "acme/api", "base"], "develop");
  assert.deepEqual(mergePatches(a, b, c), {
    build: { repos: { "acme/api": { notes: "x", base: "develop" } } },
    review: { repos: { "acme/api": { notes: "x" } } },
  });
  assert.deepEqual(a, { build: { repos: { "acme/api": { notes: "x" } } } }, "the first patch isn't changed");
  // A list replaces, as the hub's merge does.
  assert.deepEqual(mergePatches({ p: { r: ["claude"] } }, { p: { r: ["codex"] } }), { p: { r: ["codex"] } });
});

test("set here means it's in settings.json, even when it's false or empty", () => {
  const set = { build: { gates: { open: "auto" }, pr: { draft: false }, prereview: { reviewers: [] } } };
  assert.ok(setHere(set, ["build", "gates", "open"]));
  assert.ok(setHere(set, ["build", "pr", "draft"]));
  assert.ok(setHere(set, ["build", "prereview", "reviewers"]));
  assert.ok(!setHere(set, ["build", "gates", "plan"]));
  assert.ok(!setHere(set, ["review", "checker"]));
  assert.ok(!setHere(null, ["build"]));
  assert.equal(valueAt({ a: "x" }, ["a", "b"]), undefined, "a string has no keys");
});

test("a role is set as a table, so build.lua's effort for it stays", () => {
  assert.deepEqual(rolePatch("build", "builder", "codex"), { build: { builder: { agent: "codex" } } });
  assert.deepEqual(rolePatch("review", "checker", "claude"), { review: { checker: { agent: "claude" } } });
});

test("a check is just its command when it's required with the default timeout, else the whole table", () => {
  assert.equal(checkValue({ run: " cargo test ", required: true, timeout: 1200 }), "cargo test");
  assert.deepEqual(checkValue({ run: "./lint", required: false, timeout: 1200 }), { run: "./lint", required: false, timeout: 1200 });
  assert.deepEqual(checkValue({ run: "./slow", required: true, timeout: 600 }), { run: "./slow", required: true, timeout: 600 });
});

test("a check's problems come back as sentences, before anything is sent", () => {
  const ok = { name: "types", run: "./typecheck", timeout: 600 };
  assert.equal(checkProblem(ok, ["lint", "test"]), null);
  assert.match(checkProblem({ ...ok, name: "" })!, /name/);
  assert.match(checkProblem({ ...ok, name: "my check" })!, /letters, digits/);
  assert.match(checkProblem({ ...ok, name: "lint" }, ["lint"])!, /already a check named lint/);
  assert.match(checkProblem({ ...ok, run: "  " })!, /command/);
  assert.match(checkProblem({ ...ok, timeout: 5 })!, /10 to 7200/);
  assert.match(checkProblem({ ...ok, timeout: 1.5 })!, /10 to 7200/);
});

test("the plan's mode as build.lua writes it, and a timeout in words", () => {
  assert.equal(planValue("off"), false);
  assert.equal(planValue("summary"), "summary");
  assert.equal(timeoutWord(1200), "20m");
  assert.equal(timeoutWord(90), "1m 30s");
  assert.equal(timeoutWord(45), "45s");
});

test("a new guide's name becomes its id, or a sentence on what's wrong", () => {
  assert.deepEqual(guideId("api-review"), { id: "guide/api-review.md" });
  assert.deepEqual(guideId(" api-review.md "), { id: "guide/api-review.md" });
  assert.ok("error" in guideId("API Review"));
  assert.ok("error" in guideId("-lead"));
  assert.ok("error" in guideId(""));
  assert.ok("error" in guideId("a".repeat(62)));
});

test("a brief's variables: the ones an edit lost, and the ones nothing fills in", () => {
  const original = "# Plan {{KEY}}: {{TITLE}}\n\n{{PLAN_FORMAT}}\n\nAt most {{MAX_DECISIONS}}, {{KEY}} again.";
  assert.deepEqual(briefVars(original), ["{{KEY}}", "{{TITLE}}", "{{PLAN_FORMAT}}", "{{MAX_DECISIONS}}"]);
  assert.deepEqual(briefVars("{{lower}} and {{ SPACED }} aren't variables"), []);
  const edited = original.replace("{{PLAN_FORMAT}}", "(see the format)") + " {{REPO_NAME}}";
  assert.deepEqual(varChanges(original, edited), { missing: ["{{PLAN_FORMAT}}"], unknown: ["{{REPO_NAME}}"] });
  assert.deepEqual(varChanges(original, original), { missing: [], unknown: [] });
});

const doc = (d: Partial<SettingsDoc>): SettingsDoc => ({ id: "", kind: "guide", protocol: "", title: "", about: "", builtin: true, overridden: false, path: "", used_by: [], ...d });

test("guides and briefs group as yours, built-in guides, Build's briefs, and Review's briefs, parts last", () => {
  const groups = docGroups([
    doc({ id: "guide/pr-style.md" }),
    doc({ id: "guide/api-review.md", builtin: false }),
    doc({ id: "review/brief/parts/post-ask.md", kind: "brief", protocol: "review" }),
    doc({ id: "review/brief/round-1.md", kind: "brief", protocol: "review" }),
    doc({ id: "build/brief/plan.md", kind: "brief", protocol: "build" }),
  ]);
  assert.deepEqual(
    groups.map((g) => [g.key, g.docs.map((d) => d.id)]),
    [
      ["own", ["guide/api-review.md"]],
      ["guides", ["guide/pr-style.md"]],
      ["build", ["build/brief/plan.md"]],
      ["review", ["review/brief/round-1.md", "review/brief/parts/post-ask.md"]],
    ],
  );
});

test("where a guide is used, in words", () => {
  assert.equal(usedWord("review: repos.acme/api.guides.review"), "acme/api's review guide");
  assert.equal(usedWord("review: guides.merge_risk"), "Review's merge-risk guide");
  assert.equal(usedWord("build: guides.pr_style"), "Build's PR style guide");
  assert.equal(usedWord("build: reviewers.1.guides"), "build.lua: reviewers.1.guides");
});

test("every repo either protocol knows, once each", () => {
  const s = {
    build: { lua: "", lua_exists: true, error: null, config: { repos: { "acme/api": {}, "acme/web": {} } } },
    review: { lua: "", lua_exists: true, error: null, config: { repos: { "acme/api": {}, "acme/docs": {} } } },
  } as unknown as Parameters<typeof repoSlugs>[0];
  assert.deepEqual(repoSlugs(s), ["acme/api", "acme/docs", "acme/web"]);
});

test("a posting rule's form reads from the rule and saves as one, or says what's wrong", () => {
  const fresh = whenDraft(null);
  assert.deepEqual(whenValue(fresh), { value: { auto: { verdict: "approve", merge_risk: "low" } } });
  const rule = { verdict: "approve" as const, merge_risk: "medium" as const, max_lines: 400, authors: ["alice", "bob"], not_touching: ["db/migrations/"] };
  const d = whenDraft(rule);
  assert.deepEqual(d, { approve: true, risk: "medium", maxLines: "400", authors: "alice, bob", labels: "", notTouching: "db/migrations/" });
  assert.deepEqual(whenValue(d), { value: { auto: rule } });
  assert.deepEqual(whenValue({ ...d, labels: " safe ,, docs\n" }), { value: { auto: { ...rule, labels: ["safe", "docs"] } } });
  assert.deepEqual(whenValue({ ...d, maxLines: "40.5" }), { error: "Lines: a whole number, 1 or more." });
  assert.deepEqual(whenValue({ ...d, maxLines: "0" }), { error: "Lines: a whole number, 1 or more." });
  assert.match((whenValue({ approve: false, risk: "", maxLines: " ", authors: "", labels: ",", notTouching: "" }) as { error: string }).error, /choose Auto/);
});

test("Review and Build run each agent the hub takes, under its own name", () => {
  // src/review/config.rs AGENTS
  assert.deepEqual(AGENTS.map((a) => a.key).sort(), ["agy", "claude", "codex", "pi"]);
  assert.equal(agentLabel("pi"), "Pi");
});
