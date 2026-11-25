import SwiftUI

/// One command in the palette.
struct Command: Identifiable {
    let id: String
    let group: String
    let label: String
    var symbol: String?
    var status: Status?
    var hint: String?
    var keywords = ""
    let run: () -> Void
}

/// How well `q` matches `text`: higher is better, nil is no match.
private func score(_ q: String, _ text: String) -> Int? {
    if q.isEmpty { return 0 }
    let t = text.lowercased()
    if let r = t.range(of: q) { return 100 - t.distance(from: t.startIndex, to: r.lowerBound) }
    var j = q.startIndex
    var s = 0
    for c in t where c == q[j] {
        j = q.index(after: j)
        s += 1
        if j == q.endIndex { return s }
    }
    return nil
}

/// ⌘K: search dockets and run any command, from the keyboard.
struct PaletteOverlay: View {
    /// In the floating panel (⌥⌘K from any app), not over the window: what
    /// needs the window brings it forward first.
    var floating = false
    var onClose: () -> Void = {}
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @Environment(\.openWindow) private var openWindow
    @Environment(\.openSettings) private var openSettings
    @State private var q = ""
    @State private var sel = 0
    @FocusState private var focused: Bool

    /// Bring the main window forward, when the palette floats over another app.
    private func reveal() {
        if floating { Windows.showMain() }
    }

