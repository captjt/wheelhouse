import SwiftUI

func verdictWord(_ v: String?) -> String {
    switch v {
    case "APPROVED": "Approved"
    case "CHANGES_REQUESTED": "Changes requested"
    case "DISMISSED": "Dismissed"
    case "COMMENTED": "Commented"
    case let v?: v.replacingOccurrences(of: "_", with: " ").lowercased()
    default: ""
    }
}

struct VerdictChip: View {
    let v: String?
    var body: some View {
        if let v {
            Chip(text: verdictWord(v), color: v == "APPROVED" ? K.working : v == "CHANGES_REQUESTED" ? K.error : K.muted)
        }
    }
}

struct RiskChip: View {
    let level: String?
    var body: some View {
        if let level { Chip(text: "Risk \(level)", color: K.risk(level)) }
    }
}

/// A protocol's steps, with where it is now.
struct Steps: View {
    let steps: [String]
    let at: Int
    var body: some View {
        HStack(spacing: 0) {
            ForEach(Array(steps.enumerated()), id: \.offset) { i, s in
                HStack(spacing: 6) {
                    Circle()
                        .fill(i < at ? K.accent : i == at ? K.blocked : K.line2)
                        .frame(width: 7, height: 7)
                        .overlay { if i == at { Circle().stroke(K.blocked.opacity(0.35), lineWidth: 4) } }
                    Text(s).font(.system(size: 12, weight: i == at ? .semibold : .regular)).foregroundStyle(i <= at ? K.text : K.muted)
                }
                if i < steps.count - 1 {
                    Rectangle().fill(i < at ? K.accent.opacity(0.5) : K.line).frame(height: 1).frame(minWidth: 12, maxWidth: 40).padding(.horizontal, 8)
                }
            }
        }
    }
}

// ---- reviews ----

/// What a PR's review is doing, in a couple of words.
func phaseText(_ p: ReviewPr, paused: Bool = false) -> String {
    if p.attention?.kind == "ready" { return "Ready to post" }
    switch p.state {
    case "queued": return p.startError != nil ? "Didn't start" : paused ? "Queued · paused" : "Queued"
    case "reviewing":
        if p.phase == "checking" { return "Checking" }
        if p.phase == "awaiting" { return "Ready to post" }
        return "Reviewing \(p.reviewersDone)/\(p.reviewersTotal)"
    case "posted": return "Waiting on the author"
    case "unrequested": return "No longer asked"
    case "merged": return "Merged"
    case "closed": return "Closed"
    case "cleaned": return "Done"
    default: return p.state
    }
}

func reviewLive(_ p: ReviewPr) -> LiveState {
    if let k = p.attention?.kind { return k == "ready" || k == "blocked" ? .blocked : .error }
    if p.state == "reviewing" { return .working }
    if ["posted", "cleaned", "merged"].contains(p.state) { return .done }
    return .idle
}

/// Post, Preview and Dismiss for a round. Post only when it waits for you:
/// the hub posts the draft you saw (its digest), or refuses.
struct ReviewActions: View {
    let p: ReviewPr
    var small = false
    @Environment(Store.self) private var store
    @State private var busy: String?
    @State private var preview: (text: String, digest: String?)?
    @State private var showing = false

    var body: some View {
        if p.state == "reviewing" {
            HStack(spacing: 6) {
                if p.ready {
                    Button(busy == "post" ? "Posting…" : "Post") { act("post") }
                        .buttonStyle(.borderedProminent).tint(K.accent)
                        .help("Post the checked review to GitHub as you")
                }
                if p.phase == "awaiting" || p.phase == "checking" {
                    Button(busy == "preview" ? "Loading…" : "Preview") { show() }
                }
                Button(busy == "dismiss" ? "Dismissing…" : "Dismiss") { act("dismiss") }
                    .help("End this round without posting anything")
            }
            .controlSize(small ? .small : .regular)
            .disabled(busy != nil)
            .sheet(isPresented: $showing) {
                ReviewPreview(p: p, text: preview?.text ?? "", ready: p.ready, post: { act("post") }, dismiss: { act("dismiss") })
            }
        }
    }

