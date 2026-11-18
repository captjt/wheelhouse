import AppKit
import CryptoKit
import SwiftUI

// Verify (DESIGN milestone 4, docs/verify.md): what a docket's done rests
// on. Witnessed checks the hub ran, the checker's verdicts, the rule for
// done, and hold.

/// Inline Markdown (`code`, **bold**) in a line of the hub's words.
func inlineMarkdown(_ s: String) -> AttributedString {
    (try? AttributedString(markdown: s, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(s)
}

/// The docket's Verify section: what its done rests on, its checks, the
/// checker's verdicts, and a close on your word. Hidden when there's
/// nothing to show.
struct VerifySection: View {
    let page: DocketPage
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @State private var starting = false
    @State private var output: CheckRun?

    var body: some View {
        let d = page.docket
        let ev = page.evidence
        if !ev.isEmpty {
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 8) {
                    Text("Verify").font(.system(size: 13, weight: .semibold)).foregroundStyle(K.text)
                    Text(subtitle(ev)).font(.system(size: 12)).foregroundStyle(K.muted).lineLimit(1)
                    Spacer()
                    if ev.running || starting {
                        HStack(spacing: 6) {
                            ProgressView().controlSize(.mini)
                            Text("Running…").font(.system(size: 12)).foregroundStyle(K.muted)
                        }
                    } else if !ev.configured.isEmpty || ev.repo != nil {
                        Button("Run Checks", systemImage: "play") { run(d.id) }
                            .buttonStyle(.borderless).controlSize(.small)
                            .disabled(ev.cantRun != nil)
                            .help(ev.cantRun ?? "Run \(ev.configured.map(\.name).joined(separator: ", ")) at the folder's commit, as the hub, and keep the results on the record")
                    }
                }
                VStack(spacing: 0) {
                    rows(d, ev)
                }
                .background(K.elev, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(K.line))
                if ev.cantRun != nil, ev.configured.isEmpty, ev.needs {
                    Text(ev.cantRun ?? "").font(.system(size: 11.5)).foregroundStyle(K.faint)
                }
            }
            .id("verify")
            .sheet(item: $output) { c in CheckOutputSheet(docket: d.id, check: c) }
            .onReceive(NotificationCenter.default.publisher(for: .snapshotOutput)) { n in
                output = ev.checks.first { $0.name == n.object as? String }
            }
        }
    }

    private func subtitle(_ ev: Evidence) -> String {
        var parts: [String] = []
        if let r = ev.repo { parts.append(r) }
        if ev.needs { parts.append(ev.lines.isEmpty ? "done needs evidence" : "done rests on \(plural(ev.lines.count, "thing"))") }
        return parts.joined(separator: " · ")
    }

    @ViewBuilder
    private func rows(_ d: Docket, _ ev: Evidence) -> some View {
        let checks = ev.checks
        let unrun = ev.configured.filter { c in !checks.contains { $0.name == c.name } }
        // What done rests on: each line with its mark.
        ForEach(Array(ev.lines.enumerated()), id: \.offset) { i, l in
            if i > 0 { Divider().opacity(0.6) }
            HStack(alignment: .firstTextBaseline, spacing: 9) {
                Image(systemName: l.how == "witnessed" ? "checkmark.seal.fill" : "checkmark.circle")
                    .font(.system(size: 12)).foregroundStyle(l.how == "witnessed" ? K.working : K.accent)
                    .frame(width: 16)
                (Text("Done rests on ").foregroundColor(K.muted) + Text(inlineMarkdown(l.what)).foregroundColor(K.text))
                    .font(.system(size: 12.5))
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 6)
                Mark(mark: l.how)
                if let at = l.at { Text(ago(at)).font(.system(size: 11.5)).foregroundStyle(K.faint).help(stamp(at)) }
            }
            .padding(.horizontal, 12).padding(.vertical, 9)
        }
        if ev.needs, ev.lines.isEmpty {
            HStack(alignment: .firstTextBaseline, spacing: 9) {
                Image(systemName: "text.bubble").font(.system(size: 12)).foregroundStyle(K.faint).frame(width: 16)
                VStack(alignment: .leading, spacing: 3) {
                    Text(d.status == .done ? "Done rests on what was said." : "Nothing yet but what was said.").font(.system(size: 12.5)).foregroundStyle(K.text)
                    Text("Its intent has acceptance items, so done needs a passing check, a merged PR, or a completed issue. Or your word, with a reason.")
                        .font(.system(size: 12)).foregroundStyle(K.muted).fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 6)
                if d.isOpen {
                    Button("Close on Your Word…") { nav.closing = Closing(id: d.id, why: "", from: d.status) }
                        .buttonStyle(.link).font(.system(size: 12)).fixedSize()
                }
            }
            .padding(.horizontal, 12).padding(.vertical, 9)
        }
        let lead = !ev.lines.isEmpty || (ev.needs && ev.lines.isEmpty)
        // The checks: the latest run of each, then the ones not run yet.
        ForEach(Array(checks.enumerated()), id: \.element.id) { i, c in
            if lead || i > 0 { Divider().opacity(0.6) }
            CheckRow(c: c) { output = c }
        }
        if !unrun.isEmpty {
            if lead || !checks.isEmpty { Divider().opacity(0.6) }
            HStack(alignment: .firstTextBaseline, spacing: 9) {
                Image(systemName: "circle.dashed").font(.system(size: 12)).foregroundStyle(K.faint).frame(width: 16)
                (Text(unrun.map(\.name).joined(separator: ", ")).font(.system(size: 12.5, weight: .medium, design: .monospaced)).foregroundColor(K.text2)
                    + Text(checks.isEmpty ? "  not run on this docket yet" : "  not run yet").foregroundColor(K.muted))
                    .font(.system(size: 12.5))
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 12).padding(.vertical, 9)
        }
        // The checker's verdicts: said by the checker, independent or not.
        ForEach(Array(ev.verdicts.enumerated()), id: \.element.id) { i, v in
            if lead || !checks.isEmpty || !unrun.isEmpty || i > 0 { Divider().opacity(0.6) }
            VerdictRow(v: v)
        }
        if let why = ev.closedOn ?? d.closedOn {
            if lead || !checks.isEmpty || !unrun.isEmpty || !ev.verdicts.isEmpty { Divider().opacity(0.6) }
            HStack(alignment: .firstTextBaseline, spacing: 9) {
                Image(systemName: "quote.bubble").font(.system(size: 12)).foregroundStyle(K.blocked).frame(width: 16)
                (Text("Closed on your word: ").fontWeight(.medium).foregroundColor(K.text) + Text(why).foregroundColor(K.text2))
                    .font(.system(size: 12.5))
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
                Spacer(minLength: 6)
                Mark(mark: "said")
            }
            .padding(.horizontal, 12).padding(.vertical, 9)
            .help("Nothing witnessed or checkable said its acceptance items were met. The record keeps your reason.")
        }
    }

    private func run(_ id: String) {
        starting = true
        Task {
            await store.runChecks(id)
            try? await Task.sleep(for: .seconds(1))
            starting = false
        }
    }
}

