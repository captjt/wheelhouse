import Foundation

// The hub's answers, as the app reads them (see src/server.rs and
// web/src/api.ts). Each type wraps its JSON and reads fields on demand, so a
// field the hub adds or drops never breaks the app.

/// A docket's status.
enum Status: String, CaseIterable, Identifiable {
    case todo, in_progress, done, dropped
    var id: String { rawValue }
    var label: String {
        switch self {
        case .todo: "To do"
        case .in_progress: "In progress"
        case .done: "Done"
        case .dropped: "Dropped"
        }
    }

    var isOpen: Bool { self == .todo || self == .in_progress }
}

/// What an agent is doing, from its program status in its runtime.
enum LiveState: String {
    case working, blocked, error, done, idle, starting

    var word: String {
        switch self {
        case .working: "Working"
        case .blocked: "Needs you"
        case .error: "Failed"
        case .done: "Finished"
        case .idle: "Idle"
        case .starting: "Starting"
        }
    }
}

/// The runtime a pane id is in, for people: Herdr's ids start `herdr:`,
/// Rex's are its own (docs/runtimes.md).
func runtimeTitle(block: String?) -> String {
    block?.hasPrefix("herdr:") == true ? "Herdr" : "Rex"
}

/// A runtime as the hub follows it: `runtimes` in /api/state.
struct RuntimeLink: Hashable {
    let j: JSON
    var name: String { j["name"].str ?? "" }
    var title: String { j["title"].str ?? name }
    var connected: Bool { j["connected"].bool }
    var installed: Bool { j["installed"].bool }
    var error: String? { j["error"].text }
    var agents: Int { j["agents"].int ?? 0 }
    var spawn: Bool { j["spawn"].bool }
}

func appName(_ app: String?) -> String {
    switch app {
    case "claude-code", "claude": "Claude Code"
    case "codex": "Codex"
    case "agy", "antigravity": "Antigravity"
    case let a? where !a.isEmpty: a
    default: "Agent"
    }
}

struct Live: Hashable {
    let j: JSON
    var block: String { j["block"].str ?? "" }
    var record: String? { j["record"].str }
    var answerable: Bool { j["answerable"].bool }
    var app: String { j["app"].str ?? "" }
    var state: LiveState { LiveState(rawValue: j["state"].str ?? "") ?? .idle }
    var kind: String? { j["kind"].text }
    var msg: String? { j["msg"].text }
    var since: String? { j["since"].str }
    /// Where it runs, for people: "Rex", "Herdr".
    var runtime: String { j["runtime"].str == "herdr" ? "Herdr" : runtimeTitle(block: block) }
    var rexSession: String { j["rex_session"].str ?? "" }
    var pane: String { j["pane"].str ?? "" }
    var cwd: String? { j["cwd"].text }
    var docket: String? { j["docket"].text }
}

struct AgentRef: Hashable {
    let j: JSON
    var app: String { j["app"].str ?? "" }
    var block: String { j["block"].str ?? "" }
    var runtime: String { runtimeTitle(block: block) }
    var rexSession: String { j["rex_session"].str ?? "" }
    var pane: String { j["pane"].str ?? "" }
    var cwd: String? { j["cwd"].text }
}

struct LinearRef: Hashable, Identifiable {
    let j: JSON
    var id: String { key }
    var key: String { j["key"].str ?? j["identifier"].str ?? "" }
    var url: URL? { j["url"].str.flatMap(URL.init(string:)) }
    var title: String? { j["title"].text }
    var state: String? { j["state"].text }
    var stateType: String? { j["state_type"].text }
    var priority: String? { j["priority"].text }
    var assignee: String? { j["assignee"].text }
    var description: String? { j["description"].text }
    var team: String? { j["team"].text }
}

struct Intent: Hashable {
    let j: JSON
    var body: String { j["body"].str ?? "" }
    var source: String { j["source"].str ?? "" }
    var at: String { j["at"].str ?? "" }
}

