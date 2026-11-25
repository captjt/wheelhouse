import SwiftUI

/// A list beside its detail, with a divider you drag. Its width is kept.
struct SplitPane<L: View, D: View>: View {
    @AppStorage("listWidth") private var width: Double = 380
    @State private var start: Double?
    @ViewBuilder var list: L
    @ViewBuilder var detail: D

    var body: some View {
        HStack(spacing: 0) {
            list.frame(width: width)
            Rectangle().fill(K.line).frame(width: 1)
                .overlay {
                    Color.clear.frame(width: 9).contentShape(Rectangle())
                        .onHover { inside in inside ? NSCursor.resizeLeftRight.push() : NSCursor.pop() }
                        .gesture(DragGesture(minimumDistance: 1, coordinateSpace: .global)
                            .onChanged { v in
                                if start == nil { start = width }
                                width = min(max((start ?? width) + v.translation.width, 280), 640)
                            }
                            .onEnded { _ in start = nil })
                }
                .zIndex(1)
            detail.frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }
}

// ---- grouping ----

struct DocketGroup: Identifiable {
    let id: String
    let label: String
    let live: LiveState?
    let status: Status?
    let session: String?
    let items: [Docket]
}

func groupDockets(_ list: [Docket], by: GroupBy) -> [DocketGroup] {
    switch by {
    case .status:
        return [Status.in_progress, .todo, .done, .dropped].map { s in
            DocketGroup(id: s.rawValue, label: s.label, live: nil, status: s, session: nil, items: list.filter { $0.status == s })
        }.filter { !$0.items.isEmpty }
    case .session:
        let m = Dictionary(grouping: list) { $0.agent?.rexSession.isEmpty == false ? $0.agent!.rexSession : "By hand" }
        return m.keys.sorted().map { k in DocketGroup(id: k, label: k, live: nil, status: nil, session: k, items: m[k]!) }
    case .attention:
        let tests: [(String, String, LiveState?, Status?, (Docket) -> Bool)] = [
            ("blocked", "Needs you", .blocked, nil, { $0.live?.state == .blocked }),
            ("error", "Failed", .error, nil, { $0.live?.state == .error }),
            ("working", "Working", .working, nil, { $0.live?.state == .working || $0.live?.state == .starting }),
            ("done", "Finished, your move", .done, nil, { $0.live?.state == .done }),
            ("idle", "Idle", .idle, nil, { $0.live != nil }),
            ("todo", "To do", nil, .todo, { $0.isOpen }),
            ("closed", "Done", nil, .done, { $0.status == .done }),
            ("dropped", "Dropped", nil, .dropped, { _ in true }),
        ]
        var buckets = Array(repeating: [Docket](), count: tests.count)
        for d in list {
            let i = tests.firstIndex { $0.4(d) } ?? tests.count - 1
            buckets[i].append(d)
        }
        return tests.indices.compactMap { i in
            buckets[i].isEmpty ? nil : DocketGroup(id: tests[i].0, label: tests[i].1, live: tests[i].2, status: tests[i].3, session: nil, items: buckets[i])
        }
    }
}

/// A group's header that folds it: a chevron, then its label.
struct FoldHeader<L: View>: View {
    let folded: Bool
    let toggle: () -> Void
    @ViewBuilder var label: L
    var body: some View {
        Button(action: toggle) {
            HStack(spacing: 6) {
                Image(systemName: "chevron.right").font(.system(size: 9, weight: .bold)).foregroundStyle(K.faint).rotationEffect(.degrees(folded ? 0 : 90))
                label
                Spacer()
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

struct GroupHeader: View {
    let g: DocketGroup
    var body: some View {
        HStack(spacing: 7) {
            if let l = g.live { LiveIcon(state: l, size: 13) } else if let s = g.status { StatusIcon(status: s, size: 13) } else if let s = g.session { SessionIcon(name: s, size: 14) }
            Text(g.label).font(.system(size: 12, weight: .semibold)).foregroundStyle(K.text)
            Text("\(g.items.count)").font(.system(size: 11.5)).foregroundStyle(.tertiary)
        }
    }
}

// ---- a docket list ----

struct DocketsScreen: View {
    let section: Place
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @Environment(\.openWindow) private var openWindow
    @AppStorage private var group: String
    @AppStorage private var layout: String

    init(section: Place) {
        self.section = section
        let key = "\(section)"
        let def: GroupBy = section == .all || section == .done ? .status : .attention
        _group = AppStorage(wrappedValue: def.rawValue, "group:\(key)")
        _layout = AppStorage(wrappedValue: "list", "layout:\(key)")
    }

    var body: some View {
        let list = store.dockets.filter(section.shows)
        let groups = groupDockets(list, by: GroupBy(rawValue: group) ?? .attention)
        Group {
            if store.state != nil && list.isEmpty {
                ContentUnavailableView(emptyTitle, systemImage: section.symbol, description: Text(emptyText))
            } else if layout == "board" {
                BoardView(groups: groups)
            } else {
                SplitPane {
                    DocketList(section: section, groups: groups)
                } detail: {
                    DocketDetail(id: nav.selected(section))
                }
            }
        }
        .navigationSubtitle(list.isEmpty ? "" : "\(list.count)")
        .toolbar {
            ToolbarItemGroup {
                Picker("Layout", selection: $layout) {
                    Image(systemName: "list.bullet").help("List").tag("list")
                    Image(systemName: "rectangle.split.3x1").help("Board").tag("board")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .help("List or board")
                Menu {
                    Picker("Group by", selection: $group) {
                        ForEach(GroupBy.allCases, id: \.rawValue) { g in Text(g.label).tag(g.rawValue) }
                    }
                    .pickerStyle(.inline)
                } label: {
                    Label("Display", systemImage: "line.3.horizontal.decrease.circle")
                }
                .help("Group by attention, status, or session")
            }
        }
    }

    private var emptyTitle: String {
        switch section {
        case .active: "No open dockets"
        case .done: "Nothing done yet"
        case .session: "Nothing open here"
        default: "No dockets yet"
        }
    }

    private var emptyText: String {
        section == .active ? "Start an agent in Rex or Herdr, or press ⌘N." : section == .done ? "Dockets you finish or drop land here." : "Every agent in Rex or Herdr gets a docket."
    }
}

/// The list of dockets: grouped, with sections you fold, and every action a
/// row has in the web app, in its context menu.
struct DocketList: View {
    let section: Place
    let groups: [DocketGroup]
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @Environment(\.openWindow) private var openWindow
    @State private var folded: Set<String> = []
    @State private var statusFor: IdBox?

    private var ids: [String] { groups.filter { !folded.contains($0.id) }.flatMap { $0.items.map(\.id) } }

    var body: some View {
        List(selection: Binding<String?>(get: { nav.selected(section) }, set: { nav.select($0, in: section) })) {
            ForEach(groups) { g in
                Section {
                    if !folded.contains(g.id) { ForEach(g.items) { d in
                        DocketRow(d: d)
                            .tag(d.id)
                            .swipeActions(edge: .trailing, allowsFullSwipe: true) { swipeEnd(d) }
                            .swipeActions(edge: .leading) {
                                Button { statusFor = IdBox(id: d.id) } label: { Label("Status", systemImage: "circle.lefthalf.filled") }.tint(K.progress)
                            }
                    } }
                } header: {
                    FoldHeader(folded: folded.contains(g.id)) { fold(g.id) } label: { GroupHeader(g: g) }
                }
            }
        }
        .listStyle(.inset)
        .scrollContentBackground(.hidden)
        .background(K.main)
        .contextMenu(forSelectionType: String.self) { sel in
            if let id = sel.first, let d = store.docket(id) { DocketMenu(d: d) }
        } primaryAction: { sel in
            if let id = sel.first { openWindow(id: "docket", value: id) }
        }
        .onKeyPress(characters: .init(charactersIn: "jkoes"), phases: .down) { press in
            guard press.modifiers.isEmpty, !typing() else { return .ignored }
            switch press.characters {
            case "j": move(1)
            case "k": move(-1)
            case "o": if let id = nav.selected(section), store.docket(id)?.live != nil { store.openInRex(id) }
            case "s": if let id = nav.selected(section) { statusFor = IdBox(id: id) }
            default: return .ignored
            }
            return .handled
        }
        .popover(item: $statusFor, arrowEdge: .trailing) { b in
            StatusPicker(id: b.id) { statusFor = nil }
        }
        .onAppear { if nav.selected(section) == nil { nav.select(ids.first, in: section) } }
    }

    private func fold(_ id: String) {
        withAnimation(.easeOut(duration: 0.15)) { if folded.contains(id) { folded.remove(id) } else { folded.insert(id) } }
    }

    private func move(_ delta: Int) {
        let all = ids
        guard !all.isEmpty else { return }
        let i = nav.selected(section).flatMap { all.firstIndex(of: $0) } ?? -1
        nav.select(all[max(0, min(all.count - 1, i < 0 ? 0 : i + delta))], in: section)
    }

    @ViewBuilder
    private func swipeEnd(_ d: Docket) -> some View {
        if d.isOpen {
            Button { store.setStatus(d.id, .done, undo: d.status) } label: { Label("Done", systemImage: "checkmark") }.tint(K.accent)
            Button { store.setStatus(d.id, .dropped, undo: d.status) } label: { Label("Drop", systemImage: "xmark") }.tint(K.faint)
        } else {
            Button { store.setStatus(d.id, .in_progress, undo: d.status) } label: { Label("Reopen", systemImage: "arrow.uturn.backward") }.tint(K.progress)
        }
    }
}

/// Pick a status: four choices, with 1–4 as keys.
struct StatusPicker: View {
    let id: String
    let done: () -> Void
    @Environment(Store.self) private var store
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("Change status").font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary).padding(.horizontal, 8).padding(.top, 4)
            ForEach(Array(Status.allCases.enumerated()), id: \.offset) { i, s in
                Button {
                    if let d = store.docket(id), d.status != s { store.setStatus(id, s, undo: d.status) }
                    done()
                } label: {
                    HStack {
                        StatusIcon(status: s)
                        Text(s.label)
                        Spacer()
                        if store.docket(id)?.status == s { Image(systemName: "checkmark").font(.system(size: 11)) }
                        Text("\(i + 1)").font(.system(size: 11, design: .monospaced)).foregroundStyle(.tertiary)
                    }
                    .padding(.horizontal, 8).padding(.vertical, 5)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .keyboardShortcut(KeyEquivalent(Character("\(i + 1)")), modifiers: [])
            }
        }
        .padding(6)
        .frame(width: 210)
    }
}

/// A docket's actions, for every context menu it has.
struct DocketMenu: View {
    let d: Docket
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Button("Open in New Window") { openWindow(id: "docket", value: d.id) }
        if d.several {
            // Each agent on it, by name: its pane, and its prompt.
            Divider()
            ForEach(Array(d.lives.enumerated()), id: \.offset) { _, x in
                if let a = x.agent {
                    let b = d.blockFor(a)
                    let l = x.live
                    Menu("\(a.label) · \(l.state.word)") {
                        Button("Show Pane in \(a.runtime)") { store.openInRex(d.id, block: b) }
                        Button("Peek at Pane") { nav.peek(d.id, block: b) }
                        if l.state == .blocked, l.short {
                            Divider()
                            Button("Approve\(l.msg.map { ": \($0.prefix(40))" } ?? "")") { Task { await store.answer(d.id, record: l.record ?? "", since: l.since, msg: l.msg, approve: true, block: b) } }
                            Button("Deny") { Task { await store.answer(d.id, record: l.record ?? "", since: l.since, msg: l.msg, approve: false, block: b) } }
                        }
                    }
                }
            }
        } else if d.live != nil {
            Divider()
            Button("Show Pane in \(d.runtime)") { store.openInRex(d.id) }
            Button("Peek at Pane") { nav.peek(d.id) }
            if d.live?.short == true {
                Button("Approve\(d.live?.msg.map { ": \($0.prefix(40))" } ?? "")") { Task { await store.answer(d, approve: true) } }
                Button("Deny") { Task { await store.answer(d, approve: false) } }
            }
        }
        Divider()
        Picker("Status", selection: Binding(get: { d.status }, set: { s in if s != d.status { store.setStatus(d.id, s, undo: d.status) } })) {
            ForEach(Status.allCases) { s in Text(s.label).tag(s) }
        }
        if d.hold != nil {
            Button("Resume") { store.resume(d.id) }
        } else {
            Button("Hold…") { nav.holding = d.id }
        }
        Button("Run Checks") { Task { await store.runChecks(d.id) } }
        if d.canStartAgent { Button(d.onAgents.isEmpty ? "Start an Agent…" : "Add an Agent…") { nav.starting = d.id } }
        Divider()
        Button("Copy ID") { copy(d.id) }
        Button("Copy Link") { copy("\(Flavor.scheme)://docket/\(d.id)") }
        if !d.head.isEmpty { Button("Copy Record Head") { copy("Docket: \(d.id) @ \(d.shortHead)") } }
    }

    private func copy(_ s: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(s, forType: .string)
    }
}

/// A docket in a list: what it is, where it runs, and what it's doing.
struct DocketRow: View {
    let d: Docket
    var body: some View {
        HStack(alignment: .top, spacing: 9) {
            Group {
                if let l = d.live { LiveIcon(state: l.state, size: 14) } else { StatusIcon(status: d.status, size: 14) }
            }
            .padding(.top, 1.5)
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text(d.title).font(.system(size: 13, weight: .medium)).lineLimit(1)
                    Spacer(minLength: 4)
                    Text(ago(d.updatedAt)).font(.system(size: 11.5)).foregroundStyle(.tertiary).monospacedDigit()
                }
                HStack(spacing: 6) {
                    IdText(id: d.id)
                    if d.live != nil { StatusIcon(status: d.status, size: 11) }
                    if let s = d.agent?.rexSession, !s.isEmpty {
                        HStack(spacing: 4) { SessionIcon(name: s, size: 12); Text(s).lineLimit(1) }
                    }
                    if d.turns > 0 { Text(plural(d.turns, "turn")) }
                    if let l = d.linear { Text(l.key).font(.system(size: 11, design: .monospaced)) }
                    if let r = d.review { Text(r.key).lineLimit(1) }
                    Spacer(minLength: 0)
                    if let a = d.agent { AppAvatar(app: a.app, size: 14) }
                }
                .font(.system(size: 11.5))
                .foregroundStyle(.secondary)
                if d.live?.state == .blocked, let m = d.live?.msg {
                    Text(m).font(.system(size: 11.5)).foregroundStyle(K.blocked).lineLimit(1)
                }
                if let why = d.hold {
                    Label("On hold: \(why)", systemImage: "pause.circle").font(.system(size: 11.5)).foregroundStyle(K.blocked).lineLimit(1)
                }
            }
        }
        .padding(.vertical, 4)
    }
}

// ---- the board ----

struct BoardView: View {
    let groups: [DocketGroup]
    @Environment(Store.self) private var store
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        ScrollView(.horizontal) {
            HStack(alignment: .top, spacing: 12) {
                ForEach(groups) { g in
                    VStack(alignment: .leading, spacing: 8) {
                        GroupHeader(g: g).padding(.horizontal, 4)
                        ScrollView {
                            LazyVStack(spacing: 8) {
                                ForEach(g.items) { d in
                                    Card(d: d)
                                        .onTapGesture(count: 2) { openWindow(id: "docket", value: d.id) }
                                        .onTapGesture { openWindow(id: "docket", value: d.id) }
                                        .contextMenu { DocketMenu(d: d) }
                                        .draggable(d.id)
                                }
                            }
                            .padding(.bottom, 12)
                        }
                    }
                    .padding(10)
                    .frame(width: 290)
                    .frame(maxHeight: .infinity, alignment: .top)
                    .background(K.recessed, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                    .dropDestination(for: String.self) { ids, _ in
                        // Drop a card on a status column to move it there.
                        guard let s = g.status, g.live == nil else { return false }
                        for id in ids { if let d = store.docket(id), d.status != s { store.setStatus(id, s, undo: d.status) } }
                        return true
                    }
                }
            }
            .padding(14)
        }
        .background(K.main)
    }