/// One check's latest run: how it went, where, how long, and its output.
struct CheckRow: View {
    let c: CheckRun
    let open: () -> Void
    @State private var hover = false

    var body: some View {
        let tint = c.ok ? K.working : c.timedOut ? K.blocked : c.required ? K.error : K.muted
        HStack(alignment: .top, spacing: 9) {
            Image(systemName: c.ok ? "checkmark.circle.fill" : c.timedOut ? "clock.badge.exclamationmark" : "xmark.circle.fill")
                .font(.system(size: 13)).foregroundStyle(tint)
                .frame(width: 16).padding(.top, 1)
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline, spacing: 7) {
                    Text(c.name).font(.system(size: 12.5, weight: .semibold, design: .monospaced)).foregroundStyle(K.text)
                    if !c.required { Chip(text: "optional", color: K.muted).help("An optional check only informs: it doesn't hold a PR or a push") }
                    Text(c.outcome).font(.system(size: 12.5)).foregroundStyle(c.ok ? K.text2 : tint)
                    Spacer(minLength: 6)
                    Mark(mark: "witnessed")
                    if let at = c.at { Text(ago(at)).font(.system(size: 11.5)).foregroundStyle(K.faint).help(stamp(at)) }
                }
                (Text(c.command).font(.system(size: 11.5, design: .monospaced))
                    + Text(c.shortHead.isEmpty ? "" : "  at \(c.shortHead)")
                    + Text(c.dirty ? " with uncommitted changes" : "")
                    + Text(c.ms.map { " · \(duration(ms: $0))" } ?? ""))
                    .font(.system(size: 12))
                    .foregroundStyle(K.muted)
                    .fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }
            if c.output != nil {
                Button("Output", action: open)
                    .buttonStyle(.borderless).controlSize(.small)
                    .help("Its output's last 256 KB, kept by its hash")
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 9)
        .background(hover ? K.hover : .clear)
        .onHover { hover = $0 }
        .help("#\(c.seq) · \(c.why == "asked" ? "run when asked" : c.why.map { "run \($0)" } ?? "run") · \(c.head)")
    }
}