struct ReviewRef: Hashable {
    let j: JSON
    var key: String { j["key"].str ?? "" }
    var url: URL? { j["url"].str.flatMap(URL.init(string:)) }
    var title: String? { j["title"].text }
    var round: Int? { j["round"].int }
    var verdict: String? { j["verdict"].text }
    var phase: String? { j["phase"].text }
    var mergeRisk: String? { j["merge_risk"].text }
}

struct Docket: Hashable, Identifiable {
    let j: JSON
    var id: String { j["id"].str ?? "" }
    var title: String { j["title"].str ?? id }
    var titleSource: String { j["title_source"].str ?? "" }
    var status: Status { Status(rawValue: j["status"].str ?? "") ?? .todo }
    var proto: String { j["protocol"].str ?? "none" }
    var agent: AgentRef? { j["agent"].object == nil ? nil : AgentRef(j: j["agent"]) }
    /// Where its agent runs, for people: "Rex", "Herdr".
    var runtime: String { live?.runtime ?? agent?.runtime ?? "Rex" }
    var session: String? { j["session"].text }
    var prompts: Int { j["prompts"].int ?? 0 }
    var turns: Int { j["turns"].int ?? 0 }
    var lastTurn: String? { j["last_turn"].text }
    var released: String? { j["released"].text }
    var createdAt: String { j["created_at"].str ?? "" }
    var updatedAt: String { j["updated_at"].str ?? "" }
    var entries: Int { j["entries"].int ?? 0 }
    var head: String { j["head"].str ?? "" }
    var shortHead: String { j["short_head"].str ?? "" }
    var live: Live? { j["live"].object == nil ? nil : Live(j: j["live"]) }
    var intent: Intent? { j["intent"].object == nil ? nil : Intent(j: j["intent"]) }
    var intentVersions: Int { j["intent_versions"].int ?? 0 }
    var linear: LinearRef? { j["linear"].object == nil ? nil : LinearRef(j: j["linear"]) }
    var linearAlso: [LinearRef] { j["linear_also"].array.map(LinearRef.init) }
    var review: ReviewRef? { j["review"].object == nil ? nil : ReviewRef(j: j["review"]) }
    var isOpen: Bool { status.isOpen }
    /// It can take an agent now: open, not a Review's, and no agent working on it.
    var canStartAgent: Bool { isOpen && proto != "review" && live == nil }
    /// On hold: Build and Review take no steps on it. The reason, or nil.
    var hold: String? { j["hold"].text }
    /// Closed on your word, with nothing witnessed or checkable: why.
    var closedOn: String? { j["closed_on"].text }
    var talks: Bool { proto == "agent" && agent != nil }
    var hasIntent: Bool { !(intent?.body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ?? true) }
}

struct Msg: Hashable, Identifiable {
    let j: JSON
    var id: Int { j["id"].int ?? 0 }
    var docket: String? { j["docket"].text }
    var from: String { j["from"].str ?? "" }
    var fromApp: String? { j["from_app"].text }
    var subject: String? { j["subject"].text }
    var body: String { j["body"].str ?? "" }
    var options: [String] { j["options"].strings }
    var createdAt: String { j["created_at"].str ?? "" }
    var preview: JSON { j["preview"] }
    var who: String { from == "wheelhouse" ? "Wheelhouse" : fromApp ?? appName(from) }
    var title: String {
        subject ?? body.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.first { !$0.isEmpty } ?? "Message #\(id)"
    }
}

struct Attention: Hashable {
    let j: JSON
    var kind: String { j["kind"].str ?? "" }
    var text: String { j["text"].str ?? "" }
    var round: Int { j["round"].int ?? 0 }
}

struct Draft: Hashable {
    let j: JSON
    var event: String { j["event"].str ?? "" }
    var verdict: String? { j["verdict"].text }
    var blocking: Int { j["blocking"].int ?? 0 }
    var nonBlocking: Int { j["non_blocking"].int ?? 0 }
    var mergeRisk: String? { j["merge_risk"].text }
    var digest: String? { j["digest"].text }
}