    private var commands: [Command] {
        var out: [Command] = []
        // What needs you, answerable right here.
        for it in store.inbox {
            if it.kind == .blocked, let d = it.docket, let l = it.live ?? d.live, l.short {
                // Each asking agent: by name, on a docket with several.
                let whose = it.who.map { "\(d.id) · \($0)" } ?? d.id
                out.append(Command(id: "ap-\(it.id)", group: "Needs you", label: "Approve \(whose): \(l.msg ?? "the prompt")", symbol: "checkmark.circle", keywords: "approve permission yes \(d.title) \(it.who ?? "")") { Task { await store.answer(it, approve: true) } })
                out.append(Command(id: "dn-\(it.id)", group: "Needs you", label: "Deny \(whose): \(l.msg ?? "the prompt")", symbol: "xmark.circle", keywords: "deny permission no \(d.title) \(it.who ?? "")") { Task { await store.answer(it, approve: false) } })
            } else if let m = it.msg, !m.options.isEmpty {
                for o in m.options {
                    out.append(Command(id: "op-\(m.id)-\(o)", group: "Needs you", label: "\(it.docketId) · \(m.title): \(o)", symbol: "arrowshape.turn.up.left", keywords: "answer reply \(m.who) \(it.title)") { Task { _ = await store.reply(m, "", choice: o) } })
                }
            } else if let p = it.pr, p.ready, p.draft?.digest != nil {
                out.append(Command(id: "post-\(p.fullKey)", group: "Needs you", label: "Post the review of \(p.key)", symbol: "paperplane", keywords: "review post github \(p.title ?? "")") { Task { _ = await store.post(p, digest: nil) } })
            }
        }
        if !floating, let id = nav.current, let d = store.docket(id) {
            if d.several {
                // Each agent's pane and prompt, by name.
                for (a, l) in d.lives {
                    guard let a else { continue }
                    let b = d.blockFor(a)
                    out.append(Command(id: "rex-\(a.block)", group: d.id, label: "Show \(a.label)'s pane in \(a.runtime)", symbol: "terminal", keywords: "\(appName(a.app)) \(a.pane)") { store.openInRex(d.id, block: b) })
                    out.append(Command(id: "peek-\(a.block)", group: d.id, label: "Peek at \(a.label)'s pane", symbol: "eye", keywords: "screen terminal message send \(appName(a.app)) \(a.pane)") { reveal(); nav.peek(d.id, block: b) })
                    if l.state == .blocked, l.short {
                        out.append(Command(id: "approve-\(a.block)", group: d.id, label: "Approve \(a.label): \(l.msg ?? "the prompt")", symbol: "checkmark.circle", keywords: "permission yes allow") { Task { await store.answer(d.id, record: l.record ?? "", since: l.since, msg: l.msg, approve: true, block: b) } })
                        out.append(Command(id: "deny-\(a.block)", group: d.id, label: "Deny \(a.label)'s prompt", symbol: "xmark.circle", keywords: "permission no reject") { Task { await store.answer(d.id, record: l.record ?? "", since: l.since, msg: l.msg, approve: false, block: b) } })
                    }
                }
            } else {
                if d.live != nil {
                    out.append(Command(id: "rex", group: d.id, label: "Show pane in \(d.runtime)", symbol: "terminal", hint: "O") { store.openInRex(d.id) })
                    out.append(Command(id: "peek", group: d.id, label: "Peek at pane", symbol: "eye", keywords: "screen terminal message send") { reveal(); nav.peek(d.id) })
                }
                if d.live?.short == true {
                    out.append(Command(id: "approve", group: d.id, label: "Approve: \(d.live?.msg ?? "the prompt")", symbol: "checkmark.circle", keywords: "permission yes allow") { Task { await store.answer(d, approve: true) } })
                    out.append(Command(id: "deny", group: d.id, label: "Deny the prompt", symbol: "xmark.circle", keywords: "permission no reject") { Task { await store.answer(d, approve: false) } })
                }
            }
            if d.hold != nil {
                out.append(Command(id: "resume", group: d.id, label: "Resume: take it off hold", symbol: "play.circle", hint: "⇧⌘H", keywords: "hold unpause continue") { store.resume(d.id) })
            } else {
                out.append(Command(id: "hold", group: d.id, label: "Hold…", symbol: "pause.circle", hint: "⇧⌘H", keywords: "pause stop wait build review") { reveal(); nav.holding = d.id })
            }
            out.append(Command(id: "checks", group: d.id, label: "Run checks", symbol: "checklist", hint: "⇧⌘U", keywords: "verify test lint witnessed") { Task { await store.runChecks(d.id) } })
            if d.canStartAgent {
                out.append(Command(id: "start-agent", group: d.id, label: d.onAgents.isEmpty ? "Start an agent…" : "Add an agent…", symbol: "terminal", hint: "⇧⌘A", keywords: "agent claude codex agy antigravity pi start pane work") { reveal(); nav.starting = d.id })
            }
            for s in Status.allCases where s != d.status {
                out.append(Command(id: "st-\(s.rawValue)", group: d.id, label: "Move to \(s.label)", status: s, keywords: "status") { store.setStatus(d.id, s, undo: d.status) })
            }
            out.append(Command(id: "win", group: d.id, label: "Open in new window", symbol: "macwindow") { openWindow(id: "docket", value: d.id) })
            if d.sharedFrom == nil {
                out.append(Command(id: "changes", group: d.id, label: "View changes", symbol: "plus.forwardslash.minus", hint: "⇧⌘D", keywords: "diff code files commits patch") { openWindow(id: "changes", value: d.id) })
            }
            out.append(Command(id: "rec", group: d.id, label: "Read the record", symbol: "checkmark.shield", keywords: "verify signed chain") { reveal(); nav.record = d.id })
        }
        out += [
            Command(id: "new", group: "Create", label: "New docket", symbol: "square.and.pencil", hint: "⌘N") { reveal(); nav.creating = .blank },
            Command(id: "new-agent", group: "Create", label: "Start an agent on a new docket", symbol: "terminal", hint: "⇧⌘N", keywords: "claude codex agy antigravity pi start run") { reveal(); nav.creating = .agent },
            Command(id: "new-linear", group: "Create", label: "New docket from a Linear issue", symbol: "circle.hexagongrid", keywords: "linear issue import") { reveal(); nav.creating = .linear },
        ]
        for (i, (p, key)) in Place.order.enumerated() {
            out.append(Command(id: "go-\(key)", group: "Go to", label: p.title, symbol: p.symbol, hint: "⌘\(i + 1)") { reveal(); nav.go(p) })
        }
        out += [
            Command(id: "verify", group: "Record", label: "Verify every docket's record", symbol: "checkmark.shield", keywords: "check chain signatures") { store.verifyAll() },
            Command(id: "settings", group: "Settings", label: "Standing orders and gates", symbol: "list.bullet.clipboard", hint: "⌘,", keywords: "settings preferences gates ask auto pause post") { openSettingsTab("orders") },
            Command(id: "settings-agents", group: "Settings", label: "Agents: builder, checker, reviewers", symbol: "person.2", keywords: "settings claude codex agy antigravity fix rounds pre-review") { openSettingsTab("agents") },
            Command(id: "settings-repos", group: "Settings", label: "Repos: notes, checks, review guide", symbol: "folder", keywords: "settings checks base branch plan notes rules") { openSettingsTab("repos") },
            Command(id: "settings-guides", group: "Settings", label: "Guides and briefs", symbol: "text.book.closed", keywords: "settings guide brief prompt markdown pr style plan format") { openSettingsTab("guides") },
            Command(id: "th-system", group: "Theme", label: "Theme: System", symbol: "circle.lefthalf.filled", keywords: "appearance mode auto") { UserDefaults.standard.set("system", forKey: "theme") },
            Command(id: "th-light", group: "Theme", label: "Theme: Light", symbol: "sun.max", keywords: "appearance mode") { UserDefaults.standard.set("light", forKey: "theme") },
            Command(id: "th-dark", group: "Theme", label: "Theme: Dark", symbol: "moon", keywords: "appearance mode") { UserDefaults.standard.set("dark", forKey: "theme") },
        ]
        for d in store.dockets {
            out.append(Command(id: "d-\(d.id)", group: "Dockets", label: d.title, status: d.status, hint: d.id, keywords: "\(d.id) \(d.agent?.cwd ?? "") \(d.agent?.rexSession ?? "")") { reveal(); nav.show(d.id, store: store) })
        }
        return out
    }

