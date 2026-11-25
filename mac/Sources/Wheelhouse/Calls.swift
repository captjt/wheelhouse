import AppKit
import SwiftUI
import UniformTypeIdentifiers

// A call's docket (src/calls.rs): the Listener reads its transcript against
// your open dockets and proposes decisions, questions and tasks, each
// quoting the call. You apply, edit then apply, or ignore each; nothing
// reaches a docket until you apply it. The words are web/src/calls.ts's.

enum ProposalKind: String {
    case decide, ask, task
}

/// What a call proposes for a docket (src/calls.rs `Proposal`), and what
/// became of it.
struct Proposal: Hashable, Identifiable {
    let j: JSON
    var id: Int { n }
    var n: Int { j["n"].int ?? 0 }
    var seq: Int { j["seq"].int ?? 0 }
    var at: String { j["at"].str ?? "" }
    /// Anything the hub didn't say is one of the three reads as a task, as the web app's words do.
    var kind: ProposalKind { ProposalKind(rawValue: j["kind"].str ?? "") ?? .task }
    var docket: String? { j["docket"].text }
    var subject: String? { j["subject"].text }
    var choice: String? { j["choice"].text }
    var question: String? { j["question"].text }
    var options: [String] { j["options"].strings }
    var title: String? { j["title"].text }
    var intent: String? { j["intent"].text }
    var quote: String { j["quote"].str ?? "" }
    var quoteFound: Bool { j["quote_found"].bool }
    var why: String? { j["why"].text }
    /// `open`, `applied` or `ignored`.
    var status: String { j["status"].str ?? "open" }
    var isOpen: Bool { status == "open" }
    var answeredAt: String? { j["outcome"]["at"].text }
    /// The docket applying it went to: the one decided or asked on, or the one it opened.
    var appliedTo: String? { j["outcome"]["applied"]["docket"].text }
    /// You changed it before applying it.
    var edited: Bool { !(j["outcome"]["edited"].object?.isEmpty ?? true) }
}

/// A call's proposals and how the Listener's reading went (src/calls.rs `view`).
struct CallView: Hashable {
    let j: JSON
    /// `started`, `done`, `failed`, `stopped` (the hub stopped while it read), or nil.
    var state: String? { j["listening"]["state"].text }
    var why: String? { j["listening"]["why"].text }
    var leftOut: [String] { j["listening"]["left_out"].strings }
    var proposals: [Proposal] { j["proposals"].array.map(Proposal.init) }
    /// The Listener is reading it now.
    var reading: Bool { state == "started" }
}

enum Calls {
    /// A transcript is at most this long, in bytes (src/calls.rs MAX_TRANSCRIPT).
    static let maxTranscript = 400_000

    static func kindWord(_ k: ProposalKind) -> String {
        switch k {
        case .decide: "Decision"
        case .ask: "Question"
        case .task: "Task"
        }
    }

    /// What applying it would do, in a line.
    static func whatItDoes(_ p: Proposal) -> String {
        if p.kind == .task { return "Open a docket: \(p.title ?? "")" }
        let on = p.docket.map { " on \($0)" } ?? ""
        if p.kind == .decide { return "Decide\(on): \(p.subject.map { "\($0): " } ?? "")\(p.choice ?? "")" }
        let first = (p.question ?? "").split(separator: "\n", omittingEmptySubsequences: false).first.map(String.init) ?? ""
        return "Ask the agents\(on): \(first)"
    }

    /// Why Apply can't go yet, if it can't.
    static func cantApply(_ p: Proposal) -> String? {
        if p.kind != .task, p.docket == nil { return "Pick the docket it's for: Edit" }
        if p.kind == .decide, p.choice == nil { return "Nothing decided" }
        if p.kind == .ask, p.question == nil { return "No question" }
        if p.kind == .task, p.title == nil { return "No title" }
        return nil
    }

    /// The fields Edit shows for a kind, in order.
    static func editable(_ k: ProposalKind) -> [String] {
        switch k {
        case .decide: ["docket", "subject", "choice"]
        case .ask: ["docket", "question", "options"]
        case .task: ["title", "intent"]
        }
    }

    enum Tone { case busy, error, quiet }