struct ReviewRound: Hashable, Identifiable {
    let j: JSON
    var id: String { "\(round)-\(at ?? "")" }
    var round: Int { j["round"].int ?? 0 }
    var verdict: String? { j["verdict"].text }
    var by: String? { j["by"].text }
    var at: String? { j["at"].text }
    var url: URL? { j["url"].str.flatMap(URL.init(string:)) }
    var blocking: Int? { j["blocking"].int }
    var nonBlocking: Int? { j["non_blocking"].int }
    var mergeRisk: String? { j["merge_risk"].text }
    var tokens: Int? { j["tokens"]["total"].int }
    var found: String {
        (j["reviewers"].object ?? [:]).sorted { $0.key < $1.key }.map { name, r in
            let kept = r["kept"].int ?? r["drafted"].int ?? 0
            return "\(name): \(kept)" + ((r["only"].int ?? 0) > 0 ? " (\(r["only"].int!) alone)" : "")
        }.joined(separator: " · ")
    }
}

struct ReviewPr: Hashable, Identifiable {
    let j: JSON
    var id: String { fullKey }
    var key: String { j["key"].str ?? "" }
    var fullKey: String { j["full_key"].str ?? key }
    var repo: String { j["repo"].str ?? "" }
    var pr: Int { j["pr"].int ?? 0 }
    var url: URL? { j["url"].str.flatMap(URL.init(string:)) }
    var title: String? { j["title"].text }
    var author: String? { j["author"].text }
    var state: String { j["state"].str ?? "" }
    var phase: String? { j["phase"].text }
    var round: Int { j["round"].int ?? 0 }
    var verdict: String? { j["verdict"].text }
    var mergeRisk: String? { j["merge_risk"].text }
    var implementation: String? { j["implementation"].text }
    var docket: String? { j["docket"].text }
    var reviewersDone: Int { j["reviewers_done"].int ?? 0 }
    var reviewersTotal: Int { j["reviewers_total"].int ?? reviewers.count }
    var reviewers: [(name: String, agent: String, effort: String?)] {
        j["reviewers"].array.map { ($0["name"].str ?? "", $0["agent"].str ?? "", $0["effort"].text) }
    }

    var checkerEffort: String? { j["checker_effort"].text }
    var startError: String? { j["start_error"].text }
    var attention: Attention? { j["attention"].object == nil ? nil : Attention(j: j["attention"]) }
    var postedAt: String? { j["posted_at"].text }
    var updatedAt: String? { j["updated_at"].text }
    var history: [ReviewRound] { j["history"].array.map(ReviewRound.init) }
    var draft: Draft? { j["draft"].object == nil ? nil : Draft(j: j["draft"]) }
    var ready: Bool { state == "reviewing" && phase == "awaiting" }
    /// How a checked review posts in this PR's repo now: ask, auto or rule.
    var post: String? { j["post"].text }
    /// Why its checked review waits for you under a posting rule, or nil.
    var waitsWhy: String? {
        let d = j["post_decision"]
        if let r = d["refused"].text { return "your rule would post it, but the post was refused: \(r)" }
        return d.exists && !d["auto"].bool ? d["why"].text : nil
    }

    static func == (a: ReviewPr, b: ReviewPr) -> Bool { a.j == b.j }
    func hash(into h: inout Hasher) { h.combine(j) }
}

struct ReviewStatus: Hashable {
    let j: JSON
    var enabled: Bool { j["enabled"].bool }
    var paused: Bool { j["paused"].bool }
    var auto: Bool { j["post"].str == "auto" }
    var config: JSON { j["config"] }
    /// `wh review post-mode ask|auto`, over every repo's rule.
    var postOverride: String? { j["post_override"].text }
    /// A rule, or a repo that posts its own way.
    var ruled: Bool { config["post"].str == "rule" || !(config["repo_posts"].object ?? [:]).isEmpty }
    /// What the toolbar's switch shows: the override, else rule, auto or ask.
    var posting: String { postOverride ?? (ruled ? "rule" : config["post"].str == "auto" ? "auto" : "ask") }
    var configError: String? { j["config_error"].text }
    var lastScanAt: String? { j["last_scan_at"].text }
    var scanError: String? { j["scan_error"].text }
    var doing: String? { j["doing"].text }
    var rexReviewerRunning: Bool { j["rex_reviewer_running"].bool }
    var prs: [ReviewPr] { j["prs"].array.map(ReviewPr.init) }
}

