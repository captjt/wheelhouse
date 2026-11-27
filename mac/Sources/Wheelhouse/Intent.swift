import SwiftUI

/// Who wrote an intent version, in words.
func intentAuthor(_ e: Entry, _ d: Docket) -> String {
    switch e.data["source"].str {
    case "linear": return e.data["taken_by"].exists ? "Linear (\(e.data["key"].str ?? "")), taken by you" : "Linear (\(e.data["key"].str ?? d.linear?.key ?? "issue"))"
    case "review": return "Wheelhouse, from the PR"
    default: return e.writer(on: d.id)
    }
}

/// The docket's living spec: what it's for, how you'll know it's done, and
/// what's out of scope. Each edit, and each box you tick, is a new version
/// on the record, by whoever made it.
struct IntentSection: View {
    let d: Docket
    let entries: [Entry]
    @Environment(Store.self) private var store
    @State private var editing = false
    @State private var text = ""
    @State private var preview = false
    @State private var busy = false
    @State private var history = false
    /// Images on their way into the text.
    @State private var adding = 0

    var body: some View {
        let versions = entries.filter { $0.kind == "intent" }
        let intent = d.intent
        let linearAhead = d.proto != "review" && intent != nil && intent?.source != "linear" && (d.linear?.description).map { $0.trimmingCharacters(in: .whitespacesAndNewlines) != (intent?.body ?? "").trimmingCharacters(in: .whitespacesAndNewlines) } == true
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Text("Intent").font(.system(size: 13, weight: .semibold)).foregroundStyle(K.text)
                if let i = intent, let cur = versions.last {
                    Text("v\(d.intentVersions) · \(intentAuthor(cur, d)) · \(agoLong(i.at))").font(.system(size: 12)).foregroundStyle(K.muted).help(stamp(i.at))
                }
                Spacer()
                if versions.count > 1, !editing {
                    Button("History", systemImage: "clock.arrow.circlepath") { history = true }.buttonStyle(.borderless).controlSize(.small)
                }
                if !editing, intent != nil {
                    Button("Edit", systemImage: "pencil") { edit() }.buttonStyle(.borderless).controlSize(.small)
                }
            }
            if linearAhead, !editing {
                HStack(spacing: 8) {
                    Image(systemName: "circle.hexagongrid")
                    Text("\(d.linear?.key ?? "The issue")'s description in Linear has changed since this was edited here.")
                    Spacer()
                    Button("Use Linear's version") { store.takeLinear(d) }.controlSize(.small)
                }
                .font(.system(size: 12.5))
                .padding(10)
                .background(K.accentSoft, in: RoundedRectangle(cornerRadius: 8))
            }
            if editing {
                VStack(alignment: .leading, spacing: 0) {
                    Picker("", selection: $preview) {
                        Text("Write").tag(false)
                        Text("Preview").tag(true)
                    }
                    .pickerStyle(.segmented).labelsHidden().fixedSize().padding(10)
                    Divider()
                    Group {
                        if preview {
                            MarkdownView(text: text.isEmpty ? "_Nothing yet._" : text).frame(minHeight: 120, alignment: .topLeading)
                        } else {
                            Editor(text: $text, placeholder: "What must be true when this is done?\n\n## Acceptance\n- [ ] …\n\n## Out of scope\n- …", minHeight: 160, maxHeight: 520, font: .monospacedSystemFont(ofSize: 12.5, weight: .regular), focus: true, onSubmit: save, onCancel: { editing = false }, onImages: { images, insert in addImages(images, insert) })
                        }
                    }
                    .padding(12)
                    Divider()
                    HStack {
                        Text(adding > 0 ? "Adding \(adding == 1 ? "an image" : "\(adding) images")…" : "Each save is a new version on the record, signed as you. Paste or drop a mockup to put it in.").font(.system(size: 11.5)).foregroundStyle(K.faint)
                        Spacer()
                        Button("Cancel") { editing = false }
                        Button("Save  ⌘↩") { save() }
                            .buttonStyle(.borderedProminent).tint(K.accent)
                            .disabled(busy || adding > 0 || text.trimmingCharacters(in: .whitespacesAndNewlines) == (intent?.body ?? "").trimmingCharacters(in: .whitespacesAndNewlines))
                    }
                    .controlSize(.small)
                    .padding(10)
                }
                .background(K.elev, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(K.accent.opacity(0.5)))
            } else if let i = intent, !i.body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                MarkdownView(text: i.body, size: 13.5) { line, checked in tick(line, checked) }
                    .padding(.vertical, 2)
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2) { edit() }
            } else {
                Button { edit() } label: {
                    (Text("Write the intent: ").bold() + Text("what this docket is for, how you'll know it's done, and what's out of scope. Agents read it with `wh brief`."))
                        .font(.system(size: 12.5))
                        .foregroundStyle(K.muted)
                        .multilineTextAlignment(.leading)
                        .padding(14)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: 10).strokeBorder(K.line2, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
                }
                .buttonStyle(.plain)
            }
        }
        .sheet(isPresented: $history) { IntentHistory(d: d, versions: versions) }
    }

    /// A mockup pasted or dropped in: staged, then its Markdown where the
    /// cursor was. The hub keeps it with the docket when you save.
    private func addImages(_ images: [(name: String, data: Data)], _ insert: @escaping (String) -> Void) {
        for (name, raw) in images {
            adding += 1
            Task {
                defer { adding -= 1 }
                // Fitted off the main actor: a phone photo is scaled and sent as JPEG.
                let fitted = await Task.detached(priority: .userInitiated) { Images.fit(name: name, data: raw) }.value
                guard let (n, data) = fitted else {
                    store.toast("Couldn't add \(name): it isn't an image this can read", error: true)
                    return
                }
                guard data.count <= Images.maxBytes else {
                    store.toast("\(n) is \(data.count >> 20) MB; an image is at most \(Images.maxBytes >> 20) MB", error: true)
                    return
                }
                do {
                    let s = try await store.stage(name: n, data: data)
                    let alt = (n as NSString).deletingPathExtension.replacingOccurrences(of: "-", with: " ")
                    insert("\n\n" + Images.markdown(s, alt: alt) + "\n\n")
                } catch {
                    store.toast("Couldn't add \(n): \(Store.words(error))", error: true)
                }
            }
        }
    }

    private func edit() {
        text = d.intent?.body ?? ""
        preview = false
        editing = true
    }

    private func save() {
        guard !busy else { return }
        busy = true
        Task {
            if await store.setIntent(d.id, text, versions: d.intentVersions) { editing = false }
            busy = false
        }
    }

    private func tick(_ line: Int, _ checked: Bool) {
        guard let body = d.intent?.body, let next = MD.toggleTask(body, line: line, checked: checked) else {
            store.toast("That box isn't plain in the text: edit the intent to change it", error: true)
            return
        }
        Task {
            if await store.act("/api/dockets/\(d.id)/intent", ["body": next]) != nil {
                store.toast("\(d.id): \(checked ? "ticked off" : "unticked"), saved as version \(d.intentVersions + 1)")
            }
        }
    }
}