    /// How listening went, in words; nil when there's nothing to say.
    static func listeningWords(_ v: CallView) -> (text: String, tone: Tone)? {
        switch v.state {
        case "started": return ("The Listener is reading the call…", .busy)
        case "failed": return ("The Listener couldn't read the call: \(v.why ?? "it failed")", .error)
        case "stopped": return ("The hub stopped while the Listener was reading. Listen again.", .error)
        case "done" where v.proposals.isEmpty: return ("The Listener found nothing the call settled.", .quiet)
        case "done" where !v.leftOut.isEmpty: return ("\(v.leftOut.count) more it proposed didn't hold up, and were left out.", .quiet)
        default: return nil
        }
    }

    /// Options as typed in Edit: one per line, or split by `|`; six at most.
    static func parseOptions(_ s: String) -> [String] {
        Array(s.split(whereSeparator: { $0 == "\n" || $0 == "|" || $0 == "\r\n" }).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.filter { !$0.isEmpty }.prefix(6))
    }

    /// Edit's fields, filled from the proposal.
    static func fields(_ p: Proposal) -> [String: String] {
        [
            "docket": p.docket ?? "",
            "subject": p.subject ?? "",
            "choice": p.choice ?? "",
            "question": p.question ?? "",
            "options": p.options.joined(separator: "\n"),
            "title": p.title ?? "",
            "intent": p.intent ?? "",
        ]
    }

    /// What Apply sends from Edit: each field its kind shows, a blank one
    /// as null, options as a list.
    static func edit(_ k: ProposalKind, _ f: [String: String]) -> [String: Any] {
        var out: [String: Any] = [:]
        for key in editable(k) {
            let v = f[key] ?? ""
            if key == "options" {
                out[key] = parseOptions(v)
            } else {
                let t = v.trimmingCharacters(in: .whitespacesAndNewlines)
                out[key] = t.isEmpty ? NSNull() : t
            }
        }
        return out
    }

    /// What applying did, for a toast (the hub's `result`).
    static func appliedToast(_ r: JSON) -> String {
        let d = r["docket"].str ?? ""
        if r["did"].str == "opened" { return "\(d) opened from the call" }
        let told = r["told"].strings
        let and = told.isEmpty ? "" : ", and told \(told.count == 1 ? told[0] : "\(told.count) agents")"
        return "Applied to \(d)\(and)"
    }

    /// A proposal on the record (its summary is the entry's body), with the
    /// words of the call it came from.
    static func proposedBody(_ body: String?, quote: String?) -> String {
        let q = (quote ?? "").replacingOccurrences(of: "\r\n", with: " ").replacingOccurrences(of: "\n", with: " ")
        return "\(body ?? "")\n\n> \(q)"
    }

    /// An answered proposal on the record (`decision` with `data.proposal`), in a line.
    static func answered(_ data: JSON, who: String = "You") -> String {
        let applied = data["choice"].str == "Apply"
        let to = data["applied"]["docket"].text.map { " to \($0)" } ?? ""
        let edited = data["edited"].object.map { !$0.isEmpty } ?? false
        return "\(who) \(applied ? "applied" : "ignored") #\(data["proposal"].int ?? 0)\(to)\(edited ? ", edited" : "")"
    }

    // ---- adding one ----

    /// Longer than the hub keeps, as it counts (bytes, without the blanks at either end).
    static func tooLong(_ text: String) -> Bool {
        text.trimmingCharacters(in: .whitespacesAndNewlines).utf8.count > maxTranscript
    }

    static func fileTooBig(_ bytes: Int) -> Bool { bytes > maxTranscript }

    /// A file's text: UTF-8, without a byte-order mark; nil for anything
    /// that isn't text.
    static func fileText(_ data: Data) -> String? {
        guard var s = String(data: data, encoding: .utf8), !s.contains("\0") else { return nil }
        if s.hasPrefix("\u{FEFF}") { s.removeFirst() }
        return s
    }

