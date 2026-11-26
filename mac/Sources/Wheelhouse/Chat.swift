import SwiftUI

/// Talking to a docket's agents, as a conversation: what you asked, what
/// each answered, what they're doing now, and a box to send one the next
/// thing. While it works, what you send waits and goes in when its turn ends.
struct ChatView: View {
    let d: Docket
    let page: DocketPage
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @State private var text = ""
    @State private var busy = false
    @State private var sent: [String] = []
    /// Only this agent's part of the conversation (its block), or everyone's.
    @State private var only: String?
    /// Who the box sends to (a block); nil, the docket's lead.
    @State private var to: String?

    enum Item: Identifiable {
        case you(Int, String, String, note: Bool, to: String?)
        /// `images`: false for words from outside (the Listener's), which load none.
        case agent(Int, String, String, app: String, who: String?, ask: [String]?, subject: String?, images: Bool = true)
        case chose(Int, String)
        case sys(Int, String, LiveState?)
        case file(Entry)
        case pause(Int, String)
        var id: Int {
            switch self {
            case let .you(s, _, _, _, _), let .agent(s, _, _, _, _, _, _, _), let .chose(s, _), let .sys(s, _, _), let .pause(s, _): s
            case let .file(e): e.seq
            }
        }
    }

    /// The conversation, each line with the agent it's by or to (nil: no
    /// one's in particular, such as your notes).
    private var lines: [(item: Item, block: String?)] {
        var out: [(item: Item, block: String?)] = []
        var session: [String: String] = [:]
        let named = d.crew.count > 1
        for e in page.entries {
            let x = e.data
            let a = d.member(for: e)
            let app = a?.app ?? (e.actorType == "agent" ? e.actorApp : d.agent?.app ?? "agent")
            let who = named ? a?.label : nil
            let by = a?.block
            func put(_ i: Item, _ b: String?) { out.append((i, b)) }
            switch e.kind {
            case "claim":
                let s = x["session"].str ?? ""
                let k = by ?? ""
                if x["source"].str == "join" {
                    put(.pause(e.seq, "\(appName(app))\(who.map { " (\($0))" } ?? "") joined"), by)
                }
                if !s.isEmpty, s != session[k] {
                    if session[k] != nil {
                        let what = x["source"].str == "resume" ? "Resumed an earlier session" : x["source"].str == "clear" ? "A new session (/clear)" : "A new session"
                        put(.pause(e.seq, "\(who.map { "\($0): " } ?? "")\(what)"), by)
                    }
                    session[k] = s
                }
            case "note":
                if x["via"].str == "prompt" { put(.you(e.seq, e.at, e.body ?? "", note: false, to: who), by) }
                else if x["message"].exists, e.actorType == "agent" { put(.agent(e.seq, e.at, "\(x["subject"].str.map { "**\($0)**\n\n" } ?? "")\(e.body ?? "")", app: app, who: who, ask: nil, subject: nil), by) }
                else if e.actorType == "person", x["via"].str == "diff", x["diff"].object != nil { put(.you(e.seq, e.at, "On `\(commentPlace(x["diff"]))`: \(e.body ?? "")", note: true, to: nil), nil) }
                else if e.actorType == "person", !x["message"].exists { put(.you(e.seq, e.at, e.body ?? "", note: true, to: nil), nil) }
            case "question":
                put(.agent(e.seq, e.at, e.body ?? "", app: app, who: who, ask: x["options"].strings, subject: x["subject"].str), by)
            case "proposal":
                put(.agent(e.seq, e.at, Calls.proposedBody("Proposed #\(x["n"].int ?? 0): \(e.body ?? "")", quote: x["quote"].str), app: "claude-code", who: "Listener", ask: nil, subject: nil, images: false), nil)
            case "decision" where x["proposal"].int != nil:
                put(.sys(e.seq, Calls.answered(x), nil), nil)
            case "decision" where x["via"].str == "composer":
                // One of yours: a subject, not a question it answers.
                let c = x["choice"].str ?? ""
                put(.chose(e.seq, x["subject"].str.map { "\($0): \(c)" } ?? c), nil)
            case "decision":
                let c = x["choice"].str ?? ""
                put(.chose(e.seq, e.body.flatMap { $0 != c ? "\(c): \($0)" : nil } ?? c), nil)
            case "turn":
                if let b = e.body, !b.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { put(.agent(e.seq, e.at, b, app: app, who: who, ask: nil, subject: nil), by) }
            case "observed":
                if x["what"].str == "blocked" { put(.sys(e.seq, "\(who.map { "\($0) waited" } ?? "Waited") on you\(x["kind"].str.map { " (\($0))" } ?? "")\(e.body.map { ": \($0)" } ?? "")", .blocked), by) }
                else if x["what"].str == "listen" { put(.sys(e.seq, e.body ?? "", x["state"].str == "failed" ? .error : nil), nil) }
                else if x["what"].str == "error" { put(.sys(e.seq, "\(who.map { "\($0) failed" } ?? "Failed")\(e.body.map { ": \($0)" } ?? "")", .error), by) }
            case "approval":
                if x["what"].exists || x["kind"].exists, x["kind"].str != "review" {
                    put(.sys(e.seq, "You \(x["decision"].str == "approve" ? "approved" : "denied") \(who.map { "\($0)'s " } ?? "")\(x["kind"].str ?? "a prompt")\(e.body.map { ": \($0)" } ?? "")", nil), by)
                }
            case "artifact":
                put(.file(e), e.actorType == "agent" ? by : nil)
            case "release":
                put(.pause(e.seq, "\(who.map { "\($0)'s session" } ?? "The session") ended: \(x["reason"].str ?? "released")"), by)
            default: break
            }
        }
        return out
    }

    var body: some View {
        let items = lines.filter { only == nil || $0.block == nil || $0.block == only }.map(\.item)
        let queued = page.queued
        let pending = sent.filter { t in
            !items.contains { if case let .you(_, _, x, _, _) = $0 { return x.trimmingCharacters(in: .whitespacesAndNewlines) == t } else { return false } }
                && !queued.contains { $0.body.trimmingCharacters(in: .whitespacesAndNewlines) == t }
        }
        let asks = (store.state?.messages ?? []).filter { $0.docket == d.id }
        // The agent the box sends to: the one picked, else the lead.
        let on = d.onAgents
        let target = d.member(block: to).flatMap { $0.on ? $0 : nil } ?? on.first
        let tlive = d.hasAgents ? target?.live : d.live
        let state = tlive?.state
        let canSend = tlive != nil && state != .error
        let willQueue = state == .working || state == .starting || state == .blocked
        // While it works: ↩ queues it for the turn's end; Send now (⌘⇧↩)
        // steers the running turn, for an agent that reads it at its next step.
        let canSteer = (state == .working || state == .starting) && tlive?.steer == true
        let empty = text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let app = target.map { d.several ? $0.label : appName($0.app) } ?? appName(d.agent?.app)
        let now = d.lives.filter { only == nil || $0.agent?.block == only }

        VStack(spacing: 0) {
            if d.crew.count > 1 {
                AgentChips(d: d, only: $only) { b in if let b, d.member(block: b)?.on == true { to = b } }
                    .padding(.horizontal, 28).padding(.vertical, 8)
                    .frame(maxWidth: 820, alignment: .leading)
                    .frame(maxWidth: .infinity)
                Divider()
            }
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 14) {
                        if items.isEmpty { Text("Nothing said yet. Send \(app) something to start.").foregroundStyle(K.muted).frame(maxWidth: .infinity).padding(.top, 40) }
                        ForEach(items) { it in bubble(it) }
                        ForEach(asks) { m in MessageCard(m: m) }
                        ForEach(queued, id: \.id) { q in
                            VStack(alignment: .trailing, spacing: 4) {
                                YouBubble(text: q.body).opacity(0.6)
                                HStack(spacing: 4) {
                                    Text("Queued · goes in when the turn ends ·").foregroundStyle(K.muted)
                                    Button("Take back") { store.unqueue(d.id, q.id) }.buttonStyle(.link)
                                }
                                .font(.system(size: 11.5))
                            }
                            .frame(maxWidth: .infinity, alignment: .trailing)
                        }
                        ForEach(pending, id: \.self) { t in YouBubble(text: t).opacity(0.6).frame(maxWidth: .infinity, alignment: .trailing) }
                        if now.isEmpty {
                            NowLine(d: d, agent: d.member(block: only), live: nil)
                        } else {
                            ForEach(Array(now.enumerated()), id: \.offset) { _, x in NowLine(d: d, agent: x.agent, live: x.live) }
                        }
                        Color.clear.frame(height: 1).id("end")
                    }
                    .padding(.horizontal, 28)
                    .padding(.vertical, 20)
                    .frame(maxWidth: 820)
                    .frame(maxWidth: .infinity)
                }
                .defaultScrollAnchor(.bottom)
                .onChange(of: items.count + queued.count + pending.count + asks.count) { proxy.scrollTo("end", anchor: .bottom) }
                .onChange(of: d.lives.map { $0.live.state.rawValue }) { proxy.scrollTo("end", anchor: .bottom) }
            }
            Divider()
            HStack(alignment: .bottom, spacing: 10) {
                if on.count > 1 {
                    Menu {
                        ForEach(on) { a in
                            Button { to = a.block } label: {
                                Label("\(a.label) · \(a.live?.state.word ?? "Not running")", systemImage: a.block == target?.block ? "checkmark" : "")
                            }
                        }
                    } label: {
                        HStack(spacing: 5) { AppAvatar(app: target?.app, size: 14); Text(target?.label ?? "Agent").lineLimit(1) }
                    }
                    .menuStyle(.button)
                    .buttonStyle(ChipButton())
                    .font(.system(size: 11.5, weight: .medium))
                    .fixedSize()
                    .padding(.bottom, 8)
                    .help("Who it goes to: one of the \(on.count) agents on \(d.id)")
                }
                Editor(text: $text, placeholder: !canSend ? (tlive != nil ? "\(app) stopped with an error" : "\(app) isn't running") : willQueue ? "Queue a message for \(app)…" : "Message \(app)…", returnSends: true, minHeight: 20, maxHeight: 180, onSubmit: { send() })
                    .padding(.horizontal, 10).padding(.vertical, 8)
                    .background(K.elev, in: RoundedRectangle(cornerRadius: 10))
                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(K.line))
                    .disabled(!canSend)
                if canSteer {
                    Button("Send now") { send(steer: true) }
                        .buttonStyle(ChipButton())
                        .font(.system(size: 12, weight: .semibold))
                        .keyboardShortcut(.return, modifiers: [.command, .shift])
                        .disabled(empty || busy)
                        .padding(.bottom, 4)
                        .help("Send now: \(app) reads it at its next step, without waiting for the turn to end (⌘⇧↩)")
                }
                Button(action: { send() }) {
                    Image(systemName: willQueue ? "clock.arrow.circlepath" : "arrow.up")
                        .font(.system(size: 13, weight: .bold))
                        .frame(width: 30, height: 30)
                        .background(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !canSend ? K.line2 : K.accent, in: Circle())
                        .foregroundStyle(.white)
                }
                .buttonStyle(.plain)
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy || !canSend)
                .help(willQueue ? "Queue it: everything queued goes in together when \(app)'s turn ends (↩)" : "Send to \(app) (↩; ⇧↩ for a new line)")
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
            .frame(maxWidth: 860)
            .frame(maxWidth: .infinity)
            .background(.bar)
        }
    }