/// Every version of the intent: as it read, and what each changed.
struct IntentHistory: View {
    let d: Docket
    let versions: [Entry]
    @State private var i: Int
    @State private var changes = false
    @Environment(\.dismiss) private var dismiss

    init(d: Docket, versions: [Entry]) {
        self.d = d
        self.versions = versions
        _i = State(initialValue: max(0, versions.count - 1))
    }

    var body: some View {
        HStack(spacing: 0) {
            List(selection: Binding<Int?>(get: { i }, set: { if let v = $0 { i = v } })) {
                Section("Versions") {
                    ForEach(Array(versions.enumerated()).reversed(), id: \.offset) { n, v in
                        VStack(alignment: .leading, spacing: 2) {
                            Text("v\(n + 1) · \(intentAuthor(v, d))").font(.system(size: 12.5, weight: .medium))
                            Text(agoLong(v.at)).font(.system(size: 11.5)).foregroundStyle(.secondary)
                        }
                        .tag(n)
                    }
                }
            }
            .listStyle(.sidebar)
            .frame(width: 220)
            Divider()
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    Text("v\(i + 1) · \(intentAuthor(versions[i], d)) · \(stamp(versions[i].at))").font(.headline)
                    Spacer()
                    if i > 0 {
                        Picker("", selection: $changes) {
                            Text("Rendered").tag(false)
                            Text("Changes").tag(true)
                        }
                        .pickerStyle(.segmented).labelsHidden().fixedSize()
                    } else {
                        Text("first version").foregroundStyle(.secondary)
                    }
                    Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
                }
                .padding(14)
                Divider()
                ScrollView {
                    if changes && i > 0 {
                        VStack(alignment: .leading, spacing: 0) {
                            ForEach(Array(lineDiff(versions[i - 1].body ?? "", versions[i].body ?? "").enumerated()), id: \.offset) { _, l in
                                HStack(spacing: 8) {
                                    Text(l.op == .add ? "+" : l.op == .del ? "−" : " ").foregroundStyle(l.op == .add ? K.working : l.op == .del ? K.error : K.faint)
                                    Text(l.text.isEmpty ? " " : l.text).foregroundStyle(l.op == .same ? K.muted : K.text)
                                    Spacer(minLength: 0)
                                }
                                .font(.system(size: 12, design: .monospaced))
                                .padding(.horizontal, 10).padding(.vertical, 1.5)
                                .background(l.op == .add ? K.successTint : l.op == .del ? K.dangerTint : .clear)
                            }
                        }
                        .textSelection(.enabled)
                        .padding(14)
                    } else {
                        MarkdownView(text: versions[i].body ?? "").padding(20)
                    }
                }
            }
        }
        .frame(minWidth: 820, idealWidth: 900, minHeight: 520, idealHeight: 640)
    }
}

