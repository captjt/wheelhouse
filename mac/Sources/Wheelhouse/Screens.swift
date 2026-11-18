import SwiftUI

// ---- the Inbox ----

/// What needs you, newest worry first: prompts, reviews to post, messages,
/// failures, then agents that finished a turn and wait for your next word.
struct InboxScreen: View {
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        let items = store.inbox
        let current = items.first { $0.id == nav.selected(.inbox) } ?? items.first
        let blocked = items.filter { $0.kind == .blocked }.count
        let failed = items.filter { $0.kind == .error }.count
        Group {
            if store.state != nil && items.isEmpty {
                ContentUnavailableView {
                    Label("You're all caught up", systemImage: "tray")
                } description: {
                    Text("Nothing is waiting on you. Agents that block, fail, or finish a turn show up here.")
                }
            } else {
                SplitPane {
                    List(selection: Binding<String?>(get: { current?.id }, set: { nav.select($0, in: .inbox) })) {
                        ForEach(items) { it in
                            InboxRow(it: it)
                                .tag(it.id)
                                .swipeActions(edge: .trailing, allowsFullSwipe: it.kind != .blocked) { trailing(it) }
                                .swipeActions(edge: .leading) { leading(it) }
                        }
                    }
                    .listStyle(.inset)
                    .scrollContentBackground(.hidden)
                    .contextMenu(forSelectionType: String.self) { sel in
                        if let it = items.first(where: { $0.id == sel.first }), let d = it.docket { DocketMenu(d: d) }
                    } primaryAction: { sel in
                        if let it = items.first(where: { $0.id == sel.first }), it.docket != nil { openWindow(id: "docket", value: it.docketId) }
                    }
                    .onKeyPress(characters: .init(charactersIn: "jkoe"), phases: .down) { press in
                        let ids = items.map(\.id)
                        guard press.modifiers.isEmpty, !typing(), !ids.isEmpty else { return .ignored }
                        let i = current.flatMap { ids.firstIndex(of: $0.id) } ?? 0
                        switch press.characters {
                        case "j": nav.select(ids[min(ids.count - 1, i + 1)], in: .inbox)
                        case "k": nav.select(ids[max(0, i - 1)], in: .inbox)
                        case "o": if let d = current?.docket, d.live != nil { store.openInRex(d.id) }
                        case "e": if let d = current?.docket { store.setStatus(d.id, .done, undo: d.status) }
                        default: return .ignored
                        }
                        return .handled
                    }
                } detail: {
                    if let it = current {
                        if let d = it.docket, d.entries > 0 {
                            DocketView(id: d.id, embedded: true).id(d.id)
                        } else if let m = it.msg {
                            ScrollView { MessageCard(m: m).padding(24).frame(maxWidth: 760) }
                        } else if let p = it.pr {
                            ContentUnavailableView("Review of \(p.key)", systemImage: "arrow.triangle.pull", description: Text(it.line))
                        }
                    }
                }
            }
        }
        .navigationSubtitle(items.isEmpty ? "" : [blocked > 0 ? "\(blocked) waiting on you" : nil, failed > 0 ? "\(failed) failed" : nil, "\(items.count) in all"].compactMap { $0 }.joined(separator: " · "))
    }

    @ViewBuilder
    private func leading(_ it: InboxItem) -> some View {
        if it.kind == .blocked, let d = it.docket, d.live?.answerable == true {
            Button { Task { await store.answer(d, approve: true) } } label: { Label("Approve", systemImage: "checkmark") }.tint(K.working)
            Button { Task { await store.answer(d, approve: false) } } label: { Label("Deny", systemImage: "xmark") }.tint(K.error)
        }
    }

    @ViewBuilder
    private func trailing(_ it: InboxItem) -> some View {
        if let m = it.msg {
            Button { store.file(m) } label: { Label("File", systemImage: "checkmark") }.tint(K.accent)
        } else if it.kind != .review, let d = it.docket, d.entries > 0 {
            Button { store.setStatus(d.id, .done, undo: d.status) } label: { Label("Done", systemImage: "checkmark") }.tint(K.accent)
        }
    }
}

