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

    enum Target { case main, sheet, docketWindow, menuBar }

    struct Shot {
        let name: String
        let place: Place
        var select: String?
        var target = Target.main
        var setup: (Nav) -> Void = { _ in }
    }

    static let shots: [Shot] = [
        Shot(name: "docket", place: .active, select: "D-6"),
        Shot(name: "inbox", place: .inbox),
        Shot(name: "chat", place: .active, select: "D-6") { _ in NotificationCenter.default.post(name: .snapshotTab, object: "chat") },
        Shot(name: "reviews", place: .reviews, select: "D-7"),
        Shot(name: "board", place: .all),
        Shot(name: "recap", place: .recap),
        Shot(name: "palette", place: .inbox) { nav in nav.palette = true },
        Shot(name: "record", place: .active, select: "D-6", target: .sheet) { nav in nav.record = "D-6" },
        Shot(name: "agent", place: .inbox, target: .sheet) { nav in nav.creating = .agent },
        Shot(name: "window", place: .inbox, target: .docketWindow),
        Shot(name: "menubar", place: .inbox, target: .menuBar),
    ]

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
                    if shot.target == .menuBar { clickStatusItem() }
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
                    case .menuBar: w = NSApp.windows.first { $0.isVisible && $0 != main && abs($0.frame.width - 380) < 2 }
                    }
                    if let w {
                        if shot.target != .menuBar { w.orderBack(nil) }
                        capture(w, to: dir.appendingPathComponent("mac-\(shot.name)-\(theme).png"))
                    } else {
                        print("no window for \(shot.name)")
                    }
                    if shot.target == .docketWindow { NSApp.windows.first { $0.title.hasPrefix("D-6") }?.close() }
                    if shot.target == .menuBar { clickStatusItem() }
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
        NotificationCenter.default.post(name: .snapshotTab, object: "docket")
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
}
