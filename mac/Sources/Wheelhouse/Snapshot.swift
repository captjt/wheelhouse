import AppKit
import SwiftUI

/// `Wheelhouse --snapshot DIR`: render the app's places, light and dark,
/// into DIR as PNGs, then quit. scripts/screenshots.sh runs it against the
/// made-up world, for the README. Windows stay behind yours, and the app
/// never takes the keyboard.
@MainActor
enum Snapshot {
    static var requested: URL? {
        let a = CommandLine.arguments
        guard let i = a.firstIndex(of: "--snapshot"), i + 1 < a.count else { return nil }
        return URL(fileURLWithPath: a[i + 1], isDirectory: true)
    }

    /// Only these shots (`--only docket,inbox`), to iterate on one.
    static var only: Set<String>? {
        let a = CommandLine.arguments
        guard let i = a.firstIndex(of: "--only"), i + 1 < a.count else { return nil }
        return Set(a[i + 1].split(separator: ",").map(String.init))
    }

    enum Target { case main, sheet, docketWindow, changesWindow, menuBar, quick, settings, settingsSheet, fixture, markup, compare }

    struct Shot {
        let name: String
        let place: Place
        var select: String?
        var target = Target.main
        /// The Settings tab, for a settings shot.
        var tab: String?
        var setup: (Nav) -> Void = { _ in }
    }

    /// The docket the envelope shot opens (`WH_SNAP_DOCKET`, for a long record).
    static let snapDocket = ProcessInfo.processInfo.environment["WH_SNAP_DOCKET"] ?? "D-7"
    /// A docket whose agent runs in Herdr (`WH_SNAP_HERDR`): the made-up world's acme-web.
    static let snapHerdr = ProcessInfo.processInfo.environment["WH_SNAP_HERDR"] ?? "D-8"
    /// A docket whose code is on this Mac, for its Changes window (`WH_SNAP_CHANGES`).
    static let snapChanges = ProcessInfo.processInfo.environment["WH_SNAP_CHANGES"] ?? "D-6"
    /// A docket with a call on it (`WH_SNAP_CALL`): the made-up world's
    /// "Retry failed webhook deliveries", which keeps "Webhooks sync".
    static let snapCall = ProcessInfo.processInfo.environment["WH_SNAP_CALL"] ?? "D-6"

