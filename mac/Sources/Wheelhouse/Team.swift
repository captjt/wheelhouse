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

/// A person's round mark: their initial on their color, the same everywhere.
struct PersonDot: View {
    let handle: String
    var size: CGFloat = 16

    private var hue: Double {
        var h = 0
        for c in handle.unicodeScalars { h = (h * 31 + Int(c.value)) % 360 }
        return Double(h) / 360
    }

    var body: some View {
        Text(handle.prefix(1).uppercased())
            .font(.system(size: size * 0.55, weight: .semibold))
            .frame(width: size, height: size)
            .background(Circle().fill(Color(hue: hue, saturation: 0.22, brightness: 0.93)))
            .foregroundStyle(Color(hue: hue, saturation: 0.65, brightness: 0.42))
    }
}

/// A handle with its mark, for the inspector.
struct PersonChip: View {
    let handle: String
    var you = false

    var body: some View {
        HStack(spacing: 5) {
            PersonDot(handle: handle)
            Text(handle).font(.system(size: 12.5)).foregroundStyle(K.text2)
        }
        .help(you ? "\(handle) (you)" : handle)
    }
}

/// Several people as overlapping marks.
struct PersonStack: View {
    let handles: [String]
    var body: some View {
        HStack(spacing: -4) {
            ForEach(handles.prefix(4), id: \.self) { h in
                PersonDot(handle: h, size: 15).overlay(Circle().strokeBorder(K.main, lineWidth: 1.5))
            }
            if handles.count > 4 { Text("+\(handles.count - 4)").font(.system(size: 11)).foregroundStyle(.tertiary).padding(.leading, 6) }
        }
        .help(handles.joined(separator: ", "))
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
                            ForEach(t.members) { m in memberRow(m) }
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
        let x = store.docket(d.id)
        let mates = Array(Set(d.agents.compactMap(\.member).filter { $0 != you })).sorted()
        return HStack(alignment: .top, spacing: 9) {
            Group {
                if d.halted != nil { LiveIcon(state: .error, size: 14) } else if let l = x?.live { LiveIcon(state: l.state, size: 14) } else { StatusIcon(status: x?.status ?? .todo, size: 14) }
            }
            .padding(.top, 1.5)
            VStack(alignment: .leading, spacing: 3) {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text(d.title).font(.system(size: 13, weight: .medium)).lineLimit(1)
                    Spacer(minLength: 4)
                    if let x { Text(ago(x.updatedAt)).font(.system(size: 11.5)).foregroundStyle(.tertiary).monospacedDigit() }
                }
                HStack(spacing: 6) {
                    IdText(id: d.id)
                    if x?.live != nil, let st = x?.status { StatusIcon(status: st, size: 11) }
                    if !d.agents.isEmpty {
                        HStack(spacing: 3) { Image(systemName: "terminal").imageScale(.small); Text("\(d.agents.count)") }
                        if !mates.isEmpty { Text(mates.joined(separator: ", ")).lineLimit(1) }
                    }
                    if d.halted != nil { Text("Not followed").foregroundStyle(K.error) }
                    Spacer(minLength: 0)
                    PersonStack(handles: d.owners + d.members)
                }
                .font(.system(size: 11.5))
                .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
    }

    private func memberRow(_ m: TeamMember) -> some View {
        HStack(spacing: 9) {
            PersonDot(handle: m.handle, size: 20)
            Text(m.handle).font(.system(size: 13, weight: .medium))
            if m.you { Text("(you)").foregroundStyle(.secondary) }
            if !m.name.isEmpty && m.name != m.handle { Text(m.name).foregroundStyle(.secondary).lineLimit(1) }
            Spacer()
            if m.role == "admin" {
                Text("Admin").font(.system(size: 10.5, weight: .medium)).foregroundStyle(.secondary)
                    .padding(.horizontal, 6).padding(.vertical, 1)
                    .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(.separator))
            }
            if let l = m.login { Text(l).font(.system(size: 11.5)).foregroundStyle(.tertiary).lineLimit(1) }
        }
        .padding(.vertical, 3)
        .selectionDisabled()
    }

    private func heldRow(_ o: OutboxItem) -> some View {
        let what = o.status == "refused" ? "Refused by the workspace" : (o.why?.range(of: "holds ").map { "Looks like " + o.why![$0.upperBound...] } ?? "Looks like a secret")
        return HStack(alignment: .top, spacing: 9) {
            Image(systemName: o.status == "held" ? "exclamationmark.square.fill" : "xmark.octagon.fill")
                .foregroundStyle(o.status == "held" ? K.blocked : K.error).padding(.top, 1.5)
            VStack(alignment: .leading, spacing: 3) {
                Text(what).font(.system(size: 13, weight: .medium)).lineLimit(1)
                HStack(spacing: 6) {
                    IdText(id: o.docket)
                    Text(o.status == "held" ? "Kept on this Mac until you say" : (o.why ?? "")).lineLimit(1)
                }
                .font(.system(size: 11.5)).foregroundStyle(.secondary)
            }
            Spacer()
            if o.status == "held" {
                Button("Send anyway") { Task { await store.act("/api/team/outbox/\(o.id)", ["action": "send"], done: "Sent to the workspace") } }
            }
            Button("Drop") { Task { await store.act("/api/team/outbox/\(o.id)", ["action": "drop"], done: "Dropped") } }
        }
        .padding(.vertical, 4)
        .selectionDisabled()
    }
}