    private func act(_ what: String) {
        busy = what
        showing = false
        Task {
            if what == "post" { _ = await store.post(p, digest: preview?.digest) } else { _ = await store.dismissReview(p) }
            busy = nil
        }
    }

    private func show() {
        busy = "preview"
        Task {
            preview = await store.preview(p)
            busy = nil
            showing = preview != nil
        }
    }
}

struct ReviewPreview: View {
    let p: ReviewPr
    let text: String
    let ready: Bool
    let post: () -> Void
    let dismiss: () -> Void
    @Environment(\.dismiss) private var close

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Review of \(p.key) · round \(p.round), as it would post").font(.headline)
                Spacer()
                if let u = p.url { Link("Open the PR", destination: u) }
            }
            .padding(16)
            Divider()
            ScrollView { MarkdownView(text: text).padding(20) }
            Divider()
            HStack {
                Text(ready ? "Posts from your GitHub account, with the footer above." : "The checker hasn't finished; this is its draft so far.")
                    .font(.system(size: 12)).foregroundStyle(.secondary)
                Spacer()
                Button("Close") { close() }.keyboardShortcut(.cancelAction)
                Button("Dismiss") { dismiss() }
                if ready { Button("Post Review") { post() }.buttonStyle(.borderedProminent).tint(K.accent).keyboardShortcut(.defaultAction) }
            }
            .padding(16)
        }
        .frame(minWidth: 680, idealWidth: 760, minHeight: 520, idealHeight: 680)
    }
}

/// A Review docket's PR: where its round is, who reviews it, what needs you,
/// and how earlier rounds went.
struct ReviewPanel: View {
    let d: Docket
    @Environment(Store.self) private var store
    @State private var allRounds = false