/// A checker's verdict, as it signed it, and whether it's independent.
struct VerdictRow: View {
    let v: Verdict

    var body: some View {
        let who = appName(String(v.by.split(separator: "@").first ?? ""))
        HStack(alignment: .top, spacing: 9) {
            Image(systemName: v.passed ? "hand.thumbsup" : "exclamationmark.bubble")
                .font(.system(size: 12)).foregroundStyle(v.passed ? K.working : K.error)
                .frame(width: 16).padding(.top, 1)
            VStack(alignment: .leading, spacing: 4) {
                HStack(alignment: .firstTextBaseline, spacing: 7) {
                    Text("\(v.role.map { $0.prefix(1).uppercased() + $0.dropFirst() } ?? "Checker")\(v.n.map { " · check \($0)" } ?? "")")
                        .font(.system(size: 12.5, weight: .semibold)).foregroundStyle(K.text)
                    Text(who).font(.system(size: 12.5)).foregroundStyle(K.muted)
                    Chip(text: v.passed ? "passed" : v.verdict.isEmpty ? "no verdict" : "found problems", color: v.passed ? K.working : K.error)
                    if v.findings > 0 { Text(plural(v.findings, "finding")).font(.system(size: 12)).foregroundStyle(K.muted) }
                    Spacer(minLength: 6)
                    Mark(mark: v.mark)
                    if let at = v.at { Text(ago(at)).font(.system(size: 11.5)).foregroundStyle(K.faint).help(stamp(at)) }
                }
                if let i = v.independent {
                    Label(i ? "Independent: its own session, apart from the builder's" : "Same session as the builder: not an independent check",
                          systemImage: i ? "person.2" : "person.fill.questionmark")
                        .font(.system(size: 11.5))
                        .foregroundStyle(i ? K.muted : K.blocked)
                }
                if let s = v.summary {
                    Text(inlineMarkdown(s)).font(.system(size: 12.5)).foregroundStyle(K.text2)
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                }
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 9)
        .help("#\(v.seq) · said by \(v.by)\(v.head.map { " · at \($0.prefix(7))" } ?? "")")
    }
}

/// A check's output: the bytes the record pinned, checked against their hash.
struct CheckOutputSheet: View {
    let docket: String
    let check: CheckRun
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var text: String?
    @State private var error: String?
    @State private var matches = false

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Text(check.name).font(.system(size: 14, weight: .semibold, design: .monospaced))
                Text("\(check.outcome)\(check.shortHead.isEmpty ? "" : " at \(check.shortHead)") · \(docket)").font(.system(size: 12.5)).foregroundStyle(.secondary)
                Spacer()
                Button("Copy") {
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(text ?? "", forType: .string)
                }
                .disabled(text == nil)
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            .padding(14)
            Divider()
            Group {
                if let text {
                    LogText(text: text.isEmpty ? "(no output)" : text)
                } else if let error {
                    ContentUnavailableView("Can't show the output", systemImage: "exclamationmark.triangle", description: Text(error))
                } else {
                    ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            Divider()
            HStack(spacing: 6) {
                if matches { Image(systemName: "checkmark.shield").foregroundStyle(K.working) }
                Text("\(check.command) · the end of its output, kept by hash\(matches ? ": it matches" : "")")
                    .font(.system(size: 11.5)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                Spacer()
                Text("sha256:\((check.output ?? "").prefix(12))…").font(.system(size: 11, design: .monospaced)).foregroundStyle(.tertiary)
                    .help(check.output ?? "")
            }
            .padding(.horizontal, 14).padding(.vertical, 9)
        }
        .frame(minWidth: 680, idealWidth: 760, minHeight: 420, idealHeight: 520)
        .task {
            guard let hex = check.output else { return }
            do {
                let data = try await store.hub.data("/api/blobs/\(hex)")
                matches = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined() == hex
                text = String(decoding: data, as: UTF8.self)
            } catch { self.error = Store.words(error) }
        }
    }
}

/// Read-only text in the system's own text view: long logs scroll down and
/// wrap, never sideways.
struct LogText: NSViewRepresentable {
    let text: String

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSTextView.scrollableTextView()
        scroll.drawsBackground = false
        let tv = scroll.documentView as! NSTextView
        tv.isEditable = false
        tv.isSelectable = true
        tv.drawsBackground = false
        tv.font = .monospacedSystemFont(ofSize: 11.5, weight: .regular)
        tv.textColor = .labelColor
        tv.textContainerInset = NSSize(width: 12, height: 10)
        tv.string = text
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let tv = scroll.documentView as? NSTextView, tv.string != text else { return }
        tv.string = text
    }
}

/// On hold: why, and Resume.
struct HoldBanner: View {
    let d: Docket
    @Environment(Store.self) private var store

