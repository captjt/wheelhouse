import SwiftUI

/// Who or what an entry's avatar shows.
enum Avatar {
    case you(String), person(String), hub, app(String), pr, issue, live(LiveState)

    @ViewBuilder func view(_ size: CGFloat = 18) -> some View {
        switch self {
        case let .you(n): PersonAvatar(name: n, size: size)
        case let .person(h): PersonDot(handle: h, size: size)
        case .hub: HubAvatar(size: size)
        case let .app(a): AppAvatar(app: a, size: size)
        case .pr: Image(systemName: "arrow.triangle.pull").font(.system(size: size * 0.75)).foregroundStyle(K.muted).frame(width: size, height: size)
        case .issue: Image(systemName: "circle.hexagongrid").font(.system(size: size * 0.75)).foregroundStyle(K.muted).frame(width: size, height: size)
        case let .live(s): LiveIcon(state: s, size: size * 0.9).frame(width: size, height: size)
        }
    }
}

/// A line of the activity: plain words, a bold name, or a link.
enum Bit {
    case t(String), b(String), link(String, URL?), mono(String)
}

func line(_ bits: [Bit]) -> AttributedString {
    var out = AttributedString()
    for bit in bits {
        switch bit {
        case let .t(s): out += AttributedString(s)
        case let .b(s):
            var a = AttributedString(s)
            a.font = .system(size: 12.5, weight: .semibold)
            a.foregroundColor = K.text
            out += a
        case let .link(s, url):
            var a = AttributedString(s)
            if let url {
                a.link = url
                a.foregroundColor = K.link
            }
            out += a
        case let .mono(s):
            var a = AttributedString(s)
            a.font = .system(size: 11.5, design: .monospaced)
            out += a
        }
    }
    return out
}

/// One entry, as the web app's activity feed shows it: a quiet line for what
/// happened, a card for what someone said.
struct ActivityRow: View {
    let e: Entry
    let d: Docket
    let owner: String
    @Environment(Nav.self) private var nav
    @Environment(\.docketImages) private var docketImages
    @Environment(\.carriedImages) private var carriedImages

    /// The images an entry carries (`data.images`: a note, a decision, a
    /// question you sent with them).
    private var carried: [DocketImage] {
        e.data["images"].array.compactMap { DocketImage($0, known: docketImages) }
    }