    /// A call's title from its file's name: the name without its extension.
    static func fileTitle(_ name: String) -> String {
        guard let dot = name.lastIndex(of: "."), dot != name.startIndex else { return name }
        let ext = name[name.index(after: dot)...]
        guard !ext.isEmpty, ext.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber) }) else { return name }
        return String(name[..<dot])
    }

    /// What Choose File offers: plain text, and the captions and notes a call's transcript comes as.
    static var transcriptTypes: [UTType] {
        [.plainText] + ["txt", "vtt", "srt", "md", "markdown"].compactMap { UTType(filenameExtension: $0) }
    }

    /// The docket link the Activity shows: it opens in the app.
    static func link(_ id: String) -> URL? { URL(string: "\(Flavor.scheme)://docket/\(id)") }
}

// ---- the page ----

/// A call's proposals, where a docket's intent would be: how listening
/// went, Listen Again, and a card for each.
struct CallSection: View {
    let id: String
    let call: CallView
    @Environment(Store.self) private var store
    @State private var busy = false

    var body: some View {
        let all = call.proposals
        let open = all.filter(\.isOpen).count
        let applied = all.filter { $0.status == "applied" }.count
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Text("Proposals").font(.system(size: 13, weight: .semibold)).foregroundStyle(K.text)
                Text(all.isEmpty ? "from the call's transcript" : "\(open) open · \(applied) applied").font(.system(size: 12)).foregroundStyle(K.muted)
                Spacer()
                if !call.reading {
                    Button("Listen Again", systemImage: "arrow.clockwise") { listen() }
                        .buttonStyle(.borderless).controlSize(.small)
                        .disabled(busy)
                        .help("Read the call again, against your open dockets as they are now")
                }
            }
            if let w = Calls.listeningWords(call) {
                HStack(spacing: 8) {
                    if w.tone == .busy { ProgressView().controlSize(.small) }
                    Text(w.text).fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                    Spacer(minLength: 0)
                }
                .font(.system(size: 13))
                .foregroundStyle(w.tone == .error ? K.error : K.text2)
                .padding(.horizontal, 12).padding(.vertical, 8)
                .background(w.tone == .busy ? K.accentSoft : w.tone == .error ? K.dangerTint : K.sel, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                .help(call.leftOut.isEmpty ? "" : "Left out:\n\(call.leftOut.joined(separator: "\n"))")
            }
            VStack(alignment: .leading, spacing: 8) {
                ForEach(all) { p in ProposalCard(call: id, p: p) }
            }
        }
    }

    private func listen() {
        guard !busy else { return }
        busy = true
        Task {
            _ = await store.listen(id)
            busy = false
        }
    }
}

