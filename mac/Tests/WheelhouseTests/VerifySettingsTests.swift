import Foundation
import Testing
@testable import Wheelhouse

// Verify and Settings: reading the evidence, the Inbox's Attention, and
// building Settings' patches.

private func json(_ s: String) throws -> JSON { try JSON.parse(Data(s.utf8)) }

/// Serialized with sorted keys, to compare patches.
private func canon(_ v: Any) throws -> String {
    String(decoding: try JSONSerialization.data(withJSONObject: v, options: [.sortedKeys]), as: UTF8.self)
}

@Test func readsEvidence() throws {
    let ev = Evidence(j: try json("""
    {"needs": true, "evidence": [{"how": "witnessed", "what": "`test` passed at 30297c9", "seq": 14, "at": "2026-11-18T06:05:55.461Z"}],
     "checks": [
       {"name": "lint", "command": "./lint", "required": false, "exit": 1, "ok": false, "ms": 263, "head": "30297c98165f", "dirty": false, "output": "sha256:48eb", "seq": 13},
       {"name": "e2e", "command": "pnpm e2e", "required": true, "exit": null, "ok": false, "ms": 1200000, "head": "30297c98165f", "dirty": true, "seq": 15}
     ],
     "repo": "acme/api", "configured": [{"name": "lint", "run": "./lint", "required": false}, {"name": "test", "run": "./run-tests"}],
     "running": false, "verdicts": [{"seq": 20, "n": 2, "verdict": "pass", "findings": [], "by": "codex@s-1", "independent": true},
                                    {"seq": 21, "verdict": "fail", "findings": 3, "independent": null}],
     "hold": null, "closed_on": null}
    """))
    #expect(ev.needs && !ev.isEmpty)
    #expect(ev.lines.first?.how == "witnessed")
    let lint = ev.checks[0], e2e = ev.checks[1]
    #expect(!lint.required && lint.exit == 1 && lint.outcome == "failed, exit 1")
    #expect(lint.output == "48eb" && lint.shortHead == "30297c9")
    // No exit: it timed out.
    #expect(e2e.timedOut && e2e.outcome == "timed out" && e2e.dirty)
    // A configured check without `required` is required.
    #expect(ev.configured[1].required)
    #expect(ev.cantRun == nil)
    #expect(ev.verdicts[0].passed && ev.verdicts[0].findings == 0 && ev.verdicts[0].independent == true)
    #expect(!ev.verdicts[1].passed && ev.verdicts[1].findings == 3 && ev.verdicts[1].independent == nil)
    #expect(duration(ms: 153) == "153 ms" && duration(ms: 4200) == "4.2 s" && duration(ms: 185_000) == "3m 05s")
}

