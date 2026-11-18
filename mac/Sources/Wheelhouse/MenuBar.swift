import AppKit
import ServiceManagement
import SwiftUI
import UserNotifications

/// The menu bar's wheel: orange, with a count, while something needs you.
struct MenuBarLabel: View {
    @Environment(Store.self) private var store
    var body: some View {
        let n = store.urgent
        HStack(spacing: 3) {
            Image(nsImage: Self.wheel(alert: n > 0))
            // Beside the daily one, Dev says which it is: a blue wheel and a DEV pill.
            if Flavor.isDev { Image(nsImage: Self.devPill) }
            if n > 0 { Text("\(n)").monospacedDigit() }
        }
    }

    /// The mark, drawn for the menu bar: a template (it follows the bar)
    /// unless it's calling for you, in orange.
    static func wheel(alert: Bool) -> NSImage {
        let size = NSSize(width: 16, height: 16)
        let img = NSImage(size: size, flipped: false) { r in
            let u = r.width / 32
            let c = NSPoint(x: r.midX, y: r.midY)
            (alert ? NSColor(srgbRed: 0.965, green: 0.51, blue: 0.122, alpha: 1) : Flavor.isDev ? Self.devBlue : NSColor.black).set()
            let ring = NSBezierPath(ovalIn: NSRect(x: c.x - 10.5 * u, y: c.y - 10.5 * u, width: 21 * u, height: 21 * u))
            ring.lineWidth = 3.4 * u
            ring.stroke()
            NSBezierPath(ovalIn: NSRect(x: c.x - 3.6 * u, y: c.y - 3.6 * u, width: 7.2 * u, height: 7.2 * u)).fill()
            for i in 0 ..< 6 {
                let a = CGFloat(i) * .pi / 3 + .pi / 2
                let p = NSBezierPath()
                p.move(to: NSPoint(x: c.x + cos(a) * 11 * u, y: c.y + sin(a) * 11 * u))
                p.line(to: NSPoint(x: c.x + cos(a) * 14.5 * u, y: c.y + sin(a) * 14.5 * u))
                p.lineWidth = 3 * u
                p.lineCapStyle = .round
                p.stroke()
            }
            return true
        }
        // Dev's stays blue in a light or dark bar; the daily one follows the bar.
        img.isTemplate = !alert && !Flavor.isDev
        return img
    }

    static let devBlue = NSColor(srgbRed: 0.184, green: 0.49, blue: 1, alpha: 1)

    /// Dev's pill: DEV in white on blue, drawn once.
    static let devPill: NSImage = {
        let text = NSAttributedString(string: "DEV", attributes: [.font: NSFont.systemFont(ofSize: 9, weight: .heavy), .foregroundColor: NSColor.white, .kern: 0.5])
        let t = text.size()
        let size = NSSize(width: ceil(t.width) + 9, height: 13)
        return NSImage(size: size, flipped: false) { r in
            devBlue.setFill()
            NSBezierPath(roundedRect: r, xRadius: r.height / 2, yRadius: r.height / 2).fill()
            text.draw(at: NSPoint(x: (r.width - t.width) / 2, y: (r.height - t.height) / 2 + 0.5))
            return true
        }
    }()
}

/// The menu bar's panel: what needs you, answerable in place.
struct MenuBarPanel: View {
    @Environment(Store.self) private var store
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings

    var body: some View {
        let items = store.inbox
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                HubAvatar(size: 20)
                Text(Flavor.name).font(.system(size: 13, weight: .semibold))
                DevBadge()
                Spacer()
                Text(items.isEmpty ? "All caught up" : "\(items.count) for you").font(.system(size: 12)).foregroundStyle(.secondary)
            }
            .padding(.horizontal, 14).padding(.vertical, 11)
            Divider()
            if store.hubDown {
                VStack(spacing: 10) {
                    Text("The hub isn't running.").font(.system(size: 12.5))
                    Button(store.startingHub ? "Starting…" : "Start the hub") { store.startHub() }.buttonStyle(.borderedProminent).controlSize(.small)
                }
                .frame(maxWidth: .infinity).padding(20)
            } else if items.isEmpty {
                VStack(spacing: 6) {
                    Image(systemName: "checkmark.circle").font(.system(size: 22)).foregroundStyle(K.working)
                    Text("Nothing is waiting on you.").font(.system(size: 12.5)).foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity).padding(22)
            } else {
                // A panel sizes to what's in it: a few items as they are,
                // more in a scroller of a fixed height.
                let list = VStack(alignment: .leading, spacing: 0) {
                    ForEach(items.prefix(12)) { it in
                        PanelItem(it: it) {
                            Windows.showMain()
                            Nav.main.show(it.docketId, store: store)
                        }
                        Divider().opacity(0.5)
                    }
                }
                if items.count <= 5 {
                    list
                } else {
                    ScrollView { list }.frame(height: 460)
                }
            }
            Divider()
            HStack(spacing: 4) {
                Circle().fill(store.state?.rexConnected == true && store.live ? K.working : K.blocked).frame(width: 6, height: 6)
                Text(store.state == nil ? "Hub unreachable" : store.state?.rexConnected == true ? "Following Rex" : "Rex: connecting").font(.system(size: 11)).foregroundStyle(.secondary)
                Spacer()
                Button { Windows.showMain() } label: { Text("Open \(Flavor.name)") }.buttonStyle(.borderless).font(.system(size: 12))
                Menu {
                    Button("New Docket…") { Windows.showMain(); Nav.main.creating = .blank }
                    Button("Start an Agent…") { Windows.showMain(); Nav.main.creating = .agent }
                    Divider()
                    Button("Settings…") { NSApp.activate(); openSettings() }
                    Button("Quit \(Flavor.name)") { NSApp.terminate(nil) }
                } label: { Image(systemName: "ellipsis.circle") }
                    .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
            }
            .padding(.horizontal, 12).padding(.vertical, 8)
        }
        .frame(width: 380)
        .opener()
    }
}

private struct PanelItem: View {
    let it: InboxItem
    let open: () -> Void
    @State private var hover = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .top, spacing: 9) {
                LiveIcon(state: it.kind == .error ? .error : it.kind == .done ? .done : .blocked, size: 13).padding(.top, 2)
                VStack(alignment: .leading, spacing: 2) {
                    HStack {
                        Text(it.title).font(.system(size: 12.5, weight: .semibold)).lineLimit(1)
                        Spacer(minLength: 4)
                        Text(ago(it.at)).font(.system(size: 11)).foregroundStyle(.tertiary)
                    }
                    Text("\(it.docketId)  \(it.line)").font(.system(size: 11.5)).foregroundStyle(.secondary).lineLimit(2)
                }
            }
            Group {
                if let p = it.pr, p.attention?.kind == "ready" {
                    ReviewActions(p: p, small: true)
                } else if let m = it.msg, !m.options.isEmpty {
                    OptionButtons(m: m)
                } else if it.kind == .blocked, let d = it.docket, d.live?.answerable == true {
                    AnswerButtons(d: d, small: true)
                }
            }
            .padding(.leading, 22)
        }
        .padding(.horizontal, 14).padding(.vertical, 9)
        .background(hover ? K.hover : .clear)
        .contentShape(Rectangle())
        .onHover { hover = $0 }
        .onTapGesture(perform: open)
    }
}

// ---- Settings ----

/// Settings: the app's own (General, Notifications, Hub), and what
/// Wheelhouse does on its own (Standing Orders, Agents, Repos, Guides &
/// Briefs), kept by the hub. Each tab sizes the window for itself.
struct SettingsView: View {
    @State private var model = SettingsModel()
    @AppStorage("settingsTab") private var tab = "general"

    var body: some View {
        TabView(selection: $tab) {
            GeneralSettings().tabItem { Label("General", systemImage: "gearshape") }.tag("general")
            OrdersSettings().tabItem { Label("Standing Orders", systemImage: "list.bullet.clipboard") }.tag("orders")
            AgentsSettings().tabItem { Label("Agents", systemImage: "person.2") }.tag("agents")
            ReposSettings().tabItem { Label("Repos", systemImage: "folder") }.tag("repos")
            GuidesSettings().tabItem { Label("Guides & Briefs", systemImage: "text.book.closed") }.tag("guides")
            NotificationSettings().tabItem { Label("Notifications", systemImage: "bell.badge") }.tag("notifications")
            HubSettings().tabItem { Label("Hub", systemImage: "bolt.horizontal.circle") }.tag("hub")
        }
        .environment(model)
        .onReceive(NotificationCenter.default.publisher(for: .snapshotPatch)) { n in
            // Snapshots: a change the hub refuses, to show where its words go.
            guard let (key, patch) = n.object as? (String, [String: Any]) else {
                model.errors = [:]
                return
            }
            Task { await model.change(key, patch) }
        }
    }
}

