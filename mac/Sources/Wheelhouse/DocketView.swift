import SwiftUI
import UniformTypeIdentifiers

/// The docket a list has selected, or a quiet empty state.
struct DocketDetail: View {
    let id: String?
    var body: some View {
        if let id {
            DocketView(id: id).id(id)
        } else {
            ContentUnavailableView("No docket selected", systemImage: "doc.text", description: Text("Pick one from the list."))
        }
    }
}

/// A docket in its own window: the same view, with its own sheets.
struct DocketWindow: View {
    let id: String
    @State private var nav = Nav()
    @Environment(Store.self) private var store

    var body: some View {
        DocketView(id: id, standalone: true)
            .environment(nav)
            .frame(minWidth: 620, minHeight: 480)
            .background(K.main)
            .overlay(alignment: .bottom) { ToastStack() }
            .sheet(item: Binding(get: { nav.pane.map(IdBox.init) }, set: { nav.pane = $0?.id })) { b in PaneSheet(id: b.id, block: nav.paneBlock) }
            .sheet(item: Binding(get: { nav.record.map(IdBox.init) }, set: { nav.record = $0?.id })) { b in RecordSheet(id: b.id) }
            .sheet(item: Binding(get: { nav.linking.map(IdBox.init) }, set: { nav.linking = $0?.id })) { b in LinkIssueSheet(docket: b.id) }
            .sheet(item: Binding(get: { nav.holding.map(IdBox.init) }, set: { nav.holding = $0?.id })) { b in HoldSheet(id: b.id) }
            .sheet(item: Binding(get: { nav.starting.map(IdBox.init) }, set: { nav.starting = $0?.id; if $0 == nil { nav.startingPrompt = nil } })) { b in StartAgentSheet(id: b.id, prompt: nav.startingPrompt) }
            .sheet(item: Binding(get: { nav.closing }, set: { nav.closing = $0 })) { c in CloseOnWordSheet(closing: c) }
            .background(WindowReader { Windows.register($0, nav) })
            .navigationTitle(store.docket(id).map { "\($0.id) · \($0.title)" } ?? id)
    }
}

struct DocketView: View {
    let id: String
    var standalone = false
    /// Inside the Inbox: no Chat/Docket switch, no inspector of its own.
    var embedded = false

    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @State private var page: DocketPage?
    @State private var failure: String?
    @State private var tab: String?
    @State private var dropping = false

    var body: some View {
        @Bindable var nav = nav
        Group {
            if let page {
                let d = page.docket
                let chatting = d.talks && !embedded && (tab ?? "docket") == "chat"
                Group {
                    if chatting {
                        ChatView(d: d, page: page)
                    } else {
                        ScrollViewReader { proxy in
                            ScrollView {
                                DocketBody(page: page, embedded: embedded)
                                    .padding(.horizontal, embedded ? 24 : 36)
                                    .padding(.vertical, 24)
                                    .frame(maxWidth: 820)
                                    .frame(maxWidth: .infinity)
                            }
                            .onReceive(NotificationCenter.default.publisher(for: .snapshotScroll)) { n in
                                if let to = n.object as? String { proxy.scrollTo(to, anchor: .top) }
                            }
                        }
                    }
                }
                .toolbar { if !embedded { toolbar(d) } }
                .inspector(isPresented: embedded ? .constant(false) : $nav.inspector) {
                    Properties(page: page)
                        .inspectorColumnWidth(min: 240, ideal: 270, max: 360)
                }
            } else if let failure {
                ContentUnavailableView("Can't open \(id)", systemImage: "exclamationmark.triangle", description: Text(failure))
            } else {
                ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .task(id: store.version) { await load() }
        // While the Listener reads a call, look again now and then: it can take minutes.
        .task(id: page?.call?.reading == true) {
            guard page?.call?.reading == true else { return }
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(3))
                if Task.isCancelled { return }
                await load()
            }
        }
        .onAppear { nav.current = id }
        .focusedSceneValue(\.docket, embedded ? nil : DocketFocus(id: id, nav: nav))
        .onReceive(NotificationCenter.default.publisher(for: .snapshotTab)) { tab = $0.object as? String }
        .overlay {
            if dropping {
                RoundedRectangle(cornerRadius: 14).strokeBorder(K.accent, style: StrokeStyle(lineWidth: 2, dash: [6, 4]))
                    .background(K.accentSoft.opacity(0.5), in: RoundedRectangle(cornerRadius: 14))
                    .overlay(Label("Keep it with \(id): the record pins it by its hash", systemImage: "tray.and.arrow.down").font(.system(size: 14, weight: .medium)))
                    .padding(10)
                    .allowsHitTesting(false)
            }
        }
        .dropDestination(for: URL.self) { urls, _ in
            Task { for u in urls { u.isFileURL ? await store.attach(id, file: u) : await store.attach(id, link: u) } }
            return !urls.isEmpty
        } isTargeted: { dropping = $0 }
    }