    static let shots: [Shot] = [
        Shot(name: "docket", place: .active, select: "D-6"),
        Shot(name: "inbox", place: .inbox),
        // A workspace's shared dockets (docs/teams.md): only against a hub in one.
        Shot(name: "team", place: .team, select: "ACME-1"),
        // Rex and Herdr side by side: every agent, and a docket whose agent is in Herdr.
        Shot(name: "agents", place: .agents, select: snapHerdr),
        Shot(name: "docket-herdr", place: .active, select: snapHerdr),
        Shot(name: "chat", place: .active, select: "D-6") { _ in NotificationCenter.default.post(name: .snapshotTab, object: "chat") },
        Shot(name: "reviews", place: .reviews, select: "D-7"),
        Shot(name: "board", place: .all),
        Shot(name: "recap", place: .recap),
        // A docket's Calls section, and adding another call to it.
        Shot(name: "call", place: .all, select: snapCall) { _ in scroll("calls") },
        Shot(name: "call-add", place: .all, select: snapCall, target: .sheet) { nav in nav.addingCall = snapCall },
        Shot(name: "palette", place: .inbox) { nav in nav.palette = true },
        Shot(name: "record", place: .active, select: "D-6", target: .sheet) { nav in nav.record = "D-6" },
        // A checkable link's signed envelope, open: it once hung the app.
        Shot(name: "envelope", place: .all, select: snapDocket, target: .sheet) { nav in
            nav.record = snapDocket
            Task {
                try? await Task.sleep(for: .milliseconds(500))
                NotificationCenter.default.post(name: .snapshotEnvelope, object: "checkable")
            }
        },
        Shot(name: "agent", place: .inbox, target: .sheet) { nav in nav.creating = .agent },
        Shot(name: "window", place: .inbox, target: .docketWindow),
        // A docket's code changes: the hub's page, in the app's web view.
        Shot(name: "docket-changes", place: .all, select: snapChanges),
        Shot(name: "changes", place: .all, select: snapChanges, target: .changesWindow),
        Shot(name: "menubar", place: .inbox, target: .menuBar),
        Shot(name: "quick", place: .inbox, target: .quick),
        Shot(name: "shortcuts", place: .inbox, target: .sheet) { nav in nav.shortcuts = true },
        // Verify: the docket's section, a check's output, hold, and done refused.
        Shot(name: "verify", place: .active, select: "D-6") { _ in scroll("verify") },
        Shot(name: "verify-held", place: .active, select: "D-3") { _ in scroll("verify") },
        Shot(name: "output", place: .active, select: "D-6", target: .sheet) { _ in
            scroll("verify")
            Task {
                try? await Task.sleep(for: .milliseconds(400))
                NotificationCenter.default.post(name: .snapshotOutput, object: "lint")
            }
        },
        Shot(name: "hold", place: .active, select: "D-6", target: .sheet) { nav in nav.holding = "D-6" },
        Shot(name: "start-agent", place: .active, select: "D-6", target: .sheet) { nav in nav.starting = "D-6" },
        // The real refusal: D-1 has acceptance items and nothing behind them.
        Shot(name: "closing", place: .active, select: "D-1", target: .sheet) { nav in
            Task {
                if let p = try? await Store.shared.page("D-1"), p.evidence.needs, p.evidence.lines.isEmpty, p.docket.status != .done {
                    Store.shared.setStatus("D-1", .done)
                } else {
                    nav.closing = Closing(id: "D-1", why: "", from: .in_progress)
                }
            }
        },
        Shot(name: "verify-fixture", place: .inbox, target: .fixture),
        // Images shared with the agents: in the Activity, marked up, and compared.
        Shot(name: "images", place: .active, select: "D-6") { _ in scroll("activity") },
        Shot(name: "markup", place: .inbox, target: .markup),
        Shot(name: "compare", place: .inbox, target: .compare),
        // Settings: each tab of what Wheelhouse does on its own.
        Shot(name: "settings-orders", place: .inbox, target: .settings, tab: "orders"),
        Shot(name: "settings-agents", place: .inbox, target: .settings, tab: "agents"),
        Shot(name: "settings-repos", place: .inbox, target: .settings, tab: "repos"),
        Shot(name: "settings-check", place: .inbox, target: .settingsSheet, tab: "repos") { _ in
            Task {
                try? await Task.sleep(for: .milliseconds(600))
                NotificationCenter.default.post(name: .snapshotCheck, object: "lint")
            }
        },
        // Who posts a checked review: a repo's own, and the rule's editor.
        Shot(name: "settings-posting", place: .inbox, target: .settings, tab: "repos") { _ in Snapshot.scroll("posting") },
        Shot(name: "settings-rule", place: .inbox, target: .settingsSheet, tab: "orders") { _ in
            Task {
                try? await Task.sleep(for: .milliseconds(600))
                NotificationCenter.default.post(name: .snapshotRule, object: "")
            }
        },
        Shot(name: "settings-guides", place: .inbox, target: .settings, tab: "guides") { _ in UserDefaults.standard.set("guide/plan-format.md", forKey: "settingsGuide") },
        Shot(name: "settings-brief", place: .inbox, target: .settings, tab: "guides") { _ in
            UserDefaults.standard.set("build/brief/plan.md", forKey: "settingsGuide")
            Task {
                try? await Task.sleep(for: .milliseconds(900))
                NotificationCenter.default.post(name: .snapshotGuide, object: "{{PLAN_GUIDE}}")
            }
        },
        Shot(name: "settings-general", place: .inbox, target: .settings, tab: "general"),
        // Sound: the macOS sound, a pack picked, and the packs to pick from
        // (WH_SOUND_PACKS and WH_SOUND_REGISTRY point them at made-up ones).
        Shot(name: "settings-notifications", place: .inbox, target: .settings, tab: "notifications") { _ in UserDefaults.standard.set("", forKey: "soundPack") },
        Shot(name: "settings-sound", place: .inbox, target: .settings, tab: "notifications") { _ in
            UserDefaults.standard.set(SoundPacks.shared.installed.first?.name ?? "", forKey: "soundPack")
        },
        Shot(name: "settings-packs", place: .inbox, target: .settingsSheet, tab: "notifications") { _ in
            Task {
                try? await Task.sleep(for: .milliseconds(600))
                NotificationCenter.default.post(name: .snapshotSounds, object: "")
            }
        },
        Shot(name: "settings-packs-search", place: .inbox, target: .settingsSheet, tab: "notifications") { _ in
            UserDefaults.standard.set(SoundPacks.shared.installed.first?.name ?? "", forKey: "soundPack")
            Task {
                try? await Task.sleep(for: .milliseconds(600))
                NotificationCenter.default.post(name: .snapshotSounds, object: "peon")
            }
        },
        // Refused (nothing is written): the hub's sentence, under the builder.
        Shot(name: "settings-error", place: .inbox, target: .settings, tab: "agents") { _ in
            Task {
                try? await Task.sleep(for: .milliseconds(600))
                NotificationCenter.default.post(name: .snapshotPatch, object: ("build.builder", SettingsPatch.make("build", ["builder"], ["agent": "gpt"])))
            }
        },
    ]