private struct GeneralSettings: View {
    @AppStorage("theme") private var theme = "system"
    @AppStorage("menuBar") private var menuBar = true
    @AppStorage("quickPanel") private var quickPanel = true
    @State private var login = SMAppService.mainApp.status == .enabled
    @State private var loginError: String?

    var body: some View {
        Form {
            Picker("Appearance", selection: $theme) {
                Text("System").tag("system")
                Text("Light").tag("light")
                Text("Dark").tag("dark")
            }
            .pickerStyle(.segmented)
            Toggle("Show Wheelhouse in the menu bar", isOn: $menuBar)
            Toggle("Open \(Flavor.name) from any app with \(Flavor.chord)", isOn: $quickPanel)
                .onChange(of: quickPanel) { QuickPanel.apply() }
            Toggle("Open at login", isOn: Binding(get: { login }, set: { on in
                do {
                    if on { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
                    login = on
                    loginError = nil
                } catch {
                    loginError = error.localizedDescription
                }
            }))
            if let loginError { Text(loginError).font(.caption).foregroundStyle(K.error) }
        }
        .formStyle(.grouped)
        .frame(width: 560, height: 250)
    }
}

private struct NotificationSettings: View {
    @AppStorage("notify") private var notify = true
    @State private var status: UNAuthorizationStatus = .notDetermined

    @AppStorage("soundPack") private var pack = ""

    var body: some View {
        Form {
            Section {
                Toggle("Notify me about what needs me", isOn: $notify)
                    .onChange(of: notify) { Store.shared.reconnect() }
                Text("Prompts to approve, messages and questions, a finished turn from an agent you messaged, and reviews ready to post. Each has its answer on the banner, and goes away once answered anywhere.")
                    .font(.caption).foregroundStyle(.secondary)
                LabeledContent("macOS") {
                    switch status {
                    case .authorized, .provisional: Text("Allowed").foregroundStyle(K.working)
                    case .denied: Text("Turned off in System Settings").foregroundStyle(K.error)
                    default: Text("Not asked yet").foregroundStyle(.secondary)
                    }
                }
                HStack {
                    Button("Send a Test") { Notifier.shared.test() }
                    Button("Open System Settings…") {
                        NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=dev.wheelhouse.app")!)
                    }
                }
            }
            SoundSection()
        }
        .formStyle(.grouped)
        .frame(width: 560, height: SoundPacks.shared.installed.contains { $0.name == pack } ? 590 : 420)
        .task { status = await Notifier.shared.status() }
    }
}

private struct HubSettings: View {
    @Environment(Store.self) private var store
    @AppStorage("hubAddress") private var address = HubClient.defaultAddress

    var body: some View {
        Form {
            TextField("Address", text: $address, prompt: Text(HubClient.defaultAddress))
                .onSubmit {
                    store.reconnect()
                    store.reload()
                }
            Text("The hub on this Mac answers at \(HubClient.defaultAddress). Another address is for a second hub you run by hand (WH_HTTP).")
                .font(.caption).foregroundStyle(.secondary)
            LabeledContent("Status") {
                if let s = store.state {
                    Text(s.rexConnected ? "Running · following Rex" : "Running · Rex: \(s.rexError ?? "connecting")").foregroundStyle(s.rexConnected ? K.working : K.blocked)
                } else {
                    Text(store.error ?? "Connecting…").foregroundStyle(K.error)
                }
            }
            HStack {
                Button(store.startingHub ? "Starting…" : "Start the Hub") { store.startHub() }.disabled(store.startingHub || store.state != nil)
                Button("Show the Log") { NSWorkspace.shared.open(URL(fileURLWithPath: NSHomeDirectory() + "/Library/Logs/wheelhouse.log")) }
                Button("Verify Every Record") { store.verifyAll() }
            }
        }
        .formStyle(.grouped)
        .frame(width: 560, height: 260)
    }
}
