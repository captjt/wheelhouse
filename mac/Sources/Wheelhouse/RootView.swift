import SwiftUI

/// The main window: the sidebar, and the place it shows.
struct RootView: View {
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @Environment(\.openWindow) private var openWindow
    @State private var pendingG = false

    var body: some View {
        @Bindable var nav = nav
        NavigationSplitView {
            Sidebar()
                .navigationSplitViewColumnWidth(min: 200, ideal: 228, max: 320)
        } detail: {
            Group {
                if store.hubDown {
                    HubDownView()
                } else {
                    switch nav.section {
                    case .inbox: InboxScreen()
                    case .reviews: ReviewsScreen()
                    case .builds: BuildsScreen()
                    case .agents: AgentsScreen()
                    case .recap: RecapScreen()
                    default: DocketsScreen(section: nav.section)
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(K.main)
        }
        .overlay(alignment: .top) {
            if nav.palette {
                PaletteOverlay()
            }
        }
        .overlay(alignment: .bottom) { ToastStack() }
        .sheet(item: $nav.creating) { mode in NewDocketSheet(mode: mode) }
        .sheet(isPresented: $nav.shortcuts) { ShortcutsSheet() }
        .sheet(item: Binding(get: { nav.pane.map(IdBox.init) }, set: { nav.pane = $0?.id })) { b in PaneSheet(id: b.id) }
        .sheet(item: Binding(get: { nav.record.map(IdBox.init) }, set: { nav.record = $0?.id })) { b in RecordSheet(id: b.id) }
        .sheet(item: Binding(get: { nav.linking.map(IdBox.init) }, set: { nav.linking = $0?.id })) { b in LinkIssueSheet(docket: b.id) }
        .sheet(item: Binding(get: { nav.holding.map(IdBox.init) }, set: { nav.holding = $0?.id })) { b in HoldSheet(id: b.id) }
        .sheet(item: Binding(get: { nav.starting.map(IdBox.init) }, set: { nav.starting = $0?.id })) { b in StartAgentSheet(id: b.id) }
        .sheet(item: $nav.closing) { c in CloseOnWordSheet(closing: c) }
        .background(WindowReader { Windows.register($0, nav) })
        // G, then a letter: go there, as in the web app.
        .onKeyPress(phases: .down) { press in
            guard press.modifiers.isEmpty, !nav.palette, nav.creating == nil, !typing() else { return .ignored }
            if pendingG {
                pendingG = false
                if let s = Place.order.first(where: { $0.1 == press.characters })?.0 {
                    nav.go(s)
                    return .handled
                }
                return .ignored
            }
            switch press.characters {
            case "g":
                pendingG = true
                Chord.gAt = .now
                Task { try? await Task.sleep(for: .milliseconds(900)); pendingG = false }
                return .handled
            case "/":
                nav.palette = true
                return .handled
            case "c":
                nav.creating = .blank
                return .handled
            default:
                return .ignored
            }
        }
        .navigationTitle(nav.section.title)
        .onOpenURL { url in open(url) }
        .environment(\.openURL, OpenURLAction { url in
            if url.scheme == "wheelhouse" {
                open(url)
                return .handled
            }
            return .systemAction
        })
    }

    /// `wheelhouse://docket/D-12`: show it.
    private func open(_ url: URL) {
        guard url.scheme == "wheelhouse" else { return }
        let parts = ([url.host] + url.pathComponents).compactMap { $0 }.filter { $0 != "/" }
        if parts.first == "docket", let id = parts.dropFirst().first {
            nav.show(id, store: store)
        } else if let s = Place.order.first(where: { "\($0.0)" == parts.first })?.0 {
            nav.go(s)
        }
    }
}

struct IdBox: Identifiable {
    let id: String
}

// ---- the sidebar ----

struct Sidebar: View {
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav

    private var sessions: [(String, Int)] {
        var m: [String: Int] = [:]
        for d in store.dockets where d.isOpen { if let s = d.agent?.rexSession, !s.isEmpty { m[s, default: 0] += 1 } }
        return m.sorted { $0.key < $1.key }.map { ($0.key, $0.value) }
    }

    var body: some View {
        @Bindable var nav = nav
        let s = store.state
        let reviewAlerts = s?.prs.filter { $0.attention != nil }.count ?? 0
        let reviewCount = s?.prs.filter { $0.state == "queued" || $0.state == "reviewing" }.count ?? 0
        let buildAlerts = s?.tasks.filter { $0.attention != nil }.count ?? 0
        let buildCount = s?.tasks.filter { $0.state == "queued" || $0.state == "active" }.count ?? 0
        List(selection: Binding<Place?>(get: { nav.section }, set: { if let v = $0 { nav.section = v } })) {
            Section {
                row(.inbox, count: store.inbox.count, alert: store.urgent > 0)
                row(.reviews, count: reviewAlerts > 0 ? reviewAlerts : reviewCount, alert: reviewAlerts > 0)
                row(.builds, count: buildAlerts > 0 ? buildAlerts : buildCount, alert: buildAlerts > 0)
                row(.active, count: store.dockets.filter(\.isOpen).count)
                row(.all)
                row(.done)
                row(.agents, count: s?.agents.count ?? 0)
                row(.recap)
            }
            if !sessions.isEmpty {
                Section("Rex sessions") {
                    ForEach(sessions, id: \.0) { name, n in
                        Label { Text(name) } icon: { SessionIcon(name: name, size: 16) }
                            .badge(n)
                            .tag(Place.session(name))
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom, spacing: 0) { SidebarFoot() }
        .toolbar {
            ToolbarItem {
                Button { nav.palette = true } label: { Label("Search", systemImage: "magnifyingglass") }
                    .help("Search dockets and commands (⌘K)")
            }
            ToolbarItem {
                Button { nav.creating = .blank } label: { Label("New docket", systemImage: "square.and.pencil") }
                    .help("New docket (⌘N)")
            }
        }
    }

    @ViewBuilder
    private func row(_ s: Place, count: Int = 0, alert: Bool = false) -> some View {
        Label(s.title, systemImage: s.symbol)
            .badge(count > 0 ? Text("\(count)").foregroundStyle(alert ? K.blocked : .secondary).fontWeight(alert ? .semibold : .regular) : nil)
            .tag(s)
    }
}

/// Under the sidebar: hints for what's off, and the hub's connection.
struct SidebarFoot: View {
    @Environment(Store.self) private var store
    @AppStorage("theme") private var theme = "system"

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let s = store.state {
                if !s.hooks { hint("Hooks off", "wh hooks install", "Claude Code's hooks are off, so dockets are named after their panes and turns carry no message.", symbol: "link") }
                if !s.linear { hint("Linear off", "wh setup-linear", "Connect Linear to start dockets from issues and link issues to dockets.", symbol: "circle.hexagongrid") }
            }
            HStack(spacing: 6) {
                Circle().fill(dotColor).frame(width: 7, height: 7)
                Text(connection).font(.system(size: 11.5)).foregroundStyle(.secondary).lineLimit(1)
                Spacer()
                Picker("Theme", selection: $theme) {
                    Image(systemName: "circle.lefthalf.filled").tag("system").help("System: follow macOS")
                    Image(systemName: "sun.max").tag("light").help("Light")
                    Image(systemName: "moon").tag("dark").help("Dark")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .controlSize(.mini)
                .frame(width: 84)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    private var dotColor: Color {
        guard let s = store.state else { return store.error == nil ? K.faint : K.error }
        return s.rexConnected && store.live ? K.working : K.blocked
    }

    private var connection: String {
        guard let s = store.state else { return store.error == nil ? "Connecting…" : "Hub unreachable" }
        if !store.live { return store.wasLive ? "Reconnecting to the hub…" : "Connecting…" }
        return s.rexConnected ? "Following Rex" : "Rex: \(s.rexError ?? "connecting")"
    }

    private func hint(_ title: String, _ command: String, _ why: String, symbol: String) -> some View {
        Button {
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(command, forType: .string)
            store.toast("Copied “\(command)”. Run it in a terminal.")
        } label: {
            HStack(spacing: 6) {
                Image(systemName: symbol).frame(width: 14)
                Text(title)
                Spacer()
                Text("Turn on").foregroundStyle(K.accent)
            }
            .font(.system(size: 11.5))
            .foregroundStyle(.secondary)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help("\(why) Click to copy the command: \(command)")
    }
}

/// The hub isn't answering: say so, and offer to start it.
struct HubDownView: View {
    @Environment(Store.self) private var store
    var body: some View {
        ContentUnavailableView {
            Label("The hub isn't running", systemImage: "bolt.horizontal.circle")
        } description: {
            Text("Wheelhouse.app shows what the hub (wh serve) keeps. Start it here, or with `wh up` in a terminal. It runs at login from then on.")
        } actions: {
            Button(store.startingHub ? "Starting…" : "Start the hub") { store.startHub() }
                .buttonStyle(.borderedProminent)
                .disabled(store.startingHub)
            SettingsLink { Text("Use another hub…") }
        }
    }
}

/// Toasts: what just happened, with Undo where it can be undone.
struct ToastStack: View {
    @Environment(Store.self) private var store
    var body: some View {
        VStack(spacing: 6) {
            ForEach(store.toasts) { t in
                HStack(spacing: 10) {
                    if t.error { Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(K.error) }
                    Text(t.text).font(.system(size: 12.5)).lineLimit(3)
                    if let a = t.action {
                        Button(a.label) {
                            a.run()
                            store.dismiss(t)
                        }
                        .buttonStyle(.borderless)
                        .foregroundStyle(K.accent)
                        .fontWeight(.semibold)
                    }
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 9)
                .background(.regularMaterial, in: Capsule())
                .overlay(Capsule().strokeBorder(K.line))
                .shadow(color: .black.opacity(0.15), radius: 12, y: 4)
                .transition(.move(edge: .bottom).combined(with: .opacity))
                .onTapGesture { store.dismiss(t) }
            }
        }
        .padding(.bottom, 18)
        .animation(.spring(duration: 0.3), value: store.toasts.map(\.id))
    }
}

/// Every key, in one place (⌘/).
struct ShortcutsSheet: View {
    @Environment(\.dismiss) private var dismiss

    private let groups: [(String, [(String, String)])] = [
        ("Anywhere", [("⌥⌘K", "Wheelhouse over any app: approve, answer, find a docket, start one")]),
        ("Go", [("⌘K  or  /", "Search dockets and commands"), ("⌘1 … ⌘8", "Inbox, Reviews, Builds, Active, All, Done, Agents, Recap"), ("G, then I R B A D X N P", "The same, as in the web app"), ("⌘R", "Refresh"), ("⌘,", "Settings: standing orders, agents, repos, guides")]),
        ("Create", [("⌘N  or  C", "New docket"), ("⇧⌘N", "Start an agent on a new docket")]),
        ("A list", [("J K  or  ↑ ↓", "Move"), ("↩  or  double-click", "Open the docket in its own window"), ("O", "Show the agent's pane in Rex"), ("S, then 1–4", "Change status"), ("E", "In the Inbox: done"), ("Swipe", "Done, Drop, Status; Approve, Deny, File in the Inbox")]),
        ("A docket", [("⇧⌘O", "Open in Rex"), ("⇧⌘P", "Peek at the pane"), ("⇧⌘R", "Read the record"), ("⌥⌘1 … ⌥⌘4", "To do, In progress, Done, Dropped"), ("⇧⌘U", "Run its checks"), ("⇧⌘H", "Hold, or resume"), ("⇧⌘A", "Start an agent on it"), ("Double-click the title", "Rename"), ("Drop a file", "Keep it with the docket, pinned by its hash")]),
        ("Writing", [("⌘↩", "Send, save, or note"), ("↩  /  ⇧↩", "In Chat: send / a new line"), ("Esc", "Cancel"), ("⌘B  ⌘I", "Bold, italic (Markdown)")]),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Keyboard Shortcuts").font(.system(size: 15, weight: .semibold))
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            .padding(16)
            Divider()
            ScrollView {
                LazyVGrid(columns: [GridItem(.flexible(), spacing: 24, alignment: .topLeading), GridItem(.flexible(), alignment: .topLeading)], alignment: .leading, spacing: 22) {
                    ForEach(groups, id: \.0) { title, keys in
                        VStack(alignment: .leading, spacing: 7) {
                            Text(title).sectionLabel()
                            ForEach(keys, id: \.0) { k, what in
                                HStack(alignment: .firstTextBaseline, spacing: 10) {
                                    Text(k).font(.system(size: 12, weight: .medium, design: .rounded)).foregroundStyle(K.text)
                                        .padding(.horizontal, 6).padding(.vertical, 2)
                                        .background(K.recessed, in: RoundedRectangle(cornerRadius: 5))
                                        .overlay(RoundedRectangle(cornerRadius: 5).strokeBorder(K.line))
                                    Text(what).font(.system(size: 12.5)).foregroundStyle(K.text2).fixedSize(horizontal: false, vertical: true)
                                }
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                .padding(20)
            }
        }
        .frame(width: 720, height: 520)
    }
}