    private func load() async {
        do {
            let p = try await store.page(id)
            if p != page { page = p }
            failure = nil
        } catch {
            if page == nil { failure = Store.words(error) }
        }
    }

    @ToolbarContentBuilder
    private func toolbar(_ d: Docket) -> some ToolbarContent {
        if d.talks {
            ToolbarItem(placement: .principal) {
                Picker("View", selection: Binding(get: { tab ?? "docket" }, set: { tab = $0 })) {
                    Text("Chat").tag("chat")
                    Text("Docket").tag("docket")
                }
                .pickerStyle(.segmented)
                .frame(width: 150)
                .help("Talk to the agent, or read the docket")
            }
        }
        ToolbarItemGroup {
            if d.live != nil {
                Button { store.openInRex(d.id) } label: { Label("Open in \(d.runtime)", systemImage: "terminal") }
                    .help("Bring the agent's pane to the front in \(d.runtime) (O)")
                Button { nav.peek(d.id) } label: { Label("Peek", systemImage: "eye") }
                    .help("Peek at the agent's pane here")
            }
        }
    }
}

/// Everything on a docket's page, top to bottom.
struct DocketBody: View {
    let page: DocketPage
    var embedded = false
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @Environment(\.openWindow) private var openWindow
    @State private var earlier = false
    /// Only this agent's activity (its block), or everyone's.
    @State private var only: String?

    var body: some View {
        let d = page.docket
        VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 8) {
                if embedded {
                    HStack(spacing: 8) {
                        IdText(id: d.id)
                        Spacer()
                        if d.live != nil {
                            Button("Open in \(d.runtime)", systemImage: "terminal") { store.openInRex(d.id) }.controlSize(.small)
                            Button { nav.peek(d.id) } label: { Image(systemName: "eye") }.controlSize(.small).help("Peek at the pane")
                        }
                        Button("Open Docket") { openWindow(id: "docket", value: d.id) }.controlSize(.small)
                    }
                }
                TitleView(d: d)
                Chips(d: d)
            }
            HoldBanner(d: d)
            // Each agent waiting on you, or failed: by name when there are several.
            ForEach(Array(d.lives.enumerated()), id: \.offset) { _, x in
                let name = x.agent.map { d.several ? $0.label : appName($0.app) } ?? appName(d.agent?.app)
                let block = d.blockFor(x.agent)
                let runtime = x.agent?.runtime ?? d.runtime
                let l = x.live
                if l.state == .blocked {
                    Callout(tone: .blocked) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text("\(name) is waiting on you\(l.kind.map { " · \($0)" } ?? "")").font(.system(size: 13, weight: .semibold))
                            if let m = l.msg { Text(m).font(.system(size: 12.5, design: .monospaced)).foregroundStyle(K.text2).textSelection(.enabled) }
                        }
                        Spacer(minLength: 8)
                        if l.answerable { AnswerButtons(d: d, block: block, live: l) } else { Button("Answer in \(runtime)") { store.openInRex(d.id, block: block) } }
                    }
                } else if l.state == .error {
                    Callout(tone: .error) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text("\(name) failed").font(.system(size: 13, weight: .semibold))
                            if let m = l.msg { Text(m).font(.system(size: 12.5)).foregroundStyle(K.text2).textSelection(.enabled) }
                        }
                    }
                }
            }
            if !d.isCall { AgentsSection(d: d) }
            ReviewPanel(d: d)
            BuildPanel(d: d)
            SubagentsSection(d: d)
            ForEach((store.state?.messages ?? []).filter { $0.docket == d.id }) { m in MessageCard(m: m) }
            if !d.isCall { IntentSection(d: d, entries: page.entries) }
            if let call = page.call { CallSection(id: d.id, call: call).id("call") }
            DecisionsSection(decisions: page.decisions, docket: d.id, repo: page.evidence.repo)
            VerifySection(page: page)
            ArtifactsSection(d: d, entries: page.entries)

            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("Activity").font(.system(size: 13, weight: .semibold)).foregroundStyle(K.text)
                    Spacer()
                    VerifiedChip(page: page)
                }
                if d.crew.count > 1 { AgentChips(d: d, only: $only) }
                // One agent's: what it did and was asked, and what's no agent's.
                let all = only.map { b in page.entries.filter { e in d.blockOf(e).map { $0 == b } ?? true } } ?? page.entries
                let shown = earlier || all.count <= 200 ? all : Array(all.suffix(150))
                if shown.count < all.count {
                    Button("Show \(all.count - shown.count) earlier entries") { earlier = true }.buttonStyle(.link)
                }
                LazyVStack(alignment: .leading, spacing: 10) {
                    ForEach(shown) { e in ActivityRow(e: e, d: d, owner: store.state?.owner ?? "you") }
                }
            }
            Composer(d: d)
        }
    }
}