    @ViewBuilder
    private func bubble(_ it: Item) -> some View {
        switch it {
        case let .you(_, at, text, note, to):
            VStack(alignment: .trailing, spacing: 4) {
                if note { Text("Your note on the record (not sent)").font(.system(size: 11)).foregroundStyle(K.muted) }
                else if let to { Text("To \(to)").font(.system(size: 11)).foregroundStyle(K.muted) }
                YouBubble(text: text, note: note).help(stamp(at))
            }
            .frame(maxWidth: .infinity, alignment: .trailing)
        case let .agent(_, at, text, app, who, ask, subject, images):
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 7) {
                    AppAvatar(app: app, size: 18)
                    Text(who ?? appName(app)).font(.system(size: 12.5, weight: .semibold))
                    if let who, who != appName(app) { Text(appName(app)).foregroundStyle(K.muted) }
                    if ask != nil { Text("asked you\(subject.map { " · \($0)" } ?? "")").foregroundStyle(K.muted) }
                    Text(agoLong(at)).foregroundStyle(K.faint).help(stamp(at))
                }
                .font(.system(size: 12.5))
                Clamp(limit: 520) {
                    MarkdownView(text: (ask?.isEmpty == false) ? "\(text)\n\nOptions: \(ask!.joined(separator: " · "))" : text, images: images)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        case let .chose(_, text):
            Text("Chose “\(text)”").font(.system(size: 12.5)).padding(.horizontal, 12).padding(.vertical, 7)
                .background(K.accentSoft, in: Capsule()).frame(maxWidth: .infinity, alignment: .trailing)
        case let .sys(_, text, tone):
            Text(text).font(.system(size: 12)).foregroundStyle(tone == .error ? K.error : tone == .blocked ? K.blocked : K.muted)
                .frame(maxWidth: .infinity, alignment: .center)
        case let .file(e):
            Button { Artifacts.open(e) } label: {
                HStack(spacing: 8) {
                    Image(systemName: Artifacts.symbol(e))
                    Text(e.body ?? e.data["name"].str ?? "File").fontWeight(.medium)
                    Text(e.data["url"].exists ? "· a link" : "· \(e.data["name"].str ?? "")").foregroundStyle(K.muted)
                }
                .font(.system(size: 12.5))
                .padding(.horizontal, 12).padding(.vertical, 8)
                .background(K.elev, in: RoundedRectangle(cornerRadius: 9))
                .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(K.line))
            }
            .buttonStyle(.plain)
        case let .pause(_, text):
            HStack(spacing: 10) {
                Rectangle().fill(K.line).frame(height: 1)
                Text(text).font(.system(size: 11.5)).foregroundStyle(K.muted).fixedSize()
                Rectangle().fill(K.line).frame(height: 1)
            }
        }
    }

    private func send(steer: Bool = false) {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        // The one picked, if it's still on; else the lead (no block).
        let target = d.member(block: to).flatMap { $0.on ? $0 : nil }
        let live = d.hasAgents ? (target ?? d.onAgents.first)?.live : d.live
        guard !t.isEmpty, !busy, live != nil else { return }
        busy = true
        Task {
            if await store.send(d.id, t, block: d.blockFor(target), steer: steer) {
                text = ""
                sent.append(t)
            }
            busy = false
        }
    }
}