    var body: some View {
        if let rv = store.state?.review, rv.enabled, let p = store.state?.pr(for: d.id) {
            let a = p.attention
            Panel(alert: a != nil) {
                HStack(spacing: 7) {
                    Image(systemName: "arrow.triangle.pull").foregroundStyle(K.muted)
                    Group {
                        if let u = p.url { Link("\(p.repo)#\(p.pr)", destination: u) } else { Text("\(p.repo)#\(p.pr)") }
                    }
                    .font(.system(size: 13, weight: .semibold)).lineLimit(1).truncationMode(.middle)
                    Text("\(p.author.map { "by \($0) · " } ?? "")round \(max(p.round, 1))").foregroundStyle(K.muted).lineLimit(1).fixedSize()
                    Spacer()
                    VerdictChip(v: p.state == "reviewing" ? nil : p.verdict)
                    RiskChip(level: p.mergeRisk)
                }
                .font(.system(size: 12.5))
                if p.state == "reviewing" || p.state == "queued" {
                    let at = p.state != "reviewing" ? -1 : p.phase == "checking" ? 1 : p.phase == "awaiting" ? 2 : 0
                    let auto = (p.post ?? rv.j["post"].str) == "auto"
                    Steps(steps: auto ? ["Reviewers \(p.reviewersDone)/\(p.reviewersTotal)", "Checker", "Posted"] : ["Reviewers \(p.reviewersDone)/\(p.reviewersTotal)", "Checker", "Your call", "Posted"], at: auto && at >= 2 ? at - 1 : at)
                } else {
                    Text("\(phaseText(p, paused: rv.paused))\(p.postedAt.map { " · last posted \(agoLong($0))" } ?? "")").font(.system(size: 12)).foregroundStyle(K.muted)
                }
                if p.state == "reviewing", !p.reviewers.isEmpty {
                    HStack(spacing: 6) {
                        ForEach(p.reviewers, id: \.name) { r in Chip(text: "\(r.name)\(r.effort.map { " · \($0)" } ?? "")", color: K.text2) }
                        if let e = p.checkerEffort { Chip(text: "checker · \(e)", color: K.text2) }
                    }
                }
                if let a {
                    Callout(tone: a.kind == "ready" || a.kind == "blocked" ? .blocked : .error) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(attentionTitle(a)).font(.system(size: 13, weight: .semibold))
                            if a.kind == "ready", p.draft != nil {
                                Text(draftLine(p) ?? "").font(.system(size: 12.5)).foregroundStyle(K.text2)
                                if let v = p.draft?.verdict { Text(v).font(.system(size: 12.5)).foregroundStyle(K.text2).lineLimit(3) }
                                if let w = p.waitsWhy { Text("Waits for you: \(w)").font(.system(size: 12.5)).foregroundStyle(K.muted).fixedSize(horizontal: false, vertical: true) }
                            } else {
                                Text(a.text).font(.system(size: 12.5)).foregroundStyle(K.text2).textSelection(.enabled)
                            }
                        }
                        Spacer(minLength: 8)
                        ReviewActions(p: p, small: true)
                    }
                } else if p.state == "reviewing" {
                    ReviewActions(p: p, small: true)
                }
                if let i = p.implementation { MarkdownView(text: i, size: 12.5) }
                if !p.history.isEmpty {
                    VStack(spacing: 0) {
                        ForEach(allRounds ? p.history : Array(p.history.prefix(2))) { h in
                            VStack(alignment: .leading, spacing: 3) {
                                HStack(spacing: 8) {
                                    Text("R\(h.round)").font(.system(size: 11.5, design: .monospaced)).foregroundStyle(K.muted)
                                    VerdictChip(v: h.verdict)
                                    RiskChip(level: h.mergeRisk)
                                    Spacer()
                                    if let t = h.tokens { Text("\(tokens(t)) tok").foregroundStyle(K.faint).fixedSize() }
                                    Text(ago(h.at)).foregroundStyle(K.faint).help(stamp(h.at)).fixedSize()
                                    if let u = h.url { Link(destination: u) { Image(systemName: "arrow.up.right.square") }.help("The review on GitHub") }
                                }
                                Text([h.blocking.map { "\($0) blocking · \(h.nonBlocking ?? 0) non-blocking" } ?? (h.by == "you" ? "by you" : nil), h.found.isEmpty ? nil : h.found].compactMap { $0 }.joined(separator: "  ·  "))
                                    .foregroundStyle(K.muted).lineLimit(1).truncationMode(.tail)
                                    .help("Findings kept from each reviewer")
                                    .padding(.leading, 28)
                            }
                            .font(.system(size: 12))
                            .padding(.vertical, 6)
                            Divider().opacity(0.5)
                        }
                        if p.history.count > 2 {
                            Button(allRounds ? "Fewer rounds" : "All \(p.history.count) rounds") { allRounds.toggle() }.buttonStyle(.link).font(.system(size: 12)).padding(.top, 4)
                        }
                    }
                }
            }
        }
    }

    private func attentionTitle(_ a: Attention) -> String {
        switch a.kind {
        case "ready": "Round \(a.round) is ready to post"
        case "start-failed": "Round \(a.round) didn't start"
        case "stalled": "The round stalled"
        case "pane-closed": "A pane closed mid-round"
        case "blocked": "An agent is blocked"
        default: "Something went wrong"
        }
    }
}

func tokens(_ n: Int) -> String {
    n >= 1_000_000 ? String(format: "%.1fM", Double(n) / 1e6) : n >= 1000 ? "\(n / 1000)k" : "\(n)"
}

// ---- builds ----

func buildPhase(_ t: BuildTask) -> String {
    switch t.state {
    case "queued": return t.startError != nil ? "Didn't start" : "Queued"
    case "open": return t.asks.contains("address") ? "Feedback: your call" : t.rounds > 0 ? "PR open · \(plural(t.rounds, "round"))" : "PR open"
    case "merged": return "Merged"
    case "closed": return "Closed"
    case "discarded": return "Discarded"
    default: break
    }
    return [
        "planning": "Planning", "not_ready": "Not ready: your call", "plan_gate": "Plan: your call", "building": "Building",
        "checking": "Checking (check \(t.checkN))", "prereview": "Pre-review \(t.j["prereview"]["n"].int ?? 1)", "preview_gate": "Preview: your call", "open_gate": "Ready to open", "opening": "Opening",
        "addressing": "Round \(t.round["n"].int ?? t.rounds): addressing", "push_gate": "Round \(t.round["n"].int ?? t.rounds): your call", "pushing": "Pushing",
    ][t.phase ?? ""] ?? "Working"
}