struct InboxRow: View {
    let it: InboxItem
    @Environment(Store.self) private var store

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            ZStack(alignment: .bottomTrailing) {
                if it.pr != nil {
                    Image(systemName: "arrow.triangle.pull").font(.system(size: 12)).foregroundStyle(K.muted).frame(width: 24, height: 24).background(K.recessed, in: RoundedRectangle(cornerRadius: 6))
                } else if it.msg?.from == "wheelhouse" {
                    HubAvatar(size: 24)
                } else {
                    AppAvatar(app: it.docket?.agent?.app ?? (it.msg?.fromApp == "Codex" ? "codex" : "claude-code"), size: 24)
                }
                LiveIcon(state: it.kind == .error ? .error : it.kind == .done ? .done : .blocked, size: 12)
                    .background(Circle().fill(K.main).padding(-1.5))
                    .offset(x: 4, y: 4)
            }
            .padding(.top, 1)
            VStack(alignment: .leading, spacing: 4) {
                HStack(alignment: .firstTextBaseline) {
                    Text(it.title).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                    Spacer(minLength: 4)
                    Text(ago(it.at)).font(.system(size: 11.5)).foregroundStyle(.tertiary)
                }
                (Text(it.docketId).font(.system(size: 11.5, design: .monospaced)) + Text("  ") + Text(prefix) + Text(it.line))
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                if let p = it.pr, p.attention?.kind == "ready" {
                    ReviewActions(p: p, small: true)
                } else if let m = it.msg, !m.options.isEmpty {
                    OptionButtons(m: m)
                } else if it.kind == .blocked, let d = it.docket, d.live?.answerable == true {
                    AnswerButtons(d: d, small: true)
                }
            }
        }
        .padding(.vertical, 5)
    }

    private var prefix: String {
        switch it.kind {
        case .blocked: it.docket?.live?.kind.map { "\($0): " } ?? "Needs you: "
        case .error: "Failed: "
        default: ""
        }
    }
}

/// A message's options, as buttons, in a row.
struct OptionButtons: View {
    let m: Msg
    @Environment(Store.self) private var store
    @State private var busy: String?
    var body: some View {
        HStack(spacing: 6) {
            ForEach(Array(m.options.enumerated()), id: \.offset) { i, o in
                if i == 0 {
                    Button(busy == o ? "Sending…" : o) { pick(o) }.buttonStyle(.borderedProminent).tint(K.accent)
                } else {
                    Button(busy == o ? "Sending…" : o) { pick(o) }
                }
            }
        }
        .controlSize(.small)
        .disabled(busy != nil)
    }

    private func pick(_ o: String) {
        busy = o
        Task {
            _ = await store.reply(m, "", choice: o)
            busy = nil
        }
    }
}

// ---- Reviews ----

struct ReviewsScreen: View {
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @State private var folded: Set<String> = ["done"]

