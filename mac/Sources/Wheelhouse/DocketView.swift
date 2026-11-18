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
            .sheet(item: Binding(get: { nav.pane.map(IdBox.init) }, set: { nav.pane = $0?.id })) { b in PaneSheet(id: b.id) }
            .sheet(item: Binding(get: { nav.record.map(IdBox.init) }, set: { nav.record = $0?.id })) { b in RecordSheet(id: b.id) }
            .sheet(item: Binding(get: { nav.linking.map(IdBox.init) }, set: { nav.linking = $0?.id })) { b in LinkIssueSheet(docket: b.id) }
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
                        ScrollView {
                            DocketBody(page: page, embedded: embedded)
                                .padding(.horizontal, embedded ? 24 : 36)
                                .padding(.vertical, 24)
                                .frame(maxWidth: 820)
                                .frame(maxWidth: .infinity)
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
                Button { store.openInRex(d.id) } label: { Label("Open in Rex", systemImage: "terminal") }
                    .help("Bring the agent's pane to the front in Rex (O)")
                Button { nav.pane = d.id } label: { Label("Peek", systemImage: "eye") }
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

    var body: some View {
        let d = page.docket
        VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 8) {
                if embedded {
                    HStack(spacing: 8) {
                        IdText(id: d.id)
                        Spacer()
                        if d.live != nil {
                            Button("Open in Rex", systemImage: "terminal") { store.openInRex(d.id) }.controlSize(.small)
                            Button { nav.pane = d.id } label: { Image(systemName: "eye") }.controlSize(.small).help("Peek at the pane")
                        }
                        Button("Open Docket") { openWindow(id: "docket", value: d.id) }.controlSize(.small)
                    }
                }
                TitleView(d: d)
                Chips(d: d)
            }
            if let l = d.live, l.state == .blocked {
                Callout(tone: .blocked) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("\(appName(d.agent?.app)) is waiting on you\(l.kind.map { " · \($0)" } ?? "")").font(.system(size: 13, weight: .semibold))
                        if let m = l.msg { Text(m).font(.system(size: 12.5, design: .monospaced)).foregroundStyle(K.text2).textSelection(.enabled) }
                    }
                    Spacer(minLength: 8)
                    if l.answerable { AnswerButtons(d: d) } else { Button("Answer in Rex") { store.openInRex(d.id) } }
                }
            }
            if let l = d.live, l.state == .error {
                Callout(tone: .error) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("\(appName(d.agent?.app)) failed").font(.system(size: 13, weight: .semibold))
                        if let m = l.msg { Text(m).font(.system(size: 12.5)).foregroundStyle(K.text2).textSelection(.enabled) }
                    }
                }
            }
            ReviewPanel(d: d)
            BuildPanel(d: d)
            ForEach((store.state?.messages ?? []).filter { $0.docket == d.id }) { m in MessageCard(m: m) }
            IntentSection(d: d, entries: page.entries)
            DecisionsSection(decisions: page.decisions)
            ArtifactsSection(d: d, entries: page.entries)

            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("Activity").font(.system(size: 13, weight: .semibold)).foregroundStyle(K.text)
                    Spacer()
                    VerifiedChip(page: page)
                }
                let all = page.entries
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

/// Approve and Deny for a prompt the hub can answer.
struct AnswerButtons: View {
    let d: Docket
    var small = false
    @Environment(Store.self) private var store
    @State private var busy: Bool?

    var body: some View {
        HStack(spacing: 6) {
            Button(busy == true ? "Approving…" : "Approve") { go(true) }
                .buttonStyle(.borderedProminent)
                .tint(K.accent)
                .help("Approve: \(d.live?.msg ?? "")")
            Button(busy == false ? "Denying…" : "Deny") { go(false) }
                .help("Deny (Esc in the pane)")
        }
        .controlSize(small ? .small : .regular)
        .disabled(busy != nil)
    }

    private func go(_ approve: Bool) {
        busy = approve
        Task {
            await store.answer(d, approve: approve)
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
                if d.proto != "review" {
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
                if let a = d.agent, d.proto != "review", task == nil {
                    prop("Agent") { HStack(spacing: 6) { AppAvatar(app: a.app, size: 15); Text(appName(a.app)) } }
                    prop("Now") {
                        HStack(spacing: 6) {
                            LiveIcon(state: d.live?.state ?? .idle, size: 12)
                            Text(d.live.map { "\($0.state.word)\($0.since.map { " · \(ago($0))" } ?? "")" } ?? (d.released.map { "Ended · \($0)" } ?? "Not running"))
                        }
                    }
                    prop("Rex") {
                        Button {
                            if d.live != nil { store.openInRex(d.id) }
                        } label: {
                            HStack(spacing: 6) { SessionIcon(name: a.rexSession, size: 14); Text("\(a.rexSession) / \(a.pane.isEmpty ? "pane" : a.pane)") }
                        }
                        .buttonStyle(.plain)
                        .disabled(d.live == nil)
                        .help(d.live == nil ? "" : "Show the pane in Rex")
                    }
                    if let cwd = a.cwd {
                        prop("Folder") {
                            Button(shortPath(cwd)) { NSWorkspace.shared.open(URL(fileURLWithPath: cwd)) }
                                .buttonStyle(.plain).help("\(cwd)\nClick to open it in Finder")
                                .contextMenu { Button("Copy Path") { copy(cwd) } }
                        }
                    }
                    if let s = d.session {
                        prop("Session") { Button(String(s.prefix(8))) { copy(s) }.buttonStyle(.plain).font(.system(size: 12, design: .monospaced)).help("Copy the agent's session id") }
                    }
                }
                prop("Protocol") { Text(task != nil ? "Build" : d.proto == "agent" ? "Agent" : d.proto == "review" ? "Review" : "By hand") }
                if d.proto != "review", task == nil {
                    prop("Work") { Text("\(plural(d.prompts, "prompt")) · \(plural(d.turns, "turn"))") }
                }
                prop("Created") { Text(stamp(d.createdAt)) }
                prop("Updated") { Text(agoLong(d.updatedAt)) }

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

// ---- the note box ----

struct Composer: View {
    let d: Docket
    @Environment(Store.self) private var store
    @State private var text = ""
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Editor(text: $text, placeholder: "Leave a note… (Markdown)", minHeight: 40, maxHeight: 240, onSubmit: send)
            HStack {
                Text("Signed as you").font(.system(size: 11.5)).foregroundStyle(K.faint)
                Spacer()
                Button(action: send) { Text("Note  ⌘↩") }
                    .buttonStyle(.borderedProminent)
                    .tint(K.accent)
                    .controlSize(.small)
                    .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy)
            }
        }
        .padding(12)
        .background(K.elev, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(K.line))
    }

    private func send() {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty, !busy else { return }
        busy = true
        Task {
            if await store.note(d.id, t) { text = "" }
            busy = false
        }
    }
}
