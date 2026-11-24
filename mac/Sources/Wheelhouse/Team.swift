import SwiftUI

/// This hub's workspace (docs/teams.md), as `/api/state`'s `team` says.
struct TeamStatus: Hashable {
    let j: JSON
    var inOne: Bool { j["workspace"].object != nil }
    var name: String { j["workspace"]["name"].str ?? "Team" }
    var prefix: String { j["workspace"]["prefix"].str ?? "" }
    var url: String { j["workspace"]["url"].str ?? "" }
    var you: String { j["workspace"]["you"].str ?? "" }
    var online: Bool { j["online"].bool }
    var error: String? { j["error"].text }
    var members: [TeamMember] { j["members"].array.map(TeamMember.init).filter(\.active) }
    var dockets: [SharedDocket] { j["dockets"].array.map(SharedDocket.init) }
    var outbox: [OutboxItem] { j["outbox"].array.map(OutboxItem.init) }
    /// Something here needs a look: not connected, or an entry held back.
    var alert: Bool { inOne && (!online || outbox.contains { $0.status != "queued" }) }
}

struct TeamMember: Hashable, Identifiable {
    let j: JSON
    var id: String { handle }
    var handle: String { j["handle"].str ?? "" }
    var name: String { j["name"].str ?? "" }
    var role: String { j["role"].str ?? "member" }
    var login: String? { j["login"].text }
    var active: Bool { j["active"].bool }
    var you: Bool { j["you"].bool }
    var joinedAt: String { j["joined_at"].str ?? "" }
}

struct SharedDocket: Hashable, Identifiable {
    let j: JSON
    var id: String { j["id"].str ?? "" }
    var title: String { j["title"].str ?? id }
    var status: String { j["status"].str ?? "" }
    var owners: [String] { j["owners"].strings }
    var members: [String] { j["members"].strings }
    /// Whose each agent on it is: the member, or none for this hub's own.
    var agents: [(member: String?, app: String, role: String?)] { j["agents"].array.map { ($0["member"].text, $0["app"].str ?? "", $0["role"].text) } }
    var halted: String? { j["halted"].text }

    static func == (a: SharedDocket, b: SharedDocket) -> Bool { a.j == b.j }
    func hash(into h: inout Hasher) { h.combine(j) }
}

struct OutboxItem: Hashable, Identifiable {
    let j: JSON
    var id: Int { j["n"].int ?? 0 }
    var docket: String { j["docket"].str ?? "" }
    /// queued, held (it looked like a secret) or refused.
    var status: String { j["status"].str ?? "" }
    var why: String? { j["why"].text }
    var at: String { j["at"].str ?? "" }
}

/// A handle as a small round mark: the same color for a person everywhere.
struct PersonChip: View {
    let handle: String
    var you = false

    private var hue: Double {
        var h = 0
        for c in handle.unicodeScalars { h = (h * 31 + Int(c.value)) % 360 }
        return Double(h) / 360
    }

    var body: some View {
        HStack(spacing: 5) {
            Text(handle.prefix(1).uppercased())
                .font(.system(size: 9.5, weight: .semibold))
                .frame(width: 16, height: 16)
                .background(Circle().fill(Color(hue: hue, saturation: 0.25, brightness: 0.9)))
                .foregroundStyle(Color(hue: hue, saturation: 0.6, brightness: 0.4))
            Text(handle).font(.system(size: 12.5)).foregroundStyle(K.text2)
        }
        .help(you ? "\(handle) (you)" : handle)
    }
}

/// Your workspace: its shared dockets, who's on each, its members, and
/// what's held back from it.
struct TeamScreen: View {
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav

