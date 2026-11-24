import SwiftUI

/// Talking to a docket's agent, as a conversation: what you asked, what it
/// answered, what it's doing now, and a box to send it the next thing.
/// While it works, what you send waits and goes in when its turn ends.
struct ChatView: View {
    let d: Docket
    let page: DocketPage
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @State private var text = ""
    @State private var busy = false
    @State private var sent: [String] = []

    enum Item: Identifiable {
        case you(Int, String, String, note: Bool)
        case agent(Int, String, String, app: String, ask: [String]?, subject: String?)
        case chose(Int, String)
        case sys(Int, String, LiveState?)
        case file(Entry)
        case pause(Int, String)
        var id: Int {
            switch self {
            case let .you(s, _, _, _), let .agent(s, _, _, _, _, _), let .chose(s, _), let .sys(s, _, _), let .pause(s, _): s
            case let .file(e): e.seq
            }
        }
    }

    private var items: [Item] {
        var out: [Item] = []
        var session: String?
        let fallback = d.agent?.app ?? "agent"
        for e in page.entries {
            let x = e.data
            let app = e.actorType == "agent" ? e.actorApp : fallback
            switch e.kind {
            case "claim":
                let s = x["session"].str ?? ""
                if !s.isEmpty, s != session {
                    if session != nil { out.append(.pause(e.seq, x["source"].str == "resume" ? "Resumed an earlier session" : x["source"].str == "clear" ? "A new session (/clear)" : "A new session")) }
                    session = s
                }
            case "note":
                if x["via"].str == "prompt" { out.append(.you(e.seq, e.at, e.body ?? "", note: false)) }
                else if x["message"].exists, e.actorType == "agent" { out.append(.agent(e.seq, e.at, "\(x["subject"].str.map { "**\($0)**\n\n" } ?? "")\(e.body ?? "")", app: app, ask: nil, subject: nil)) }
                else if e.actorType == "person", !x["message"].exists { out.append(.you(e.seq, e.at, e.body ?? "", note: true)) }
            case "question":
                out.append(.agent(e.seq, e.at, e.body ?? "", app: app, ask: x["options"].strings, subject: x["subject"].str))
            case "decision":
                let c = x["choice"].str ?? ""
                out.append(.chose(e.seq, e.body.flatMap { $0 != c ? "\(c): \($0)" : nil } ?? c))
            case "turn":
                if let b = e.body, !b.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { out.append(.agent(e.seq, e.at, b, app: fallback, ask: nil, subject: nil)) }
            case "observed":
                if x["what"].str == "blocked" { out.append(.sys(e.seq, "Waited on you\(x["kind"].str.map { " (\($0))" } ?? "")\(e.body.map { ": \($0)" } ?? "")", .blocked)) }
                else if x["what"].str == "error" { out.append(.sys(e.seq, "Failed\(e.body.map { ": \($0)" } ?? "")", .error)) }
            case "approval":
                if x["what"].exists || x["kind"].exists, x["kind"].str != "review" {
                    out.append(.sys(e.seq, "You \(x["decision"].str == "approve" ? "approved" : "denied") \(x["kind"].str ?? "a prompt")\(e.body.map { ": \($0)" } ?? "")", nil))
                }
            case "artifact":
                out.append(.file(e))
            case "release":
                out.append(.pause(e.seq, "The session ended: \(x["reason"].str ?? "released")"))
            default: break
            }
        }
        return out
    }

