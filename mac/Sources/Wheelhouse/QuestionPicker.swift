import SwiftUI

/// Claude Code's question picker (its AskUserQuestion tool), as the hub read
/// it off the pane's screen (src/picker.rs).
struct AgentQuestion: Equatable {
    struct Opt: Equatable, Identifiable {
        let n: Int
        let label: String
        let description: String?
        let checked: Bool?
        let kind: String
        var id: Int { n }
    }

    let tabs: [(label: String, answered: Bool)]
    let submitTab: Bool
    let review: Bool
    let question: String
    let multi: Bool
    let options: [Opt]
    let next: String?
    let answers: [(question: String, answer: String)]
    let notes: [String]
    /// What's typed in the pane's box for your own words already.
    let draft: String?

    init?(_ j: JSON) {
        guard !j.isNull, !j["options"].array.isEmpty else { return nil }
        tabs = j["tabs"].array.map { ($0["label"].str ?? "", $0["answered"].bool) }
        submitTab = j["submit_tab"].bool
        review = j["review"].bool
        question = j["question"].str ?? ""
        multi = j["multi"].bool
        options = j["options"].array.compactMap { o in
            guard let n = o["n"].int, let label = o["label"].str else { return nil }
            return Opt(n: n, label: label, description: o["description"].text, checked: o["checked"].exists ? o["checked"].bool : nil, kind: o["kind"].str ?? "choice")
        }
        next = j["next"].text
        answers = j["answers"].array.map { ($0["question"].str ?? "", $0["answer"].str ?? "") }
        notes = j["notes"].strings
        draft = j["draft"].text
    }

    static func == (a: AgentQuestion, b: AgentQuestion) -> Bool {
        a.question == b.question && a.review == b.review && a.options == b.options && a.next == b.next
            && a.tabs.map(\.label) == b.tabs.map(\.label) && a.tabs.map(\.answered) == b.tabs.map(\.answered)
            && a.answers.map(\.question) == b.answers.map(\.question) && a.answers.map(\.answer) == b.answers.map(\.answer) && a.draft == b.draft
    }
}

/// The agent's question with its answers as buttons. Picked here, the hub
/// presses the keys you would in the pane, once it has checked the pane
/// still shows this question. `fallback` while it isn't read, or can't be.
struct QuestionPicker<Fallback: View>: View {
    let id: String
    var block: String? = nil
    let live: Live
    @ViewBuilder var fallback: Fallback
    @Environment(Store.self) private var store
    @State private var picker: AgentQuestion?
    /// When the question was asked: sent back with an answer, so the hub
    /// refuses one asked again since.
    @State private var since: String?
    @State private var typed = false
    /// Bumped by each answer: a read that started before it is dropped.
    @State private var gen = 0
    /// Reads in a row with no picker: one is a redraw, two is gone.
    @State private var misses = 0
    @State private var busy: String?
    @State private var text = ""

    private var on: Bool { live.pickable && live.state == .blocked }

    var body: some View {
        Group {
            if on, let p = picker { content(p) } else { fallback }
        }
        // Read it now, when the question changes, and every few seconds: it
        // may be answered in the pane, or move on to the next question.
        .task(id: "\(id) \(block ?? "") \(on) \(live.since ?? "")") {
            guard on else { picker = nil; return }
            while !Task.isCancelled {
                if busy == nil { await load() }
                try? await Task.sleep(for: .seconds(2.5))
            }
        }
    }

    private func load() async {
        let at = gen
        // The hub out of reach for a moment: keep what's shown.
        guard let r = await store.picker(id, block: block ?? live.block), busy == nil, at == gen else { return }
        if let p = AgentQuestion(r["picker"]) {
            misses = 0
            picker = p
            since = r["since"].str
            typed = r["typed"].bool
        } else {
            misses += 1
            if misses >= 2 { picker = nil }
        }
    }