    struct Card: View {
        let d: Docket
        @State private var hover = false
        var body: some View {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    IdText(id: d.id)
                    Spacer()
                    if let a = d.agent { AppAvatar(app: a.app, size: 16) }
                }
                Text(d.title).font(.system(size: 13, weight: .medium)).foregroundStyle(K.text).lineLimit(3).frame(maxWidth: .infinity, alignment: .leading)
                HStack(spacing: 6) {
                    StatusIcon(status: d.status, size: 13)
                    if let l = d.live {
                        LiveIcon(state: l.state, size: 12)
                        Text(l.state.word).font(.system(size: 11.5)).foregroundStyle(.secondary)
                    }
                    if let why = d.hold {
                        Label("On hold", systemImage: "pause.circle").font(.system(size: 11.5)).foregroundStyle(K.blocked).help(why)
                    }
                    Spacer()
                    if let s = d.agent?.rexSession, !s.isEmpty { SessionIcon(name: s, size: 14) }
                }
            }
            .padding(11)
            .background(K.elev, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous).strokeBorder(hover ? K.line2 : K.line))
            .shadow(color: .black.opacity(hover ? 0.08 : 0.03), radius: hover ? 6 : 2, y: 1)
            .onHover { hover = $0 }
            .contentShape(Rectangle())
        }
    }
}