/// Everyone's, or one agent's: a chip for each agent a docket has had.
/// `picked` hears the block chosen (nil for everyone's).
struct AgentChips: View {
    let d: Docket
    @Binding var only: String?
    var picked: (String?) -> Void = { _ in }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                chip(nil) { Text("All agents") }
                ForEach(d.crew) { a in
                    chip(a.block) {
                        HStack(spacing: 5) {
                            AppAvatar(app: a.app, size: 13)
                            Text(a.label).lineLimit(1)
                            if let l = a.live, a.on { LiveIcon(state: l.state, size: 10) }
                        }
                        .opacity(a.on ? 1 : 0.65)
                    }
                    .help("\(appName(a.app))\(a.role.map { " · \($0)" } ?? "") · \(a.place)\(a.on ? "" : " · left")")
                }
            }
            .font(.system(size: 11.5, weight: .medium))
        }
    }

    private func chip(_ block: String?, @ViewBuilder _ label: () -> some View) -> some View {
        let on = only == block
        return Button {
            only = block
            picked(block)
        } label: {
            label()
                .foregroundStyle(on ? K.text : K.text2)
                .padding(.horizontal, 8).padding(.vertical, 3)
                .background(on ? K.accentSoft : K.recessed, in: Capsule())
                .overlay(Capsule().strokeBorder(on ? K.accent.opacity(0.45) : K.line))
        }
        .buttonStyle(.plain)
    }
}