    var body: some View {
        let rv = store.state?.review
        let prs = rv?.prs ?? []
        let sorted = { (f: (ReviewPr) -> Bool) in prs.filter(f).sorted { ($0.updatedAt ?? "") > ($1.updatedAt ?? "") } }
        let groups: [(String, String, LiveState, [ReviewPr])] = [
            ("needs", "Needs you", .blocked, sorted { $0.attention != nil }),
            ("reviewing", "Reviewing", .working, sorted { $0.attention == nil && $0.state == "reviewing" }),
            ("queued", "Queued", .idle, sorted { $0.attention == nil && $0.state == "queued" }),
            ("posted", "Waiting on the author", .done, sorted { $0.attention == nil && $0.state == "posted" }),
            ("done", "Done this week", .idle, sorted { $0.attention == nil && ["merged", "closed", "cleaned", "unrequested"].contains($0.state) }),
        ].filter { !$0.3.isEmpty }
        let live = prs.filter { ["queued", "reviewing", "posted"].contains($0.state) }.count

        Group {
            if let rv, !rv.enabled {
                ReviewsOff(rv: rv)
            } else if store.state != nil && groups.isEmpty {
                ContentUnavailableView("No reviews asked of you", systemImage: "arrow.triangle.pull", description: Text("Each PR that requests your review, in the repos review.lua lists, shows up here."))
            } else {
                VStack(spacing: 0) {
                    if let rv { ReviewsBar(rv: rv) }
                    Divider()
                    SplitPane {
                        List(selection: Binding<String?>(get: { nav.selected(.reviews) }, set: { nav.select($0, in: .reviews) })) {
                            ForEach(groups, id: \.0) { key, label, live, items in
                                Section {
                                    if !folded.contains(key) { ForEach(items) { p in ReviewRow(p: p, paused: rv?.paused ?? false).tag(p.docket ?? p.fullKey) } }
                                } header: {
                                    FoldHeader(folded: folded.contains(key)) { if folded.contains(key) { folded.remove(key) } else { folded.insert(key) } } label: { HStack(spacing: 7) {
                                        LiveIcon(state: live, size: 13)
                                        Text(label).font(.system(size: 12, weight: .semibold)).foregroundStyle(K.text)
                                        Text("\(items.count)").font(.system(size: 11.5)).foregroundStyle(.tertiary)
                                    } }
                                }
                            }
                        }
                        .listStyle(.inset)
                        .scrollContentBackground(.hidden)
                    } detail: {
                        let sel = nav.selected(.reviews)
                        if let sel, store.docket(sel) != nil {
                            DocketView(id: sel).id(sel)
                        } else if let p = prs.first(where: { $0.fullKey == sel }) {
                            ContentUnavailableView("\(p.key)", systemImage: "arrow.triangle.pull", description: Text(p.title ?? ""))
                        } else {
                            ContentUnavailableView("No review selected", systemImage: "arrow.triangle.pull")
                        }
                    }
                }
            }
        }
        .navigationSubtitle(live > 0 ? "\(live) live" : "")
        .toolbar {
            if let rv, rv.enabled {
                ToolbarItemGroup {
                    Picker("Post", selection: Binding(get: { rv.auto ? "auto" : "ask" }, set: { v in
                        store.reviewSettings(["post": v], done: v == "auto" ? "The checker posts each review" : "You post each review")
                    })) {
                        Text("Ask").tag("ask")
                        Text("Auto").tag("auto")
                    }
                    .pickerStyle(.segmented)
                    .help("Who posts a checked review: you (Ask), or the checker (Auto)")
                    Button { store.reviewSettings(["paused": !rv.paused], done: rv.paused ? "Reviews resumed" : "Reviews paused: no new rounds start") } label: {
                        Label(rv.paused ? "Resume" : "Pause", systemImage: rv.paused ? "play.fill" : "pause.fill")
                    }
                    .help(rv.paused ? "Resume: new rounds start" : "Pause: no new rounds start")
                    Button { store.checkGitHub() } label: { Label("Check now", systemImage: "arrow.clockwise") }
                        .help("Check GitHub for review requests now")
                }
            }
        }
    }
}

struct ReviewsBar: View {
    let rv: ReviewStatus
    var body: some View {
        HStack(spacing: 8) {
            Chip(text: rv.paused ? "Paused" : "On", color: rv.paused ? K.blocked : K.working, filled: false)
            if let doing = rv.doing { Text("\(doing)…").foregroundStyle(K.text2) } else if let e = rv.scanError { Text("GitHub: \(e)").foregroundStyle(K.error) } else {
                Text(rv.lastScanAt == nil ? "Not checked GitHub yet" : ago(rv.lastScanAt) == "now" ? "Checked GitHub just now" : "Checked GitHub \(ago(rv.lastScanAt)) ago").foregroundStyle(K.muted)
            }
            let c = rv.config
            if c.exists {
                Text("·").foregroundStyle(K.faint)
                Text("\(c["reviewers"].array.compactMap { $0["name"].str }.joined(separator: ", ")) → checker \(c["checker"].str ?? "")").foregroundStyle(K.muted).help(c["path"].str ?? "")
                Text("·").foregroundStyle(K.faint)
                Text("up to \(c["max_rounds"].int ?? 0) at once").foregroundStyle(K.muted)
                Text("·").foregroundStyle(K.faint)
                Text(plural(c["repos"].array.count, "repo")).foregroundStyle(K.muted).help(c["repos"].strings.joined(separator: "\n"))
            }
            if let e = rv.configError { Text("Config: \(e)").foregroundStyle(K.error) }
            Spacer()
        }
        .font(.system(size: 12))
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
    }
}