/// The record's check, as a chip: click to read the record.
struct VerifiedChip: View {
    let page: DocketPage
    @Environment(Nav.self) private var nav
    var body: some View {
        let v = page.verify
        Button { nav.record = page.docket.id } label: {
            Label(v.ok ? "Verified · \(v.entries) signed" : "Record doesn't check", systemImage: v.ok ? "checkmark.shield" : "exclamationmark.shield")
                .font(.system(size: 11.5, weight: .medium))
                .foregroundStyle(v.ok ? K.working : K.error)
        }
        .buttonStyle(.plain)
        .help(v.ok ? "Every entry's id, chain link and signature checks. Head \(v.head ?? ""). Click to read the record." : v.problems.joined(separator: "\n"))
    }
}

/// The title: click it (or R) to rename.
struct TitleView: View {
    let d: Docket
    @Environment(Store.self) private var store
    @State private var editing = false
    @State private var value = ""
    @FocusState private var focused: Bool

    var body: some View {
        if editing {
            TextField("Title", text: $value, axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: 24, weight: .semibold))
                .focused($focused)
                .onSubmit(save)
                .onExitCommand { editing = false }
                .onChange(of: focused) { _, f in if !f { save() } }
                .onAppear { focused = true }
        } else {
            Text(d.title)
                .font(.system(size: 24, weight: .semibold))
                .foregroundStyle(K.text)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
                .onTapGesture(count: 2) { start() }
                .contextMenu { Button("Rename") { start() } }
                .help("Double-click to rename")
        }
    }

    private func start() {
        value = d.title
        editing = true
    }

    private func save() {
        guard editing else { return }
        editing = false
        let t = value.trimmingCharacters(in: .whitespacesAndNewlines)
        if !t.isEmpty, t != d.title { store.rename(d.id, t) }
    }
}

/// The docket's status, what it's doing, and what it's linked to, as chips.
struct Chips: View {
    let d: Docket
    @Environment(Store.self) private var store
    var body: some View {
        HStack(spacing: 6) {
            Menu {
                ForEach(Status.allCases) { s in
                    Button { if s != d.status { store.setStatus(d.id, s, undo: d.status) } } label: { Label(s.label, systemImage: d.status == s ? "checkmark" : "") }
                }
            } label: {
                HStack(spacing: 5) { StatusIcon(status: d.status, size: 12); Text(d.status.label) }
            }
            .menuStyle(.button)
            .buttonStyle(ChipButton())
            .fixedSize()
            if let l = d.live {
                HStack(spacing: 5) { LiveIcon(state: l.state, size: 12); Text(l.state.word) }.modifier(ChipLook(tint: K.live(l.state)))
            }
            if let r = d.review {
                Link(destination: r.url ?? URL(string: "https://github.com")!) {
                    HStack(spacing: 5) { Image(systemName: "arrow.triangle.pull"); Text(r.key) }
                }
                .modifier(ChipLook())
            }
            ForEach([d.linear].compactMap { $0 } + d.linearAlso) { l in
                Link(destination: l.url ?? URL(string: "https://linear.app")!) {
                    HStack(spacing: 5) { Image(systemName: "circle.hexagongrid"); Text(l.key) }
                }
                .modifier(ChipLook())
                .help(l.title ?? l.key)
            }
            if store.state?.task(for: d.id) != nil {
                HStack(spacing: 5) { Image(systemName: "hammer"); Text("Building") }.modifier(ChipLook())
            }
            ChangesChip(d: d)
            if d.hold != nil {
                HStack(spacing: 5) { Image(systemName: "pause.circle"); Text("On hold") }.modifier(ChipLook(tint: K.blocked))
                    .help(d.hold ?? "")
            }
            if d.status == .done, let why = d.closedOn {
                HStack(spacing: 5) { Image(systemName: "quote.bubble"); Text("On your word") }.modifier(ChipLook())
                    .help("Closed on your word: \(why)")
            }
        }
        .font(.system(size: 11.5, weight: .medium))
    }
}