struct YouBubble: View {
    let text: String
    var note = false
    var body: some View {
        MarkdownView(text: text, fill: false)
            .padding(.horizontal, 13).padding(.vertical, 9)
            .background(note ? K.recessed : K.accentSoft, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(note ? K.line : .clear, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
            .frame(maxWidth: 560, alignment: .trailing)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// What one agent is doing now, at the end of the conversation: the
/// agent's (nil for an older hub's one) and its live.
struct NowLine: View {
    let d: Docket
    var agent: DocketAgent?
    var live: Live?
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @State private var peek = false
    @State private var tail = ""

    /// The block to act on: this agent's when the docket has several.
    private var block: String? { d.blockFor(agent) }

    var body: some View {
        let app = agent.map { d.several ? $0.label : appName($0.app) } ?? appName(d.agent?.app)
        if let l = live {
            switch l.state {
            case .blocked:
                Callout(tone: .blocked) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("\(app) is asking\(l.kind.map { " (\($0))" } ?? "")").font(.system(size: 13, weight: .semibold))
                        if let m = l.msg { Text(m).font(.system(size: 12.5, design: .monospaced)).foregroundStyle(K.text2) }
                    }
                    Spacer(minLength: 8)
                    if l.answerable { AnswerButtons(d: d, block: block, live: l, small: true) }
                    Button("Terminal", systemImage: "terminal") { nav.peek(d.id, block: block) }.controlSize(.small)
                }
            case .error:
                HStack(spacing: 8) {
                    LiveIcon(state: .error, size: 15)
                    Text("\(app) stopped with an error\(l.msg.map { ": \($0)" } ?? ".")").foregroundStyle(K.error)
                    Button("Look at the terminal") { nav.peek(d.id, block: block) }.buttonStyle(.link)
                }
                .font(.system(size: 12.5))
            case .working, .starting:
                VStack(alignment: .leading, spacing: 8) {
                    HStack(spacing: 8) {
                        Dots()
                        TimelineView(.periodic(from: .now, by: 1)) { ctx in
                            Text("\(app) is \(l.state == .starting ? "starting" : "working")\(elapsed(l.since, ctx.date))").foregroundStyle(K.muted)
                        }
                        Spacer()
                        Button(peek ? "Hide" : "Peek") { peek.toggle() }.buttonStyle(.link)
                    }
                    .font(.system(size: 12.5))
                    if peek {
                        Text(tail.isEmpty ? "…" : tail)
                            .font(.system(size: 11, design: .monospaced))
                            .foregroundStyle(K.text2)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(10)
                            .background(K.recessed, in: RoundedRectangle(cornerRadius: 8))
                            .task(id: peek) { await follow() }
                    }
                }
            default:
                if d.several {
                    HStack(spacing: 6) {
                        LiveIcon(state: l.state, size: 12)
                        Text("\(app) · \(l.state.word.lowercased())")
                    }
                    .font(.system(size: 12)).foregroundStyle(K.muted)
                }
            }
        } else if let a = agent, !a.on {
            Text("\(a.label) left\(a.left.map { " (\($0))" } ?? "").").font(.system(size: 12.5)).foregroundStyle(K.muted)
        } else {
            Text("\(d.released.map { "The session ended (\($0))." } ?? "\(app) isn't running in \(agent?.runtime ?? d.runtime).") Start it again on the Mac to keep going.")
                .font(.system(size: 12.5)).foregroundStyle(K.muted)
        }
    }

    private func follow() async {
        while !Task.isCancelled && peek {
            if let s = try? await store.hub.get("/api/dockets/\(d.id)/screen\(Store.blockQuery(block))") {
                let lines = (s["text"].str ?? "").replacingOccurrences(of: #"\s+$"#, with: "", options: .regularExpression).components(separatedBy: "\n")
                tail = lines.suffix(14).joined(separator: "\n")
            }
            try? await Task.sleep(for: .seconds(2))
        }
    }

    private func elapsed(_ since: String?, _ now: Date) -> String {
        guard let d = date(since) else { return "" }
        let s = max(0, now.timeIntervalSince(d))
        return " · " + (s < 60 ? "\(Int(s))s" : s < 3600 ? "\(Int(s / 60))m" : "\(Int(s / 3600))h \(Int(s.truncatingRemainder(dividingBy: 3600) / 60))m")
    }
}

/// Three dots, breathing: an agent at work. Drawn from the clock, not an
/// animation, so they stay put when the conversation moves.
struct Dots: View {
    var body: some View {
        TimelineView(.animation(minimumInterval: 1 / 24)) { ctx in
            let t = ctx.date.timeIntervalSinceReferenceDate
            HStack(spacing: 3) {
                ForEach(0 ..< 3) { i in
                    Circle().fill(K.working).frame(width: 5, height: 5)
                        .opacity(0.25 + 0.75 * (0.5 + 0.5 * sin(t * 4.5 - Double(i) * 0.9)))
                }
            }
        }
    }
}

/// A live look at an agent's pane, and a box to message it between turns:
/// that block's agent, or the docket's lead.
struct PaneSheet: View {
    let id: String
    var block: String?
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var screen = ""
    @State private var error: String?
    @State private var text = ""
    @State private var busy = false
    /// The agent shown, once you pick another; else `block`'s.
    @State private var picked: String?

    var body: some View {
        let d = store.docket(id)
        let shownBlock = picked ?? block
        // Asked for one agent: that one, even after it leaves (then the sheet
        // says so and sends nothing), never another that's on the docket.
        let gone = shownBlock != nil && d?.hasAgents == true && d?.onAgents.contains { $0.block == shownBlock } != true
        let a = shownBlock != nil ? d?.member(block: shownBlock) : d?.onAgents.first
        let live = gone ? nil : d?.hasAgents == true ? a?.live : d?.live
        let state = live?.state
        let several = d?.several == true
        let app = a.map { several ? $0.label : appName($0.app) } ?? appName(d?.agent?.app)
        let runtime = a?.runtime ?? d?.runtime ?? "Rex"
        let why: String? = gone ? "\(a.map { several ? $0.label : appName($0.app) } ?? "That agent") left \(id)." : state == .error ? "\(app) stopped with an error." : state == nil ? "The pane isn't open in \(runtime)." : nil
        let queues = state == .working || state == .starting || state == .blocked
        let act = d?.blockFor(a)
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                AppAvatar(app: a?.app ?? d?.agent?.app, size: 22)
                VStack(alignment: .leading, spacing: 2) {
                    Text(d?.title ?? id).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                    HStack(spacing: 6) {
                        IdText(id: id)
                        if several, let a { Text(a.label).fontWeight(.medium) }
                        if let s = state { LiveIcon(state: s, size: 11); Text(s.word) }
                        if let a { Text(a.place).foregroundStyle(.secondary) }
                    }
                    .font(.system(size: 11.5))
                }
                Spacer()
                if several, let d {
                    Picker("Agent", selection: Binding(get: { a?.block ?? "" }, set: { picked = $0; screen = "" })) {
                        ForEach(d.onAgents) { x in Text("\(x.label) · \(x.live?.state.word ?? "Not running")").tag(x.block) }
                    }
                    .labelsHidden()
                    .fixedSize()
                    .help("Which of \(d.id)'s agents to look at")
                }
                Button("Open in \(runtime)", systemImage: "terminal") { store.openInRex(id, block: act) }
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            .padding(14)
            Divider()
            ScrollViewReader { proxy in
                ScrollView([.vertical, .horizontal]) {
                    Text(screen.isEmpty ? (error ?? "Loading the pane…") : screen)
                        .font(.system(size: 11.5, design: .monospaced))
                        .foregroundStyle(Color(white: 0.88))
                        .textSelection(.enabled)
                        .fixedSize()
                        .padding(14)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Color.clear.frame(height: 1).id("end")
                }
                .background(Color(white: 0.07))
                .defaultScrollAnchor(.bottom)
                .onChange(of: screen) { proxy.scrollTo("end", anchor: .bottom) }
            }
            Divider()
            if let d, let live, live.answerable, state == .blocked {
                HStack(spacing: 10) {
                    LiveIcon(state: .blocked, size: 15)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(app) is asking\(live.kind.map { " (\($0))" } ?? "")").font(.system(size: 12.5, weight: .semibold))
                        if let m = live.msg { Text(m).font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary).lineLimit(2) }
                    }
                    Spacer()
                    AnswerButtons(d: d, block: act, live: live)
                }
                .padding(14)
            } else {
                HStack(alignment: .bottom, spacing: 8) {
                    Editor(text: $text, placeholder: why ?? (queues ? "Queue a message for \(app)…" : "Message \(app)…"), minHeight: 20, maxHeight: 120, onSubmit: { send(act) })
                        .disabled(why != nil)
                    Button(queues ? "Queue  ⌘↩" : "Send  ⌘↩") { send(act) }
                        .buttonStyle(.borderedProminent).tint(K.accent)
                        .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy || why != nil)
                }
                .padding(14)
            }
        }
        .frame(minWidth: 760, idealWidth: 900, minHeight: 520, idealHeight: 640)
        .task(id: shownBlock) { await follow(act) }
    }

    private func follow(_ block: String?) async {
        while !Task.isCancelled {
            do {
                let s = try await store.hub.get("/api/dockets/\(id)/screen\(Store.blockQuery(block))")
                screen = s["text"].str ?? ""
                error = nil
            } catch {
                self.error = Store.words(error)
            }
            try? await Task.sleep(for: .seconds(1.5))
        }
    }

    private func send(_ block: String?) {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty, !busy else { return }
        busy = true
        Task {
            if await store.send(id, t, block: block) { text = "" }
            busy = false
        }
    }
}
