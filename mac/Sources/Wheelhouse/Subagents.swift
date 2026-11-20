import SwiftUI

/// What the docket's agents spawned (src/subagents.rs): read live from Claude
/// Code's and Codex's own files, so it's fetched again while it's on screen.
struct SubagentsSection: View {
    let d: Docket
    @Environment(Store.self) private var store
    @State private var answer: JSON?
    @State private var earlier = false
    @State private var open: String?

    private struct Group: Identifiable {
        let id: String
        let round: Int?
        var agents: [JSON]
    }

    var body: some View {
        let agents = (answer?["agents"].array ?? []).filter { !$0["subagents"].array.isEmpty }
        let groups = Self.group(agents)
        VStack(alignment: .leading, spacing: 10) {
            if !agents.isEmpty, let a = answer {
                HStack(spacing: 8) {
                    Text("Subagents").font(.system(size: 13, weight: .semibold)).foregroundStyle(K.text)
                    Text(summary(a)).font(.system(size: 12)).foregroundStyle(K.muted)
                }
                ForEach(earlier ? groups : Array(groups.prefix(1))) { g in
                    VStack(alignment: .leading, spacing: 8) {
                        if groups.count > 1, let r = g.round { Text("Round \(r)").sectionLabel() }
                        ForEach(g.agents, id: \.["session"].str) { agent in AgentCard(agent: agent, open: $open) }
                    }
                }
                if groups.count > 1 {
                    let rest = groups.dropFirst().reduce(0) { $0 + $1.agents.reduce(0) { $0 + $1["subagents"].array.count } }
                    Button(earlier ? "Show the latest only" : "Show \(groups.count - 1) earlier round\(groups.count == 2 ? "" : "s") · \(rest) subagents") { earlier.toggle() }
                        .buttonStyle(.link).font(.system(size: 12))
                }
            }
        }
        .task(id: d.id) {
            while !Task.isCancelled {
                if let a = try? await store.hub.get("/api/dockets/\(d.id)/subagents") { answer = a }
                try? await Task.sleep(for: .seconds(10))
            }
        }
    }

    private func summary(_ a: JSON) -> String {
        let running = a["running"].int ?? 0
        let tok = a["tokens"].int ?? 0
        return (running > 0 ? "\(running) running · " : "") + "\(a["total"].int ?? 0) in all" + (tok > 0 ? " · \(Self.tokens(tok)) tokens" : "")
    }

    private static func group(_ agents: [JSON]) -> [Group] {
        var out: [Group] = []
        for a in agents {
            let round = a["round"].int
            let key = round.map { "r\($0)" } ?? "now"
            if let i = out.firstIndex(where: { $0.id == key }) { out[i].agents.append(a) } else { out.append(Group(id: key, round: round, agents: [a])) }
        }
        return out
    }

    static func tokens(_ n: Int) -> String {
        n >= 1_000_000 ? String(format: "%.1fM", Double(n) / 1e6) : n >= 1000 ? "\(n / 1000)k" : "\(n)"
    }
}

private struct AgentCard: View {
    let agent: JSON
    @Binding var open: String?

    var body: some View {
        let subs = agent["subagents"].array
        let session = agent["session"].str ?? ""
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                AppAvatar(app: agent["app"].str, size: 16)
                Text(agent["label"].str ?? "agent").font(.system(size: 12.5, weight: .medium)).foregroundStyle(K.text).lineLimit(1)
                Text("\(subs.count) subagent\(subs.count == 1 ? "" : "s")\(agent["began_at"].str.map { " · \(ago($0))" } ?? "")")
                    .font(.system(size: 11.5)).foregroundStyle(K.muted)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 11).padding(.vertical, 7)
            .background(K.group)
            .help("\(agent["cwd"].str ?? "")\nsession \(session)")
            ForEach(Array(subs.enumerated()), id: \.offset) { _, s in
                Rectangle().fill(K.line).frame(height: 1)
                SubRow(s: s, key: "\(session)/\(s["id"].str ?? "")", open: $open)
            }
        }
        .background(K.elev, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
        .clipShape(RoundedRectangle(cornerRadius: 9, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous).strokeBorder(K.line))
    }
}

private struct SubRow: View {
    let s: JSON
    let key: String
    @Binding var open: String?
    @State private var hover = false

    var body: some View {
        let state = s["state"].str ?? "idle"
        let expanded = open == key
        let quiet = state == "idle" || state == "stopped"
        VStack(alignment: .leading, spacing: 6) {
            Button { open = expanded ? nil : key } label: {
                HStack(spacing: 8) {
                    LiveIcon(state: Self.live(state), size: 12).help(Self.word(state))
                    Text(s["name"].str ?? "subagent").font(.system(size: 12.5, weight: .medium)).foregroundStyle(quiet ? K.muted : K.text)
                    if let t = s["task"].text { Text(t).font(.system(size: 12.5)).foregroundStyle(K.text2).lineLimit(1).truncationMode(.tail) }
                    Spacer(minLength: 8)
                    HStack(spacing: 10) {
                        Text([s["model"].str, s["effort"].str].compactMap { $0 }.joined(separator: " · "))
                        if let n = s["tokens"].int, n > 0 { Text(SubagentsSection.tokens(n)).frame(minWidth: 38, alignment: .trailing) }
                        Text(state == "running" ? "Running" : s["last_at"].str.map { ago($0) } ?? "")
                            .foregroundStyle(state == "running" ? K.accent : K.muted)
                            .frame(minWidth: 52, alignment: .trailing)
                    }
                    .font(.system(size: 11.5).monospacedDigit()).foregroundStyle(K.muted).lineLimit(1)
                }
                .padding(.leading, 6 + CGFloat(max(0, (s["depth"].int ?? 1) - 1)) * 16).padding(.trailing, 11).padding(.vertical, 7)
                .background(hover ? K.hover : .clear)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onHover { hover = $0 }
            .help(s["title"].text ?? s["task"].text ?? s["name"].str ?? "")
            if expanded {
                VStack(alignment: .leading, spacing: 6) {
                    if let w = s["words"].text { MarkdownView(text: w, size: 12.5) } else { Text("Nothing said yet.").font(.system(size: 12)).foregroundStyle(K.muted) }
                    Text("\(Self.word(state))\(s["started_at"].str.map { " · started \(ago($0))" } ?? "")\(s["background"].bool ? " · in the background" : "") · \(s["id"].str ?? "")")
                        .font(.system(size: 11.5)).foregroundStyle(K.muted).textSelection(.enabled)
                }
                .padding(.leading, 26).padding(.trailing, 14).padding(.bottom, 10)
            }
        }
    }

    static func live(_ s: String) -> LiveState {
        switch s {
        case "running": .working
        case "done": .done
        case "stopped": .error
        default: .idle
        }
    }

    static func word(_ s: String) -> String {
        switch s {
        case "running": "Running"
        case "done": "Done"
        case "stopped": "Stopped"
        default: "Quiet"
        }
    }
}
