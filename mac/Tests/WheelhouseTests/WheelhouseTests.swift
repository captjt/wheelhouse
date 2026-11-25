import Foundation
import Testing
@testable import Wheelhouse

// The app's logic that doesn't need a window: Markdown's task boxes and
// wrapping, the intent's diff, the Inbox's order, and reading the hub's JSON.

/// Every box the renderer shows, as (line, checked), in order.
private func boxes(_ text: String) -> [(Int, Bool)] {
    var out: [(Int, Bool)] = []
    func walk(_ bs: [MDBlock]) {
        for b in bs {
            switch b {
            case let .list(_, _, items):
                for i in items {
                    if let c = i.checked, let l = i.line { out.append((l, c)) }
                    walk(i.blocks)
                }
            case let .quote(inner), let .alert(_, inner): walk(inner)
            default: break
            }
        }
    }
    walk(MD.blocks(text, size: 13))
    return out
}

@Test func ticksTheBoxYouSee() {
    let text = """
    ## Acceptance
    - [ ] first
    - [x] second
    ```
    - [ ] not a box: it's code
    ```
    - [ ]
    <!-- - [ ] a comment -->
    - \\[ ] escaped, not a box
    1. [ ] third

    > - [ ] quoted
    >   - [ ] nested
    """
    let shown = boxes(text)
    // Only the boxes that render, each by its line.
    #expect(shown.map(\.0) == [2, 3, 10, 12, 13])
    // Ticking each changes that line, and only it.
    for (line, checked) in shown {
        let next = MD.toggleTask(text, line: line, checked: !checked)
        let before = text.components(separatedBy: "\n"), after = next?.components(separatedBy: "\n") ?? []
        #expect(after.count == before.count)
        #expect(zip(before, after).enumerated().filter { $0.element.0 != $0.element.1 }.map { $0.offset + 1 } == [line])
    }
    #expect(MD.toggleTask(text, line: 2, checked: true)?.contains("- [x] first") == true)
    #expect(MD.toggleTask(text, line: 13, checked: true)?.contains(">   - [x] nested") == true)
    // An escaped bracket is text, not a box: nothing to tick.
    #expect(MD.toggleTask(text, line: 9, checked: true) == nil)
}

@Test func tellsHardWrappedProse() {
    let wrapped = """
    Deliveries that fail with a 5xx or time out are retried three times, waiting
    one second, then two, then four, each with up to a quarter second of jitter
    so that one outage doesn't make every retry land at the very same moment.
    """
    #expect(MD.hardWrapped(wrapped))
    #expect(!MD.hardWrapped("Line one.\nLine two.\nLine three."))
    #expect(!MD.hardWrapped("One line only"))
}

@Test func readsAlerts() {
    let blocks = MD.blocks("> [!WARNING]\n> Don't run this on main.", size: 13)
    guard case let .alert(kind, inner)? = blocks.first else {
        Issue.record("not an alert: \(blocks)")
        return
    }
    #expect(kind == "WARNING")
    #expect(inner.count == 1)
}

@Test func diffsLines() {
    let d = lineDiff("a\nb\nc", "a\nc\nd")
    #expect(d.map(\.op) == [.same, .del, .same, .add])
    #expect(d.map(\.text) == ["a", "b", "c", "d"])
}