struct BuildTask: Hashable, Identifiable {
    let j: JSON
    var id: String { docket }
    var docket: String { j["docket"].str ?? "" }
    var key: String { j["key"].str ?? "" }
    var title: String { j["title"].str ?? "" }
    var repo: String { j["repo"].str ?? "" }
    var state: String { j["state"].str ?? "" }
    var phase: String? { j["phase"].text }
    var branch: String? { j["branch"].text }
    var base: String? { j["base"].text }
    var planVersion: Int { j["plan_version"].int ?? 0 }
    var checkN: Int { j["check_n"].int ?? 0 }
    var fixUsed: Int { j["fix_used"].int ?? 0 }
    var lastCheck: JSON { j["last_check"] }
    var prNumber: Int? { j["pr_number"].int }
    var prURL: URL? { j["pr_url"].str.flatMap(URL.init(string:)) }
    var builder: JSON { j["builder"] }
    var checker: JSON { j["checker"] }
    var decisions: [JSON] { j["decisions"].array }
    var asks: [String] { j["asks"].strings }
    var why: String? { j["why"].text }
    var startError: String? { j["start_error"].text }
    var updatedAt: String? { j["updated_at"].text }
    var attention: Attention? { j["attention"].object == nil ? nil : Attention(j: j["attention"]) }
    var needsYou: Bool { attention != nil || (!asks.isEmpty && (state == "active" || state == "open")) }
    /// Address rounds since the PR opened, and the one in progress.
    var rounds: Int { j["rounds"].int ?? 0 }
    var round: JSON { j["round"] }
    var finished: Bool { ["merged", "closed", "discarded"].contains(state) }
}

struct BuildStatus: Hashable {
    let j: JSON
    var paused: Bool { j["paused"].bool }
    var doing: String? { j["doing"].text }
    var config: JSON { j["config"] }
    var repos: [String] { j["config"]["repos"].strings }
    var configError: String? { j["config_error"].text }
    var tasks: [BuildTask] { j["tasks"].array.map(BuildTask.init) }
}

/// What `/api/state` says: every docket, what needs you, and the protocols.
struct HubState: Hashable {
    let j: JSON
    let dockets: [Docket]
    let byId: [String: Docket]
    let messages: [Msg]
    let review: ReviewStatus?
    let build: BuildStatus?