    private var shown: [Command] {
        let all = commands
        let query = q.trimmingCharacters(in: .whitespaces).lowercased()
        if query.isEmpty {
            // Floating, the palette is mostly for what needs you and finding a docket.
            let keep = floating ? all.filter { ["Needs you", "Create"].contains($0.group) } : all.filter { $0.group != "Dockets" }
            return keep + all.filter { $0.group == "Dockets" }.prefix(floating ? 8 : 6)
        }
        return all.compactMap { c -> (Command, Int)? in
            let s = max(score(query, c.label) ?? -1, (score(query, c.keywords) ?? -100) - 20)
            return s >= 0 ? (c, s) : nil
        }
        .sorted { $0.1 > $1.1 }
        .prefix(30)
        .map(\.0)
    }

    var body: some View {
        if floating {
            card.padding(24)
        } else {
            ZStack(alignment: .top) {
                Color.black.opacity(0.18).ignoresSafeArea().onTapGesture { close() }
                card.padding(.top, 90)
            }
        }
    }

    private var card: some View {
        let list = shown
        return VStack(spacing: 0) {
                HStack(spacing: 10) {
                    Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                    if floating {
                        Logo().frame(width: 16, height: 16)
                        DevBadge()
                    } else if let id = nav.current {
                        Text(id).font(.system(size: 11.5, design: .monospaced)).padding(.horizontal, 6).padding(.vertical, 2).background(K.recessed, in: Capsule())
                    }
                    TextField("Type a command or search dockets…", text: $q)
                        .textFieldStyle(.plain)
                        .font(.system(size: 16))
                        .focused($focused)
                        .onSubmit { run(list[safe: sel]) }
                        .onKeyPress(.downArrow) { sel = min(list.count - 1, sel + 1); return .handled }
                        .onKeyPress(.upArrow) { sel = max(0, sel - 1); return .handled }
                        .onKeyPress(.escape) { close(); return .handled }
                }
                .padding(14)
                Divider()
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 0) {
                            if list.isEmpty { Text("No results").foregroundStyle(.secondary).padding(16) }
                            ForEach(Array(list.enumerated()), id: \.element.id) { i, c in
                                if i == 0 || list[i - 1].group != c.group {
                                    Text(c.group).font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary).padding(.horizontal, 14).padding(.top, 10).padding(.bottom, 4)
                                }
                                HStack(spacing: 10) {
                                    Group {
                                        if let s = c.status { StatusIcon(status: s) } else if let sym = c.symbol { Image(systemName: sym) } else { Color.clear }
                                    }
                                    .frame(width: 18)
                                    Text(c.label).lineLimit(1)
                                    Spacer()
                                    if let h = c.hint { Text(h).font(.system(size: 11.5, design: c.group == "Dockets" ? .monospaced : .default)).foregroundStyle(.secondary) }
                                }
                                .font(.system(size: 13))
                                .padding(.horizontal, 14).padding(.vertical, 7)
                                .background(i == sel ? K.accent.opacity(0.14) : .clear, in: RoundedRectangle(cornerRadius: 7))
                                .padding(.horizontal, 6)
                                .contentShape(Rectangle())
                                .onTapGesture { run(c) }
                                .onHover { if $0 { sel = i } }
                                .id(i)
                            }
                        }
                        .padding(.bottom, 8)
                    }
                    .frame(maxHeight: 380)
                    .onChange(of: sel) { proxy.scrollTo(sel) }
                }
            }
            .frame(width: 600)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(K.line2))
            .shadow(color: .black.opacity(0.25), radius: 30, y: 12)
            .onAppear { focused = true }
            .onReceive(NotificationCenter.default.publisher(for: .paletteShown)) { _ in
                q = ""
                sel = 0
                focused = true
            }
            .onChange(of: q) { sel = 0 }
    }

    /// Settings, on one of its tabs.
    private func openSettingsTab(_ tab: String) {
        UserDefaults.standard.set(tab, forKey: "settingsTab")
        NSApp.activate()
        openSettings()
    }

    private func close() {
        if !floating { nav.palette = false }
        q = ""
        onClose()
    }

    private func run(_ c: Command?) {
        guard let c else { return }
        close()
        c.run()
    }
}