    var body: some View {
        let items = items
        let queued = page.queued
        let pending = sent.filter { t in
            !items.contains { if case let .you(_, _, x, _) = $0 { return x.trimmingCharacters(in: .whitespacesAndNewlines) == t } else { return false } }
                && !queued.contains { $0.body.trimmingCharacters(in: .whitespacesAndNewlines) == t }
        }
        let asks = (store.state?.messages ?? []).filter { $0.docket == d.id }
        let state = d.live?.state
        let canSend = d.live != nil && state != .error
        let willQueue = state == .working || state == .starting || state == .blocked
        let app = appName(d.agent?.app)

        VStack(spacing: 0) {
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
                        NowLine(d: d)
                        Color.clear.frame(height: 1).id("end")
                    }
                    .padding(.horizontal, 28)
                    .padding(.vertical, 20)
                    .frame(maxWidth: 820)
                    .frame(maxWidth: .infinity)
                }
                .defaultScrollAnchor(.bottom)
                .onChange(of: items.count + queued.count + pending.count + asks.count) { proxy.scrollTo("end", anchor: .bottom) }
                .onChange(of: state) { proxy.scrollTo("end", anchor: .bottom) }
            }
            Divider()
            HStack(alignment: .bottom, spacing: 10) {
                Editor(text: $text, placeholder: !canSend ? (d.live != nil ? "\(app) stopped with an error" : "\(app) isn't running") : willQueue ? "Queue a message for \(app)…" : "Message \(app)…", returnSends: true, minHeight: 20, maxHeight: 180, onSubmit: send)
                    .padding(.horizontal, 10).padding(.vertical, 8)
                    .background(K.elev, in: RoundedRectangle(cornerRadius: 10))
                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(K.line))
                    .disabled(!canSend)
                Button(action: send) {
                    Image(systemName: willQueue ? "clock.arrow.circlepath" : "arrow.up")
                        .font(.system(size: 13, weight: .bold))
                        .frame(width: 30, height: 30)
                        .background(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !canSend ? K.line2 : K.accent, in: Circle())
                        .foregroundStyle(.white)
                }
                .buttonStyle(.plain)
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy || !canSend)
                .help(willQueue ? "Queue it: it goes in when \(app)'s turn ends" : "Send to \(app) (↩; ⇧↩ for a new line)")
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
        case let .you(_, at, text, note):
            VStack(alignment: .trailing, spacing: 4) {
                if note { Text("Your note on the record (not sent)").font(.system(size: 11)).foregroundStyle(K.muted) }
                YouBubble(text: text, note: note).help(stamp(at))
            }
            .frame(maxWidth: .infinity, alignment: .trailing)
        case let .agent(_, at, text, app, ask, subject):
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 7) {
                    AppAvatar(app: app, size: 18)
                    Text(appName(app)).font(.system(size: 12.5, weight: .semibold))
                    if ask != nil { Text("asked you\(subject.map { " · \($0)" } ?? "")").foregroundStyle(K.muted) }
                    Text(agoLong(at)).foregroundStyle(K.faint).help(stamp(at))
                }
                .font(.system(size: 12.5))
                Clamp(limit: 520) {
                    MarkdownView(text: (ask?.isEmpty == false) ? "\(text)\n\nOptions: \(ask!.joined(separator: " · "))" : text)
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

    private func send() {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty, !busy, d.live != nil else { return }
        busy = true
        Task {
            if await store.send(d.id, t) {
                text = ""
                sent.append(t)
            }
            busy = false
        }
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

/// What the agent is doing now, at the end of the conversation.
struct NowLine: View {
    let d: Docket
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @State private var peek = false
    @State private var tail = ""

    var body: some View {
        let app = appName(d.agent?.app)
        if let l = d.live {
            switch l.state {
            case .blocked:
                Callout(tone: .blocked) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("\(app) is asking\(l.kind.map { " (\($0))" } ?? "")").font(.system(size: 13, weight: .semibold))
                        if let m = l.msg { Text(m).font(.system(size: 12.5, design: .monospaced)).foregroundStyle(K.text2) }
                    }
                    Spacer(minLength: 8)
                    AnswerButtons(d: d, small: true)
                    Button("Terminal", systemImage: "terminal") { nav.pane = d.id }.controlSize(.small)
                }
            case .error:
                HStack(spacing: 8) {
                    LiveIcon(state: .error, size: 15)
                    Text("\(app) stopped with an error\(l.msg.map { ": \($0)" } ?? ".")").foregroundStyle(K.error)
                    Button("Look at the terminal") { nav.pane = d.id }.buttonStyle(.link)
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
                EmptyView()
            }
        } else {
            Text("\(d.released.map { "The session ended (\($0))." } ?? "\(app) isn't running in \(d.runtime).") Start it again on the Mac to keep going.")
                .font(.system(size: 12.5)).foregroundStyle(K.muted)
        }
    }

    private func follow() async {
        while !Task.isCancelled && peek {
            if let s = try? await store.hub.get("/api/dockets/\(d.id)/screen") {
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

/// A live look at an agent's pane, and a box to message it between turns.
struct PaneSheet: View {
    let id: String
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var screen = ""
    @State private var error: String?
    @State private var text = ""
    @State private var busy = false

    var body: some View {
        let d = store.docket(id)
        let state = d?.live?.state
        let app = appName(d?.agent?.app)
        let why: String? = state == .error ? "\(app) stopped with an error." : state == nil ? "The pane isn't open in \(d?.runtime ?? "Rex")." : nil
        let queues = state == .working || state == .starting || state == .blocked
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                AppAvatar(app: d?.agent?.app, size: 22)
                VStack(alignment: .leading, spacing: 2) {
                    Text(d?.title ?? id).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                    HStack(spacing: 6) {
                        IdText(id: id)
                        if let s = state { LiveIcon(state: s, size: 11); Text(s.word) }
                        if let a = d?.agent { Text("\(a.rexSession) / \(a.pane.isEmpty ? "pane" : a.pane)").foregroundStyle(.secondary) }
                    }
                    .font(.system(size: 11.5))
                }
                Spacer()
                Button("Open in \(d?.runtime ?? "Rex")", systemImage: "terminal") { store.openInRex(id) }
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
            if let d, d.live?.answerable == true, state == .blocked {
                HStack(spacing: 10) {
                    LiveIcon(state: .blocked, size: 15)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(app) is asking\(d.live?.kind.map { " (\($0))" } ?? "")").font(.system(size: 12.5, weight: .semibold))
                        if let m = d.live?.msg { Text(m).font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary).lineLimit(2) }
                    }
                    Spacer()
                    AnswerButtons(d: d)
                }
                .padding(14)
            } else {
                HStack(alignment: .bottom, spacing: 8) {
                    Editor(text: $text, placeholder: why ?? (queues ? "Queue a message for \(app)…" : "Message \(app)…"), minHeight: 20, maxHeight: 120, onSubmit: send)
                        .disabled(why != nil)
                    Button(queues ? "Queue  ⌘↩" : "Send  ⌘↩", action: send)
                        .buttonStyle(.borderedProminent).tint(K.accent)
                        .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy || why != nil)
                }
                .padding(14)
            }
        }
        .frame(minWidth: 760, idealWidth: 900, minHeight: 520, idealHeight: 640)
        .task { await follow() }
    }

    private func follow() async {
        while !Task.isCancelled {
            do {
                let s = try await store.hub.get("/api/dockets/\(id)/screen")
                screen = s["text"].str ?? ""
                error = nil
            } catch {
                self.error = Store.words(error)
            }
            try? await Task.sleep(for: .seconds(1.5))
        }
    }

    private func send() {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty, !busy else { return }
        busy = true
        Task {
            if await store.send(id, t) { text = "" }
            busy = false
        }
    }
}