    var body: some View {
        switch shape {
        case let .sys(avatar, text, tone):
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                avatar.view(16).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 3 }
                (Text(text) + Text("  ·  ").foregroundColor(K.faint) + Text(agoLong(e.at)).foregroundColor(K.faint))
                    .font(.system(size: 12.5))
                    .foregroundStyle(toneColor(tone) ?? K.text2)
                    .fixedSize(horizontal: false, vertical: true)
                    .help("\(stamp(e.at))\n#\(e.seq) · \(e.mark), signed by \(e.key ?? "?")")
                Spacer(minLength: 6)
                Mark(mark: e.mark)
            }
            .padding(.leading, 4)
        case let .comment(avatar, name, verb, body, tone):
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 7) {
                    avatar.view(18)
                    Text(name).font(.system(size: 12.5, weight: .semibold)).foregroundStyle(K.text)
                    Text(verb).font(.system(size: 12.5)).foregroundStyle(K.muted).lineLimit(1)
                    Text("·  \(agoLong(e.at))").font(.system(size: 12)).foregroundStyle(K.faint).help(stamp(e.at))
                    Spacer()
                    Mark(mark: e.mark)
                }
                // The Listener's words, from a call: no image of theirs loads.
                Clamp { MarkdownView(text: body, images: e.kind != "proposal") }
                if !carried.isEmpty { ImageStrip(images: carried) }
            }
            .padding(12)
            .background(tone == "you" ? K.accentSoft.opacity(0.5) : K.elev, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(tone == "ask" ? K.blocked.opacity(0.35) : K.line))
        case let .file(avatar, name, title, group):
            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    avatar.view(16).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 3 }
                    (Text(name).bold() + Text(e.data["url"].exists ? " linked " : " kept ") + Text(title).foregroundColor(K.link) + Text(group.map { " · \($0)" } ?? "").foregroundColor(K.muted) + Text("  ·  \(agoLong(e.at))").foregroundColor(K.faint))
                        .font(.system(size: 12.5))
                        .foregroundStyle(K.text2)
                        .onTapGesture { Artifacts.open(e) }
                    Spacer(minLength: 6)
                    Mark(mark: e.mark)
                }
                // An image shows as itself; the original of a marked-up one is beside it already.
                if let im = DocketImage(artifact: e), im.role != "original" {
                    ImageStrip(images: [im], side: 96).padding(.leading, 24)
                }
            }
            .padding(.leading, 4)
        case .none:
            EmptyView()
        }
    }

    private func toneColor(_ tone: String?) -> Color? {
        switch tone {
        case "blocked": K.blocked
        case "error", "denied": K.error
        case "approved": K.working
        default: nil
        }
    }

    enum Shape {
        case sys(Avatar, AttributedString, String?)
        case comment(Avatar, String, String, String, String?)
        case file(Avatar, String, String, String?)
        case none
    }

    private var shape: Shape {
        let data = e.data
        // The agent it's by or about: its own, by block, on a docket with
        // several; the docket's agent for an older entry.
        // On a shared docket, a teammate's entry (`sam`, `hub@sam`,
        // `codex@sam`) is named for them, never "You".
        let mate = e.mate(on: d.id)
        let youName = mate != nil && e.actorType == "person" ? mate! : "You"
        let mates = mate.map { "\($0)'s " } ?? ""
        let who = d.member(for: e)
        let agent = mates + d.agentName(who)
        let you: Avatar = mate != nil && e.actorType == "person" ? .person(mate!) : .you(owner)
        let app = Avatar.app(who?.app ?? d.agent?.app ?? "")
        let writer = e.actorType == "agent" ? (d.crew.count > 1 && who != nil ? agent : mates + appName(e.actorApp)) : e.actorType == "person" ? youName : "Wheelhouse"
        let writerAvatar: Avatar = e.actorType == "agent" ? .app(e.actorApp) : e.actorType == "person" ? you : .hub
        func sys(_ a: Avatar, _ bits: [Bit], _ tone: String? = nil) -> Shape { .sys(a, line(bits), tone) }

        switch e.kind {
        case "share":
            return sys(you, [.b(youName), .t(" shared this docket with the team"), data["local"].str.map { .mono(" from \($0)") } ?? .t("")])
        case "people":
            let h = data["handle"].str
            let them = h == Docket.me ? "you" : (h ?? "")
            let text: String
            if h == nil {
                text = data["answer"].str == "members" ? "let anyone on it answer its agents' questions" : "let only owners answer its agents' questions"
            } else if data["action"].str == "remove" {
                text = h == e.actorId ? "left this docket" : "took \(them) off this docket"
            } else {
                let role = data["role"].str == "owner" ? "an owner" : "a member"
                text = h == e.actorId ? "joined as \(role)" : "made \(them) \(role)"
            }
            return sys(you, [.b(youName), .t(" \(text)")])
        case "trust":
            var parts: [String] = []
            if let t = data["teammate"].str {
                let whose = t == Docket.me ? "your" : "\(t)'s"
                parts.append(data["on"].bool == false && data["on"].exists ? "asks before letting \(whose) agents' messages in" : "lets \(whose) agents' messages in")
            }
            if data["private_prompts"].exists { parts.append(data["private_prompts"].bool ? "keeps their prompts here private" : "shares their prompts here") }
            return sys(you, [.b(youName), .t(" \(parts.joined(separator: " and "))")])
        case "open":
            if data["protocol"].str == "review" { return sys(.pr, [.b("Wheelhouse"), .t(" opened this Review docket\(data["review"]["key"].str.map { " for \($0)" } ?? "")")]) }
            if data["protocol"].str == "agent" { return sys(.hub, [.b("Wheelhouse"), .t(" picked up \(agent)\(d.agent?.cwd.map { " in \(shortPath($0))" } ?? "")")]) }
            return e.actorType == "person" ? sys(you, [.b(youName), .t(" opened this docket")]) : sys(.hub, [.b("Wheelhouse"), .t(" opened this docket")])
        case "claim":
            let src = data["source"].str
            if src == "rex-reviewer" { return sys(.hub, [.b("rex-reviewer"), .t(" started its \(data["role"].str ?? "")\(data["round"].int.map { " for round \($0)" } ?? "")")]) }
            if src == "wheelhouse", let role = data["role"].str { return sys(.hub, [.b("Wheelhouse"), .t(" \(data["round"].int != nil ? "started" : "found") the \(role)\(data["round"].int.map { " for round \($0)" } ?? "")")]) }
            if src == "wheelhouse" { return sys(.hub, [.b("Wheelhouse"), .t(" started \(agent) in \(runtimeTitle(block: data["agent"]["block"].str))'s wheelhouse session")]) }
            let s = String((data["session"].str ?? "").prefix(8))
            return sys(.hub, [.b(agent), .t(src == "resume" ? " resumed session " : src == "clear" ? " cleared into session " : " started session "), .mono(s)])
        case "question":
            return .comment(writerAvatar, writer, "asked \(data["to"].str == "me" ? "you" : data["to"].str ?? "")\(data["subject"].str.map { " · \($0)" } ?? "") (#\(data["message"].int ?? 0))", "\(e.body ?? "")\n\nOptions: \(data["options"].strings.joined(separator: " · "))", "ask")
        case "proposal":
            // The Listener's, from a call's transcript (src/calls.rs).
            return .comment(.app("claude-code"), "Listener", "proposed #\(data["n"].int ?? 0)", Calls.proposedBody(e.body, quote: data["quote"].str), nil)
        case "decision" where data["proposal"].int != nil:
            let n = data["proposal"].int ?? 0
            var bits: [Bit] = [.b(youName), .t(" \(data["choice"].str == "Apply" ? "applied" : "ignored") #\(n)")]
            if let to = data["applied"]["docket"].text { bits += [.t(" to "), .link(to, Calls.link(to))] }
            if !(data["edited"].object?.isEmpty ?? true) { bits.append(.t(", edited")) }
            return sys(you, bits)
        case "decision" where data["via"].str == "composer":
            return .comment(you, youName, "decided\(data["subject"].str.map { " · \($0)" } ?? "")", "**\(data["choice"].str ?? "")**", "you")
        case "decision":
            let choice = data["choice"].str ?? ""
            return .comment(writerAvatar, writer, "chose “\(choice)” (#\(data["replies_to"].int ?? 0))", e.body.flatMap { $0 != choice ? $0 : nil } ?? "**\(choice)**", e.actorType == "person" ? "you" : nil)
        case "note":
            if data["message"].exists {
                let verb = data["replies_to"].int.map { "replied to #\($0)" } ?? (data["to"].str == "me" ? "wrote to you\(data["subject"].str.map { " · \($0)" } ?? "")" : "wrote to \(data["to"].str ?? "")")
                return .comment(writerAvatar, writer, verb, e.body ?? "", e.actorType == "person" ? "you" : nil)
            }
            if data["via"].str == "prompt" { return .comment(you, youName, "asked \(agent)", e.body ?? "", "you") }
            // A comment on a diff's lines (src/comments.rs): where, what it says, and the code it's on.
            if data["via"].str == "diff", data["diff"].object != nil {
                let ex = data["diff"]["excerpt"].str ?? ""
                let fence = fenceFor(ex)
                let code = ex.isEmpty ? "" : "\n\n\(fence)\n\(ex)\n\(fence)"
                return .comment(you, youName, "commented on \(commentPlace(data["diff"]))", "\(e.body ?? "")\(code)", mate == nil ? "you" : nil)
            }
            if data["via"].str == "message" { return .comment(.hub, "rex-inbox", "delivered a message", e.body ?? "", nil) }
            if e.actorType == "agent" { return .comment(app, agent, "noted", e.body ?? "", nil) }
            return .comment(you, youName, "noted", e.body ?? "", mate == nil ? "you" : nil)
        case "turn":
            guard let b = e.body, !b.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                let st = data["state"].str
                return sys(app, [.b(agent), .t(" finished a turn\(st != nil && st != "done" ? " (\(st!))" : "")")])
            }
            return .comment(app, agent, "finished a turn", b, "agent")
        case "intent":
            switch data["source"].str {
            case "linear": return sys(.issue, [.b("Linear"), .t(" set the intent from \(data["key"].str ?? "the issue")'s description\(data["taken_by"].exists ? " (taken by you)" : "")")], "intent")
            case "review": return sys(.hub, [.b("Wheelhouse"), .t(" wrote the intent from the PR\(data["linear"].strings.isEmpty ? "" : " and \(data["linear"].strings.joined(separator: ", "))")")], "intent")
            default: return sys(writerAvatar, [.b(writer), .t(e.actorType == "person" ? " updated the intent" : " proposed the intent")], "intent")
            }
        case "link":
            let key = data["key"].str ?? ""
            let url = data["url"].str.flatMap(URL.init(string:))
            if data["kind"].str == "linear" {
                let how = ["linkback": "Linear links it to the PR", "closes": "the PR closes it", "refs": "the PR is part of it", "branch": "named in the branch", "title": "named in the PR's title"][data["how"].str ?? ""]
                return sys(e.actorType == "person" ? you : .issue, [.b(e.actorType == "person" ? youName : "Wheelhouse"), .t(" linked "), .link(key, url), .t(data["title"].str.map { " · \($0)" } ?? ""), .t(how.map { ": \($0)" } ?? "")])
            }
            if data["kind"].str == "github-pr" { return sys(.pr, [.t("Following "), .link(key, url), .t(" on GitHub")]) }
            if data["kind"].str == "github-review" { return sys(.pr, [.t("Round \(data["round"].int ?? 0)'s review is "), .link("on GitHub", url)], "approved") }
            return sys(.hub, [.t("Linked \(e.body ?? "")")])
        case "observed":
            switch data["what"].str {
            case "review" where data["source"].str == "wheelhouse": return .none
            case "listen":
                return sys(.hub, [.t(e.body ?? "")], data["state"].str == "failed" ? "error" : nil)
            case "review-event":
                let ev = data["event"].str
                return sys(.pr, [.t(e.body ?? "")], ev == "posted" || ev == "merged" ? "approved" : ev == "dismissed" || ev == "post-refused" ? "denied" : nil)
            case "review-attention":
                let k = data["kind"].str
                let tone = k == "ready" || k == "blocked" ? "blocked" : "error"
                return sys(.live(tone == "blocked" ? .blocked : .error), [.t(e.body ?? "")], tone)
            case "review":
                let r = data["review"]
                let bits = [r["round"].int.map { "round \($0)" }, r["state"].str == "reviewing" ? r["phase"].str : r["state"].str, r["verdict"].str.map { $0.replacingOccurrences(of: "_", with: " ").lowercased() }, r["merge_risk"].str.map { "risk \($0)" }].compactMap { $0 }
                return sys(.hub, [.b("rex-reviewer"), .t(" · \(r["key"].str ?? ""): \(bits.joined(separator: " · "))")])
            case "linear-comment":
                return .comment(.issue, data["author"].str ?? "Someone", "commented in Linear", e.body ?? "", "linear")
            case "linear":
                let i = data["issue"]
                let changed = data["changed"].strings
                if changed.isEmpty { return sys(.issue, [.b("Linear"), .t(": \(i["key"].str ?? "") is \(i["state"].str ?? "open")\(i["assignee"].str.map { ", assigned to \($0)" } ?? "")")]) }
                let parts = changed.map { c -> String in
                    switch c {
                    case "state": "\(data["from"]["state"].str ?? "?") → \(i["state"].str ?? "")"
                    case "title": "title “\(i["title"].str ?? "")”"
                    case "assignee": "assigned to \(i["assignee"].str ?? "no one")"
                    case "priority": "priority \(i["priority"].str ?? "none")"
                    default: "description changed"
                    }
                }
                return sys(.issue, [.b("Linear"), .t(" · \(i["key"].str ?? ""): \(parts.joined(separator: " · "))")])
            case "error":
                return sys(.live(.error), [.b(agent), .t(" failed\(e.body.map { ": \($0)" } ?? "")")], "error")
            default:
                return sys(.live(.blocked), [.b(agent), .t(" waited on you\(data["kind"].str.map { " (\($0))" } ?? "")\(e.body.map { ": \($0)" } ?? "")")], "blocked")
            }
        case "set":
            var parts: [String] = []
            if let t = data["title"].str { parts.append(data["title_source"].str == "prompt" ? "named it from your prompt" : "renamed it to “\(t)”") }
            if let s = data["status"].str {
                parts.append(s == "done" && data["on"].str == "word" ? "closed it on your word: “\(data["reason"].str ?? e.body ?? "")”" : "moved it to \(Status(rawValue: s)?.label ?? s)")
            }
            if let o = data.object, o.keys.contains("hold") {
                parts.append(data["hold"].text.map { "put it on hold: “\($0)”" } ?? "resumed it")
            }
            let tone: String? = data["hold"].text != nil || data["on"].str == "word" ? "blocked" : nil
            return sys(e.actorType == "person" ? you : .hub, [.b(e.actorType == "person" ? youName : "Wheelhouse"), .t(" \(parts.joined(separator: " and "))")], tone)
        case "check":
            let ok = data["ok"].bool
            let how = ok ? "passed" : data["exit"].isNull ? "timed out" : "failed, exit \(data["exit"].int ?? 1)"
            return sys(.hub, [.b("Wheelhouse"), .t(" ran "), .mono(data["name"].str ?? "a check"), .t(data["required"].exists && !data["required"].bool ? " (optional)" : ""), .t(" · \(how) at "), .mono(String((data["head"].str ?? "").prefix(7)))], ok ? "approved" : data["required"].exists && !data["required"].bool ? nil : "error")
        case "verdict":
            let passed = data["verdict"].str == "pass"
            let n = data["findings"].int ?? data["findings"].array.count
            let ind = data["independent"].isNull ? "" : data["independent"].bool ? ", independent" : ", same session as the builder"
            return .comment(writerAvatar, writer, "\(data["role"].str ?? "checker")\(data["n"].int.map { ", check \($0)" } ?? ""): \(passed ? "passed" : "found problems")\(n > 0 ? " · \(plural(n, "finding"))" : "")\(ind)", e.body ?? "", nil)
        case "approval":
            if data["what"].str == "post-review" { return sys(you, [.b(youName), .t(" approved posting round \(data["round"].int ?? 0)'s review")], "approved") }
            if data["kind"].str == "review" {
                let posted = data["decision"].str == "post"
                return sys(you, [.b(youName), .t(posted ? " posted the review" : " dismissed the review"), .t(e.body.map { ": \($0)" } ?? ""), data["url"].str.map { .link(" · on GitHub", URL(string: $0)) } ?? .t("")], posted ? "approved" : "denied")
            }
            let ok = data["decision"].str == "approve"
            return sys(you, [.b(youName), .t(" \(ok ? "approved" : "denied") \(agent)'s \(data["kind"].str ?? "prompt")"), e.body.map { .mono(": \($0)") } ?? .t("")], ok ? "approved" : "denied")
        case "artifact":
            // Shown already, with what was said, by the entry that carried it.
            if DocketImage.hiddenInActivity(e, carried: carriedImages) { return .none }
            return .file(writerAvatar, writer, e.body ?? data["name"].str ?? "a file", data["group"].str)
        case "release":
            return sys(.hub, [.b("Wheelhouse"), .t(" saw the session end: \(data["reason"].str ?? "released")")])
        default:
            return sys(.hub, [.t("\(e.kind)\(e.body.map { ": \($0)" } ?? "")")])
        }
    }
}