extension Array {
    subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}

extension Notification.Name {
    /// The floating palette opened again: empty and focused.
    static let paletteShown = Notification.Name("wheelhouse.paletteShown")
}

// ---- new dockets ----

struct NewDocketSheet: View {
    @State var mode: NewMode
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @Environment(\.dismiss) private var dismiss
    @State private var title = ""
    @State private var note = ""
    @State private var cwd = ""
    @State private var app = "claude-code"
    @State private var busy = false

    private var folders: [String] {
        Array(Set(store.dockets.compactMap { $0.agent?.cwd }.filter { !$0.contains("/.local/share/") })).sorted()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("New docket").font(.system(size: 14, weight: .semibold))
                Spacer()
                Picker("", selection: $mode) {
                    Text("Blank").tag(NewMode.blank)
                    Text("From Linear").tag(NewMode.linear)
                    Text("Agent").tag(NewMode.agent)
                }
                .pickerStyle(.segmented).labelsHidden().fixedSize()
            }
            .padding(16)
            Divider()
            Group {
                switch mode {
                case .blank: blank
                case .agent: agent
                case .linear:
                    LinearPicker(busy: busy) { issue in
                        busy = true
                        Task {
                            if let id = await store.fromLinear(issue.key) { done(id) } else { busy = false }
                        }
                    }
                }
            }
        }
        .frame(width: 620)
    }

    private var blank: some View {
        VStack(alignment: .leading, spacing: 12) {
            TextField("Docket title", text: $title).textFieldStyle(.plain).font(.system(size: 18, weight: .semibold))
            Editor(text: $note, placeholder: "Add a note… (Markdown, optional)", minHeight: 90, maxHeight: 260, onSubmit: create)
            HStack {
                Text("Signed as you, on the record.").font(.system(size: 11.5)).foregroundStyle(.secondary)
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(busy ? "Creating…" : "Create docket") { create() }
                    .buttonStyle(.borderedProminent).tint(K.accent)
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(title.trimmingCharacters(in: .whitespaces).isEmpty || busy)
            }
        }
        .padding(16)
    }

    private var agent: some View {
        VStack(alignment: .leading, spacing: 12) {
            Editor(text: $note, placeholder: "What should the agent do? (its first prompt; the docket is named from it)", minHeight: 110, maxHeight: 280, focus: true, onSubmit: start)
            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 10) {
                GridRow {
                    Text("Folder").foregroundStyle(.secondary)
                    HStack {
                        TextField("~/git/project", text: $cwd).textFieldStyle(.roundedBorder)
                        Menu {
                            ForEach(folders, id: \.self) { f in Button(shortPath(f)) { cwd = shortPath(f) } }
                            if !folders.isEmpty { Divider() }
                            Button("Choose…") { choose() }
                        } label: { Image(systemName: "folder") }
                            .menuStyle(.borderlessButton).fixedSize()
                    }
                }
                GridRow {
                    Text("Agent").foregroundStyle(.secondary)
                    Picker("", selection: $app) {
                        ForEach(startApps, id: \.self) { Text(appName($0)).tag($0) }
                    }
                    .pickerStyle(.segmented).labelsHidden().fixedSize()
                }
                GridRow {
                    Text("Title").foregroundStyle(.secondary)
                    TextField("Optional: named from the prompt otherwise", text: $title).textFieldStyle(.roundedBorder)
                }
            }
            HStack {
                Text("A new tab in \(store.state?.spawnRuntime ?? "Rex")'s wheelhouse session, named after the docket.").font(.system(size: 11.5)).foregroundStyle(.secondary)
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(busy ? "Starting…" : "Start agent") { start() }
                    .buttonStyle(.borderedProminent).tint(K.accent)
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(cwd.trimmingCharacters(in: .whitespaces).isEmpty || busy)
            }
        }
        .padding(16)
    }

    private func choose() {
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = false
        p.prompt = "Choose"
        if p.runModal() == .OK, let u = p.url { cwd = shortPath(u.path) }
    }

    private func create() {
        let t = title.trimmingCharacters(in: .whitespaces)
        guard !t.isEmpty, !busy else { return }
        busy = true
        Task {
            if let id = await store.create(title: t, note: note) { done(id) } else { busy = false }
        }
    }

    private func start() {
        let dir = cwd.trimmingCharacters(in: .whitespaces)
        guard !dir.isEmpty, !busy else { return }
        busy = true
        Task {
            if let id = await store.startAgent(app: app, cwd: dir, prompt: note, title: title.isEmpty ? nil : title) { done(id) } else { busy = false }
        }
    }

    private func done(_ id: String) {
        dismiss()
        store.reload()
        Task {
            try? await Task.sleep(for: .milliseconds(300))
            nav.show(id, store: store)
        }
    }
}