/// One proposal: what it is and for which docket, what it says, the words
/// of the call it came from, and Apply, Edit, Ignore while it's open.
struct ProposalCard: View {
    let call: String
    let p: Proposal
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @State private var editing = false
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            top
            if editing {
                EditProposal(p: p, busy: busy, onCancel: { editing = false }, onApply: { act(apply: true, edit: $0) })
            } else {
                what
                quote
                if let why = p.why {
                    Text(why).font(.system(size: 12)).foregroundStyle(K.muted).fixedSize(horizontal: false, vertical: true)
                }
                if p.isOpen { bar }
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(K.elev, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous).strokeBorder(editing ? K.accent.opacity(0.5) : K.line))
        .opacity(p.isOpen ? 1 : 0.72)
        .contextMenu {
            Button("Copy Quote") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(p.quote, forType: .string)
            }
        }
    }

    private var top: some View {
        HStack(spacing: 8) {
            KindTag(kind: p.kind)
            if p.kind == .task {
                Text("a new docket").font(.system(size: 12)).foregroundStyle(K.muted)
            } else if let d = p.docket {
                Button(d) { open(d) }
                    .buttonStyle(ChipButton())
                    .font(.system(size: 11.5, weight: .medium, design: .monospaced))
                    .help(store.docket(d).map { "\($0.id) · \($0.title)" } ?? d)
            } else {
                Text("no docket yet").font(.system(size: 12)).foregroundStyle(K.blocked)
            }
            Text("#\(p.n)").font(.system(size: 12)).foregroundStyle(K.muted)
            Spacer(minLength: 6)
            if p.status == "applied" {
                HStack(spacing: 0) {
                    Text("Applied")
                    if let to = p.appliedTo {
                        Text(" to ")
                        Button(to) { open(to) }.buttonStyle(.link)
                    }
                    if p.edited { Text(", edited") }
                    if let at = p.answeredAt { Text(" · \(agoLong(at))").help(stamp(at)) }
                }
                .font(.system(size: 12))
                .foregroundStyle(K.working)
            } else if p.status == "ignored" {
                Text("Ignored\(p.answeredAt.map { " · \(agoLong($0))" } ?? "")").font(.system(size: 12)).foregroundStyle(K.muted)
            }
        }
    }

    @ViewBuilder private var what: some View {
        switch p.kind {
        case .decide:
            HStack(alignment: .firstTextBaseline, spacing: 5) {
                if let s = p.subject {
                    Text(s).foregroundStyle(K.text2)
                    Text("→").foregroundStyle(K.muted)
                }
                Text(p.choice ?? "").fontWeight(.semibold).foregroundStyle(K.text)
            }
            .font(.system(size: 13.5))
            .fixedSize(horizontal: false, vertical: true)
            .textSelection(.enabled)
        case .ask:
            VStack(alignment: .leading, spacing: 6) {
                Text(p.question ?? "").font(.system(size: 13.5)).foregroundStyle(K.text)
                    .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                if !p.options.isEmpty {
                    ChipFlow(spacing: 4) {
                        ForEach(p.options, id: \.self) { o in Text(o).font(.system(size: 11.5, weight: .medium)).modifier(ChipLook()) }
                    }
                }
            }
        case .task:
            VStack(alignment: .leading, spacing: 4) {
                Text(p.title ?? "").font(.system(size: 13.5, weight: .semibold)).foregroundStyle(K.text)
                    .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                if let i = p.intent {
                    Clamp(limit: 220) { MarkdownView(text: i, size: 13) }
                        .foregroundStyle(K.text2)
                }
            }
        }
    }

    /// The call's words, as the Listener quoted them: flagged when they aren't in the transcript.
    private var quote: some View {
        HStack(alignment: .top, spacing: 10) {
            Rectangle().fill(p.quoteFound ? K.line2 : K.blocked).frame(width: 2)
            VStack(alignment: .leading, spacing: 3) {
                Text("“\(p.quote)”").italic().foregroundStyle(K.text2)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                if !p.quoteFound {
                    Text("Not in the transcript").font(.system(size: 12)).foregroundStyle(K.blocked)
                }
            }
            .padding(.vertical, 2)
        }
        .font(.system(size: 13))
        .fixedSize(horizontal: false, vertical: true)
        .help(p.quoteFound ? "From the transcript" : "These words aren't in the transcript")
    }

    private var bar: some View {
        let blocked = Calls.cantApply(p)
        return HStack(spacing: 6) {
            Button("Apply") { act(apply: true) }
                .buttonStyle(.borderedProminent).tint(K.accent)
                .disabled(busy || blocked != nil)
                .help(blocked ?? Calls.whatItDoes(p))
            Button("Edit") { editing = true }
                .disabled(busy)
                .help(p.kind == .task ? "Change its title or intent, then apply it" : "Change its docket or its words, then apply it")
            Button("Ignore") { act(apply: false) }
                .buttonStyle(.borderless)
                .disabled(busy)
                .help("Leave it out: nothing reaches a docket")
            if let blocked {
                Text(blocked).font(.system(size: 12)).foregroundStyle(K.blocked).lineLimit(1)
            }
        }
        .controlSize(.small)
    }

    private func act(apply: Bool, edit: [String: Any]? = nil) {
        guard !busy else { return }
        busy = true
        Task {
            if await store.answerProposal(call, p.n, apply: apply, edit: edit) { editing = false }
            busy = false
        }
    }

    private func open(_ id: String) {
        if nav === Nav.main {
            nav.show(id, store: store)
        } else {
            // A docket's own window: show it in the main one.
            Windows.showMain()
            Nav.main.show(id, store: store)
        }
    }
}

/// Decision, Question or Task, as a tag.
private struct KindTag: View {
    let kind: ProposalKind
    var body: some View {
        let (ink, fill): (Color, Color) = switch kind {
        case .decide: (K.accent, K.accentSoft)
        case .ask: (K.blocked, K.warningTint)
        case .task: (K.text2, K.sel)
        }
        Text(Calls.kindWord(kind))
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(ink)
            .padding(.horizontal, 7).padding(.vertical, 2)
            .background(fill, in: RoundedRectangle(cornerRadius: 5, style: .continuous))
    }
}