// ---- agents ----

struct AgentsScreen: View {
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav

    /// Group the agents by their session, or by the docket they're on.
    @AppStorage("agents:group") private var group = "session"

    var body: some View {
        let agents = store.state?.agents ?? []
        let byDocket = group == "docket"
        // A docket's agents together, newest docket first; those on none last.
        let groups: [(key: String, agents: [Live])] = byDocket
            ? Dictionary(grouping: agents) { $0.docket ?? "" }.map { ($0.key, $0.value) }.sorted { a, b in
                a.key.isEmpty != b.key.isEmpty ? !a.key.isEmpty : (store.docket(a.key)?.updatedAt ?? "") > (store.docket(b.key)?.updatedAt ?? "")
            }
            : Dictionary(grouping: agents) { $0.rexSession.isEmpty ? "—" : $0.rexSession }.map { ($0.key, $0.value) }.sorted { $0.key < $1.key }
        Group {
            if store.state != nil && agents.isEmpty {
                ContentUnavailableView("No agents running", systemImage: "terminal", description: Text("Every Claude Code, Codex, Antigravity and Pi pane in \(store.state?.followed ?? "Rex") shows up here, live."))
            } else {
                SplitPane {
                    List(selection: Binding<String?>(get: { nav.selected(.agents) }, set: { nav.select($0, in: .agents) })) {
                        ForEach(groups, id: \.key) { g in
                            Section {
                                ForEach(g.agents, id: \.block) { a in
                                    AgentRow(a: a, d: store.docket(a.docket), inDocket: byDocket)
                                        .tag(a.docket ?? a.block)
                                        .contextMenu { if let d = store.docket(a.docket) { DocketMenu(d: d) } }
                                }
                            } header: {
                                if byDocket {
                                    docketHeader(g.key, g.agents)
                                } else {
                                    sessionHeader(g.key, g.agents)
                                }
                            }
                        }
                    }
                    .listStyle(.inset)
                    .scrollContentBackground(.hidden)
                } detail: {
                    DocketDetail(id: nav.selected(.agents).flatMap { sel in store.docket(sel) != nil ? sel : agents.first { $0.block == sel }?.docket })
                }
            }
        }
        .navigationSubtitle(agents.isEmpty ? "" : "\(agents.count) in \(store.state?.followed ?? "Rex")")
        .toolbar {
            ToolbarItem {
                Picker("Group by", selection: $group) {
                    Text("Session").tag("session")
                    Text("Docket").tag("docket")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .help("Group the agents by their session, or by the docket they're on")
            }
        }
    }

    private func sessionHeader(_ s: String, _ agents: [Live]) -> some View {
        HStack(spacing: 7) {
            SessionIcon(name: s, size: 14)
            Text(s).font(.system(size: 12, weight: .semibold)).foregroundStyle(K.text)
            Text("\(agents.count)").font(.system(size: 11.5)).foregroundStyle(.tertiary)
            // Which runtime the session's agents are in: Rex, Herdr.
            ForEach(Array(Set(agents.map(\.runtime))).sorted(), id: \.self) { r in
                Text(r).font(.system(size: 10.5, weight: .medium)).foregroundStyle(.secondary)
                    .padding(.horizontal, 6).padding(.vertical, 1)
                    .overlay(Capsule().strokeBorder(.separator))
                    .help("In \(r)")
            }
        }
    }

    private func docketHeader(_ id: String, _ agents: [Live]) -> some View {
        let d = store.docket(id)
        return HStack(spacing: 7) {
            if let d { StatusIcon(status: d.status, size: 12) } else { Image(systemName: "questionmark.circle").font(.system(size: 11)).foregroundStyle(K.muted) }
            if !id.isEmpty { IdText(id: id) }
            Text(id.isEmpty ? "On no docket" : d?.title ?? id).font(.system(size: 12, weight: .semibold)).foregroundStyle(K.text).lineLimit(1)
            Text("\(agents.count)").font(.system(size: 11.5)).foregroundStyle(.tertiary)
        }
    }
}

struct AgentRow: View {
    let a: Live
    let d: Docket?
    /// Under its docket's header: name the agent, not the docket again.
    var inDocket = false
    var body: some View {
        // Its role or pane among its docket's others, as the docket names it.
        let member = d?.member(block: a.block)
        let name = inDocket ? (member?.label ?? a.role ?? (a.pane.isEmpty ? appName(a.app) : a.pane)) : d?.title ?? appName(a.app)
        HStack(alignment: .top, spacing: 9) {
            LiveIcon(state: a.state, size: 14).padding(.top, 1.5)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(name).font(.system(size: 13, weight: .medium)).lineLimit(1)
                    if !inDocket, d?.several == true, let label = member?.label ?? a.role {
                        Text(label).font(.system(size: 11.5)).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer(minLength: 4)
                    AppAvatar(app: a.app, size: 14)
                }
                HStack(spacing: 6) {
                    if !inDocket, let id = a.docket { IdText(id: id) }
                    Text(a.state == .blocked ? a.msg ?? a.state.word : a.state.word).foregroundStyle(a.state == .blocked ? K.blocked : a.state == .error ? K.error : .secondary).lineLimit(1)
                    Text("· \(a.pane.isEmpty ? "pane" : a.pane)")
                    Spacer(minLength: 0)
                    Text(shortPath(a.cwd)).lineLimit(1).truncationMode(.head)
                }
                .font(.system(size: 11.5))
                .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
    }
}