    init(_ j: JSON) {
        self.j = j
        dockets = j["dockets"].array.map(Docket.init)
        byId = Dictionary(dockets.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        messages = j["messages"].array.map(Msg.init)
        review = j["review"].object == nil ? nil : ReviewStatus(j: j["review"])
        build = j["build"].object == nil ? nil : BuildStatus(j: j["build"])
    }

    static func == (a: HubState, b: HubState) -> Bool { a.j == b.j }
    func hash(into h: inout Hasher) { h.combine(j) }

    var owner: String { j["owner"].str ?? "you" }
    var hooks: Bool { j["hooks"].bool }
    var linear: Bool { j["linear"].bool }
    var rexConnected: Bool { j["rex"]["connected"].bool }
    var rexError: String? { j["rex"]["error"].text }
    /// Every runtime; an older hub says only Rex's.
    var runtimes: [RuntimeLink] {
        let a = j["runtimes"].array.map(RuntimeLink.init)
        return a.isEmpty ? [RuntimeLink(j: .object(["name": .string("rex"), "title": .string("Rex"), "connected": .bool(rexConnected), "error": j["rex"]["error"], "installed": .bool(true), "spawn": .bool(true)]))] : a
    }
    /// Where new agents open, for people.
    var spawnRuntime: String { runtimes.first { $0.spawn }?.title ?? "Rex" }
    /// The runtimes followed now: "Rex and Herdr".
    var followed: String {
        let on = runtimes.filter(\.connected).map(\.title)
        return on.isEmpty ? "Rex or Herdr" : on.joined(separator: " and ")
    }
    /// The status line: the runtimes followed, or the one that's down. One
    /// that isn't installed is no trouble; one that is and isn't followed is
    /// when new agents open there, or when nothing is followed.
    var connection: (ok: Bool, text: String) {
        let rs = runtimes
        if let down = rs.first(where: { $0.installed && !$0.connected && ($0.spawn || rs.allSatisfy { !$0.connected }) }) {
            return (false, "\(down.title): \(down.error ?? "connecting")")
        }
        let on = rs.filter(\.connected).map(\.title)
        // Two fit the sidebar's footer as "Rex · Herdr".
        return on.isEmpty ? (false, "\(spawnRuntime) isn't installed") : (true, on.count > 1 ? on.joined(separator: " · ") : "Following \(on[0])")
    }
    var agents: [Live] { j["agents"].array.map(Live.init) }
    var prs: [ReviewPr] { review?.prs ?? [] }
    var tasks: [BuildTask] { build?.tasks ?? [] }
    /// What needs a look beyond your asks, worst first.
    var attention: [Concern] { j["attention"].array.map(Concern.init) }
    func task(for docket: String) -> BuildTask? { tasks.first { $0.docket == docket } }
    func pr(for docket: String) -> ReviewPr? { prs.first { $0.docket == docket } }
}

/// One entry of a docket's record, as its page shows it.
struct Entry: Hashable, Identifiable {
    let j: JSON
    var id: Int { seq }
    var seq: Int { j["seq"].int ?? 0 }
    var entryId: String { j["id"].str ?? "" }
    var chain: String { j["chain"].str ?? "" }
    var kind: String { j["kind"].str ?? "" }
    var at: String { j["at"].str ?? "" }
    var actorType: String { j["actor"]["type"].str ?? "" }
    var actorId: String { j["actor"]["id"].str ?? "" }
    /// The app an agent actor speaks for (`claude-code@block` → `claude-code`).
    var actorApp: String { String(actorId.split(separator: "@").first ?? "") }
    var body: String? { j["body"].str }
    var data: JSON { j["data"] }
    var mark: String { j["mark"].str ?? "said" }
    var key: String? { j["key"].text }
    // The record view's extras.
    var blobs: [String] { j["blobs"].strings }
    var keyid: String? { j["keyid"].text }
    var problems: [String] { j["problems"].strings }
}

struct Verify: Hashable {
    let j: JSON
    var ok: Bool { j["ok"].bool }
    var entries: Int { j["entries"].int ?? 0 }
    var head: String? { j["head"].text }
    var problems: [String] { j["problems"].strings }
}

struct DocketPage: Hashable {
    let j: JSON
    let docket: Docket
    let entries: [Entry]
    init(_ j: JSON) {
        self.j = j
        docket = Docket(j: j["docket"])
        entries = j["entries"].array.map(Entry.init)
    }

    static func == (a: DocketPage, b: DocketPage) -> Bool { a.j == b.j }
    func hash(into h: inout Hasher) { h.combine(j) }

