import AppKit
import SwiftUI

/// Wheelhouse.app: the hub's work, native on the Mac. The main window (a
/// sidebar, lists beside their dockets, an inspector), dockets in windows of
/// their own, a menu bar panel, and notifications you answer in place.
@main
struct WheelhouseApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @State private var store = Store.shared
    @State private var nav = Nav.main
    @AppStorage("menuBar") private var menuBar = true
    @AppStorage("theme") private var theme = "system"

    var body: some Scene {
        Window(Flavor.name, id: "main") {
            RootView()
                .environment(store)
                .environment(nav)
                .frame(minWidth: 960, minHeight: 580)
                .onAppear { store.start() }
                .onChange(of: theme, initial: true) { _, t in Appearance.apply(t) }
                .opener()
        }
        .defaultSize(width: 1400, height: 880)
        .commands { AppCommands(store: store, nav: nav) }

        WindowGroup("Docket", id: "docket", for: String.self) { $id in
            if let id {
                DocketWindow(id: id).environment(store).opener()
            }
        }
        .defaultSize(width: 1000, height: 840)

        // A docket's code changes (Changes.swift), in a window of its own.
        WindowGroup("Changes", id: "changes", for: String.self) { $id in
            if let id {
                ChangesWindow(id: id).environment(store)
            }
        }
        .defaultSize(width: 1280, height: 860)

        MenuBarExtra(isInserted: $menuBar) {
            MenuBarPanel().environment(store).environment(nav)
        } label: {
            MenuBarLabel().environment(store)
        }
        .menuBarExtraStyle(.window)

        Settings {
            SettingsView().environment(store)
        }
    }
}

extension View {
    /// Lets code outside views (a notification, the Dock) open windows.
    func opener() -> some View { modifier(Opener()) }
}

private struct Opener: ViewModifier {
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings
    func body(content: Content) -> some View {
        content.onAppear {
            Windows.openWindow = { openWindow(id: $0) }
            Windows.openDocket = { openWindow(id: "docket", value: $0) }
            Windows.openChanges = { openWindow(id: "changes", value: $0) }
            Windows.openSettings = { openSettings() }
        }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ n: Notification) {
        // Snapshots post nothing, so they never ask for notifications (a first
        // launch's prompt, on someone's screen).
        if Snapshot.requested == nil { Notifier.shared.setUp() }
        MarkdownKeys.install()
        if Snapshot.requested == nil { QuickPanel.apply() }
        Store.shared.start()
        Appearance.apply(UserDefaults.standard.string(forKey: "theme") ?? "system")
        if let dir = Snapshot.requested { Snapshot.run(into: dir) }
    }