    var body: some View {
        if let why = d.hold {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "pause.circle.fill").font(.system(size: 16)).foregroundStyle(K.blocked).padding(.top, 1)
                VStack(alignment: .leading, spacing: 4) {
                    Text("On hold").font(.system(size: 13, weight: .semibold))
                    (Text(why).foregroundColor(K.text) + Text("  Build and Review take no steps on it until you resume it.").foregroundColor(K.muted))
                        .font(.system(size: 12.5))
                        .fixedSize(horizontal: false, vertical: true)
                        .textSelection(.enabled)
                }
                Spacer(minLength: 8)
                Button("Resume") { store.resume(d.id) }.controlSize(.small)
                    .help("Take it off hold: Build and Review carry on")
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(K.warningTint, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(K.blocked.opacity(0.25)))
        }
    }
}

/// Hold a docket, with why.
struct HoldSheet: View {
    let id: String
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var reason = ""
    @State private var busy = false
    @State private var error: String?
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: "pause.circle").font(.system(size: 26)).foregroundStyle(K.blocked)
                VStack(alignment: .leading, spacing: 4) {
                    Text("Hold \(id)").font(.system(size: 14, weight: .semibold))
                    Text(Store.shared.docket(id)?.title ?? "").font(.system(size: 12.5)).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Text("Build and Review take no steps on it, polls included, until you resume it. Agents in its panes carry on.")
                .font(.system(size: 12.5)).foregroundStyle(K.text2).fixedSize(horizontal: false, vertical: true)
            TextField("Why? It goes on the record, and on the docket.", text: $reason, axis: .vertical)
                .lineLimit(2 ... 4)
                .textFieldStyle(.roundedBorder)
                .focused($focused)
                .onSubmit(hold)
            if let error { Label(error, systemImage: "exclamationmark.triangle.fill").font(.system(size: 12)).foregroundStyle(K.error).fixedSize(horizontal: false, vertical: true) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(busy ? "Holding…" : "Hold") { hold() }
                    .buttonStyle(.borderedProminent).tint(K.accent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(busy)
            }
        }
        .padding(20)
        .frame(width: 460)
        .onAppear { focused = true }
    }

    private func hold() {
        guard !busy else { return }
        busy = true
        Task {
            do {
                try await store.hold(id, reason: reason)
                dismiss()
            } catch { self.error = Store.words(error) }
            busy = false
        }
    }
}