struct DiffLine: Equatable {
    enum Op { case same, add, del }
    let op: Op
    let text: String
}

/// A line diff (longest common subsequence): enough for a spec's versions.
func lineDiff(_ a: String, _ b: String) -> [DiffLine] {
    let x = a.isEmpty ? [] : a.components(separatedBy: "\n")
    let y = b.isEmpty ? [] : b.components(separatedBy: "\n")
    let n = x.count, m = y.count
    var lcs = Array(repeating: Array(repeating: 0, count: m + 1), count: n + 1)
    for i in stride(from: n - 1, through: 0, by: -1) {
        for j in stride(from: m - 1, through: 0, by: -1) {
            lcs[i][j] = x[i] == y[j] ? lcs[i + 1][j + 1] + 1 : max(lcs[i + 1][j], lcs[i][j + 1])
        }
    }
    var out: [DiffLine] = []
    var i = 0, j = 0
    while i < n, j < m {
        if x[i] == y[j] {
            out.append(DiffLine(op: .same, text: x[i])); i += 1; j += 1
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.append(DiffLine(op: .del, text: x[i])); i += 1
        } else {
            out.append(DiffLine(op: .add, text: y[j])); j += 1
        }
    }
    while i < n { out.append(DiffLine(op: .del, text: x[i])); i += 1 }
    while j < m { out.append(DiffLine(op: .add, text: y[j])); j += 1 }
    return out
}

/// A diff line, or a run of unchanged ones folded away.
enum FoldedLine: Equatable {
    case line(DiffLine)
    case skip(Int)
}

/// A long diff, folded (web/src/diff.ts `foldDiff`): changed lines with
/// `context` lines around them, and each run of unchanged lines between as
/// one `skip` with its count.
func foldDiff(_ d: [DiffLine], context: Int = 3) -> [FoldedLine] {
    var near = Array(repeating: false, count: d.count)
    for (i, l) in d.enumerated() where l.op != .same {
        for k in max(0, i - context) ... min(d.count - 1, i + context) { near[k] = true }
    }
    var out: [FoldedLine] = []
    var skipped = 0
    for (i, l) in d.enumerated() {
        if near[i] {
            if skipped > 0 { out.append(.skip(skipped)) }
            skipped = 0
            out.append(.line(l))
        } else {
            skipped += 1
        }
    }
    if skipped > 0 { out.append(.skip(skipped)) }
    return out
}

