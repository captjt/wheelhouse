import Foundation

extension Notification.Name {
    /// An agent the composer's `/task` asked for started (the docket's id).
    static let taskStarted = Notification.Name("WheelhouseTaskStarted")
}

/// The composer on a docket's Activity (src/compose.rs reads what's sent):
/// a note, `@agent` to instruct one, `/ask`, `/decide`, `/task`. Here is
/// only what the box shows while you type, as web/src/compose.ts does.
enum ComposeKind: Equatable {
    case note, instruct, ask, decide, task
}

struct ComposeSuggestion: Equatable {
    let insert: String
    let label: String
    let sub: String
}

enum Compose {
    static let commands: [(word: String, kind: ComposeKind, what: String)] = [
        ("ask", .ask, "Ask the agents here: Which store? | Postgres | SQLite"),
        ("decide", .decide, "Record a decision (Subject: answer), or answer a question (#12 …)"),
        ("task", .task, "Start another agent here with this prompt"),
        ("note", .note, "A note that starts with / or @"),
    ]

    /// The first word after `/` or `@`, and which it was; nil for plain text.
    private static func head(_ text: String) -> (sigil: Character, word: String)? {
        let t = text.drop(while: { $0.isWhitespace })
        guard let c = t.first, c == "/" || c == "@" else { return nil }
        return (c, String(t.dropFirst().prefix(while: { !$0.isWhitespace })))
    }

    private static func isName(_ w: String) -> Bool {
        !w.isEmpty && w.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || "-_.".contains($0)) }
    }

    /// Which it'll be, by its first word, as the hub reads it. A path
    /// (`/tmp/x`) or a lone `@` is a note.
    static func kind(_ text: String) -> ComposeKind {
        guard let (sigil, word) = head(text) else { return .note }
        if sigil == "@" { return isName(word) ? .instruct : .note }
        guard !word.isEmpty, word.allSatisfy({ $0.isASCII && $0.isLetter }) else { return .note }
        return commands.first { $0.word == word.lowercased() }?.kind ?? .note
    }

    static func button(_ k: ComposeKind) -> String {
        switch k {
        case .instruct: "Send"
        case .ask: "Ask"
        case .decide: "Decide"
        case .task: "Add an Agent…"
        case .note: "Note"
        }
    }

    static func hint(_ k: ComposeKind, _ id: String) -> String {
        switch k {
        case .instruct: "A prompt to that agent, queued while it works"
        case .ask: "Asks the agents on \(id); their answer stands under Decisions"
        case .decide: "Stands under Decisions, and the agents on \(id) are told"
        case .task: "Opens Add an Agent with this prompt"
        case .note: "Signed as you · @agent, /ask, /decide, /task"
        }
    }

    /// What an agent answers to after `@`: its pane's name, else its role,
    /// else its app (src/compose.rs `names`).
    static func atName(pane: String, role: String?, app: String) -> String {
        let form = { (s: String) in s.split(whereSeparator: \.isWhitespace).joined(separator: "-").lowercased() }
        if !pane.trimmingCharacters(in: .whitespaces).isEmpty { return form(pane) }
        if let r = role?.trimmingCharacters(in: .whitespaces), !r.isEmpty { return form(r) }
        return app == "claude-code" ? "claude" : app.lowercased()
    }

    /// Something the hub would refuse, said before you send it; `blocked`
    /// when sending can't work.
    static func warning(_ text: String, agents: [(pane: String, role: String?, app: String)]) -> (text: String, blocked: Bool)? {
        let t = text.drop(while: { $0.isWhitespace })
        guard let (sigil, word) = head(text), t.dropFirst().contains(where: { $0.isWhitespace }) else { return nil }
        let w = word.lowercased()
        if sigil == "/", !w.isEmpty, w.allSatisfy({ $0.isASCII && $0.isLetter }), !commands.contains(where: { $0.word == w }) {
            return ("There's no /\(w): try /ask, /decide, /task, or /note to keep it as a note", true)
        }
        if sigil == "@", kind(text) == .instruct, !["all", "agents", "everyone"].contains(w) {
            if agents.isEmpty { return ("No agent is on this docket now: /note keeps it as a note", true) }
            let known = agents.contains { a in
                atName(pane: a.pane, role: a.role, app: a.app) == w
                    || (a.role ?? "").lowercased().split(separator: " ").contains { String($0) == w }
                    || atName(pane: "", role: nil, app: a.app) == w
                    || (a.app == "claude-code" && w == "claude-code")
            }
            if !known { return ("No agent here is @\(w): /note keeps it as a note", false) }
        }
        return nil
    }

    /// While only the first word is typed: the commands `/` could be, or the
    /// agents `@` could name, each `(pane, role, app)`.
    static func suggestions(_ text: String, agents: [(pane: String, role: String?, app: String)], docket: String) -> [ComposeSuggestion] {
        let t = text.drop(while: { $0.isWhitespace })
        guard let (sigil, word) = head(text), !t.dropFirst().contains(where: { $0.isWhitespace }) else { return [] }
        let typed = word.lowercased()
        if sigil == "/" {
            return commands.filter { $0.word.hasPrefix(typed) }.map { ComposeSuggestion(insert: "/\($0.word) ", label: "/\($0.word)", sub: $0.what) }
        }
        var out: [ComposeSuggestion] = agents.map { a in
            let n = atName(pane: a.pane, role: a.role, app: a.app)
            return ComposeSuggestion(insert: "@\(n) ", label: "@\(n)", sub: [appName(a.app), a.role ?? ""].filter { !$0.isEmpty }.joined(separator: " · "))
        }
        if agents.count > 1 { out.append(ComposeSuggestion(insert: "@all ", label: "@all", sub: "Every agent on \(docket)")) }
        var seen = Set<String>()
        return out.filter { $0.label.dropFirst().hasPrefix(typed) && seen.insert($0.label).inserted }
    }

    /// What the hub did, for a toast: none for a note (it's in the Activity)
    /// or a task (its sheet opens).
    static func toast(_ r: JSON) -> (text: String, error: Bool)? {
        switch r["did"].str {
        case "instructed":
            let sent = r["sent"].array
            let ok = sent.filter { $0["ok"].bool }
            let failed = sent.filter { !$0["ok"].bool }
            var parts = ok.map { "\($0["queued"].bool ? "queued for" : "sent to") \($0["to"].str ?? "")" }
            if !failed.isEmpty { parts.append("not " + failed.map { "\($0["to"].str ?? "") (\($0["error"].str ?? ""))" }.joined(separator: ", ")) }
            let text = parts.joined(separator: "; ")
            return (text.prefix(1).uppercased() + text.dropFirst(), !failed.isEmpty)
        case "asked":
            let opts = r["options"].strings
            return ("Asked the agents (#\(r["message"].int ?? 0))\(opts.isEmpty ? "" : ": " + opts.joined(separator: " / "))", false)
        case "answered":
            return ("Answered \(r["key"].str ?? "")\(r["choice"].str.map { ": \($0)" } ?? "")", false)
        case "decided":
            let told = r["told"].strings
            return (told.isEmpty ? "Decided. No agent is on it now; the next one reads it in its brief" : "Decided, and told \(told.count == 1 ? told[0] : "\(told.count) agents")", false)
        default:
            return nil
        }
    }
}