/// Start an agent on a docket you already have: it claims this docket, in a
/// new pane of the wheelhouse session, in the runtime new agents open in.
struct StartAgentSheet: View {
    let id: String
    /// From the composer's `/task`: what the agent should do.
    var prompt: String?
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var note = ""
    @State private var cwd = ""
    @State private var app = "claude-code"
    @State private var folders: [String] = []
    @State private var busy = false

    var body: some View {
        let d = store.docket(id)
        let others = d?.onAgents.count ?? 0
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 8) {
                Text(others > 0 ? "Add an agent to \(id)" : "Start an agent on \(id)").font(.system(size: 14, weight: .semibold))
                if let t = d?.title { Text(t).foregroundStyle(.secondary).lineLimit(1).truncationMode(.tail) }
            }
            Editor(text: $note, placeholder: "What should the agent do first? Leave it empty to start it idle.", minHeight: 80, maxHeight: 220, focus: true, onSubmit: start)
            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 10) {
                GridRow {
                    Text("Folder").foregroundStyle(.secondary)
                    HStack {
                        TextField("~/git/project", text: $cwd).textFieldStyle(.roundedBorder)
                        Menu {
                            ForEach(folders, id: \.self) { f in Button(shortPath(f)) { cwd = shortPath(f) } }
                            if !folders.isEmpty { Divider() }
                            Button("Choose…") { choose() }
                        } label: { Image(systemName: "folder") }
                            .menuStyle(.borderlessButton).fixedSize()
                    }
                }
                GridRow {
                    Text("Agent").foregroundStyle(.secondary)
                    Picker("", selection: $app) {
                        ForEach(startApps, id: \.self) { Text(appName($0)).tag($0) }
                    }
                    .pickerStyle(.segmented).labelsHidden().fixedSize()
                }
            }
            HStack {
                Text(others > 0
                    ? "A new tab in \(store.state?.spawnRuntime ?? "Rex")'s wheelhouse session. It joins \(id) beside the \(others == 1 ? "one" : "\(others)") already there\(others == 1 ? "" : ", each in its own pane"), and reads its intent."
                    : "A new tab in \(store.state?.spawnRuntime ?? "Rex")'s wheelhouse session, named \(id). The agent works on this docket and reads its intent.")
                    .font(.system(size: 11.5)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(busy ? "Starting…" : others > 0 ? "Add Agent" : "Start Agent") { start() }
                    .buttonStyle(.borderedProminent).tint(K.accent)
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(cwd.trimmingCharacters(in: .whitespaces).isEmpty || busy)
            }
        }
        .padding(16)
        .frame(width: 560)
        .task {
            if note.isEmpty { note = prompt.map { "Read wh brief \(id), then: \($0)" } ?? "Read wh brief \(id), then work on its intent." }
            if let a = d?.agent {
                app = startApp(for: a.app)
                if cwd.isEmpty, let c = a.cwd { cwd = shortPath(c) }
            }
            let f = await store.folders(for: id)
            folders = f.all
            if cwd.isEmpty, let s = f.suggested { cwd = shortPath(s) }
        }
    }

    private func choose() {
        let p = NSOpenPanel()
        p.canChooseDirectories = true
        p.canChooseFiles = false
        p.prompt = "Choose"
        if p.runModal() == .OK, let u = p.url { cwd = shortPath(u.path) }
    }

    private func start() {
        let dir = cwd.trimmingCharacters(in: .whitespaces)
        guard !dir.isEmpty, !busy else { return }
        busy = true
        Task {
            if await store.startAgent(app: app, cwd: dir, prompt: note.trimmingCharacters(in: .whitespacesAndNewlines), title: nil, docket: id) != nil {
                store.reload()
                // The composer's /task that asked for it is done with.
                if prompt != nil { NotificationCenter.default.post(name: .taskStarted, object: id) }
                dismiss()
            } else {
                busy = false
            }
        }
    }
}