func buildLive(_ t: BuildTask) -> LiveState {
    if let a = t.attention { return a.kind == "blocked" ? .blocked : .error }
    if t.needsYou { return .blocked }
    if t.state == "active" { return .working }
    if t.state == "open" || t.state == "merged" { return .done }
    return .idle
}

struct BuildPanel: View {
    let d: Docket
    @Environment(Store.self) private var store

    var body: some View {
        if let t = store.state?.task(for: d.id) {
            Panel(alert: t.attention != nil) {
                HStack(spacing: 7) {
                    Image(systemName: "hammer").foregroundStyle(K.muted)
                    Text("Build").font(.system(size: 13, weight: .semibold))
                    Text("into \(t.repo)").foregroundStyle(K.muted)
                    if let b = t.branch { Text("· \(b)").font(.system(size: 12, design: .monospaced)).foregroundStyle(K.muted).help("from \(t.base ?? "main")") }
                    Spacer()
                    if let u = t.prURL { Link("PR #\(t.prNumber ?? 0)", destination: u) }
                    if t.state == "active" || t.state == "queued" {
                        Button("Discard") { store.discardBuild(d.id) }.controlSize(.small)
                            .help("Stop the Build: its panes close and its worktree goes; the branch stays")
                    }
                }
                .font(.system(size: 12.5))
                if let n = t.round["n"].int {
                    let round: [(String, [String])] = [("Feedback", []), ("Round \(n)", ["addressing"]), ("Check", ["checking"]), ("Your call", ["push_gate"]), ("Pushed", ["pushing"])]
                    Steps(steps: round.map(\.0), at: round.firstIndex { $0.1.contains(t.phase ?? "") } ?? 1)
                    if let sum = t.round["summary"].text {
                        Text("Round \(n): \(sum)\(t.round["update"].str.map { $0 == "rebase" ? " · rebasing onto \(t.base ?? "the base")" : " · merging \(t.base ?? "the base")" } ?? "")")
                            .font(.system(size: 12)).foregroundStyle(K.muted)
                    }
                } else if ["active", "queued", "open"].contains(t.state) {
                    let steps: [(String, [String])] = [("Plan", ["planning", "not_ready"]), ("Your call", ["plan_gate"]), ("Build", ["building"]), ("Check", ["checking", "prereview", "preview_gate"]), ("Open", ["open_gate", "opening"])]
                    let at = t.state == "open" || t.state == "merged" ? steps.count : t.state == "queued" ? -1 : steps.firstIndex { $0.1.contains(t.phase ?? "") } ?? 0
                    Steps(steps: steps.enumerated().map { i, s in s.0 == "Check" && i == at && t.checkN > 0 ? "Check \(t.checkN)" : s.0 == "Your call" && i == at ? "Plan: your call" : s.0 }, at: at)
                } else {
                    Text("\(buildPhase(t))\(t.why.map { ": \($0)" } ?? "")").font(.system(size: 12)).foregroundStyle(K.muted)
                }
                if let a = t.attention {
                    Callout(tone: a.kind == "blocked" ? .blocked : .error) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(["stalled": "The Build stalled", "start-failed": "The Build didn't start", "pane-closed": "An agent's pane closed", "blocked": "An agent is blocked"][a.kind] ?? "Something went wrong").font(.system(size: 13, weight: .semibold))
                            Text(a.text).font(.system(size: 12.5)).foregroundStyle(K.text2).textSelection(.enabled)
                        }
                    }
                }
                HStack(spacing: 6) {
                    Chip(text: "builder · \(t.builder["agent"].str ?? "claude")\(t.builder["effort"].str.map { " · \($0)" } ?? "")", color: K.text2)
                    Chip(text: "checker · \(t.checker["agent"].str ?? "claude")\(t.checker["effort"].str.map { " · \($0)" } ?? "")", color: K.text2)
                    if t.planVersion > 0 { Chip(text: "plan v\(t.planVersion)", color: K.text2) }
                    if t.fixUsed > 0 { Chip(text: plural(t.fixUsed, "fix round"), color: K.text2) }
                }
                let c = t.lastCheck
                if c.exists {
                    let n = c["findings"].array.count
                    (Text("Check \(c["n"].int ?? 0): ").bold() + Text(c["verdict"].str == "pass" ? "passed. " : "found problems. ") + Text(c["summary"].str ?? "") + Text(n > 0 ? " (\(plural(n, "finding")))" : "").foregroundColor(K.muted))
                        .font(.system(size: 12.5))
                }
                if !t.decisions.isEmpty {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(t.decisions, id: \.self) { x in
                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                Text(x["id"].str ?? "").font(.system(size: 11.5, design: .monospaced)).foregroundStyle(K.muted)
                                Text(x["question"].str ?? "").foregroundStyle(K.text)
                                Spacer()
                                Text(x["answered"].bool ? x["label"].str ?? "answered" : "open").foregroundStyle(x["answered"].bool ? K.working : K.blocked)
                            }
                            .font(.system(size: 12.5))
                        }
                    }
                }
            }
        }
    }
}