    /// Scroll the docket to a section, once it's there.
    static func scroll(_ id: String) {
        Task {
            try? await Task.sleep(for: .milliseconds(500))
            NotificationCenter.default.post(name: .snapshotScroll, object: id)
        }
    }

    static var settingsWindow: NSWindow? {
        NSApp.windows.first { $0.isVisible && ($0.identifier?.rawValue ?? "").localizedCaseInsensitiveContains("settings") }
    }

    /// Verify's rows with what the made-up world lacks: verdicts, a timed-out
    /// check, a close on your word. D-6's page, with those laid in.
    static func fixture() async -> NSWindow? {
        guard let live = try? await Store.shared.hub.get("/api/dockets/D-6"), var page = live.object, var ev = live["evidence"].object else { return nil }
        var checks = live["evidence"]["checks"].array
        if var e2e = checks.first?.object {
            e2e["name"] = .string("e2e")
            e2e["command"] = .string("pnpm e2e --project chromium --grep 'webhook retries' --reporter line --workers 2")
            e2e["exit"] = .null
            e2e["ok"] = .bool(false)
            e2e["required"] = .bool(true)
            e2e["dirty"] = .bool(true)
            e2e["ms"] = .number(1_200_000)
            checks.append(.object(e2e))
        }
        ev["checks"] = .array(checks)
        ev["verdicts"] = .array([
            .object(["seq": .number(20), "at": live["evidence"]["checks"][0]["at"], "n": .number(1), "verdict": .string("fail"), "findings": .number(2), "summary": .string("Retries don't log the attempt number, and `deliver.ts` retries a 409, which can't succeed."), "by": .string("claude-code@s-webhooks"), "role": .string("checker"), "independent": .bool(false), "head": .string("30297c98"), "mark": .string("said")]),
            .object(["seq": .number(24), "at": live["evidence"]["checks"][1]["at"], "n": .number(2), "verdict": .string("pass"), "findings": .array([]), "summary": .string("Each retry logs its attempt; 4xx responses fail at once. Matches the intent and the plan."), "by": .string("codex@s-check"), "role": .string("checker"), "independent": .bool(true), "head": .string("30297c98"), "mark": .string("said")]),
        ])
        ev["closed_on"] = .string("Covered by the wiki page; the tests run in CI there")
        page["evidence"] = .object(ev)
        let p = DocketPage(.object(page))
        let view = ScrollView {
            VerifySection(page: p).padding(24).frame(width: 760)
        }
        .frame(width: 808, height: 560)
        .background(K.main)
        .environment(Store.shared)
        .environment(Nav())
        let w = NSWindow(contentRect: NSRect(x: 80, y: 80, width: 808, height: 560), styleMask: [.titled], backing: .buffered, defer: false)
        w.title = "Verify"
        w.isReleasedWhenClosed = false
        w.contentView = NSHostingView(rootView: view)
        w.orderBack(nil)
        return w
    }

