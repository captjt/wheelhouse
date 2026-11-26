import AppKit
import Foundation
import Observation

/// A short note at the bottom of the window: what just happened, and its undo.
struct Toast: Identifiable {
    let id = UUID()
    let text: String
    let error: Bool
    let action: (label: String, run: () -> Void)?
}

/// One thing in the Inbox: a prompt, a review to post, a message, a failure,
/// or a finished turn.
struct InboxItem: Identifiable, Hashable {
    enum Kind: Int { case blocked, review, message, error, done }
    let kind: Kind
    let docket: Docket?
    let docketId: String
    let title: String
    let line: String
    let at: String?
    let msg: Msg?
    let pr: ReviewPr?
    let id: String
    /// The agent asking, on a docket with several (nil on an older hub's).
    var agent: DocketAgent? = nil
    /// What it's doing: the prompt it waits on.
    var live: Live? = nil

    /// Which agent, when its docket has more than one on it.
    var who: String? { docket?.several == true ? agent?.label : nil }
    /// The block to act on: nil means the docket's lead.
    var block: String? { agent?.block }

    static func == (a: InboxItem, b: InboxItem) -> Bool { a.id == b.id && a.line == b.line && a.docket == b.docket && a.msg == b.msg && a.pr == b.pr && a.live == b.live }
    func hash(into h: inout Hasher) { h.combine(id) }
}

/// The app's one view of the hub: its state, kept live, and what you do to it.
@Observable @MainActor
final class Store {
    static let shared = Store()

    let hub = HubClient()
    private(set) var state: HubState?
    private(set) var error: String?
    /// Following the hub's events right now; and whether it ever has.
    private(set) var live = false {
        didSet { if live { wasLive = true } }
    }

    private(set) var wasLive = false
    /// Bumps on every change the hub reports, so open dockets reload.
    private(set) var version = 0
    private(set) var toasts: [Toast] = []
    /// Starting the hub (`wh up`) from the app.
    private(set) var startingHub = false

    private var started = false
    private var refreshing = false
    private var again = false
    private var scheduled = false

    var dockets: [Docket] { state?.dockets ?? [] }
    func docket(_ id: String?) -> Docket? { id.flatMap { state?.byId[$0] } }

    // ---- following the hub ----

