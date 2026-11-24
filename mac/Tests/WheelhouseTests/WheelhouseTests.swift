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
    #expect(both.connection.ok && both.connection.text == "Following Rex and Herdr")
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
