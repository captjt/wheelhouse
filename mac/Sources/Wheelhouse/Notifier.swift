import AppKit
@preconcurrency import UserNotifications

/// The Mac's notifications, posted by the app while it runs (the hub stays
/// quiet then; see src/notify.rs). Each carries what you can answer from the
/// banner: Approve or Deny a prompt, reply to an agent, pick one of its
/// options, or post a checked review. One that no longer needs you (answered
/// here, on your phone, or in Rex) is withdrawn.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    static let shared = Notifier()
    private var center: UNUserNotificationCenter? { Bundle.main.bundleIdentifier == nil ? nil : UNUserNotificationCenter.current() }
    private var categories: Set<UNNotificationCategory> = []

    var enabled: Bool { UserDefaults.standard.object(forKey: "notify") as? Bool ?? true }

    func setUp() {
        guard let center else { return }
        center.delegate = self
        center.requestAuthorization(options: [.alert, .sound, .badge]) { _, _ in
            Task { @MainActor in Store.shared.reconnect() }
        }
        let reply = UNTextInputNotificationAction(identifier: "reply", title: "Reply", options: [], textInputButtonTitle: "Send", textInputPlaceholder: "Message the agent…")
        categories = [
            UNNotificationCategory(identifier: "prompt", actions: [
                UNNotificationAction(identifier: "approve", title: "Approve", options: []),
                UNNotificationAction(identifier: "deny", title: "Deny", options: [.destructive]),
            ], intentIdentifiers: []),
            UNNotificationCategory(identifier: "turn", actions: [reply], intentIdentifiers: []),
            UNNotificationCategory(identifier: "message", actions: [UNTextInputNotificationAction(identifier: "reply", title: "Reply", options: [], textInputButtonTitle: "Send", textInputPlaceholder: "Your answer…")], intentIdentifiers: []),
            UNNotificationCategory(identifier: "review", actions: [
                UNNotificationAction(identifier: "post", title: "Post Review", options: []),
                UNNotificationAction(identifier: "open", title: "Preview", options: [.foreground]),
            ], intentIdentifiers: []),
        ]
        center.setNotificationCategories(categories)
    }

    /// A `notify` event from the hub: post it, with the actions it allows now.
    func post(_ ev: JSON) {
        // Only while the hub knows the app posts them; otherwise it does.
        guard enabled, let center, Store.shared.notifying else { return }
        let state = Store.shared.state
        let kind = ev["kind"].str ?? ""
        let docket = ev["docket"].text
        let c = UNMutableNotificationContent()
        c.title = ev["title"].str ?? "Wheelhouse"
        c.subtitle = ev["subtitle"].str ?? ""
        c.body = ev["body"].str ?? ""
        c.threadIdentifier = docket ?? ev["group"].str ?? "wheelhouse"
        var info: [String: Any] = ["kind": kind, "group": ev["group"].str ?? ""]
        if let docket { info["docket"] = docket }
        if let m = ev["message"].int { info["message"] = m }
        if let k = ev["key"].str { info["key"] = k }
        // What the banner shows, so an answer from it is to exactly that.
        for (k, v) in ev["seen"].object ?? [:] { if let s = v.str { info["seen_\(k)"] = s } }
        c.userInfo = info
        if SoundPacks.shared.chosen != nil {
            // A pack's line in place of the macOS sound, when the banner would
            // sound: not while you're in the app, nor with sounds turned off
            // for it in System Settings.
            c.sound = nil
            if let sound = SoundCategory.of(ev), !NSApp.isActive {
                Task {
                    if await center.notificationSettings().soundSetting == .enabled { SoundPacks.shared.play(sound) }
                }
            }
        } else {
            c.sound = kind == "turn" ? nil : .default
        }
        c.interruptionLevel = kind == "needs" || kind == "message" ? .timeSensitive : .active

        switch kind {
        case "needs":
            if let d = state?.byId[docket ?? ""], d.live?.answerable == true, ev["seen"]["record"].str != nil { c.categoryIdentifier = "prompt" }
        case "turn":
            c.categoryIdentifier = "turn"
        case "message":
            if let id = ev["message"].int, let m = state?.messages.first(where: { $0.id == id }), !m.options.isEmpty {
                // An agent's options, as the banner's buttons (macOS shows up to four).
                let cat = UNNotificationCategory(identifier: "msg-\(id)", actions: m.options.prefix(4).enumerated().map { i, o in
                    UNNotificationAction(identifier: "opt-\(i)", title: o, options: [])
                } + [UNTextInputNotificationAction(identifier: "reply", title: "Reply", options: [], textInputButtonTitle: "Send", textInputPlaceholder: "Your answer…")], intentIdentifiers: [])
                categories.insert(cat)
                center.setNotificationCategories(categories)
                c.categoryIdentifier = cat.identifier
            } else {
                c.categoryIdentifier = "message"
            }
        case "review":
            if let k = ev["key"].str, let p = state?.prs.first(where: { $0.fullKey == k || $0.key == k }), p.ready, ev["seen"]["digest"].str != nil { c.categoryIdentifier = "review" }
        default: break
        }
        let id = ev["group"].str ?? UUID().uuidString
        center.add(UNNotificationRequest(identifier: id, content: c, trigger: nil))
    }

    /// Withdraw what no longer needs you.
    func reconcile(_ state: HubState) {
        guard let center else { return }
        center.getDeliveredNotifications { notes in
            let stale = notes.compactMap { n -> String? in
                let u = n.request.content.userInfo
                let docket = u["docket"] as? String
                let d = docket.flatMap { state.byId[$0] }
                let due: Bool
                switch u["kind"] as? String {
                case "needs": due = d?.live?.state == .blocked || d?.live?.state == .error
                case "turn": due = ![LiveState.working, .starting, .blocked].contains(d?.live?.state ?? .idle)
                case "message": due = state.messages.contains { $0.id == (u["message"] as? NSNumber)?.intValue }
                case "review": due = state.prs.contains { ($0.fullKey == u["key"] as? String || $0.key == u["key"] as? String) && $0.attention != nil }
                case "build": due = state.task(for: docket ?? "")?.attention != nil
                default: due = true
                }
                return due ? nil : n.request.identifier
            }
            if !stale.isEmpty { center.removeDeliveredNotifications(withIdentifiers: stale) }
        }
    }

    // ---- the delegate ----

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler done: @escaping (UNNotificationPresentationOptions) -> Void) {
        // In the app already: into Notification Center, without a banner.
        Task { @MainActor in done(NSApp.isActive ? [.list] : [.banner, .list, .sound]) }
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler done: @escaping () -> Void) {
        let u = response.notification.request.content.userInfo
        let action = response.actionIdentifier
        let text = (response as? UNTextInputNotificationResponse)?.userText
        let docket = u["docket"] as? String
        let message = (u["message"] as? NSNumber)?.intValue
        let key = u["key"] as? String
        var seen: [String: String] = [:]
        for (k, v) in u { if let k = k as? String, k.hasPrefix("seen_"), let v = v as? String { seen[String(k.dropFirst(5))] = v } }
        Task { @MainActor in
            await self.respond(action: action, text: text, docket: docket, message: message, key: key, seen: seen)
            done()
        }
    }

    private func respond(action: String, text: String?, docket: String?, message: Int?, key: String?, seen: [String: String]) async {
        let store = Store.shared
        await store.refresh()
        let state = store.state
        switch action {
        case "approve", "deny":
            // The prompt the banner showed, not whatever waits now: the hub
            // refuses if they differ.
            if let docket, let record = seen["record"] {
                await store.answer(docket, record: record, since: seen["updated_at"], msg: seen["msg"], approve: action == "approve")
            }
        case "reply":
            guard let t = text?.trimmingCharacters(in: .whitespacesAndNewlines), !t.isEmpty else { return }
            if let id = message {
                if let m = state?.messages.first(where: { $0.id == id }) {
                    _ = await store.reply(m, t)
                } else {
                    store.toast("Message #\(id) was already answered; your reply wasn't sent", error: true)
                    Windows.showMain()
                }
            } else if let docket {
                _ = await store.send(docket, t)
            }
        case let a where a.hasPrefix("opt-"):
            if let id = message, let m = state?.messages.first(where: { $0.id == id }), let i = Int(a.dropFirst(4)), m.options.indices.contains(i) {
                _ = await store.reply(m, "", choice: m.options[i])
            }
        case "post":
            // The draft whose summary the banner showed; the hub refuses a changed one.
            if let p = state?.prs.first(where: { $0.fullKey == key || $0.key == key }), let digest = seen["digest"] { _ = await store.post(p, digest: digest) }
        default:
            // A click, or Preview: the docket, in the main window.
            Windows.showMain()
            if let docket { Nav.main.show(docket, store: store) }
        }
    }

    /// For Settings: post a sample, so you can see how they look.
    func test() {
        guard let center else { return }
        let c = UNMutableNotificationContent()
        c.title = "Wheelhouse"
        c.body = "Notifications are on. Prompts, messages, finished turns and reviews show up like this."
        if let pack = SoundPacks.shared.chosen {
            c.sound = nil
            SoundPacks.shared.preview(.needs, pack: pack)
        } else {
            c.sound = .default
        }
        center.add(UNNotificationRequest(identifier: "test", content: c, trigger: nil))
    }

    /// Whether the app can post them now: on in Settings, and allowed by macOS.
    func canPost() async -> Bool {
        guard enabled, center != nil, Snapshot.requested == nil else { return false }
        let s = await status()
        return s == .authorized || s == .provisional
    }

    func status() async -> UNAuthorizationStatus {
        guard let center else { return .notDetermined }
        return await center.notificationSettings().authorizationStatus
    }
}

/// Opening the app's windows from outside a view (a notification, the Dock).
@MainActor
enum Windows {
    static var openWindow: ((String) -> Void)?
    static var openDocket: ((String) -> Void)?

    /// The main window, to the front: reopened if it was closed.
    static func showMain() {
        NSApp.activate()
        openWindow?("main")
    }
}