struct ReviewsOff: View {
    let rv: ReviewStatus
    @Environment(Store.self) private var store
    var body: some View {
        ContentUnavailableView {
            Label("Reviews run in Wheelhouse", systemImage: "arrow.triangle.pull")
        } description: {
            Text("Each PR that asks you for a review becomes a Review docket: blind reviewers in Rex, a checker that merges their drafts and rates the merge risk, and a post you approve (or let the checker make).")
            Text(rv.config.exists ? "Its settings are in \(rv.config["path"].str ?? "review.lua"): repos, reviewers, the checker, and your effort policy." : "Set it up in ~/.config/wheelhouse/review.lua (docs/review.md). Moving from rex-reviewer? `wh import rex-reviewer` brings its config and history over.")
            if let e = rv.configError { Text(e).foregroundStyle(K.error) }
        } actions: {
            Button("Turn On Reviews") { store.reviewSettings(["enabled": true], done: "Reviews are on, paused until you resume") }
                .buttonStyle(.borderedProminent)
                .disabled(rv.rexReviewerRunning || rv.configError != nil || !rv.config.exists)
        }
    }
}

struct ReviewRow: View {
    let p: ReviewPr
    let paused: Bool
    @Environment(Store.self) private var store

    var body: some View {
        HStack(alignment: .top, spacing: 9) {
            LiveIcon(state: reviewLive(p), size: 14).padding(.top, 1.5).help(phaseText(p, paused: paused))
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text(p.title ?? p.key).font(.system(size: 13, weight: .medium)).lineLimit(1)
                    Spacer(minLength: 4)
                    Text(ago(p.updatedAt)).font(.system(size: 11.5)).foregroundStyle(.tertiary)
                }
                HStack(spacing: 6) {
                    if let d = p.docket { IdText(id: d) }
                    Text(p.key).lineLimit(1).truncationMode(.middle)
                    if let a = p.author { Text("· \(a)").lineLimit(1).fixedSize() }
                    Spacer(minLength: 0)
                    if p.round > 0 { Text("R\(p.round)").font(.system(size: 11, design: .monospaced)).fixedSize() }
                }
                .font(.system(size: 11.5))
                .foregroundStyle(.secondary)
                let v = p.state == "reviewing" ? nil : p.verdict ?? p.history.first?.verdict
                if v != nil || p.mergeRisk != nil {
                    HStack(spacing: 5) {
                        VerdictChip(v: v)
                        RiskChip(level: p.mergeRisk)
                    }
                }
                if let a = p.attention {
                    Text(a.kind == "ready" ? draftLine(p) ?? a.text : a.text).font(.system(size: 11.5)).foregroundStyle(K.blocked).lineLimit(2)
                    if a.kind == "ready" { ReviewActions(p: p, small: true) }
                } else if p.state == "reviewing" || p.state == "queued" {
                    Text(phaseText(p, paused: paused)).font(.system(size: 11.5)).foregroundStyle(K.text2)
                }
            }
        }
        .padding(.vertical, 4)
    }
}

// ---- Builds ----

struct BuildsScreen: View {
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @State private var folded: Set<String> = ["done"]