    /// D-6's images, in a window of their own: the markup editor on the
    /// newest reference with a few marks, or Compare on the newest pair.
    static func imagesFixture(_ t: Target) async -> NSWindow? {
        guard let page = try? await Store.shared.page("D-6") else { return nil }
        let all = DocketImage.all(page.entries)
        let view: AnyView
        let size: NSSize
        switch t {
        case .compare:
            guard let shot = all.first(where: { $0.against != nil }), let ref = all.first(where: { $0.seq == shot.against }) else { return nil }
            view = AnyView(CompareView(left: shot, right: ref))
            size = NSSize(width: 1200, height: 760)
        default:
            guard let ref = all.first(where: { $0.role != "attached" }), let data = try? await ImageCache.shared.data(ref.digest) else { return nil }
            let c = ImageMarks.defaultColor
            let marks = [
                MarkupMark(tool: .box, color: c, points: [MarkupPoint(CGPoint(x: 280, y: 95)), MarkupPoint(CGPoint(x: 720, y: 185))]),
                MarkupMark(tool: .arrow, color: ImageMarks.colors[4], points: [MarkupPoint(CGPoint(x: 160, y: 520)), MarkupPoint(CGPoint(x: 150, y: 390))]),
                MarkupMark(tool: .text, color: c, points: [MarkupPoint(CGPoint(x: 300, y: 300))], text: "Too much space"),
                MarkupMark(tool: .pin, color: c, points: [MarkupPoint(CGPoint(x: 720, y: 95))], text: "Make this the primary action", n: 1),
                MarkupMark(tool: .pin, color: c, points: [MarkupPoint(CGPoint(x: 260, y: 210))], text: "Less padding here", n: 2),
            ]
            view = AnyView(MarkupSheet(data: data, name: ref.name, initial: marks) { _ in })
            size = NSSize(width: 1180, height: 780)
        }
        let w = NSWindow(contentRect: NSRect(origin: CGPoint(x: 80, y: 80), size: size), styleMask: [.titled], backing: .buffered, defer: false)
        w.title = t == .compare ? "Compare" : "Mark Up"
        w.isReleasedWhenClosed = false
        w.contentView = NSHostingView(rootView: view.environment(Store.shared).environment(Nav()).environment(\.docketImages, all))
        w.orderBack(nil)
        try? await Task.sleep(for: .seconds(1.5))
        return w
    }

    static func run(into dir: URL) {
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        Task {
            let store = Store.shared
            for _ in 0 ..< 100 where store.state == nil { try? await Task.sleep(for: .milliseconds(200)) }
            try? await Task.sleep(for: .seconds(1))
            for theme in ["light", "dark"] {
                Appearance.apply(theme)
                for shot in shots where only?.contains(shot.name) ?? true {
                    let nav = Nav.main
                    reset(nav)
                    UserDefaults.standard.set(shot.name == "board" ? "board" : "list", forKey: "layout:\(shot.place)")
                    nav.section = shot.place
                    if let s = shot.select { nav.select(s, in: shot.place) }
                    try? await Task.sleep(for: .seconds(1.2))
                    if shot.target == .docketWindow { Windows.openDocket?("D-6") }
                    if shot.target == .changesWindow { Windows.openChanges?(snapChanges) }
                    if shot.target == .menuBar { clickStatusItem() }
                    if shot.target == .quick { QuickPanel.show(key: false) }
                    if let tab = shot.tab {
                        UserDefaults.standard.set(tab, forKey: "settingsTab")
                        if settingsWindow == nil { Windows.openSettings?() }
                    }
                    var fixtureWindow: NSWindow?
                    if shot.target == .fixture { fixtureWindow = await fixture() }
                    if shot.target == .markup || shot.target == .compare { fixtureWindow = await imagesFixture(shot.target) }
                    shot.setup(nav)
                    try? await Task.sleep(for: .seconds(1.8))
                    guard let main = mainWindow else {
                        print("no main window among \(NSApp.windows.map { "\($0.identifier?.rawValue ?? "-") \($0.title)" })")
                        continue
                    }
                    if main.frame.size != NSSize(width: 1440, height: 900) {
                        main.setFrame(NSRect(x: main.frame.minX, y: main.frame.minY, width: 1440, height: 900), display: true)
                        try? await Task.sleep(for: .seconds(1))
                    }
                    let w: NSWindow?
                    switch shot.target {
                    case .main: w = main
                    case .sheet: w = main.attachedSheet
                    case .docketWindow: w = NSApp.windows.first { $0.isVisible && $0 != main && $0.title.hasPrefix("D-6") }
                    case .changesWindow:
                        w = NSApp.windows.first { $0.isVisible && $0 != main && $0.title.hasPrefix("\(snapChanges) · Changes") }
                        // The page loads its highlighter on first open: a moment more.
                        if let w, w.frame.size != NSSize(width: 1440, height: 900) { w.setFrame(NSRect(x: w.frame.minX, y: w.frame.minY, width: 1440, height: 900), display: true) }
                        try? await Task.sleep(for: .seconds(2.5))
                    case .menuBar: w = NSApp.windows.first { $0.isVisible && $0 != main && abs($0.frame.width - 380) < 2 }
                    case .quick: w = QuickPanel.window
                    case .settings: w = settingsWindow
                    case .settingsSheet: w = settingsWindow?.attachedSheet
                    case .fixture, .markup, .compare: w = fixtureWindow
                    }
                    if let w {
                        if shot.target != .menuBar && shot.target != .quick { w.orderBack(nil) }
                        capture(w, to: dir.appendingPathComponent("mac-\(shot.name)-\(theme).png"))
                    } else {
                        print("no window for \(shot.name) among \(NSApp.windows.filter(\.isVisible).map { "\($0.identifier?.rawValue ?? "-") \($0.title)" })")
                    }
                    fixtureWindow?.close()
                    if shot.target == .docketWindow { NSApp.windows.first { $0.title.hasPrefix("D-6") }?.close() }
                    if shot.target == .changesWindow { NSApp.windows.first { $0.title.hasPrefix("\(snapChanges) · Changes") }?.close() }
                    if shot.target == .menuBar { clickStatusItem() }
                    if shot.target == .quick { QuickPanel.close() }
                    reset(nav)
                    try? await Task.sleep(for: .seconds(0.6))
                }
            }
            NSApp.terminate(nil)
        }
    }