/// Done was refused: say why, and offer its checks, or your word.
struct CloseOnWordSheet: View {
    let closing: Closing
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var reason = ""
    @State private var busy = false
    @State private var ev: Evidence?
    @State private var error: String?
    @FocusState private var focused: Bool

    var body: some View {
        let id = closing.id
        let canRun = ev.map { $0.cantRun == nil } ?? false
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: "checkmark.seal").font(.system(size: 26)).foregroundStyle(K.blocked)
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(id) needs evidence to be done").font(.system(size: 14, weight: .semibold))
                    Text(Store.shared.docket(id)?.title ?? "").font(.system(size: 12.5)).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Text("Its intent has acceptance items, and nothing witnessed or checkable says they're met: no passing checks, no merged PR, no completed issue. Run its checks, or close it on your word.")
                .font(.system(size: 12.5)).foregroundStyle(K.text2).fixedSize(horizontal: false, vertical: true)
                .help(closing.why)
            if let ev, let why = ev.cantRun {
                Label(why, systemImage: "info.circle").font(.system(size: 12)).foregroundStyle(K.muted)
            } else if let ev, !ev.configured.isEmpty {
                Label("\(ev.repo ?? "Its repo")'s checks: \(ev.configured.map { $0.required ? $0.name : "\($0.name) (optional)" }.joined(separator: ", "))", systemImage: "checklist")
                    .font(.system(size: 12)).foregroundStyle(K.muted)
            }
            VStack(alignment: .leading, spacing: 6) {
                Text("Close on your word").font(.system(size: 12.5, weight: .semibold))
                TextField("Why it's done (required)", text: $reason, axis: .vertical)
                    .lineLimit(2 ... 4)
                    .textFieldStyle(.roundedBorder)
                    .focused($focused)
                Text("The record keeps your reason, and the docket shows it was closed on your word.")
                    .font(.system(size: 11.5)).foregroundStyle(.secondary)
                if let error { Label(error, systemImage: "exclamationmark.triangle.fill").font(.system(size: 12)).foregroundStyle(K.error).fixedSize(horizontal: false, vertical: true) }
            }
            HStack {
                Button("Run Checks") {
                    Task {
                        do {
                            try await store.startChecks(id)
                            dismiss()
                        } catch { self.error = Store.words(error) }
                    }
                }
                .disabled(!canRun || busy)
                .help(ev?.cantRun ?? "Run its checks now; a passing run is evidence")
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(busy ? "Closing…" : "Close on Your Word") { close() }
                    .buttonStyle(.borderedProminent).tint(K.accent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(reason.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || busy)
            }
        }
        .padding(20)
        .frame(width: 500)
        .onAppear { focused = true }
        .task { ev = try? await store.page(id).evidence }
    }

    private func close() {
        let r = reason.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !r.isEmpty, !busy else { return }
        busy = true
        Task {
            do {
                try await store.closeOnWord(closing.id, reason: r, undo: closing.from)
                dismiss()
            } catch { self.error = Store.words(error) }
            busy = false
        }
    }
}

extension Notification.Name {
    static let snapshotOutput = Notification.Name("wheelhouse.snapshotOutput")
    static let snapshotScroll = Notification.Name("wheelhouse.snapshotScroll")
}