    /// The Dock icon's menu: what needs you, and a new docket or agent.
    func applicationDockMenu(_ sender: NSApplication) -> NSMenu? {
        do {
            let menu = NSMenu()
            for it in Store.shared.inbox.prefix(6) {
                let item = NSMenuItem(title: "\(it.docketId)  \(it.title)", action: #selector(showItem(_:)), keyEquivalent: "")
                item.target = self
                item.representedObject = it.docketId
                menu.addItem(item)
            }
            if menu.numberOfItems > 0 { menu.addItem(.separator()) }
            for (title, sel) in [("New Docket", #selector(newDocket)), ("Start an Agent…", #selector(newAgent))] {
                let item = NSMenuItem(title: title, action: sel, keyEquivalent: "")
                item.target = self
                menu.addItem(item)
            }
            return menu
        }
    }

    @objc private func showItem(_ sender: NSMenuItem) {
        guard let id = sender.representedObject as? String else { return }
        Windows.showMain()
        Nav.main.show(id, store: Store.shared)
    }

    @objc private func newDocket() {
        Windows.showMain()
        Nav.main.creating = .blank
    }

    @objc private func newAgent() {
        Windows.showMain()
        Nav.main.creating = .agent
    }

    /// The menu bar keeps it running after its windows close.
    func applicationShouldTerminateAfterLastWindowClosed(_ app: NSApplication) -> Bool { false }

    /// A click on the Dock icon with no window open: the main window.
    func applicationShouldHandleReopen(_ app: NSApplication, hasVisibleWindows: Bool) -> Bool {
        if !hasVisibleWindows { Windows.showMain() }
        return true
    }
}

enum Appearance {
    /// System leaves it to macOS; Light and Dark pin it, for every window.
    @MainActor static func apply(_ theme: String) {
        NSApp?.appearance = theme == "light" ? NSAppearance(named: .aqua) : theme == "dark" ? NSAppearance(named: .darkAqua) : nil
    }
}

/// The menus: New, Go (⌘1–⌘8, as the sidebar's order), and Docket.
struct AppCommands: Commands {
    let store: Store
    let nav: Nav
    @FocusedValue(\.docket) private var focused

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New Docket") { Windows.showMain(); nav.creating = .blank }.keyboardShortcut("n")
            Button("Start an Agent…") { Windows.showMain(); nav.creating = .agent }.keyboardShortcut("n", modifiers: [.command, .shift])
            Button("New Docket from Linear…") { Windows.showMain(); nav.creating = .linear }
            Button("Add a Call's Transcript…") { Windows.showMain(); nav.newCall(about: store.docket(focused?.id ?? nav.current)) }
        }
        CommandMenu("Go") {
            Button("Search…") { Windows.showMain(); nav.palette.toggle() }.keyboardShortcut("k")
            Divider()
            ForEach(Array(Place.order.enumerated()), id: \.offset) { i, p in
                Button(p.0.title) { Windows.showMain(); nav.go(p.0) }.keyboardShortcut(KeyEquivalent(Character("\(i + 1)")))
            }
            Divider()
            Button("Refresh") { store.reload() }.keyboardShortcut("r")
        }
        CommandMenu("Docket") {
            // The docket in the window you're in (the main one, or its own).
            let here = focused?.nav ?? nav
            let d = store.docket(focused?.id ?? nav.current)
            Button(d.map { "Open in \($0.runtime)" } ?? "Open in Rex") { if let d { store.openInRex(d.id) } }.keyboardShortcut("o", modifiers: [.command, .shift]).disabled(d?.live == nil)
            Button("Peek at Pane") { if let d { here.peek(d.id) } }.keyboardShortcut("p", modifiers: [.command, .shift]).disabled(d?.live == nil)
            Button("Read the Record") { if let d { here.record = d.id } }.keyboardShortcut("r", modifiers: [.command, .shift]).disabled(d == nil)
            Button("View Changes") { if let d { Windows.openChanges?(d.id) } }.keyboardShortcut("d", modifiers: [.command, .shift]).disabled(d == nil || d?.sharedFrom != nil)
            Button("Link a Linear Issue…") { if let d { here.linking = d.id } }.disabled(d == nil)
            Button("Add a Call…") { if let d { here.addingCall = d.id } }.disabled(d?.callable != true)
            Divider()
            Menu("Status") {
                ForEach(Array(Status.allCases.enumerated()), id: \.offset) { i, s in
                    Button(s.label) { if let d, d.status != s { store.setStatus(d.id, s, undo: d.status) } }
                        .keyboardShortcut(KeyEquivalent(Character("\(i + 1)")), modifiers: [.command, .option])
                }
            }
            .disabled(d == nil)
            Divider()
            Button("Run Checks") { if let d { Task { await store.runChecks(d.id) } } }
                .keyboardShortcut("u", modifiers: [.command, .shift]).disabled(d == nil)
            if let d, d.hold != nil {
                Button("Resume") { store.resume(d.id) }.keyboardShortcut("h", modifiers: [.command, .shift])
            } else {
                Button("Hold…") { if let d { here.holding = d.id } }.keyboardShortcut("h", modifiers: [.command, .shift]).disabled(d == nil)
            }
            Button(d?.onAgents.isEmpty == false ? "Add an Agent…" : "Start an Agent…") { if let d { here.starting = d.id } }
                .keyboardShortcut("a", modifiers: [.command, .shift]).disabled(d?.canStartAgent != true)
            Divider()
            Button("Verify Every Record") { store.verifyAll() }
        }
        InspectorCommands()
        CommandGroup(after: .help) {
            Button("Keyboard Shortcuts") { Windows.showMain(); nav.shortcuts = true }.keyboardShortcut("/")
        }
    }
}