@Test func hidesEmptyEvidence() throws {
    let none = Evidence(j: try json(#"{"needs": false, "evidence": [], "checks": [], "repo": null, "configured": [], "running": false, "verdicts": []}"#))
    #expect(none.isEmpty)
    #expect(none.cantRun?.contains("isn't in one of Build's repos") == true)
    // Done needing evidence shows, even with no checks to run.
    let needs = Evidence(j: try json(#"{"needs": true, "evidence": [], "checks": [], "repo": "acme/web", "configured": []}"#))
    #expect(!needs.isEmpty)
    #expect(needs.cantRun == "acme/web has no checks: add them in Settings › Repos")
    let running = Evidence(j: try json(#"{"configured": [{"name": "test", "run": "t"}], "running": true}"#))
    #expect(running.cantRun == "They're running")
    // Checks its repo has, never run here: nothing to clutter the docket with.
    #expect(running.isEmpty)
}

@Test func attentionLeavesOutWhatsAlreadyAnAsk() throws {
    let state = HubState(try json("""
    {"dockets": [
      {"id": "D-3", "title": "Region column", "status": "in_progress", "hold": "Waiting on the DBA", "live": {"state": "error", "msg": "Migration fails"}},
      {"id": "D-9", "title": "Build me", "status": "in_progress"},
      {"id": "D-7", "title": "Review", "status": "todo"}
    ],
    "messages": [{"id": 4, "docket": "D-9", "from": "wheelhouse", "body": "Approve the plan?", "options": ["Approve"]}],
    "review": {"enabled": true, "prs": [{"key": "api#9", "full_key": "acme/api#9", "docket": "D-7", "state": "reviewing", "attention": {"kind": "ready", "text": "Ready", "round": 1}}]},
    "attention": [
      {"docket": "D-9", "title": "Build me", "kind": "build", "text": "Build: Plan: your call"},
      {"docket": "D-7", "title": "api#9", "kind": "review", "text": "Review of api#9: ready"},
      {"docket": "D-3", "title": "Region column", "kind": "held", "text": "On hold: Waiting on the DBA"},
      {"docket": "D-3", "title": "Region column", "kind": "failing", "text": "Failing: `test`"},
      {"docket": null, "title": "acme/api#12", "kind": "review", "text": "Review of acme/api#12: stalled"}
    ]}
    """))
    let inbox = Store.inbox(state)
    let c = Store.concerns(state, inbox: inbox)
    // The Build's stop is its message; the review is its card. A hold and a
    // failing check aren't asks, even on a docket that also failed.
    #expect(c.map(\.kind) == ["held", "failing", "review"])
    #expect(c.last?.docket == nil)
    #expect(Set(c.map(\.id)).count == c.count)
    #expect(Docket(j: state.j["dockets"][0]).hold == "Waiting on the DBA")
}

@Test func buildsAPostingRule() throws {
    let r = SettingsPatch.rule(approve: true, risk: "medium", maxLines: "400", authors: "alice, bob", labels: " safe ,, docs\n", notTouching: "db/migrations/")
    #expect(r.error == nil)
    #expect(try canon(r.value!) == #"{"auto":{"authors":["alice","bob"],"labels":["safe","docs"],"max_lines":400,"merge_risk":"medium","not_touching":["db\/migrations\/"],"verdict":"approve"}}"#)
    #expect(SettingsPatch.rule(approve: true, risk: "", maxLines: "40.5", authors: "", labels: "", notTouching: "").error == "Lines: a whole number, 1 or more.")
    #expect(SettingsPatch.rule(approve: true, risk: "", maxLines: "0", authors: "", labels: "", notTouching: "").error == "Lines: a whole number, 1 or more.")
    #expect(SettingsPatch.rule(approve: false, risk: "", maxLines: " ", authors: "", labels: ",", notTouching: "").error?.contains("choose Auto") == true)
    let patch = SettingsPatch.make("review", ["repos", "o/a", "post"], r.value)
    #expect(try canon(patch).hasPrefix(#"{"review":{"repos":{"o\/a":{"post":{"auto":{"#))
}

@Test func buildsSettingsPatches() throws {
    #expect(try canon(SettingsPatch.make("build", ["gates", "open"], "auto")) == #"{"build":{"gates":{"open":"auto"}}}"#)
    // Nil is null: what's set here goes, and the Lua's applies again.
    #expect(try canon(SettingsPatch.make("build", ["repos", "acme/api", "checks", "lint"], nil)) == #"{"build":{"repos":{"acme\/api":{"checks":{"lint":null}}}}}"#)
    #expect(try canon(SettingsPatch.make("build", ["repos", "acme/api", "plan"], false)) == #"{"build":{"repos":{"acme\/api":{"plan":false}}}}"#)
    let both = SettingsPatch.merge(SettingsPatch.make("build", ["repos", "o/a", "notes"], "n"), SettingsPatch.make("review", ["repos", "o/a", "notes"], "n"))
    #expect(try canon(both) == #"{"build":{"repos":{"o\/a":{"notes":"n"}}},"review":{"repos":{"o\/a":{"notes":"n"}}}}"#)
    let deeper = SettingsPatch.merge(SettingsPatch.make("build", ["gates", "open"], "auto"), SettingsPatch.make("build", ["gates", "push"], nil))
    #expect(try canon(deeper) == #"{"build":{"gates":{"open":"auto","push":null}}}"#)
    #expect(try canon(SettingsPatch.check(run: " cargo test ", required: false, timeout: 5)) == #"{"required":false,"run":"cargo test","timeout":10}"#)

    let set = try json(#"{"build": {"gates": {"open": "auto"}, "repos": {"o/a": {"plan": false}}}}"#)
    #expect(SettingsPatch.isSet(set, "build", ["gates", "open"]))
    #expect(!SettingsPatch.isSet(set, "build", ["gates", "plan"]))
    // `false` is a value set here.
    #expect(SettingsPatch.isSet(set, "build", ["repos", "o/a", "plan"]))
    #expect(!SettingsPatch.isSet(set, "review", ["repos", "o/a", "notes"]))
}

@Test func namesGuidesAndChecks() {
    #expect(SettingsPatch.guideFile("API review") == "api-review.md")
    #expect(SettingsPatch.guideFile("  Storefront: checkout & cart! ") == "storefront-checkout-cart.md")
    #expect(SettingsPatch.guideFile("¿?") == nil)
    #expect(SettingsPatch.validCheckName("e2e_web-1"))
    #expect(!SettingsPatch.validCheckName("run tests"))
    #expect(!SettingsPatch.validCheckName(""))
}

@Test func findsBriefVariables() {
    let original = "# Plan {{KEY}}: {{TITLE}}\n{{PLAN_FORMAT}}\n{{PLAN_GUIDE}} and {{KEY}} again, not {{lower}} or {{ SPACED }}"
    #expect(SettingsPatch.variables(original) == ["{{KEY}}", "{{TITLE}}", "{{PLAN_FORMAT}}", "{{PLAN_GUIDE}}"])
    let edited = "# Plan {{KEY}}: {{TITLE}}\n{{PLAN_FORMAT}}\nMy own words."
    #expect(SettingsPatch.dropped(from: original, in: edited) == ["{{PLAN_GUIDE}}"])
    #expect(SettingsPatch.dropped(from: original, in: original).isEmpty)
}

@Test func saysWhereAGuideIsUsed() {
    #expect(SettingsPatch.usedBy("review: repos.acme/api.guides.review") == "acme/api's review guide")
    #expect(SettingsPatch.usedBy("review: repos.my.dotted/repo.guides.merge_risk") == "my.dotted/repo's merge risk guide")
    #expect(SettingsPatch.usedBy("build: guides.plan_format") == "Build's plan format")
    #expect(SettingsPatch.usedBy("odd") == "odd")
}