/// Decisions in force: what an agent asked you, and the answer that stands.
struct DecisionsSection: View {
    let decisions: [JSON]
    var docket: String?
    /// The docket's repo: an answered decision can stand in its notes.
    var repo: String?

    var body: some View {
        if !decisions.isEmpty {
            let open = decisions.filter { $0["answer"].isNull }.count
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 8) {
                    Text("Decisions").font(.system(size: 13, weight: .semibold)).foregroundStyle(K.text)
                    Text("\(decisions.count - open) in force\(open > 0 ? " · \(open) waiting on you" : "")").font(.system(size: 12)).foregroundStyle(K.muted)
                }
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(Array(decisions.enumerated()), id: \.offset) { _, d in
                        DecisionRow(d: d, docket: docket, repo: repo)
                    }
                }
            }
        }
    }
}

private struct DecisionRow: View {
    let d: JSON
    var docket: String?
    var repo: String?
    @Environment(Store.self) private var store
    @State private var hover = false

    var body: some View {
        let answered = d["answer"].exists
        let by: String = {
            switch d["by"].str {
            case "stood": return "the recommendation stood"
            case let b?: return b
            default: return ""
            }
        }()
        VStack(alignment: .leading, spacing: 2) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Image(systemName: answered ? "checkmark" : "questionmark")
                    .font(.system(size: 10, weight: .bold))
                    .foregroundStyle(answered ? K.working : K.blocked)
                    .frame(width: 14)
                // One of yours from the composer may have no question.
                if let q = d["question"].str, !q.isEmpty {
                    Text(q).foregroundStyle(K.text2)
                    Text("→").foregroundStyle(K.faint)
                }
                Text(d["answer"].str ?? "waiting on you").fontWeight(answered ? .semibold : .regular).foregroundStyle(answered ? K.text : K.blocked)
                if answered {
                    Text("\(by)\(d["at"].str.map { " · \(agoLong($0))" } ?? "")").font(.system(size: 11.5)).foregroundStyle(K.faint)
                }
                Spacer(minLength: 0)
                if canStand {
                    // Always laid out, shown on hover: the row never jumps.
                    Button("Make Standing") { stand() }
                        .buttonStyle(.borderless).controlSize(.small)
                        .opacity(hover ? 1 : 0)
                        .allowsHitTesting(hover)
                        .help("Add “\(d["question"].str ?? "") \(d["answer"].str ?? "")” to \(repo ?? "")'s notes as a standing rule: later briefs there carry it")
                }
            }
            if let note = d["note"].text {
                Text("“\(note)”").font(.system(size: 12)).foregroundStyle(K.muted).padding(.leading, 20)
            }
        }
        .font(.system(size: 13))
        .padding(.horizontal, 8).padding(.vertical, 5)
        .background(hover ? K.hover : .clear, in: RoundedRectangle(cornerRadius: 6))
        .onHover { hover = $0 }
        .help("Asked by \(d["asked_by"].str ?? "?"), \(stamp(d["asked_at"].str))\(d["options"].strings.isEmpty ? "" : "\nOptions: \(d["options"].strings.joined(separator: " · "))")")
        .contextMenu {
            if canStand {
                Button("Make Standing for \(repo ?? "the Repo")") { stand() }
            }
            Button("Copy Decision") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString("\(d["question"].str ?? "") → \(d["answer"].str ?? "")", forType: .string)
            }
        }
    }

    /// Answered, in a known repo, with a key to name it by.
    private var canStand: Bool { d["answer"].exists && repo != nil && docket != nil && d["key"].text != nil }

    private func stand() {
        guard let docket, let key = d["key"].text else { return }
        store.makeStanding(docket, key: key)
    }
}