// ---- messages ----

/// A message waiting for you: its words, its options as buttons, and a reply.
struct MessageCard: View {
    let m: Msg
    var compact = false
    @Environment(Store.self) private var store
    @State private var text = ""
    @State private var busy: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 7) {
                if m.from == "wheelhouse" { HubAvatar(size: 18) } else { AppAvatar(app: appOfName(m.fromApp), size: 18) }
                Text(m.who).font(.system(size: 12.5, weight: .semibold))
                Text(m.options.isEmpty ? "wrote to you" : "asks you").foregroundStyle(K.muted)
                if let s = m.subject { Text("· \(s)").foregroundStyle(K.text2).lineLimit(1) }
                Spacer()
                Text(agoLong(m.createdAt)).foregroundStyle(K.faint)
                Text("#\(m.id)").font(.system(size: 11.5, design: .monospaced)).foregroundStyle(K.faint)
            }
            .font(.system(size: 12.5))
            if !compact { MarkdownView(text: m.body) }
            if m.preview.exists {
                HStack(spacing: 8) {
                    if let down = m.preview["down"].text {
                        Text("The preview is down: \(down).").foregroundStyle(K.muted)
                    } else if let u = (m.preview["local"].str ?? m.preview["url"].str).flatMap(URL.init(string:)) {
                        Link(destination: u) { Label("Open preview", systemImage: "arrow.up.right.square") }.buttonStyle(.borderedProminent).tint(K.accent).controlSize(.small)
                    }
                    Text(":\(m.preview["app_port"].int ?? 0)").font(.system(size: 11.5, design: .monospaced)).foregroundStyle(K.faint)
                }
            }
            if !m.options.isEmpty {
                HStack(spacing: 6) {
                    ForEach(Array(m.options.enumerated()), id: \.offset) { i, o in
                        if i == 0 {
                            Button(busy == o ? "Sending…" : o) { answer(o) }.buttonStyle(.borderedProminent).tint(K.accent)
                        } else {
                            Button(busy == o ? "Sending…" : o) { answer(o) }
                        }
                    }
                }
                .controlSize(.small)
                .disabled(busy != nil)
            }
            if !compact {
                HStack(alignment: .bottom, spacing: 8) {
                    Editor(text: $text, placeholder: m.options.isEmpty ? "Reply to \(m.who)…" : "Add words to your choice, or answer in your own…", minHeight: 20, maxHeight: 140, onSubmit: { answer(nil) })
                        .padding(7)
                        .background(K.main, in: RoundedRectangle(cornerRadius: 7))
                        .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(K.line))
                    Button("Reply") { answer(nil) }.disabled(text.trimmingCharacters(in: .whitespaces).isEmpty || busy != nil).help("Reply (⌘↩)")
                    Button("Done") { store.file(m) }.help("File it without answering")
                }
                .controlSize(.small)
            }
        }
        .padding(14)
        .background(K.warningTint.opacity(0.55), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(K.blocked.opacity(0.3)))
    }

    private func answer(_ choice: String?) {
        guard busy == nil, choice != nil || !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        busy = choice ?? "reply"
        Task {
            if await store.reply(m, text, choice: choice) { text = "" }
            busy = nil
        }
    }
}
