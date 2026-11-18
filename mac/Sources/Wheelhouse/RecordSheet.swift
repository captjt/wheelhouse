import SwiftUI

/// A docket's record, readable: what the check means, who signed, and every
/// entry with its signature.
struct RecordSheet: View {
    let id: String
    @Environment(Store.self) private var store
    @Environment(\.dismiss) private var dismiss
    @State private var rec: JSON?
    @State private var error: String?
    @State private var mark = "all"
    @State private var q = ""
    @State private var explain = false

    private func keyName(_ k: String?) -> String {
        switch k {
        case "owner": "your key"
        case "hub": "Wheelhouse's key"
        case let k? where k.hasPrefix("agent-"): "\(appName(String(k.dropFirst(6).split(separator: "@").first ?? "")))'s key"
        case let k?: k
        default: "an unknown key"
        }
    }

    var body: some View {
        let entries = rec?["entries"].array.map(Entry.init) ?? []
        let v = Verify(j: rec?["verify"] ?? .null)
        let shown = entries.filter { e in
            (mark == "all" || e.mark == mark || (mark == "problems" && !e.problems.isEmpty))
                && (q.isEmpty || "\(e.kind) \(e.body ?? "") \(e.data.pretty) #\(e.seq)".localizedCaseInsensitiveContains(q))
        }.reversed()
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text("\(id)'s record").font(.system(size: 15, weight: .semibold))
                if let rec { Text("\(entries.count) entries · head \(rec["short_head"].str ?? "")").foregroundStyle(.secondary).font(.system(size: 12.5)) }
                Spacer()
                Button("Export…", systemImage: "square.and.arrow.down") { Exporter.export(id, store: store) }
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            .padding(16)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    if let error { Text(error).foregroundStyle(K.error) }
                    if rec != nil {
                        HStack(alignment: .top, spacing: 10) {
                            Image(systemName: v.ok ? "checkmark.shield.fill" : "exclamationmark.shield.fill").font(.system(size: 20)).foregroundStyle(v.ok ? K.working : K.error)
                            VStack(alignment: .leading, spacing: 4) {
                                Text(v.ok ? "Verified: the record is intact." : "The record doesn't check.").font(.system(size: 13.5, weight: .semibold))
                                if v.ok {
                                    Text("All \(v.entries) entries are as they were written: none changed, removed, added in between or reordered, each signed by the key it names, and every file they keep matches its hash.").font(.system(size: 12.5)).foregroundStyle(K.text2)
                                } else {
                                    ForEach(v.problems.prefix(6), id: \.self) { p in Text("• \(p)").font(.system(size: 12.5)) }
                                }
                            }
                        }
                        .padding(14)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(v.ok ? K.successTint : K.dangerTint, in: RoundedRectangle(cornerRadius: 10))
                        DisclosureGroup("What this means", isExpanded: $explain) {
                            VStack(alignment: .leading, spacing: 8) {
                                bullet("Each entry is signed", "by whoever wrote it, with their own key on this Mac: you, Wheelhouse, or an agent (each app has its own). Nobody can sign as someone else through Wheelhouse.")
                                bullet("Each entry is chained", "to the one before by hash. Its id is the hash of its exact bytes. The head (\(rec?["short_head"].str ?? "…")) names this exact record: a PR footer that says \(id) @ \(rec?["short_head"].str ?? "…") points here.")
                                bullet("Verified", "means nothing was changed, removed, slipped in or reordered since it was written, and every kept file is byte for byte what the record pinned.")
                                bullet("It doesn't mean an entry is true.", "Each one's mark says how it's known. Witnessed: \(Mark.words["witnessed"]!) Checkable: \(Mark.words["checkable"]!) Said: \(Mark.words["said"]!)")
                                bullet("The export", "(\(id).jsonl) checks without the hub: wh verify --file \(id).jsonl.")
                            }
                            .padding(.top, 8)
                        }
                        .font(.system(size: 12.5))
                        HStack(spacing: 8) {
                            Text("Signed by").font(.system(size: 12)).foregroundStyle(K.muted)
                            ForEach(rec?["keys"].array ?? [], id: \.self) { k in
                                (Text(keyName(k["name"].str)) + Text(" · \(k["entries"].int ?? 0)").foregroundColor(K.muted) + Text(" \((k["keyid"].str ?? "").replacingOccurrences(of: "ed25519:", with: "").prefix(8))").font(.system(size: 11, design: .monospaced)).foregroundColor(K.faint))
                                    .font(.system(size: 12))
                                    .padding(.horizontal, 8).padding(.vertical, 3)
                                    .background(K.recessed, in: Capsule())
                                    .help(k["keyid"].str ?? "")
                            }
                        }
                        HStack {
                            Picker("", selection: $mark) {
                                Text("All").tag("all")
                                Text("Witnessed").tag("witnessed")
                                Text("Checkable").tag("checkable")
                                Text("Said").tag("said")
                                if !v.ok { Text("Problems").tag("problems") }
                            }
                            .pickerStyle(.segmented).labelsHidden().fixedSize()
                            Spacer()
                            TextField("Find in the record…", text: $q).textFieldStyle(.roundedBorder).frame(width: 240)
                        }
                        LazyVStack(spacing: 0) {
                            ForEach(Array(shown)) { e in RecordRow(id: id, e: e, owner: store.state?.owner ?? "you", keyName: keyName(e.key)) }
                        }
                        .background(K.elev, in: RoundedRectangle(cornerRadius: 10))
                        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(K.line))
                    } else if error == nil {
                        ProgressView().frame(maxWidth: .infinity).padding(40)
                    }
                }
                .padding(18)
            }
        }
        .frame(minWidth: 820, idealWidth: 920, minHeight: 600, idealHeight: 760)
        .task {
            do { rec = try await store.hub.get("/api/dockets/\(id)/record") } catch { self.error = Store.words(error) }
        }
    }

    private func bullet(_ head: String, _ rest: String) -> some View {
        (Text(head).bold() + Text(" ") + Text(rest)).foregroundStyle(K.text2).fixedSize(horizontal: false, vertical: true)
    }
}