struct ChipLook: ViewModifier {
    var tint: Color = K.text2
    func body(content: Content) -> some View {
        content.foregroundStyle(tint).padding(.horizontal, 8).padding(.vertical, 3)
            .background(K.recessed, in: Capsule())
            .overlay(Capsule().strokeBorder(K.line))
    }
}

struct ChipButton: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.modifier(ChipLook()).opacity(configuration.isPressed ? 0.7 : 1)
    }
}

/// Approve and Deny for a prompt the hub can answer: the agent's at
/// `block`, or the docket's lead's.
struct AnswerButtons: View {
    let d: Docket
    var block: String? = nil
    /// The prompt shown, when it's not the lead's.
    var live: Live? = nil
    var small = false
    @Environment(Store.self) private var store
    @State private var busy: Bool?

    var body: some View {
        HStack(spacing: 6) {
            Button(busy == true ? "Approving…" : "Approve") { go(true) }
                .buttonStyle(.borderedProminent)
                .tint(K.accent)
                .help("Approve: \((live ?? d.live)?.msg ?? "")")
            Button(busy == false ? "Denying…" : "Deny") { go(false) }
                .help("Deny (Esc in the pane)")
        }
        .controlSize(small ? .small : .regular)
        .disabled(busy != nil)
    }

    private func go(_ approve: Bool) {
        busy = approve
        Task {
            if let l = live {
                // Exactly the prompt shown: the hub refuses if it changed.
                await store.answer(d.id, record: l.record ?? "", since: l.since, msg: l.msg, approve: approve, block: block)
            } else {
                await store.answer(d, block: block, approve: approve)
            }
            busy = nil
        }
    }
}

// ---- properties ----

struct Properties: View {
    let page: DocketPage
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav

    var body: some View {
        let d = page.docket
        let task = store.state?.task(for: d.id)
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                prop("Status") {
                    Menu {
                        ForEach(Status.allCases) { s in Button(s.label) { if s != d.status { store.setStatus(d.id, s, undo: d.status) } } }
                    } label: { HStack(spacing: 6) { StatusIcon(status: d.status); Text(d.status.label) } }
                        .menuStyle(.borderlessButton).fixedSize()
                }
                if let r = d.review {
                    prop("Review") {
                        Link(destination: r.url ?? URL(string: "https://github.com")!) {
                            Text("\(r.key)\(r.round.map { " · round \($0)" } ?? "")\(r.verdict.map { " · \($0.replacingOccurrences(of: "_", with: " ").lowercased())" } ?? "")")
                        }
                    }
                }
                if d.proto != "review", !d.isCall {
                    prop("Build") {
                        if let t = task {
                            Text("\(t.repo) · \(t.state == "active" ? (t.phase ?? "").replacingOccurrences(of: "_", with: " ") : t.state)")
                        } else if d.hasIntent {
                            BuildItButton(d: d)
                        } else {
                            Text("Needs an intent").foregroundStyle(.tertiary).help("Write the intent first: what this is for, and how you'll know it's done")
                        }
                    }
                }
                prop("Linear") {
                    if let l = d.linear {
                        VStack(alignment: .leading, spacing: 3) {
                            ForEach([l] + d.linearAlso) { x in
                                Link(destination: x.url ?? URL(string: "https://linear.app")!) { Text(x.key) }.help(x.title ?? "")
                            }
                        }
                    } else {
                        Button("Link issue…") { nav.linking = d.id }.buttonStyle(.link)
                    }
                }
                if d.crew.isEmpty, task == nil, d.canStartAgent {
                    prop("Agent") {
                        Button("Start an agent…") { nav.starting = d.id }.buttonStyle(.link)
                            .help("Start Claude Code, Codex, Antigravity or Pi on this docket, in a new \(store.state?.spawnRuntime ?? "Rex") pane")
                    }
                }
                // The lead (or the one agent); the Agents section lists them all.
                if let a = d.onAgents.first ?? d.leftAgents.first, d.proto != "review", !d.isCall, task == nil {
                    let on = d.onAgents.count
                    prop(d.several ? "Lead" : "Agent") {
                        HStack(spacing: 6) {
                            AppAvatar(app: a.app, size: 15)
                            Text(d.several ? a.label : appName(a.app))
                            if on > 1 { Text("+\(on - 1)").foregroundStyle(K.muted).help("\(plural(on, "agent")) are on \(d.id)") }
                        }
                    }
                    prop("Now") {
                        HStack(spacing: 6) {
                            LiveIcon(state: a.live?.state ?? .idle, size: 12)
                            Text(a.live.map { "\($0.state.word)\($0.since.map { " · \(ago($0))" } ?? "")" } ?? (a.on ? "Not running" : "Ended\(a.left.map { " · \($0)" } ?? "")"))
                            if d.canStartAgent {
                                Button(on > 0 ? "Add one…" : "Start one…") { nav.starting = d.id }.buttonStyle(.link)
                                    .help(on > 0 ? "Add an agent beside the \(plural(on, "one")) on this docket, in a new \(store.state?.spawnRuntime ?? "Rex") pane" : "Start an agent on this docket again, in a new \(store.state?.spawnRuntime ?? "Rex") pane")
                            }
                        }
                    }
                    prop(a.runtime) {
                        Button {
                            if a.live != nil { store.openInRex(d.id, block: d.blockFor(a)) }
                        } label: {
                            HStack(spacing: 6) { SessionIcon(name: a.rexSession, size: 14); Text(a.place) }
                        }
                        .buttonStyle(.plain)
                        .disabled(a.live == nil)
                        .help(a.live == nil ? "" : "Show the pane in \(a.runtime)")
                    }
                    if let cwd = a.cwd {
                        prop("Folder") {
                            Button(shortPath(cwd)) { NSWorkspace.shared.open(URL(fileURLWithPath: cwd)) }
                                .buttonStyle(.plain).help("\(cwd)\nClick to open it in Finder")
                                .contextMenu { Button("Copy Path") { copy(cwd) } }
                        }
                    }
                    if let s = a.session ?? d.session {
                        prop("Session") { Button(String(s.prefix(8))) { copy(s) }.buttonStyle(.plain).font(.system(size: 12, design: .monospaced)).help("Copy the agent's session id") }
                    }
                }
                prop("Protocol") { Text(task != nil ? "Build" : d.proto == "agent" ? "Agent" : d.proto == "review" ? "Review" : d.isCall ? "Call" : "By hand") }
                prop("Hold") {
                    if let why = d.hold {
                        HStack(spacing: 8) {
                            Text("On hold").foregroundStyle(K.blocked).help(why)
                            Button("Resume") { store.resume(d.id) }.buttonStyle(.link)
                        }
                    } else {
                        Button("Hold…") { nav.holding = d.id }.buttonStyle(.link)
                            .help("Build and Review take no steps on it until you resume it")
                    }
                }
                if d.proto != "review", !d.isCall, task == nil {
                    prop("Work") { Text("\(plural(d.prompts, "prompt")) · \(plural(d.turns, "turn"))") }
                }
                prop("Created") { Text(stamp(d.createdAt)) }
                prop("Updated") { Text(agoLong(d.updatedAt)) }
                if let to = d.sharedAs {
                    prop("Shared as") {
                        Button(to) { nav.show(to, store: store) }.buttonStyle(.link)
                            .help("This docket went on the workspace; it continues there")
                    }
                }
                if !d.owners.isEmpty {
                    let me = store.state?.team?.you ?? ""
                    Text("Team").sectionLabel().padding(.top, 18).padding(.bottom, 6)
                    prop("Owners") { HStack(spacing: 8) { ForEach(d.owners, id: \.self) { PersonChip(handle: $0, you: $0 == me) } } }
                    prop("Members") {
                        if d.members.isEmpty {
                            Text("None yet").foregroundStyle(.secondary)
                        } else {
                            HStack(spacing: 8) { ForEach(d.members, id: \.self) { PersonChip(handle: $0, you: $0 == me) } }
                        }
                    }
                    if !me.isEmpty, !d.owners.contains(me), !d.members.contains(me) {
                        prop("") {
                            Button("Join as a member") { Task { await store.act("/api/team/dockets/\(d.id)/people", ["handle": me, "role": "member"], done: "You're on \(d.id)") } }.buttonStyle(.link)
                        }
                    }
                    let theirs = d.agents.filter { $0.on && $0.member != nil && $0.member != me }
                    if !theirs.isEmpty {
                        prop("Agents") {
                            VStack(alignment: .leading, spacing: 3) {
                                ForEach(theirs) { a in
                                    Text("\(a.member ?? "")'s \(appName(a.app))\(a.role.map { " · \($0)" } ?? "")")
                                        .help("On \(a.member ?? "")'s Mac. Reach them with wh send --to @\(d.id)")
                                }
                            }
                        }
                    }
                    if let from = d.sharedFrom { prop("From") { Text(from).font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary) } }
                }