    private func act(_ action: String, _ o: AgentQuestion.Opt? = nil, on p: AgentQuestion) {
        guard busy == nil else { return }
        busy = o.map { "\(action)-\($0.n)" } ?? action
        gen += 1
        var body: [String: Any] = ["question": p.question, "review": p.review, "action": action, "block": block ?? live.block, "since": since ?? NSNull()]
        if let o {
            body["n"] = o.n
            body["label"] = o.label
            if let c = o.checked { body["checked"] = c }
            if o.kind == "other" { body["text"] = text }
        }
        Task {
            if let r = await store.pick(id, body) {
                if o?.kind == "other" { text = "" }
                if r["cleared"].bool { store.toast(o?.kind == "submit" || !p.submitTab ? "Answered" : r["did"].str ?? "Answered") }
                else if r["picker"].isNull { store.toast("Sent, but \(r["now"].str ?? "the question didn't change"). Look at the pane.", error: true) }
                if let next = AgentQuestion(r["picker"]) {
                    misses = 0
                    picker = next
                    since = r["since"].str
                    typed = r["typed"].bool
                }
            }
            gen += 1
            busy = nil
            await load()
        }
    }

    @ViewBuilder private func content(_ p: AgentQuestion) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            if p.tabs.count > 1 || p.submitTab {
                HStack(spacing: 4) {
                    ForEach(Array(p.tabs.enumerated()), id: \.offset) { _, t in
                        TabChip(text: (t.answered ? "✓ " : "") + t.label, tone: t.answered ? K.working : K.muted, on: false)
                    }
                    if p.submitTab { TabChip(text: "Submit", tone: p.review ? K.text : K.muted, on: p.review) }
                }
            }
            if p.review {
                Text("Review your answers").font(.system(size: 13, weight: .semibold))
                ForEach(Array(p.answers.enumerated()), id: \.offset) { _, a in
                    VStack(alignment: .leading, spacing: 1) {
                        Text(a.question).font(.system(size: 12)).foregroundStyle(K.muted)
                        Text(a.answer.isEmpty ? "Not answered" : a.answer).font(.system(size: 13, weight: .medium)).foregroundStyle(a.answer.isEmpty ? K.muted : K.text)
                    }
                    .textSelection(.enabled)
                }
                ForEach(p.notes, id: \.self) { Text($0).font(.system(size: 12)).foregroundStyle(K.muted) }
                HStack(spacing: 6) {
                    if let s = p.options.first(where: { $0.kind == "submit" }) {
                        Button(busy == "pick-\(s.n)" ? "Submitting…" : s.label) { act("pick", s, on: p) }
                            .buttonStyle(.borderedProminent).tint(K.accent)
                    }
                    Button("Back") { act("back", on: p) }.help("Back to the last question (← in the pane)")
                    if let c = p.options.first(where: { $0.kind == "cancel" }) { Button(c.label) { act("pick", c, on: p) } }
                }
                .disabled(busy != nil)
            } else {
                Text(p.question).font(.system(size: 13, weight: .semibold)).foregroundStyle(K.text).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                if p.multi { Text("Pick any that apply, then \(p.next == "Submit" ? "review them" : "go on").").font(.system(size: 12)).foregroundStyle(K.muted) }
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(p.options.filter { $0.kind == "choice" }) { o in
                        OptionRow(o: o, multi: p.multi, busy: busy == "pick-\(o.n)") { act("pick", o, on: p) }
                    }
                }
                .disabled(busy != nil)
                let other = p.options.first { $0.kind == "other" }
                if let other, typed {
                    HStack(spacing: 6) {
                        TextField(p.draft.map { "Or type your own (replaces “\($0)” in the pane)…" } ?? "Or type your own answer…", text: $text)
                            .textFieldStyle(.roundedBorder)
                            .onSubmit { if !text.trimmingCharacters(in: .whitespaces).isEmpty { act("pick", other, on: p) } }
                        Button(busy == "pick-\(other.n)" ? "Sending…" : "Answer") { act("pick", other, on: p) }
                            .disabled(text.trimmingCharacters(in: .whitespaces).isEmpty || busy != nil)
                    }
                }
                HStack(spacing: 6) {
                    if p.multi, let next = p.next {
                        Button(next == "Submit" ? "Review" : "Next") { act("next", on: p) }
                            .buttonStyle(.borderedProminent).tint(K.accent)
                    }
                    if let chat = p.options.first(where: { $0.kind == "chat" }) {
                        Button(chat.label) { act("pick", chat, on: p) }
                            .buttonStyle(.borderless).foregroundStyle(K.link)
                            .help("No answer: Claude Code waits for you to tell it instead")
                    }
                    if other != nil, !typed { Text("Type your own answer in the pane.").font(.system(size: 12)).foregroundStyle(K.muted) }
                }
                .disabled(busy != nil)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        // Your words were for that question: not the next one.
        .onChange(of: "\(p.review) \(p.question)") { text = "" }
    }
}

private struct TabChip: View {
    let text: String
    let tone: Color
    let on: Bool
    var body: some View {
        Text(text)
            .font(.system(size: 11.5))
            .foregroundStyle(tone)
            .padding(.horizontal, 7).padding(.vertical, 1)
            .background(K.main, in: Capsule())
            .overlay(Capsule().strokeBorder(on ? K.accent : K.line))
    }
}

private struct OptionRow: View {
    let o: AgentQuestion.Opt
    let multi: Bool
    let busy: Bool
    let go: () -> Void
    @State private var hover = false

    var body: some View {
        Button(action: go) {
            HStack(alignment: .top, spacing: 10) {
                Text(multi ? (o.checked == true ? "✓" : "") : "\(o.n)")
                    .font(.system(size: 11.5, weight: .medium, design: .monospaced))
                    .foregroundStyle(o.checked == true ? .white : K.muted)
                    .frame(width: 20, height: 20)
                    .background(o.checked == true ? K.accent : .clear, in: RoundedRectangle(cornerRadius: 5))
                    .overlay(RoundedRectangle(cornerRadius: 5).strokeBorder(o.checked == true ? K.accent : K.line2))
                VStack(alignment: .leading, spacing: 2) {
                    Text(busy ? "\(o.label)…" : o.label).font(.system(size: 13, weight: .medium)).foregroundStyle(K.text)
                    if let d = o.description, d != o.label {
                        Text(d).font(.system(size: 12.5)).foregroundStyle(K.text2).fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 10).padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(o.checked == true ? K.accentSoft : hover ? K.hover : K.main, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(o.checked == true ? K.accent : K.line2))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
        .accessibilityAddTraits(multi && o.checked == true ? .isSelected : [])
        .help(multi ? (o.checked == true ? "Untick “\(o.label)”" : "Tick “\(o.label)”") : "Answer “\(o.label)”")
    }
}