    var body: some View {
        let t = store.state?.team
        Group {
            if let t, t.inOne {
                SplitPane {
                    List(selection: Binding<String?>(get: { nav.selected(.team) }, set: { nav.select($0, in: .team) })) {
                        let held = t.outbox.filter { $0.status != "queued" }
                        if !held.isEmpty {
                            Section("Held back") {
                                ForEach(held) { o in heldRow(o) }
                            }
                        }
                        Section("Shared dockets") {
                            if t.dockets.isEmpty {
                                Text("Nothing shared yet. Share one of yours with wh team share D-12, or open one shared with wh new --team.")
                                    .font(.system(size: 12)).foregroundStyle(.secondary)
                            }
                            ForEach(t.dockets) { d in
                                sharedRow(d, you: t.you).tag(d.id)
                                    .contextMenu { if let x = store.docket(d.id) { DocketMenu(d: x) } }
                            }
                        }
                        Section("Members") {
                            ForEach(t.members) { m in
                                HStack(spacing: 8) {
                                    PersonChip(handle: m.handle, you: m.you)
                                    if !m.name.isEmpty && m.name != m.handle { Text(m.name).foregroundStyle(.secondary) }
                                    Spacer()
                                    if m.role == "admin" { Text("admin").font(.system(size: 11)).foregroundStyle(.tertiary) }
                                    if m.you { Text("you").font(.system(size: 11)).foregroundStyle(.tertiary) }
                                    if let l = m.login { Text(l).font(.system(size: 11, design: .monospaced)).foregroundStyle(.tertiary) }
                                }
                                .selectionDisabled()
                            }
                        }
                    }
                    .listStyle(.inset)
                    .scrollContentBackground(.hidden)
                } detail: {
                    DocketDetail(id: nav.selected(.team))
                }
            } else if store.state != nil {
                ContentUnavailableView(
                    "Not in a workspace",
                    systemImage: "person.2",
                    description: Text("A workspace joins your Wheelhouse with your teammates': you share dockets you choose, and your agents work with theirs across Macs. Ask an admin for an invite, then run wh team join CODE.")
                )
            }
        }
        .navigationSubtitle(t.map { $0.inOne ? "\($0.prefix) · \($0.online ? "connected" : "not connected")" : "" } ?? "")
        .toolbar {
            if t?.inOne == true {
                ToolbarItem {
                    Button { Task { await store.act("/api/team/sync", done: "Up to date with the workspace") } } label: { Label("Sync", systemImage: "arrow.triangle.2.circlepath") }
                        .help("Send what's waiting and take in what's new, now")
                }
            }
        }
    }

    private func sharedRow(_ d: SharedDocket, you: String) -> some View {
        HStack(spacing: 10) {
            Circle().fill(d.halted != nil ? K.error : d.agents.isEmpty ? K.faint : K.working).frame(width: 7, height: 7)
            Text(d.id).font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary)
            Text(d.title).lineLimit(1)
            if let h = d.halted { Text("Not followed: \(h)").font(.system(size: 11.5)).foregroundStyle(K.error).lineLimit(1) }
            Spacer()
            ForEach(d.owners, id: \.self) { PersonChip(handle: $0, you: $0 == you) }
            if !d.members.isEmpty { Text("+\(d.members.count)").font(.system(size: 11.5)).foregroundStyle(.tertiary) }
            if !d.agents.isEmpty {
                let whose = Array(Set(d.agents.map { $0.member ?? you })).sorted()
                Text("\(d.agents.count) agent\(d.agents.count == 1 ? "" : "s") · \(whose.joined(separator: ", "))")
                    .font(.system(size: 11.5)).foregroundStyle(.tertiary)
            }
        }
    }

    private func heldRow(_ o: OutboxItem) -> some View {
        HStack(spacing: 10) {
            Image(systemName: o.status == "held" ? "hand.raised" : "xmark.octagon").foregroundStyle(o.status == "held" ? K.blocked : K.error)
            Text(o.docket).font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary)
            Text(o.status == "held" ? "Held: it looks like it holds a secret" : "Refused by the workspace").lineLimit(1)
            if let w = o.why { Text(w).font(.system(size: 11.5)).foregroundStyle(.secondary).lineLimit(1) }
            Spacer()
            if o.status == "held" {
                Button("Send anyway") { Task { await store.act("/api/team/outbox/\(o.id)", ["action": "send"], done: "#\(o.id) goes to the workspace") } }
            }
            Button("Drop") { Task { await store.act("/api/team/outbox/\(o.id)", ["action": "drop"], done: "#\(o.id) dropped") } }
        }
        .selectionDisabled()
    }
}