                Text("Record").sectionLabel().padding(.top, 18).padding(.bottom, 6)
                prop("Check") {
                    Button { nav.record = d.id } label: {
                        Label(page.verify.ok ? "Intact · \(page.verify.entries) signed" : page.verify.problems.first ?? "Doesn't check", systemImage: "checkmark.shield")
                            .foregroundStyle(page.verify.ok ? K.working : K.error)
                    }
                    .buttonStyle(.plain)
                    .help("Open the record: every signed entry, and what the check means")
                }
                prop("Head") {
                    Button(d.shortHead) { copy("Docket: \(d.id) @ \(d.shortHead)") }.buttonStyle(.plain).font(.system(size: 12, design: .monospaced))
                        .help("Copy “Docket: \(d.id) @ \(d.shortHead)”, for a PR or a note")
                }
                prop("Record") {
                    HStack(spacing: 10) {
                        Button("Open") { nav.record = d.id }.buttonStyle(.link)
                        Button("Export…") { Exporter.export(d.id, store: store) }.buttonStyle(.link)
                            .help("Save \(d.id).jsonl: it checks without the hub (wh verify --file)")
                    }
                }
            }
            .padding(16)
            .font(.system(size: 12.5))
        }
        .background(K.main)
    }

    private func prop(_ label: String, @ViewBuilder _ value: () -> some View) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(label).foregroundStyle(K.muted).frame(width: 64, alignment: .leading)
            value().foregroundStyle(K.text).frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, 5)
    }

    private func copy(_ s: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(s, forType: .string)
        store.toast("Copied \(s)")
    }
}

/// Build it: give the docket's intent to a builder and a checker.
struct BuildItButton: View {
    let d: Docket
    @Environment(Store.self) private var store
    var body: some View {
        let repos = store.state?.build?.repos ?? []
        if repos.count == 1 {
            Button("Build it", systemImage: "hammer") { store.build(d, into: repos[0]) }.buttonStyle(.link)
                .help("Give this docket's intent to a builder and a checker, into \(repos[0])")
        } else if repos.isEmpty {
            Text("No repos in build.lua").foregroundStyle(.tertiary)
        } else {
            Menu("Build it…") {
                Section("Build into") { ForEach(repos, id: \.self) { r in Button(r) { store.build(d, into: r) } } }
            }
            .menuStyle(.borderlessButton).fixedSize()
        }
    }
}

/// Save a docket's record as JSON lines.
enum Exporter {
    @MainActor static func export(_ id: String, store: Store) {
        let panel = NSSavePanel()
        panel.nameFieldStringValue = "\(id).jsonl"
        panel.allowedContentTypes = [UTType(filenameExtension: "jsonl") ?? .json]
        panel.message = "The record checks without the hub: wh verify --file \(id).jsonl"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task {
            do {
                try await store.hub.data("/api/dockets/\(id)/export").write(to: url)
                store.toast("Saved \(url.lastPathComponent)")
            } catch {
                store.toast(Store.words(error), error: true)
            }
        }
    }
}