/// How an entry is known: witnessed, checkable, or said.
struct Mark: View {
    let mark: String
    static let words = [
        "witnessed": "Wheelhouse saw it happen and wrote it down.",
        "checkable": "Read from a source anyone can read again (GitHub, Linear).",
        "said": "A person or an agent said it: true if you trust them.",
    ]
    var body: some View {
        Text(mark)
            .font(.system(size: 10.5, weight: .medium))
            .foregroundStyle(mark == "witnessed" ? K.working : mark == "checkable" ? K.accent : K.faint)
            .help(Self.words[mark] ?? "")
    }
}

/// Long text folds, with Show more.
struct Clamp<Content: View>: View {
    var limit: CGFloat = 280
    @ViewBuilder var content: Content
    @State private var full: CGFloat = 0
    @State private var open = false

    var body: some View {
        let long = full > limit + 40
        VStack(alignment: .leading, spacing: 6) {
            content
                .fixedSize(horizontal: false, vertical: true)
                .background(GeometryReader { g in Color.clear.onAppear { full = g.size.height }.onChange(of: g.size.height) { _, h in full = h } })
                .frame(maxHeight: long && !open ? limit : nil, alignment: .top)
                .clipped()
                .mask {
                    if long && !open {
                        LinearGradient(stops: [.init(color: .black, location: 0.8), .init(color: .clear, location: 1)], startPoint: .top, endPoint: .bottom)
                    } else {
                        Rectangle()
                    }
                }
            if long {
                Button(open ? "Show less" : "Show more") { withAnimation(.easeOut(duration: 0.2)) { open.toggle() } }
                    .buttonStyle(.link).font(.system(size: 12))
            }
        }
    }
}