/// Change a proposal before applying it: its docket and its words.
private struct EditProposal: View {
    let p: Proposal
    let busy: Bool
    let onCancel: () -> Void
    let onApply: ([String: Any]) -> Void
    @Environment(Store.self) private var store
    @State private var f: [String: String]

    init(p: Proposal, busy: Bool, onCancel: @escaping () -> Void, onApply: @escaping ([String: Any]) -> Void) {
        self.p = p
        self.busy = busy
        self.onCancel = onCancel
        self.onApply = onApply
        _f = State(initialValue: Calls.fields(p))
    }

    var body: some View {
        let fields = Calls.editable(p.kind)
        // A call's proposal goes to an open docket that isn't a call.
        let open = store.dockets.filter { $0.isOpen && !$0.isCall }
        let picked = f["docket"] ?? ""
        VStack(alignment: .leading, spacing: 8) {
            Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 8) {
                if fields.contains("docket") {
                    GridRow {
                        label("Docket")
                        Picker("", selection: bind("docket")) {
                            Text("Pick one…").tag("")
                            if !picked.isEmpty, !open.contains(where: { $0.id == picked }) {
                                Text("\(picked) (not open)").tag(picked)
                            }
                            ForEach(open) { d in Text("\(d.id) · \(d.title)").tag(d.id) }
                        }
                        .labelsHidden()
                        .frame(maxWidth: 420, alignment: .leading)
                    }
                }
                if fields.contains("subject") {
                    GridRow {
                        label("Subject")
                        TextField("What was decided about (Retries)", text: bind("subject")).textFieldStyle(.roundedBorder)
                    }
                }
                if fields.contains("choice") {
                    GridRow {
                        label("Decided")
                        TextField("", text: bind("choice")).textFieldStyle(.roundedBorder)
                    }
                }
                if fields.contains("question") {
                    GridRow(alignment: .top) {
                        label("Question").padding(.top, 5)
                        box(Editor(text: bind("question"), minHeight: 40, maxHeight: 160, onSubmit: apply, onCancel: onCancel))
                    }
                }
                if fields.contains("options") {
                    GridRow(alignment: .top) {
                        label("Options").padding(.top, 5)
                        box(Editor(text: bind("options"), placeholder: "One per line, or none", minHeight: 40, maxHeight: 140, onSubmit: apply, onCancel: onCancel))
                    }
                }
                if fields.contains("title") {
                    GridRow {
                        label("Title")
                        TextField("", text: bind("title")).textFieldStyle(.roundedBorder)
                    }
                }
                if fields.contains("intent") {
                    GridRow(alignment: .top) {
                        label("Intent").padding(.top, 5)
                        box(Editor(text: bind("intent"), placeholder: "What must be true when this is done? (Markdown)", minHeight: 100, maxHeight: 320, font: .monospacedSystemFont(ofSize: 12.5, weight: .regular), onSubmit: apply, onCancel: onCancel))
                    }
                }
            }
            HStack(spacing: 6) {
                Button("Apply") { apply() }
                    .buttonStyle(.borderedProminent).tint(K.accent)
                    .disabled(busy)
                    .help("Apply it as you've changed it (⌘↩ in a text box)")
                Button("Cancel") { onCancel() }.buttonStyle(.borderless).disabled(busy)
            }
            .controlSize(.small)
        }
        .font(.system(size: 13))
    }

    private func label(_ s: String) -> some View {
        Text(s).foregroundStyle(K.muted).frame(width: 70, alignment: .leading)
    }

    private func box(_ e: Editor) -> some View {
        e.padding(.horizontal, 6).padding(.vertical, 4)
            .background(K.main, in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(K.line))
    }

    private func bind(_ k: String) -> Binding<String> {
        Binding(get: { f[k] ?? "" }, set: { f[k] = $0 })
    }

    private func apply() {
        guard !busy else { return }
        onApply(Calls.edit(p.kind, f))
    }
}