private struct RecordRow: View {
    let id: String
    let e: Entry
    let owner: String
    let keyName: String
    @Environment(Store.self) private var store
    @State private var open = false
    @State private var envelope: JSON?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Button { withAnimation(.easeOut(duration: 0.15)) { open.toggle() } } label: {
                HStack(spacing: 10) {
                    Text("#\(e.seq)").font(.system(size: 11.5, design: .monospaced)).foregroundStyle(K.faint).frame(width: 40, alignment: .leading)
                    (e.actorType == "person" ? Avatar.you(owner) : e.actorType == "agent" ? .app(e.actorApp) : .hub).view(16)
                    Text(e.kind).font(.system(size: 12.5, weight: .medium)).frame(width: 74, alignment: .leading)
                    Text(summary).font(.system(size: 12.5)).foregroundStyle(K.text2).lineLimit(1)
                    Spacer()
                    Mark(mark: e.mark)
                    Text(agoLong(e.at)).font(.system(size: 11.5)).foregroundStyle(K.faint).frame(width: 70, alignment: .trailing).help(stamp(e.at))
                    Image(systemName: "chevron.right").font(.system(size: 10, weight: .semibold)).foregroundStyle(K.faint).rotationEffect(.degrees(open ? 90 : 0))
                }
                .padding(.horizontal, 12).padding(.vertical, 8)
                .background(e.problems.isEmpty ? .clear : K.dangerTint)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if open {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(e.problems, id: \.self) { p in Text("• \(p)").foregroundStyle(K.error) }
                    Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 5) {
                        fact("Written by", "\(e.actorType == "person" ? "You" : e.actorType == "agent" ? appName(e.actorApp) : "Wheelhouse") (\(e.actorType): \(e.actorId))")
                        fact("Signed with", "\(keyName) \(e.keyid ?? "")")
                        fact("When", stamp(e.at))
                        fact("How it's known", "\(e.mark): \(Mark.words[e.mark] ?? "")")
                        fact("Id", e.entryId, mono: true)
                        fact("Chain", e.chain, mono: true)
                        if !e.blobs.isEmpty { fact("Files", e.blobs.joined(separator: "\n"), mono: true) }
                    }
                    .font(.system(size: 12))
                    if let b = e.body, !b.isEmpty { MarkdownView(text: b, size: 12.5).padding(10).background(K.recessed, in: RoundedRectangle(cornerRadius: 8)) }
                    if e.data.object?.isEmpty == false { code(e.data.pretty) }
                    if let env = envelope {
                        Text("The envelope, exactly as stored and signed (DSSE, Ed25519):").font(.system(size: 11.5)).foregroundStyle(K.muted)
                        code(((try? JSON.parse(Data((env["envelope"].str ?? "").utf8)))?.pretty) ?? env["envelope"].str ?? "")
                        Text("Its payload, the bytes the signature covers:").font(.system(size: 11.5)).foregroundStyle(K.muted)
                        code(env["payload"].str ?? "")
                    } else {
                        Button("Show the signed envelope") {
                            Task { envelope = try? await store.hub.get("/api/dockets/\(id)/record/\(e.seq)") }
                        }
                        .buttonStyle(.link).font(.system(size: 12))
                    }
                }
                .textSelection(.enabled)
                .padding(.horizontal, 14).padding(.bottom, 14).padding(.top, 4)
            }
            Divider().opacity(0.6)
        }
    }

    private var summary: String {
        if let b = e.body, !b.isEmpty { return String(b.replacingOccurrences(of: #"\s+"#, with: " ", options: .regularExpression).prefix(140)) }
        return (e.data.object ?? [:]).sorted { $0.key < $1.key }.filter { $0.value.object == nil && $0.value.array.isEmpty }.prefix(3).map { "\($0.key): \($0.value.str ?? ($0.value.bool ? "true" : "false"))" }.joined(separator: " · ")
    }

    private func fact(_ k: String, _ v: String, mono: Bool = false) -> some View {
        GridRow {
            Text(k).foregroundStyle(K.muted)
            Text(v).font(mono ? .system(size: 11.5, design: .monospaced) : .system(size: 12)).foregroundStyle(K.text)
        }
    }

    private func code(_ s: String) -> some View {
        ScrollView(.horizontal) {
            Text(s).font(.system(size: 11.5, design: .monospaced)).foregroundStyle(K.text2).fixedSize().padding(10)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(K.recessed, in: RoundedRectangle(cornerRadius: 8))
    }
}