    func start() {
        guard !started else { return }
        started = true
        Task { await refresh() }
        Task { await follow() }
        // Times and anything missed: every 30 seconds, as the web app does.
        Task {
            while true {
                try? await Task.sleep(for: .seconds(30))
                await self.runRefresh()
                await self.checkNotifying()
            }
        }
        NotificationCenter.default.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil, queue: .main) { _ in
            Task { @MainActor in await Store.shared.checkNotifying() }
        }
    }

    private var stream: Task<Void, Never>?
    /// The current stream told the hub the app posts notifications: only
    /// then does it post them (the hub posts none of its own).
    private(set) var notifying = false

    /// Whether the app may post changed (System Settings, or Settings here):
    /// follow again, telling the hub.
    func checkNotifying() async {
        if await Notifier.shared.canPost() != notifying { reconnect() }
    }

    /// Follow again from the top: the hub address changed, or whether the
    /// app posts notifications did.
    func reconnect() {
        stream?.cancel()
    }

    private func follow() async {
        var wait = 1.0
        while true {
            let notifies = await Notifier.shared.canPost()
            notifying = notifies
            let task = Task { @MainActor in
                do {
                    for try await ev in hub.events(notifies: notifies) {
                        live = true
                        wait = 1
                        if ev["type"].str == "notify" { Notifier.shared.post(ev) }
                        bump()
                    }
                } catch {}
            }
            stream = task
            await task.value
            live = false
            try? await Task.sleep(for: .seconds(wait))
            wait = min(wait * 2, 10)
        }
    }

    /// Something changed: refresh soon, once for a burst of changes.
    func bump() {
        if refreshing {
            again = true
            return
        }
        guard !scheduled else { return }
        scheduled = true
        Task {
            try? await Task.sleep(for: .milliseconds(150))
            scheduled = false
            await runRefresh()
        }
    }

    private func runRefresh() async {
        if refreshing {
            again = true
            return
        }
        refreshing = true
        repeat {
            again = false
            await refresh()
            version += 1
        } while again
        refreshing = false
    }

    func refresh() async {
        do {
            let next = HubState(try await hub.get("/api/state"))
            Docket.me = next.team?.you
            if next != state { state = next }
            error = nil
            Notifier.shared.reconcile(next)
            Badge.update(urgent)
        } catch {
            self.error = Self.words(error)
            Badge.update(0)
        }
    }

    /// Refresh now, and tell open dockets to reload.
    func reload() {
        Task { await runRefresh() }
    }

    static func words(_ e: Error) -> String {
        if let h = e as? HubError { return h.message }
        let ns = e as NSError
        if ns.domain == NSURLErrorDomain, [NSURLErrorCannotConnectToHost, NSURLErrorNetworkConnectionLost, NSURLErrorTimedOut, NSURLErrorCannotFindHost].contains(ns.code) {
            return "The hub isn't running"
        }
        return e.localizedDescription
    }

    /// The hub isn't answering at all (as opposed to answering with an error).
    var hubDown: Bool { state == nil && error == "The hub isn't running" }

    /// Start the hub at login and now: `wh up`, as in a terminal.
    func startHub() {
        guard !startingHub else { return }
        startingHub = true
        Task.detached {
            let result = Shell.run(Shell.wh, ["up"])
            await MainActor.run {
                self.startingHub = false
                if result.ok {
                    self.toast("The hub is starting")
                } else {
                    self.toast(result.output.isEmpty ? "Couldn't find wh: install it with `\(Flavor.setup)`" : result.output, error: true)
                }
                Task {
                    try? await Task.sleep(for: .seconds(1.5))
                    await self.runRefresh()
                }
            }
        }
    }

    // ---- what needs you ----

    var inbox: [InboxItem] { Self.inbox(state) }

    nonisolated static func inbox(_ state: HubState?) -> [InboxItem] {
        guard let state else { return [] }
        var items: [InboxItem] = []
        let attention = ["ready": "Ready to post", "blocked": "Blocked", "start-failed": "Didn't start", "stalled": "Stalled", "pane-closed": "Pane closed", "error": "Failed"]
        for p in state.prs {
            guard let a = p.attention else { continue }
            let d = p.docket.flatMap { state.byId[$0] }
            let what = a.kind == "ready" ? [draftLine(p) ?? a.text, p.waitsWhy.map { "waits for you: \($0)" }].compactMap { $0 }.joined(separator: " · ") : a.text
            items.append(InboxItem(kind: .review, docket: d, docketId: p.docket ?? p.key, title: d?.title ?? "Review \(p.key)\(p.title.map { ": \($0)" } ?? "")", line: "\(attention[a.kind] ?? "Needs you") (round \(a.round)): \(what)", at: p.updatedAt, msg: nil, pr: p, id: "rv:\(p.fullKey)"))
        }
        for m in state.messages {
            let d = m.docket.flatMap { state.byId[$0] }
            items.append(InboxItem(kind: .message, docket: d, docketId: m.docket ?? "#\(m.id)", title: d?.title ?? m.title, line: "\(m.who): \(m.title)", at: m.createdAt, msg: m, pr: nil, id: "m\(m.id)"))
        }
        // One for each agent that needs you, not one for each docket: the
        // lead's is the docket's own (its id), the others' are by block.
        for d in state.dockets {
            for (i, (a, l)) in d.lives.enumerated() {
                let id = i == 0 ? d.id : "\(d.id)@\(a?.block ?? "\(i)")"
                let name = a.map { d.several ? $0.label : appName($0.app) } ?? appName(d.agent?.app)
                switch l.state {
                case .blocked:
                    items.append(InboxItem(kind: .blocked, docket: d, docketId: d.id, title: d.title, line: l.msg ?? "\(name) needs you", at: l.since, msg: nil, pr: nil, id: id, agent: a, live: l))
                case .error where d.isOpen:
                    items.append(InboxItem(kind: .error, docket: d, docketId: d.id, title: d.title, line: l.msg ?? "Failed", at: l.since, msg: nil, pr: nil, id: id, agent: a, live: l))
                case .done where d.isOpen:
                    // The docket's last turn is the lead's, or the only one's.
                    let said = i == 0 ? firstLine(d.lastTurn) : nil
                    items.append(InboxItem(kind: .done, docket: d, docketId: d.id, title: d.title, line: said ?? "Finished its turn", at: l.since ?? d.updatedAt, msg: nil, pr: nil, id: id, agent: a, live: l))
                default: break
                }
            }
        }
        return items.sorted { a, b in a.kind.rawValue != b.kind.rawValue ? a.kind.rawValue < b.kind.rawValue : (a.at ?? "") > (b.at ?? "") }
    }

    /// What needs a look beyond your asks: the Inbox's Attention.
    var concerns: [Concern] { Self.concerns(state, inbox: inbox) }

    /// The hub's attention list, less what's already an ask: a Review's or
    /// a Build's stop shows in the Inbox as its ask (a review to post, a
    /// message from Wheelhouse), so it isn't listed twice.
    nonisolated static func concerns(_ state: HubState?, inbox: [InboxItem]) -> [Concern] {
        guard let state else { return [] }
        return state.attention.filter { c in
            switch c.kind {
            case "review":
                return !inbox.contains { $0.kind == .review && ($0.docketId == c.docket || $0.pr?.key == c.title || $0.pr?.fullKey == c.title) }
            case "build":
                return !inbox.contains { $0.docketId == c.docket }
            default:
                return true
            }
        }
    }

    /// What needs you now: the Dock badge and the menu bar's count.
    var urgent: Int {
        guard let s = state else { return 0 }
        return s.dockets.reduce(0) { n, d in n + d.lives.filter { $0.live.state == .blocked || $0.live.state == .error }.count } + s.messages.count + s.prs.filter { $0.attention != nil }.count + s.tasks.filter { $0.attention != nil }.count
    }

    // ---- toasts ----

    func toast(_ text: String, error: Bool = false, action: (label: String, run: () -> Void)? = nil) {
        let t = Toast(text: text, error: error, action: action)
        toasts = Array(toasts.suffix(2)) + [t]
        let wait: Double = action != nil ? 5.5 : error ? 6 : 3.2
        Task {
            try? await Task.sleep(for: .seconds(wait))
            toasts.removeAll { $0.id == t.id }
        }
    }

    func dismiss(_ t: Toast) { toasts.removeAll { $0.id == t.id } }

    /// Run a write; on failure, say why. True if it went through.
    @discardableResult
    func act(_ path: String, _ body: [String: Any] = [:], done: String? = nil, slow: Bool = false) async -> JSON? {
        do {
            let r = try await hub.post(path, body, slow: slow)
            if let done { toast(done) }
            reload()
            return r
        } catch {
            toast(Self.words(error), error: true)
            reload()
            return nil
        }
    }

    // ---- dockets ----

    func page(_ id: String) async throws -> DocketPage {
        DocketPage(try await hub.get("/api/dockets/\(id)"))
    }

    func setStatus(_ id: String, _ status: Status, undo: Status? = nil) {
        Task {
            do {
                try await hub.post("/api/dockets/\(id)/set", ["status": status.rawValue])
                reload()
            } catch let e as HubError where e.status == 409 && status == .done {
                // The rule for done: it needs evidence. Say why, in the
                // window you're in, and offer its checks or your word.
                reload()
                Windows.presenting().closing = Closing(id: id, why: e.message, from: undo ?? docket(id)?.status ?? .in_progress)
                return
            } catch {
                toast(Self.words(error), error: true)
                reload()
                return
            }
            if let undo, undo != status {
                toast("\(id) moved to \(status.label)", action: ("Undo", { [weak self] in
                    Task { await self?.act("/api/dockets/\(id)/set", ["status": undo.rawValue], done: "\(id) is back in \(undo.label)") }
                }))
            } else {
                toast("\(id) moved to \(status.label)")
            }
        }
    }

    /// Close it with nothing witnessed or checkable behind it: on your word,
    /// with why. The record keeps the reason.
    /// Throws the hub's refusal, for the sheet to show.
    func closeOnWord(_ id: String, reason: String, undo: Status? = nil) async throws {
        defer { reload() }
        try await hub.post("/api/dockets/\(id)/set", ["status": Status.done.rawValue, "anyway": true, "reason": reason])
        if let undo, undo != .done {
            toast("\(id) closed on your word", action: ("Undo", { [weak self] in
                Task { await self?.act("/api/dockets/\(id)/set", ["status": undo.rawValue], done: "\(id) is back in \(undo.label)") }
            }))
        } else {
            toast("\(id) closed on your word")
        }
    }

    /// Hold it: Build and Review take no steps on it until you resume.
    /// Throws the hub's refusal, for the sheet to show.
    func hold(_ id: String, reason: String) async throws {
        let r = reason.trimmingCharacters(in: .whitespacesAndNewlines)
        defer { reload() }
        try await hub.post("/api/dockets/\(id)/hold", r.isEmpty ? [:] : ["reason": r])
        toast("\(id) is on hold: Build and Review take no steps on it")
    }

    func resume(_ id: String) {
        Task { await act("/api/dockets/\(id)/hold", ["off": true], done: "\(id) resumed") }
    }

    /// Run its witnessed checks now; their results arrive as entries.
    @discardableResult
    func runChecks(_ id: String) async -> Bool {
        do {
            try await startChecks(id)
            return true
        } catch {
            toast(Self.words(error), error: true)
            return false
        }
    }

    /// The same, throwing the hub's refusal (no checks, or running already).
    func startChecks(_ id: String) async throws {
        defer { reload() }
        let names = try await hub.post("/api/dockets/\(id)/check")["checks"].strings
        toast("Running \(id)'s checks\(names.isEmpty ? "" : ": " + names.joined(separator: ", "))")
    }

    /// An answered decision, as a standing rule in its repo's notes: later
    /// briefs there carry it.
    func makeStanding(_ id: String, key: String) {
        Task {
            guard let r = await act("/api/dockets/\(id)/standing", ["key": key]) else { return }
            toast("Standing in \(r["repo"].str ?? "the repo")'s notes: \(r["rule"].str ?? key)")
        }
    }

    func rename(_ id: String, _ title: String) {
        Task { await act("/api/dockets/\(id)/set", ["title": title]) }
    }

    /// What you typed in a docket's composer: a note, `@agent`, `/ask`,
    /// `/decide` or `/task` (src/compose.rs). The hub's answer, or nil when
    /// it refused (and said why in a toast).
    func compose(_ id: String, _ text: String) async -> JSON? {
        guard let r = await act("/api/dockets/\(id)/compose", ["text": text]) else { return nil }
        if let t = Compose.toast(r) { toast(t.text, error: t.error) }
        return r
    }

    func note(_ id: String, _ body: String) async -> Bool {
        await act("/api/dockets/\(id)/note", ["body": body]) != nil
    }

    func create(title: String, note: String) async -> String? {
        guard let r = await act("/api/dockets", ["title": title]), let id = r["docket"].str else { return nil }
        if !note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { _ = await self.note(id, note) }
        toast("\(id) created")
        return id
    }

    // ---- calls (src/calls.rs) ----

    /// A call's transcript, kept on the docket it was about (nil: on a
    /// docket of its own), and the Listener starts reading it. The docket
    /// that keeps it, or nil when it was refused.
    func addCall(docket: String?, title: String, transcript: String) async -> String? {
        var body: [String: Any] = ["transcript": transcript]
        if let docket { body["docket"] = docket }
        let t = title.trimmingCharacters(in: .whitespacesAndNewlines)
        if !t.isEmpty { body["title"] = t }
        guard let r = await act("/api/calls", body), let id = r["docket"].text else { return nil }
        toast("\(id): the Listener is reading the call")
        return id
    }

    /// Read a docket's call again: after a failure, or with dockets that
    /// have changed. `call`: the entry keeping its transcript.
    @discardableResult
    func listen(_ id: String, call: Int) async -> Bool {
        await act("/api/dockets/\(id)/calls/\(call)/listen") != nil
    }

    /// Apply a proposal from one of a docket's calls (as edited), or ignore
    /// it. True if it went through.
    func answerProposal(_ id: String, _ n: Int, apply: Bool, edit: [String: Any]? = nil) async -> Bool {
        var body: [String: Any] = ["action": apply ? "apply" : "ignore"]
        if let edit { body["edit"] = edit }
        guard let r = await act("/api/dockets/\(id)/proposals/\(n)", body) else { return false }
        if apply { toast(Calls.appliedToast(r["result"])) }
        return true
    }

    /// `?block=B`, for a pane action on one of a docket's agents; nothing
    /// for its lead.
    nonisolated static func blockQuery(_ block: String?) -> String {
        guard let block, !block.isEmpty else { return "" }
        return "?block=" + (block.addingPercentEncoding(withAllowedCharacters: .alphanumerics.union(CharacterSet(charactersIn: "-._~"))) ?? block)
    }

    /// Bring an agent's pane forward: that block's, or the docket's lead's.
    func openInRex(_ id: String, block: String? = nil) {
        let d = docket(id)
        let a = d?.member(block: block)
        let whose = a.map { d?.several == true ? "\($0.label)'s" : "\(id)'s" } ?? "\(id)'s"
        Task { await act("/api/dockets/\(id)/open\(Self.blockQuery(block))", done: "Showing \(whose) pane in \(a?.runtime ?? d?.runtime ?? "Rex")") }
    }

    /// Approve or deny an agent's prompt: that agent's, or the lead's. The
    /// hub reads the prompt from its runtime again and answers only if it's
    /// the one shown here.
    func answer(_ d: Docket, block: String? = nil, approve: Bool) async {
        let a = d.member(block: block)
        guard let l = a?.live ?? (block == nil ? d.live : nil) else { return }
        await answer(d.id, record: l.record ?? "", since: l.since, msg: l.msg, approve: approve, block: a?.block)
    }

    /// Answer the prompt an Inbox item shows: its agent's, exactly as shown.
    func answer(_ it: InboxItem, approve: Bool) async {
        guard let d = it.docket else { return }
        guard let l = it.live else { return await answer(d, approve: approve) }
        await answer(d.id, record: l.record ?? "", since: l.since, msg: l.msg, approve: approve, block: d.blockFor(it.agent))
    }

    /// Answer the prompt as it was shown: the hub refuses if what waits in
    /// the pane now is a different prompt.
    func answer(_ id: String, record: String, since: String?, msg: String?, approve: Bool, block: String? = nil) async {
        var body: [String: Any] = ["record": record, "updated_at": since ?? NSNull(), "msg": msg ?? NSNull()]
        if let block, !block.isEmpty { body["block"] = block }
        guard let r = await act("/api/dockets/\(id)/\(approve ? "approve" : "deny")", body) else { return }
        if r["cleared"].bool {
            toast("\(approve ? "Approved" : "Denied")\(msg.map { ": \($0)" } ?? "")")
        } else {
            toast("Sent, but the prompt is still showing. Look at the pane.", error: true)
        }
    }

    /// Message an agent on the docket: that block's, or the lead.
    /// `steer`: while it works, send it now, for it to read at its next
    /// step, instead of queuing it for the turn's end.
    func send(_ id: String, _ text: String, block: String? = nil, steer: Bool = false) async -> Bool {
        var body: [String: Any] = ["text": text, "queue": true, "steer": steer]
        if let block, !block.isEmpty { body["block"] = block }
        guard let r = await act("/api/dockets/\(id)/send", body) else { return false }
        if r["steered"].bool { toast("Sent: it reads it at its next step") }
        else if r["queued"].int != nil { toast("Queued: everything queued goes in together when the turn ends") }
        return true
    }

    func unqueue(_ id: String, _ q: Int) {
        Task { await act("/api/dockets/\(id)/queue/\(q)/cancel", done: "Taken back") }
    }

    func setIntent(_ id: String, _ body: String, versions: Int) async -> Bool {
        await act("/api/dockets/\(id)/intent", ["body": body], done: "\(id): intent saved as version \(versions + 1)") != nil
    }

    func takeLinear(_ d: Docket) {
        Task { await act("/api/dockets/\(d.id)/intent", ["take_linear": true], done: "\(d.id) follows \(d.linear?.key ?? "the issue")'s description again") }
    }

    func link(_ id: String, issue: String, hadIntent: Bool) async {
        await act("/api/dockets/\(id)/linear", ["key": issue], done: "\(id) linked to \(issue)\(hadIntent ? "" : ": its description is the intent")")
    }

    func attach(_ id: String, file: URL) async {
        guard let bytes = try? Data(contentsOf: file) else {
            toast("Couldn't read \(file.lastPathComponent)", error: true)
            return
        }
        await act("/api/dockets/\(id)/artifacts", ["name": file.lastPathComponent, "content": bytes.base64EncodedString()], done: "Kept \(file.lastPathComponent) with \(id), pinned by its hash")
    }

    func attach(_ id: String, link: URL) async {
        await act("/api/dockets/\(id)/artifacts", ["url": link.absoluteString], done: "Linked it to \(id)")
    }

    /// `docket`: work on that docket instead of opening a new one.
    func startAgent(app: String, cwd: String, prompt: String, title: String?, docket: String? = nil) async -> String? {
        var body: [String: Any] = ["app": app, "cwd": cwd, "prompt": prompt]
        if let title, !title.isEmpty { body["title"] = title }
        if let docket { body["docket"] = docket }
        guard let r = await act("/api/agents/start", body), let id = r["docket"].str else { return nil }
        let pane = r["pane"].text ?? id
        let others = r["others"].int ?? 0
        if docket == nil {
            toast("\(id): \(appName(app)) started in \(state?.spawnRuntime ?? "Rex")'s wheelhouse session")
        } else if others > 0 {
            toast("\(appName(app)) joined \(id) beside \(plural(others, "other agent")), in wheelhouse/\(pane)")
        } else {
            toast("\(appName(app)) is on \(id), in wheelhouse/\(pane)")
        }
        return id
    }

    /// Where an agent could start, and where this docket's should.
    func folders(for docket: String?) async -> (all: [String], suggested: String?) {
        let q = docket.map { "?docket=\($0)" } ?? ""
        guard let r = try? await hub.get("/api/folders\(q)") else { return ([], nil) }
        return (r["folders"].strings, r["suggested"].text)
    }

    func fromLinear(_ key: String) async -> String? {
        guard let r = await act("/api/dockets/from-linear", ["key": key]), let id = r["docket"].str else { return nil }
        toast(r["existing"].bool ? "\(key) is already \(id)" : "\(id) created from \(key)")
        return id
    }

    func verifyAll() {
        Task {
            do {
                let r = try await hub.get("/api/verify")
                let n = r["dockets"].array.count
                if r["ok"].bool { toast("All \(n) records check: chains intact, signatures good") } else { toast("\(r["dockets"].array.filter { !$0["ok"].bool }.count) records don't check", error: true) }
            } catch { toast(Self.words(error), error: true) }
        }
    }

    // ---- messages ----

    func reply(_ m: Msg, _ body: String, choice: String? = nil) async -> Bool {
        var b: [String: Any] = ["body": body]
        if let choice { b["choice"] = choice }
        guard let r = await act("/api/messages/\(m.id)/reply", b) else { return false }
        toast("\(choice.map { "Chose \($0)" } ?? "Replied") · it goes to \(r["to"].str ?? m.who) between turns")
        return true
    }

    func file(_ m: Msg) {
        Task { await act("/api/messages/\(m.id)/done") }
    }

    // ---- reviews and builds ----

    func post(_ p: ReviewPr, digest: String?) async -> Bool {
        var b: [String: Any] = ["key": p.fullKey]
        if let d = digest ?? p.draft?.digest { b["digest"] = d }
        return await act("/api/review/post", b, done: "Posted the review of \(p.key)", slow: true) != nil
    }

    func dismissReview(_ p: ReviewPr) async -> Bool {
        // A round already on GitHub is recorded as posted, not dismissed.
        guard let r = await act("/api/review/dismiss", ["key": p.fullKey], slow: true) else { return false }
        toast(r["dismissed"].bool ? "Dismissed round \(p.round) of \(p.key); nothing was posted" : "Round \(p.round) of \(p.key) was on GitHub already; it's recorded as posted")
        return true
    }

    func preview(_ p: ReviewPr) async -> (text: String, digest: String?)? {
        do {
            let r = try await hub.post("/api/review/preview", ["key": p.fullKey], slow: true)
            return (r["preview"].str ?? "", r["digest"].text)
        } catch {
            toast(Self.words(error), error: true)
            return nil
        }
    }

    func reviewSettings(_ body: [String: Any], done: String) {
        Task { await act("/api/review/settings", body, done: done) }
    }

    func checkGitHub() {
        Task { await act("/api/review/check", done: "Checking GitHub") }
    }

    func build(_ d: Docket, into repo: String) {
        Task { await act("/api/build/start", ["from": d.id, "repo": repo], done: "\(d.id) is queued to build into \(repo)") }
    }

    func discardBuild(_ id: String) {
        Task { await act("/api/build/discard", ["docket": id], done: "Discarded \(id)'s Build; its branch stays") }
    }

    func pauseBuilds(_ paused: Bool) {
        Task { await act("/api/build/settings", ["paused": paused], done: paused ? "Builds paused: nothing new starts" : "Builds resumed") }
    }
}