// ---- its agents ----

/// Every agent on the docket, side by side: what each is, where it runs,
/// what it's doing, and its pane and prompt to act on. Those that left fold
/// into Earlier.
struct AgentsSection: View {
    let d: Docket
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @State private var earlier = false

    var body: some View {
        let on = d.onAgents
        let left = d.leftAgents
        // One agent, ever: the inspector says all there is, as it always has.
        if d.crew.count > 1 {
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 8) {
                    Text("Agents").font(.system(size: 13, weight: .semibold)).foregroundStyle(K.text)
                    Text(on.isEmpty ? "none on it now" : "\(on.count) on it").font(.system(size: 12)).foregroundStyle(K.muted)
                    Spacer()
                    if d.canStartAgent, store.state?.task(for: d.id) == nil {
                        Button(on.isEmpty ? "Start an agent…" : "Add an agent…", systemImage: "plus") { nav.starting = d.id }
                            .buttonStyle(.link).font(.system(size: 12))
                            .help(on.isEmpty ? "Start an agent on this docket, in a new \(store.state?.spawnRuntime ?? "Rex") pane" : "Add an agent beside the \(plural(on.count, "one")) on it, in a new \(store.state?.spawnRuntime ?? "Rex") pane")
                    }
                }
                if !on.isEmpty {
                    VStack(spacing: 0) {
                        ForEach(Array(on.enumerated()), id: \.element.id) { i, a in
                            if i > 0 { Divider() }
                            AgentLine(d: d, a: a, lead: i == 0 && on.count > 1)
                        }
                    }
                    .background(K.elev, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                    .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(K.line))
                }
                if !left.isEmpty {
                    Button {
                        withAnimation(.easeOut(duration: 0.15)) { earlier.toggle() }
                    } label: {
                        HStack(spacing: 5) {
                            Image(systemName: "chevron.right").font(.system(size: 9, weight: .semibold)).rotationEffect(.degrees(earlier ? 90 : 0))
                            Text("Earlier").fontWeight(.medium)
                            Text("\(left.count)").foregroundStyle(.tertiary)
                        }
                        .font(.system(size: 12))
                        .foregroundStyle(K.muted)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help("The agents that left this docket")
                    if earlier {
                        VStack(spacing: 0) {
                            ForEach(Array(left.enumerated()), id: \.element.id) { i, a in
                                if i > 0 { Divider() }
                                AgentLine(d: d, a: a, lead: false)
                            }
                        }
                        .background(K.recessed, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(K.line))
                    }
                }
            }
        }
    }
}

/// One agent on a docket, as a row: who, where, what now, and its actions.
struct AgentLine: View {
    let d: Docket
    let a: DocketAgent
    /// It's first, and there are others: the one that needs you most.
    var lead = false
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav

    var body: some View {
        let l = a.live
        let block = d.blockFor(a)
        HStack(alignment: .top, spacing: 10) {
            AppAvatar(app: a.app, size: 20).padding(.top, 1)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(a.label).font(.system(size: 13, weight: .medium)).foregroundStyle(K.text).lineLimit(1)
                    if a.label != appName(a.app) { Text(appName(a.app)).foregroundStyle(K.muted) }
                    if a.role != nil, let r = a.round { Text("round \(r)").foregroundStyle(K.muted) }
                    if lead { Text("Lead").font(.system(size: 10.5, weight: .medium)).foregroundStyle(K.accent).help("The one that needs you most: the apps put it first, and pane actions without a choice go to it") }
                    Text(a.runtime).font(.system(size: 10.5, weight: .medium)).foregroundStyle(.secondary)
                        .padding(.horizontal, 6).padding(.vertical, 1)
                        .overlay(Capsule().strokeBorder(K.line))
                }
                .font(.system(size: 12))
                HStack(spacing: 6) {
                    if a.on {
                        LiveIcon(state: l?.state ?? .idle, size: 11)
                        Text(l.map { "\($0.state.word)\($0.since.map { " · \(ago($0))" } ?? "")" } ?? "Not running")
                            .foregroundStyle(l?.state == .blocked ? K.blocked : l?.state == .error ? K.error : K.text2)
                            .lineLimit(1)
                            .fixedSize()
                    } else {
                        Text("Left\(a.leftAt.map { " \(agoLong($0))" } ?? "")\(a.left.map { " · \($0)" } ?? "")")
                    }
                    Text("·").foregroundStyle(K.faint)
                    Text(a.place).lineLimit(1)
                    if let cwd = a.cwd {
                        Text("·").foregroundStyle(K.faint)
                        Text(shortPath(cwd)).lineLimit(1).truncationMode(.head).help(cwd)
                    }
                }
                .font(.system(size: 11.5))
                .foregroundStyle(K.muted)
                if let l, l.state == .blocked, let m = l.msg {
                    Text(m).font(.system(size: 11.5, design: .monospaced)).foregroundStyle(K.text2).lineLimit(2).textSelection(.enabled)
                }
                // Under what it asks, so a narrow column keeps the line above whole.
                if let l, a.on, l.state == .blocked, l.answerable {
                    AnswerButtons(d: d, block: block, live: l, small: true).padding(.top, 3)
                }
            }
            Spacer(minLength: 8)
            if l != nil, a.on {
                HStack(spacing: 6) {
                    Button("Open in \(a.runtime)", systemImage: "terminal") { store.openInRex(d.id, block: block) }
                        .labelStyle(.iconOnly)
                        .help("Bring \(a.label)'s pane to the front in \(a.runtime)")
                    Button { nav.peek(d.id, block: block) } label: { Image(systemName: "eye") }
                        .help("Peek at \(a.label)'s pane here")
                }
                .controlSize(.small)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 9)
        .contextMenu {
            if let addr = a.addr {
                Button("Copy Address") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(addr, forType: .string)
                    store.toast("Copied \(addr)")
                }
            }
            if let s = a.session {
                Button("Copy Session ID") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(s, forType: .string)
                }
            }
        }
    }
}

// ---- the note box ----

struct Composer: View {
    let d: Docket
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @State private var text = ""
    @State private var busy = false

    var body: some View {
        let kind = Compose.kind(text)
        let agents = d.onAgents.filter { $0.live != nil }.map { (pane: $0.pane, role: $0.role, app: $0.app) }
        let sugg = Compose.suggestions(text, agents: agents, docket: d.id)
        let warn = Compose.warning(text, agents: agents)
        VStack(alignment: .leading, spacing: 8) {
            if !sugg.isEmpty {
                VStack(alignment: .leading, spacing: 1) {
                    ForEach(sugg, id: \.label) { s in
                        Button { text = s.insert } label: {
                            HStack(spacing: 10) {
                                Text(s.label).font(.system(size: 12, design: .monospaced)).foregroundStyle(K.text)
                                Text(s.sub).font(.system(size: 11.5)).foregroundStyle(K.muted)
                                Spacer(minLength: 0)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .padding(.vertical, 3)
                    }
                }
                Divider()
            }
            Editor(text: $text, placeholder: "Leave a note… (Markdown) · @agent, /ask, /decide, /task", minHeight: 40, maxHeight: 240, onSubmit: send)
            HStack {
                Text(warn?.text ?? Compose.hint(kind, d.id)).font(.system(size: 11.5)).foregroundStyle(warn == nil ? K.faint : K.blocked)
                Spacer()
                Button(action: send) { Text("\(Compose.button(kind))  ⌘↩") }
                    .buttonStyle(.borderedProminent)
                    .tint(K.accent)
                    .controlSize(.small)
                    .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy || warn?.blocked == true)
            }
        }
        .padding(12)
        .background(K.elev, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(K.line))
        .onReceive(NotificationCenter.default.publisher(for: .taskStarted)) { n in
            if n.object as? String == d.id, Compose.kind(text) == .task { text = "" }
        }
    }

    private func send() {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty, !busy else { return }
        busy = true
        Task {
            if let r = await store.compose(d.id, t) {
                // A task's text stays until its sheet starts the agent: cancelling it loses nothing.
                if r["did"].str == "task" {
                    nav.startingPrompt = r["prompt"].str
                    nav.starting = d.id
                } else {
                    text = ""
                }
            }
            busy = false
        }
    }
}