    /// The main window: SwiftUI names it after its scene ("main").
    static var mainWindow: NSWindow? {
        NSApp.windows.first { $0.isVisible && ($0.identifier?.rawValue ?? "").hasPrefix("main") }
            ?? NSApp.windows.filter { $0.isVisible && !$0.title.hasPrefix("D-") }.max { $0.frame.width < $1.frame.width }
    }

    private static func reset(_ nav: Nav) {
        nav.palette = false
        nav.record = nil
        nav.creating = nil
        nav.shortcuts = false
        nav.holding = nil
        nav.starting = nil
        nav.closing = nil
        nav.addingCall = nil
        nav.callOn = nil
        NotificationCenter.default.post(name: .snapshotTab, object: "docket")
        NotificationCenter.default.post(name: .snapshotOutput, object: nil)
        NotificationCenter.default.post(name: .snapshotCheck, object: nil)
        NotificationCenter.default.post(name: .snapshotGuide, object: "")
        NotificationCenter.default.post(name: .snapshotPatch, object: nil)
        NotificationCenter.default.post(name: .snapshotSounds, object: nil)
    }

    /// The menu bar item, clicked: its panel opens (or closes).
    static func clickStatusItem() {
        for w in NSApp.windows where w.className.contains("StatusBar") {
            func find(_ v: NSView) -> NSButton? { (v as? NSButton) ?? v.subviews.lazy.compactMap(find).first }
            if let v = w.contentView, let b = find(v) {
                b.performClick(nil)
                return
            }
        }
    }

    /// The window as the window server draws it (glass and all).
    static func capture(_ w: NSWindow, to url: URL) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        p.arguments = ["-x", "-o", "-l\(w.windowNumber)", url.path]
        try? p.run()
        p.waitUntilExit()
        if p.terminationStatus == 0 { print("wrote \(url.path)") }
    }
}

extension Notification.Name {
    static let snapshotTab = Notification.Name("wheelhouse.snapshotTab")
    static let snapshotEnvelope = Notification.Name("wheelhouse.snapshotEnvelope")
    static let snapshotEnvelopeRow = Notification.Name("wheelhouse.snapshotEnvelopeRow")
    static let snapshotPatch = Notification.Name("wheelhouse.snapshotPatch")
}