/// The checked review, in a line: what it asks and what it found.
func draftLine(_ p: ReviewPr) -> String? {
    guard let d = p.draft else { return nil }
    let ask = d.event == "APPROVE" ? "Approve" : d.event == "REQUEST_CHANGES" ? "Request changes" : d.event
    return [ask, "\(d.blocking) blocking, \(d.nonBlocking) non-blocking", d.mergeRisk.map { "merge risk \($0)" }].compactMap { $0 }.joined(separator: " · ")
}

/// The Dock icon's badge: what needs you.
enum Badge {
    @MainActor static func update(_ n: Int) {
        NSApp?.dockTile.badgeLabel = n > 0 ? String(n) : nil
    }
}

/// Running `wh` and friends, as from a login shell.
enum Shell {
    /// `wh`: on PATH, or where `cargo install` and Homebrew put it. Wheelhouse
    /// Dev's is its own hub's, and never falls back to the daily one.
    static var wh: String {
        if let own = Flavor.command { return own }
        let home = NSHomeDirectory()
        for p in ["\(home)/.cargo/bin/wh", "/opt/homebrew/bin/wh", "/usr/local/bin/wh"] where FileManager.default.isExecutableFile(atPath: p) {
            return p
        }
        return "wh"
    }

    static func run(_ cmd: String, _ args: [String]) -> (ok: Bool, output: String) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        p.arguments = [cmd] + args
        var env = ProcessInfo.processInfo.environment
        env["PATH"] = "\(NSHomeDirectory())/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:" + (env["PATH"] ?? "/usr/bin:/bin")
        p.environment = env
        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = pipe
        do {
            try p.run()
            p.waitUntilExit()
        } catch {
            return (false, "")
        }
        let out = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
        return (p.terminationStatus == 0, out)
    }
}