    var body: some View {
        let bs = store.state?.build
        let tasks = bs?.tasks ?? []
        let sorted = { (f: (BuildTask) -> Bool) in tasks.filter(f).sorted { ($0.updatedAt ?? "") > ($1.updatedAt ?? "") } }
        let groups: [(String, String, LiveState, [BuildTask])] = [
            ("needs", "Needs you", .blocked, sorted { $0.needsYou }),
            ("working", "Working", .working, sorted { !$0.needsYou && $0.state == "active" }),
            ("queued", "Queued", .idle, sorted { !$0.needsYou && $0.state == "queued" }),
            ("open", "PRs open", .done, sorted { $0.state == "open" }),
            ("done", "Done this week", .idle, sorted { $0.finished }),
        ].filter { !$0.3.isEmpty }
        let live = tasks.filter { ["queued", "active", "open"].contains($0.state) }.count

        Group {
            if store.state != nil && groups.isEmpty {
                ContentUnavailableView {
                    Label("Nothing's building", systemImage: "hammer")
                } description: {
                    Text("Open a docket, write what it's for in its intent, and press Build it: a builder plans it, you approve the plan, a checker checks the work, and you approve the PR before it opens.")
                    Text("Or from a terminal: `wh build start D-12`, or a GitHub issue (`owner/name#45`).")
                }
            } else {
                VStack(spacing: 0) {
                    if let bs {
                        HStack(spacing: 8) {
                            Chip(text: bs.paused ? "Paused" : "On", color: bs.paused ? K.blocked : K.working)
                            if let doing = bs.doing { Text("\(doing)…").foregroundStyle(K.text2) }
                            let c = bs.config
                            if c.exists {
                                Text("builder \(c["builder"].str ?? "") → checker \(c["checker"].str ?? "")").foregroundStyle(K.muted)
                                Text("·").foregroundStyle(K.faint)
                                Text("plan \(c["gates"]["plan"].str ?? "ask"), open \(c["gates"]["open"].str ?? "ask")").foregroundStyle(K.muted)
                                Text("·").foregroundStyle(K.faint)
                                Text(plural(bs.repos.count, "repo")).foregroundStyle(K.muted).help(bs.repos.joined(separator: "\n"))
                            }
                            if let e = bs.configError { Text("Config: \(e)").foregroundStyle(K.error) }
                            Spacer()
                        }
                        .font(.system(size: 12))
                        .padding(.horizontal, 16).padding(.vertical, 8)
                        Divider()
                    }
                    SplitPane {
                        List(selection: Binding<String?>(get: { nav.selected(.builds) }, set: { nav.select($0, in: .builds) })) {
                            ForEach(groups, id: \.0) { key, label, live, items in
                                Section {
                                    if !folded.contains(key) { ForEach(items) { t in BuildRow(t: t).tag(t.docket) } }
                                } header: {
                                    FoldHeader(folded: folded.contains(key)) { if folded.contains(key) { folded.remove(key) } else { folded.insert(key) } } label: { HStack(spacing: 7) {
                                        LiveIcon(state: live, size: 13)
                                        Text(label).font(.system(size: 12, weight: .semibold)).foregroundStyle(K.text)
                                        Text("\(items.count)").font(.system(size: 11.5)).foregroundStyle(.tertiary)
                                    } }
                                }
                            }
                        }
                        .listStyle(.inset)
                        .scrollContentBackground(.hidden)
                    } detail: {
                        DocketDetail(id: nav.selected(.builds))
                    }
                }
            }
        }
        .navigationSubtitle(live > 0 ? "\(live) live" : "")
        .toolbar {
            if let bs {
                ToolbarItem {
                    Button { store.pauseBuilds(!bs.paused) } label: { Label(bs.paused ? "Resume" : "Pause", systemImage: bs.paused ? "play.fill" : "pause.fill") }
                        .help(bs.paused ? "Resume: queued Builds start" : "Pause: nothing new starts")
                }
            }
        }
    }
}

struct BuildRow: View {
    let t: BuildTask
    var body: some View {
        HStack(alignment: .top, spacing: 9) {
            LiveIcon(state: buildLive(t), size: 14).padding(.top, 1.5)
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline) {
                    Text(t.title).font(.system(size: 13, weight: .medium)).lineLimit(1)
                    Spacer(minLength: 4)
                    Text(ago(t.updatedAt)).font(.system(size: 11.5)).foregroundStyle(.tertiary)
                }
                HStack(spacing: 6) {
                    IdText(id: t.docket)
                    if t.key != t.docket { Text(t.key) }
                    Text("· \(buildPhase(t))")
                    Spacer(minLength: 0)
                    if let b = t.branch { Text(b).font(.system(size: 11, design: .monospaced)).lineLimit(1) }
                }
                .font(.system(size: 11.5))
                .foregroundStyle(.secondary)
                if let a = t.attention { Text(a.text).font(.system(size: 11.5)).foregroundStyle(K.blocked).lineLimit(2) }
            }
        }
        .padding(.vertical, 4)
    }
}