/// Chips that wrap onto the next line when they run out of room.
private struct ChipFlow: Layout {
    var spacing: CGFloat = 4

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = arrange(width: proposal.width ?? .infinity, subviews)
        let w = rows.map { $0.width }.max() ?? 0
        let h = rows.reduce(0) { $0 + $1.height } + spacing * CGFloat(max(rows.count - 1, 0))
        return CGSize(width: w, height: h)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in arrange(width: bounds.width, subviews) {
            var x = bounds.minX
            for i in row.items {
                let s = subviews[i].sizeThatFits(.unspecified)
                subviews[i].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(width: min(s.width, bounds.width), height: s.height))
                x += min(s.width, bounds.width) + spacing
            }
            y += row.height + spacing
        }
    }

    private struct Row {
        var items: [Int] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func arrange(width: CGFloat, _ subviews: Subviews) -> [Row] {
        var rows: [Row] = []
        var cur = Row()
        for (i, v) in subviews.enumerated() {
            let s = v.sizeThatFits(.unspecified)
            let w = min(s.width, width)
            if !cur.items.isEmpty, cur.width + spacing + w > width {
                rows.append(cur)
                cur = Row()
            }
            cur.width += (cur.items.isEmpty ? 0 : spacing) + w
            cur.height = max(cur.height, s.height)
            cur.items.append(i)
        }
        if !cur.items.isEmpty { rows.append(cur) }
        return rows
    }
}

// ---- adding one ----

/// A call's transcript, pasted or from a file: a call docket that keeps it,
/// and the Listener reading it (src/calls.rs). New Docket's Call.
struct NewCallForm: View {
    /// The new docket's id, once it's made.
    let done: (String) -> Void
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var title = ""
    @State private var text = ""
    @State private var busy = false

    var body: some View {
        let long = Calls.tooLong(text)
        let empty = text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        VStack(alignment: .leading, spacing: 12) {
            TextField("What the call was (Webhooks sync)", text: $title).textFieldStyle(.plain).font(.system(size: 18, weight: .semibold))
            Editor(text: $text, placeholder: "Paste the call's transcript, or choose its file (.txt, .vtt, .srt, .md)", minHeight: 180, maxHeight: 360, font: .monospacedSystemFont(ofSize: 12, weight: .regular), focus: true, onSubmit: add)
                .padding(8)
                .background(K.recessed, in: RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(long ? K.error : K.line))
            if long {
                Text("It's over \(Calls.maxTranscript / 1000) KB: a transcript is at most that.").font(.system(size: 12)).foregroundStyle(K.error)
            }
            HStack(alignment: .center, spacing: 10) {
                Button("Choose File…") { choose() }
                    .help("A transcript as plain text: .txt, .vtt, .srt or .md")
                Text("The Listener reads it against your open dockets and proposes; nothing reaches a docket until you apply it.")
                    .font(.system(size: 11.5)).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 6)
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(busy ? "Adding…" : "Add Call") { add() }
                    .buttonStyle(.borderedProminent).tint(K.accent)
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(empty || long || busy)
                    .help("Add the call, and the Listener starts reading it (⌘↩)")
            }
        }
        .padding(16)
    }

    private func choose() {
        let p = NSOpenPanel()
        p.canChooseFiles = true
        p.canChooseDirectories = false
        p.allowsMultipleSelection = false
        p.allowedContentTypes = Calls.transcriptTypes
        p.prompt = "Choose"
        p.message = "A call's transcript, as plain text"
        guard p.runModal() == .OK, let u = p.url else { return }
        read(u)
    }

    private func read(_ u: URL) {
        let name = u.lastPathComponent
        let size = (try? u.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
        if Calls.fileTooBig(size) {
            store.toast("\(name) is over \(Calls.maxTranscript / 1000) KB: a transcript is at most that", error: true)
            return
        }
        guard let data = try? Data(contentsOf: u), !Calls.fileTooBig(data.count), let t = Calls.fileText(data) else {
            store.toast("\(name) couldn't be read as text", error: true)
            return
        }
        text = t
        if title.trimmingCharacters(in: .whitespaces).isEmpty { title = Calls.fileTitle(name) }
    }

    private func add() {
        guard !busy, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, !Calls.tooLong(text) else { return }
        busy = true
        Task {
            if let id = await store.addCall(title: title, transcript: text) { done(id) } else { busy = false }
        }
    }
}