    var verify: Verify { Verify(j: j["verify"]) }
    /// What its done rests on: its checks, the checker's verdicts, its hold.
    var evidence: Evidence { Evidence(j: j["evidence"]) }
    /// What agents asked, and the answers that stand (src/decisions.rs).
    var decisions: [JSON] { j["decisions"].array }
    var queued: [(id: Int, body: String, at: String)] { j["queued"].array.map { ($0["id"].int ?? 0, $0["body"].str ?? "", $0["at"].str ?? "") } }
}

// ---- Verify ----

/// What a docket's done rests on (src/evidence.rs): witnessed checks, the
/// checker's verdicts, and whether it's held or was closed on your word.
struct Evidence: Hashable {
    let j: JSON
    /// The intent has acceptance items, so done needs evidence.
    var needs: Bool { j["needs"].bool }
    /// What done can rest on, newest first. Empty: only what was said.
    var lines: [EvidenceLine] { j["evidence"].array.map(EvidenceLine.init) }
    /// Each check's latest run.
    var checks: [CheckRun] { j["checks"].array.map(CheckRun.init) }
    var repo: String? { j["repo"].text }
    /// The checks its repo has, run or not.
    var configured: [(name: String, run: String, required: Bool)] {
        j["configured"].array.map { ($0["name"].str ?? "", $0["run"].str ?? "", $0["required"].exists ? $0["required"].bool : true) }
    }
    var running: Bool { j["running"].bool }
    var verdicts: [Verdict] { j["verdicts"].array.map(Verdict.init) }
    var hold: String? { j["hold"].text }
    var closedOn: String? { j["closed_on"].text }

    /// Nothing to show: no checks run, no verdicts, and done doesn't need
    /// evidence. Checks its repo has but never ran don't count: Run Checks
    /// is in the Docket menu, the context menu and the palette.
    var isEmpty: Bool {
        !needs && lines.isEmpty && checks.isEmpty && verdicts.isEmpty && closedOn == nil
    }

    /// Why Run checks can't run now, or nil when it can.
    var cantRun: String? {
        if running { return "They're running" }
        if configured.isEmpty {
            return repo.map { "\($0) has no checks: add them in Settings › Repos" } ?? "Its folder isn't in one of Build's repos, so it has no checks"
        }
        return nil
    }
}

struct EvidenceLine: Hashable {
    let j: JSON
    /// `witnessed` or `checkable`.
    var how: String { j["how"].str ?? "said" }
    var what: String { j["what"].str ?? "" }
    var seq: Int? { j["seq"].int }
    var at: String? { j["at"].text }
}

/// One witnessed check: the hub ran it, at a commit, and kept its output.
struct CheckRun: Hashable, Identifiable {
    let j: JSON
    var id: String { name }
    var name: String { j["name"].str ?? "" }
    var command: String { j["command"].str ?? "" }
    var required: Bool { j["required"].exists ? j["required"].bool : true }
    var ok: Bool { j["ok"].bool }
    /// Its exit code; nil when it timed out.
    var exit: Int? { j["exit"].int }
    var timedOut: Bool { j["exit"].isNull }
    var ms: Int? { j["ms"].int }
    var head: String { j["head"].str ?? "" }
    var shortHead: String { String(head.prefix(7)) }
    /// The folder had uncommitted changes: it covered more than the commit.
    var dirty: Bool { j["dirty"].bool }
    /// Its output's hash, without `sha256:`.
    var output: String? { j["output"].text.map { $0.replacingOccurrences(of: "sha256:", with: "") } }
    var why: String? { j["why"].text }
    var seq: Int { j["seq"].int ?? 0 }
    var at: String? { j["at"].text }

    /// How it went, in a few words: passed, failed (exit 1), timed out.
    var outcome: String {
        if ok { return dirty ? "passed, with uncommitted changes" : "passed" }
        if timedOut { return "timed out" }
        return "failed\(exit.map { ", exit \($0)" } ?? "")"
    }
}

/// A checker's verdict: said by the checker, signed for its session.
struct Verdict: Hashable, Identifiable {
    let j: JSON
    var id: Int { seq }
    var seq: Int { j["seq"].int ?? 0 }
    var at: String? { j["at"].text }
    var n: Int? { j["n"].int }
    var verdict: String { j["verdict"].str ?? "" }
    var passed: Bool { verdict == "pass" }
    /// How many findings: a list, or a count.
    var findings: Int { j["findings"].int ?? j["findings"].array.count }
    var summary: String? { j["summary"].text }
    var by: String { j["by"].str ?? "" }
    var role: String? { j["role"].text }
    /// From a session other than the builder's; nil when it can't be told.
    var independent: Bool? { j["independent"].isNull ? nil : j["independent"].bool }
    var head: String? { j["head"].text }
    var mark: String { j["mark"].str ?? "said" }
}

/// Something worth a look that isn't an ask: a failing check, a hold, an
/// agent working long, a quiet docket, a close on your word.
struct Concern: Hashable, Identifiable {
    let j: JSON
    var id: String { "at:\(docket ?? title):\(kind)" }
    var docket: String? { j["docket"].text }
    var title: String { j["title"].str ?? "" }
    /// build, review, failing, held, long, quiet, or word.
    var kind: String { j["kind"].str ?? "" }
    var text: String { j["text"].str ?? "" }
    var at: String? { j["at"].text }