@Test func readsJSONLeniently() throws {
    let j = try JSON.parse(Data(#"{"id":"D-1","turns":"3","live":null,"ok":true,"n":2.0}"#.utf8))
    #expect(j["id"].str == "D-1")
    #expect(j["turns"].int == 3)
    #expect(j["live"].isNull)
    #expect(j["missing"]["deeper"].str == nil)
    #expect(j["ok"].bool)
    #expect(j["n"].str == "2")
    let d = Docket(j: j)
    #expect(d.live == nil)
    #expect(d.status == .todo)
}

@Test func ordersTheInbox() throws {
    let state = HubState(try JSON.parse(Data("""
    {"dockets": [
      {"id": "D-1", "title": "done turn", "status": "in_progress", "live": {"state": "done", "since": "2026-11-17T10:00:00Z"}},
      {"id": "D-2", "title": "blocked", "status": "in_progress", "live": {"state": "blocked", "msg": "Bash: ls", "since": "2026-11-17T09:00:00Z"}},
      {"id": "D-3", "title": "failed but closed", "status": "done", "live": {"state": "error"}},
      {"id": "D-4", "title": "failed", "status": "todo", "live": {"state": "error", "since": "2026-11-17T08:00:00Z"}}
    ], "messages": [{"id": 7, "docket": "D-1", "from": "t/a", "body": "Ship it?", "options": ["Ship"], "created_at": "2026-11-17T11:00:00Z"}]}
    """.utf8)))
    let items = Store.inbox(state)
    // Waiting on you first, then messages, then failures, then finished turns;
    // a failure on a docket you closed isn't your move.
    #expect(items.map(\.id) == ["D-2", "m7", "D-4", "D-1"])
    #expect(items.first?.line == "Bash: ls")
}

@Test func agesInFewCharacters() {
    let now = Date(timeIntervalSince1970: 1_800_000_000)
    let iso = { (s: Double) in ISO8601DateFormatter().string(from: now.addingTimeInterval(-s)) }
    #expect(ago(iso(20), now: now) == "now")
    #expect(ago(iso(5 * 60), now: now) == "5m")
    #expect(ago(iso(3 * 3600), now: now) == "3h")
    #expect(ago(iso(2 * 86400), now: now) == "2d")
}

/// Unbundled (tests, `swift run`, `--snapshot`) it's the daily Wheelhouse:
/// only Wheelhouse Dev's Info.plist (`build.sh --dev`) makes it Dev.
@Test func isTheDailyFlavorUnbundled() {
    #expect(!Flavor.isDev)
    #expect(Flavor.name == "Wheelhouse")
    #expect(Flavor.scheme == "wheelhouse")
    #expect(Flavor.defaultHub == "http://127.0.0.1:7500")
    #expect(Flavor.command == nil)
    #expect(Flavor.chord == "⌥⌘K")
    #expect(HubClient.defaultAddress == "http://127.0.0.1:7500")
}

@Test func namesEachPanesRuntime() throws {
    let herdr = Docket(j: try JSON.parse(Data(#"{"id":"D-2","agent":{"block":"herdr:t2h","rex_session":"wheelhouse","pane":"D-2"}}"#.utf8)))
    #expect(herdr.runtime == "Herdr")
    let rex = Docket(j: try JSON.parse(Data(#"{"id":"D-3","agent":{"block":"block:01a2"},"live":{"block":"block:01a2","runtime":"rex"}}"#.utf8)))
    #expect(rex.runtime == "Rex")
    #expect(Docket(j: .null).runtime == "Rex")
}

@Test func followsEveryRuntimeCalmlyAboutOneNotInstalled() throws {
    let s = { (json: String) throws in HubState(try JSON.parse(Data(json.utf8))) }
    let both = try s(#"{"rex":{"connected":true},"runtimes":[{"name":"rex","title":"Rex","connected":true,"installed":true,"spawn":true},{"name":"herdr","title":"Herdr","connected":true,"installed":true}]}"#)
    #expect(both.connection.ok && both.connection.text == "Rex · Herdr")
    #expect(both.spawnRuntime == "Rex")
    let noHerdr = try s(#"{"runtimes":[{"name":"rex","title":"Rex","connected":true,"installed":true,"spawn":true},{"name":"herdr","title":"Herdr","connected":false,"installed":false}]}"#)
    #expect(noHerdr.connection.ok && noHerdr.connection.text == "Following Rex")
    let down = try s(#"{"runtimes":[{"name":"rex","title":"Rex","connected":true,"installed":true},{"name":"herdr","title":"Herdr","connected":false,"installed":true,"error":"no core","spawn":true}]}"#)
    #expect(!down.connection.ok && down.connection.text == "Herdr: no core")
    #expect(down.spawnRuntime == "Herdr")
    // An older hub says only Rex's.
    let old = try s(#"{"rex":{"connected":true}}"#)
    #expect(old.connection.text == "Following Rex")
}

// ---- several agents on a docket ----

private func docket(_ json: String) throws -> Docket { Docket(j: try JSON.parse(Data(json.utf8))) }

@Test func readsEachAgentOnADocket() throws {
    let d = try docket("""
    {"id": "D-5", "status": "in_progress", "protocol": "agent", "lead": "b2",
     "agent": {"app": "claude-code", "block": "b1", "pane": "D-5"},
     "live": {"block": "b2", "state": "blocked", "answerable": true, "msg": "Bash: rm", "record": "r2"},
     "agents": [
       {"app": "claude-code", "host": "mac", "block": "b1", "rex_session": "wheelhouse", "pane": "D-5", "joined_at": "2026-11-20T10:00:00Z", "active_at": "2026-11-20T11:00:00Z", "on": true, "runtime": "rex", "live": {"block": "b1", "state": "working"}, "addr": "wheelhouse/D-5"},
       {"app": "codex", "host": "mac", "block": "herdr:t2", "rex_session": "wheelhouse", "pane": "D-5-2", "role": "checker", "round": 2, "joined_at": "2026-11-20T10:30:00Z", "on": true, "runtime": "herdr", "live": null, "addr": null},
       {"app": "codex", "block": "b2", "pane": "D-5-3", "on": true, "runtime": "rex", "live": {"block": "b2", "state": "blocked", "answerable": true, "msg": "Bash: rm", "record": "r2"}},
       {"app": "agy", "block": "b0", "pane": "D-5-0", "on": false, "left_at": "2026-11-20T09:00:00Z", "left": "pane closed", "runtime": "rex", "live": null}
     ]}
    """)
    #expect(d.hasAgents)
    #expect(d.agents.count == 4)
    // The lead first, then the others in the hub's order.
    #expect(d.crew.map(\.block) == ["b2", "b1", "herdr:t2", "b0"])
    #expect(d.onAgents.map(\.block) == ["b2", "b1", "herdr:t2"])
    #expect(d.leftAgents.map(\.block) == ["b0"])
    #expect(d.leftAgents.first?.left == "pane closed")
    #expect(d.several)
    // Named by role, else pane, else app.
    let checker = try #require(d.member(block: "herdr:t2"))
    #expect(checker.label == "checker")
    #expect(checker.round == 2)
    #expect(checker.runtime == "Herdr")
    #expect(checker.addr == nil)
    #expect(d.member(block: "b1")?.label == "D-5")
    #expect(d.member(block: "b1")?.addr == "wheelhouse/D-5")
    // Each live agent, the lead first; and who's asking.
    #expect(d.lives.map { $0.agent?.block } == ["b2", "b1"])
    #expect(d.asking.map { $0.agent?.block } == ["b2"])
    // Pane actions name the agent's own block.
    #expect(d.blockFor(checker) == "herdr:t2")
    // A hub that lists agents adds one beside those on it.
    #expect(d.canStartAgent)
}

@Test func tellsWhichAgentWroteAnEntry() throws {
    let d = try docket("""
    {"id": "D-5", "agent": {"app": "claude-code", "block": "b1"},
     "agents": [{"app": "claude-code", "block": "b1", "pane": "D-5", "on": true}, {"app": "codex", "block": "b2", "pane": "D-5-2", "role": "checker", "on": true}]}
    """)
    let entry = { (json: String) throws in Entry(j: try JSON.parse(Data(json.utf8))) }
    // An agent's own entry says its block; the hub's says it in data.
    let turn = try entry(#"{"seq": 3, "kind": "turn", "actor": {"type": "hub", "id": "wheelhouse"}, "data": {"block": "b2"}}"#)
    #expect(d.member(for: turn)?.label == "checker")
    #expect(d.blockOf(turn) == "b2")
    #expect(d.agentName(d.member(for: turn)) == "checker")
    let note = try entry(#"{"seq": 4, "kind": "note", "actor": {"type": "agent", "id": "codex@b2", "block": "b2"}}"#)
    #expect(d.member(for: note)?.block == "b2")
    // An older entry has neither: on a docket that's had several agents,
    // it's no one's in particular, never the one that leads now.
    let old = try entry(#"{"seq": 1, "kind": "turn", "actor": {"type": "hub", "id": "wheelhouse"}, "data": {}}"#)
    #expect(d.member(for: old) == nil)
    #expect(d.blockOf(old) == nil)
    // Your note is no agent's.
    let mine = try entry(#"{"seq": 2, "kind": "note", "actor": {"type": "person", "id": "jordan"}, "data": {}}"#)
    #expect(d.blockOf(mine) == nil)
}

@Test func readsAnOlderHubsDocketWithNoAgents() throws {
    let d = try docket("""
    {"id": "D-2", "status": "in_progress", "protocol": "agent", "session": "s-1",
     "agent": {"app": "codex", "block": "herdr:t2h", "rex_session": "wheelhouse", "pane": "D-2", "cwd": "/w"},
     "live": {"block": "herdr:t2h", "state": "blocked", "answerable": true, "msg": "Bash: ls"}}
    """)
    #expect(!d.hasAgents)
    #expect(d.agents.isEmpty)
    #expect(d.lead == nil)
    // Its one agent stands in for the list, with the docket's live.
    #expect(d.crew.count == 1)
    #expect(d.crew.first?.app == "codex")
    #expect(d.crew.first?.on == true)
    #expect(d.crew.first?.session == "s-1")
    #expect(d.crew.first?.live?.state == .blocked)
    #expect(d.crew.first?.runtime == "Herdr")
    #expect(!d.several)
    // One live, about no agent in particular, and no block to send.
    #expect(d.lives.count == 1)
    #expect(d.lives.first?.agent == nil)
    #expect(d.blockFor(d.crew.first) == nil)
    // An older hub refuses a second agent: no start while one is live.
    #expect(!d.canStartAgent)
    #expect(d.talks)
    // One that ended is in Earlier.
    let ended = try docket(#"{"id": "D-3", "status": "todo", "agent": {"app": "claude-code", "block": "b"}, "released": "pane closed"}"#)
    #expect(ended.onAgents.isEmpty)
    #expect(ended.leftAgents.first?.left == "pane closed")
    #expect(ended.canStartAgent)
    // And a docket with no agent at all has none.
    #expect(try docket(#"{"id": "D-4"}"#).crew.isEmpty)
}

@Test func asksOncePerAgentThatNeedsYou() throws {
    let state = HubState(try JSON.parse(Data("""
    {"dockets": [
      {"id": "D-7", "title": "two ask", "status": "in_progress", "lead": "b1",
       "live": {"block": "b1", "state": "blocked", "msg": "Bash: ls", "since": "2026-11-17T10:00:00Z"},
       "agents": [
         {"app": "claude-code", "block": "b1", "pane": "D-7", "on": true, "live": {"block": "b1", "state": "blocked", "msg": "Bash: ls", "since": "2026-11-17T10:00:00Z"}},
         {"app": "codex", "block": "b2", "pane": "D-7-2", "role": "checker", "on": true, "live": {"block": "b2", "state": "blocked", "msg": "Edit: a.rs", "since": "2026-11-17T09:00:00Z"}},
         {"app": "codex", "block": "b3", "pane": "D-7-3", "on": true, "live": {"block": "b3", "state": "working"}}
       ]}
    ], "asks": [{"docket": "D-7", "block": "b2", "what": "blocked"}], "agents": [{"block": "b2", "docket": "D-7", "role": "checker", "state": "blocked"}]}
    """.utf8)))
    let items = Store.inbox(state)
    // One for each asking agent; the lead's is the docket's own.
    #expect(items.map(\.id) == ["D-7", "D-7@b2"])
    #expect(items.map(\.block) == ["b1", "b2"])
    #expect(items.map(\.who) == ["D-7", "checker"])
    #expect(items[1].line == "Edit: a.rs")
    #expect(state.asks.first?.block == "b2")
    #expect(state.agents.first?.role == "checker")
}

@Test func namesTheBlockInAPaneAction() {
    #expect(Store.blockQuery(nil) == "")
    #expect(Store.blockQuery("") == "")
    #expect(Store.blockQuery("b1") == "?block=b1")
    #expect(Store.blockQuery("herdr:t2 x") == "?block=herdr%3At2%20x")
}

@Test func namesEachAgentItStartsAndFindsItAgainByName() {
    #expect(startApps.map(appName) == ["Claude Code", "Codex", "Antigravity", "Pi"])
    for a in startApps { #expect(appOfName(appName(a)) == a) }
    #expect(appOfName(nil) == "claude-code")
    #expect(startApp(for: "pi") == "pi")
    #expect(startApp(for: "antigravity") == "agy")
    #expect(startApp(for: "rex") == "claude-code")
    #expect(reviewAgents.map(appName) == ["Claude Code", "Codex", "Antigravity", "Pi"])
}

// ---- the composer (Compose.swift, as web/src/compose.ts) ----

@Test func composerReadsWhatYouType() {
    #expect(Compose.kind("Looks right.") == .note)
    #expect(Compose.kind("@builder run it") == .instruct)
    #expect(Compose.kind("  /ask Which? | a | b") == .ask)
    #expect(Compose.kind("/DECIDE Retries: 5xx") == .decide)
    #expect(Compose.kind("/task write tests") == .task)
    #expect(Compose.kind("/note @all isn't one") == .note)
    // A path, a lone @, and an @ that isn't a name are notes.
    #expect(Compose.kind("/tmp/x is full") == .note)
    #expect(Compose.kind("@ noon") == .note)
    #expect(Compose.kind("@sam's idea") == .note)
    #expect(Compose.kind("/decied 5xx") == .note, "the hub says it's a slip")
    #expect(Compose.button(.task) == "Add an Agent…")
    #expect(Compose.hint(.decide, "D-12").contains("agents on D-12 are told"))
}

@Test func composerCompletesCommandsAndAgents() {
    #expect(Compose.suggestions("/", agents: [], docket: "D-12").map(\.label) == ["/ask", "/decide", "/task", "/note"])
    #expect(Compose.suggestions("/de", agents: [], docket: "D-12").map(\.insert) == ["/decide "])
    #expect(Compose.suggestions("/decide Retries", agents: [], docket: "D-12").isEmpty)
    let agents: [(pane: String, role: String?, app: String)] = [("builder", "Build builder", "claude-code"), ("", "tests", "codex")]
    #expect(Compose.suggestions("@", agents: agents, docket: "D-12").map(\.label) == ["@builder", "@tests", "@all"])
    #expect(Compose.suggestions("@", agents: agents, docket: "D-12")[1].sub == "Codex · tests")
    #expect(Compose.suggestions("@b", agents: agents, docket: "D-12").map(\.insert) == ["@builder "])
    #expect(Compose.suggestions("@", agents: [("D-12", nil, "claude-code")], docket: "D-12").map(\.label) == ["@d-12"])
    #expect(Compose.atName(pane: "", role: nil, app: "claude-code") == "claude")
}

@Test func composerSaysWhatTheHubDid() {
    #expect(Compose.toast(JSON(any: ["did": "note", "seq": 3])) == nil)
    #expect(Compose.toast(JSON(any: ["did": "task", "prompt": "x"])) == nil)
    let q = Compose.toast(JSON(any: ["did": "instructed", "sent": [["to": "builder (Claude Code)", "ok": true, "queued": true]]]))
    #expect(q?.text == "Queued for builder (Claude Code)" && q?.error == false)
    let some = Compose.toast(JSON(any: ["did": "instructed", "sent": [["to": "a", "ok": true], ["to": "b", "ok": false, "error": "the pane closed"]]]))
    #expect(some?.text == "Sent to a; not b (the pane closed)" && some?.error == true)
    #expect(Compose.toast(JSON(any: ["did": "asked", "message": 7, "options": ["Postgres", "SQLite"]]))?.text == "Asked the agents (#7): Postgres / SQLite")
    #expect(Compose.toast(JSON(any: ["did": "answered", "key": "#3", "message": 9, "choice": "Postgres"]))?.text == "Answered #3: Postgres")
    #expect(Compose.toast(JSON(any: ["did": "decided", "key": "=ab", "seq": 4, "told": ["wheelhouse/D-12"]]))?.text == "Decided, and told wheelhouse/D-12")
}