/// Search Linear: your open assigned issues, or by words, or a key.
struct LinearPicker: View {
    var busy = false
    let pick: (LinearRef) -> Void
    @Environment(Store.self) private var store
    @State private var q = ""
    @State private var issues: [LinearRef] = []
    @State private var error: String?
    @State private var loading = false
    @State private var sel = 0
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            TextField("Search Linear, or paste a key (ENG-142)…", text: $q)
                .textFieldStyle(.plain).font(.system(size: 15)).padding(14)
                .focused($focused)
                .onSubmit { if let i = issues[safe: sel] { pick(i) } }
                .onKeyPress(.downArrow) { sel = min(issues.count - 1, sel + 1); return .handled }
                .onKeyPress(.upArrow) { sel = max(0, sel - 1); return .handled }
            Divider()
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if let error { Text(error).foregroundStyle(K.error).padding(14) }
                    if !loading, issues.isEmpty, error == nil { Text(q.isEmpty ? "No open issues assigned to you." : "No issues match.").foregroundStyle(.secondary).padding(14) }
                    ForEach(Array(issues.enumerated()), id: \.offset) { i, issue in
                        HStack(spacing: 10) {
                            StatusIcon(status: issue.stateType == "started" ? .in_progress : issue.stateType == "completed" ? .done : issue.stateType == "canceled" ? .dropped : .todo)
                            Text(issue.key).font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary).frame(width: 74, alignment: .leading)
                            Text(issue.title ?? "").lineLimit(1)
                            Spacer()
                            if let s = issue.state { Text(s).font(.system(size: 11.5)).foregroundStyle(.secondary) }
                        }
                        .font(.system(size: 13))
                        .padding(.horizontal, 14).padding(.vertical, 8)
                        .background(i == sel ? K.accent.opacity(0.14) : .clear)
                        .contentShape(Rectangle())
                        .onTapGesture { pick(issue) }
                        .onHover { if $0 { sel = i } }
                    }
                }
            }
            .frame(height: 320)
            .disabled(busy)
            .overlay { if busy || loading { ProgressView().controlSize(.small) } }
        }
        .onAppear { focused = true }
        .task(id: q) {
            try? await Task.sleep(for: .milliseconds(220))
            guard !Task.isCancelled else { return }
            loading = true
            do {
                let r = try await store.hub.get("/api/linear/search?q=\(q.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? "")")
                issues = r["issues"].array.map(LinearRef.init)
                error = nil
                sel = 0
            } catch {
                self.error = Store.words(error)
                issues = []
            }
            loading = false
        }
    }
}

/// Link a Linear issue to a docket.
struct LinkIssueSheet: View {
    let docket: String
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Link a Linear issue to \(docket)").font(.system(size: 14, weight: .semibold))
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            .padding(16)
            Divider()
            LinearPicker(busy: busy) { issue in
                busy = true
                Task {
                    await store.link(docket, issue: issue.key, hadIntent: store.docket(docket)?.intent != nil)
                    dismiss()
                }
            }
        }
        .frame(width: 620)
    }
}