    var symbol: String {
        switch kind {
        case "build": "hammer"
        case "review": "arrow.triangle.pull"
        case "failing": "xmark.octagon"
        case "held": "pause.circle"
        case "long": "clock"
        case "quiet": "moon.zzz"
        case "word": "quote.bubble"
        default: "exclamationmark.circle"
        }
    }

    var label: String {
        switch kind {
        case "build": "Build"
        case "review": "Review"
        case "failing": "Failing"
        case "held": "On hold"
        case "long": "Working long"
        case "quiet": "Quiet"
        case "word": "Your word"
        default: kind.capitalized
        }
    }
}

/// How long a check took: 153 ms, 4.2 s, 3m 05s.
func duration(ms: Int?) -> String {
    guard let ms else { return "" }
    if ms < 1000 { return "\(ms) ms" }
    if ms < 60000 { return String(format: "%.1f s", Double(ms) / 1000) }
    let s = ms / 1000
    return String(format: "%dm %02ds", s / 60, s % 60)
}

// ---- dates ----

private let isoFrac: ISO8601DateFormatter = {
    let f = ISO8601DateFormatter()
    f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    return f
}()

private let isoPlain = ISO8601DateFormatter()

func date(_ iso: String?) -> Date? {
    guard let iso, !iso.isEmpty else { return nil }
    return isoFrac.date(from: iso) ?? isoPlain.date(from: iso)
}

/// How long ago, in the list's few characters: now, 5m, 3h, 2d, Mar 4.
func ago(_ iso: String?, now: Date = Date()) -> String {
    guard let d = date(iso) else { return "" }
    let s = max(0, now.timeIntervalSince(d))
    if s < 60 { return "now" }
    if s < 3600 { return "\(Int(s / 60))m" }
    if s < 86400 { return "\(Int(s / 3600))h" }
    if s < 86400 * 7 { return "\(Int(s / 86400))d" }
    return d.formatted(.dateTime.month(.abbreviated).day())
}

/// The same, in words: just now, 5m ago, Mar 4.
func agoLong(_ iso: String?) -> String {
    let a = ago(iso)
    if a.isEmpty { return "" }
    if a == "now" { return "just now" }
    return a.last.map { "mhd".contains($0) } == true && a.first?.isNumber == true ? "\(a) ago" : a
}

/// A timestamp: Mar 4, 14:05.
func stamp(_ iso: String?) -> String {
    guard let d = date(iso) else { return iso ?? "" }
    return d.formatted(.dateTime.month(.abbreviated).day().hour(.twoDigits(amPM: .omitted)).minute())
}

func shortPath(_ p: String?) -> String {
    guard let p else { return "" }
    let home = NSHomeDirectory()
    return p.hasPrefix(home) ? "~" + p.dropFirst(home.count) : p
}

func plural(_ n: Int, _ word: String, _ many: String? = nil) -> String {
    "\(n) \(n == 1 ? word : many ?? word + "s")"
}

/// The first line worth showing of an agent's turn: no Markdown marks.
func firstLine(_ s: String?) -> String? {
    s?.split(separator: "\n").lazy.map {
        $0.replacingOccurrences(of: "**", with: "").trimmingCharacters(in: CharacterSet(charactersIn: "#>*- \t"))
    }.first { !$0.isEmpty }
}
