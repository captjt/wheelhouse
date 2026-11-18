import Foundation
import Observation

// Settings (docs/settings.md): what Wheelhouse does on its own, and what you
// can change of it. Values go in settings.json, laid over build.lua and
// review.lua; `null` takes a value set here back out.

/// Building and reading Settings' patches: no window needed.
enum SettingsPatch {
    /// `{part: {a: {b: value}}}`. Nil sends null: the value set here goes,
    /// and the Lua's (or the default) applies again.
    static func make(_ part: String, _ path: [String], _ value: Any?) -> [String: Any] {
        var v: Any = value ?? NSNull()
        for k in path.reversed() { v = [k: v] }
        return [part: v]
    }

    /// Two patches as one, merged key by key.
    static func merge(_ a: [String: Any], _ b: [String: Any]) -> [String: Any] {
        var out = a
        for (k, v) in b {
            if let x = out[k] as? [String: Any], let y = v as? [String: Any] { out[k] = merge(x, y) } else { out[k] = v }
        }
        return out
    }

    /// Whether settings.json (`set`) has a value there: it's set here, not
    /// read from the Lua or the default.
    static func isSet(_ set: JSON, _ part: String, _ path: [String]) -> Bool {
        var j = set[part]
        for k in path { j = j[k] }
        return j.exists
    }

    /// A check as settings.json keeps it: all three, so nothing of the
    /// Lua's shows through.
    static func check(run: String, required: Bool, timeout: Int) -> [String: Any] {
        ["run": run.trimmingCharacters(in: .whitespacesAndNewlines), "required": required, "timeout": min(max(timeout, 10), 7200)]
    }

    /// A posting rule (`{auto: {…}}`) from its form, or what's wrong with
    /// it. Lists are typed comma-separated. docs/review.md#who-posts.
    static func rule(approve: Bool, risk: String, maxLines: String, authors: String, labels: String, notTouching: String) -> (value: [String: Any]?, error: String?) {
        var auto: [String: Any] = [:]
        if approve { auto["verdict"] = "approve" }
        if risk == "low" || risk == "medium" { auto["merge_risk"] = risk }
        let n = maxLines.trimmingCharacters(in: .whitespaces)
        if !n.isEmpty {
            guard let lines = Int(n), lines >= 1, n.allSatisfy(\.isNumber) else { return (nil, "Lines: a whole number, 1 or more.") }
            auto["max_lines"] = lines
        }
        let items = { (s: String) in s.split(whereSeparator: { $0 == "," || $0 == "\n" }).map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty } }
        for (k, v) in [("authors", authors), ("labels", labels), ("not_touching", notTouching)] where !items(v).isEmpty {
            auto[k] = items(v)
        }
        if auto.isEmpty { return (nil, "Pick at least one condition. For a review that always posts itself, choose Auto.") }
        return (["auto": auto], nil)
    }

    /// A check's name: letters, digits, dashes and underscores.
    static func validCheckName(_ s: String) -> Bool {
        !s.isEmpty && s.count <= 40 && s.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-" || $0 == "_") }
    }

    /// A guide's file from what you call it: "API review" → "api-review.md".
    /// Nil when nothing usable is left.
    static func guideFile(_ title: String) -> String? {
        var out = ""
        var dash = false
        for c in title.lowercased().unicodeScalars {
            if ("a" ... "z").contains(c) || ("0" ... "9").contains(c) {
                out.unicodeScalars.append(c)
                dash = false
            } else if !out.isEmpty, !dash {
                out += "-"
                dash = true
            }
        }
        while out.hasSuffix("-") { out.removeLast() }
        guard !out.isEmpty else { return nil }
        return String(out.prefix(60)).trimmingCharacters(in: CharacterSet(charactersIn: "-")) + ".md"
    }

    /// The `{{VARIABLES}}` a brief uses, each once, in order.
    static func variables(_ text: String) -> [String] {
        var out: [String] = []
        var rest = Substring(text)
        while let open = rest.range(of: "{{") {
            let after = rest[open.upperBound...]
            guard let close = after.range(of: "}}") else { break }
            let name = after[..<close.lowerBound]
            if !name.isEmpty, name.allSatisfy({ $0.isUppercase || $0.isNumber || $0 == "_" }) {
                let v = "{{\(name)}}"
                if !out.contains(v) { out.append(v) }
            }
            rest = after[close.upperBound...]
        }
        return out
    }

    /// The variables the built-in uses that `text` leaves out: what the
    /// agents would no longer be told.
    static func dropped(from original: String, in text: String) -> [String] {
        let kept = Set(variables(text))
        return variables(original).filter { !kept.contains($0) }
    }

    /// Where a guide is used, in words.
    static func usedBy(_ s: String) -> String {
        let parts = s.split(separator: ":", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
        guard parts.count == 2 else { return s }
        let proto = parts[0] == "build" ? "Build" : parts[0] == "review" ? "Review" : parts[0]
        let key = parts[1]
        if key.hasPrefix("repos.") {
            // repos.<slug>.guides.<slot>: the slug may have dots of its own.
            let body = key.dropFirst("repos.".count)
            if let r = body.range(of: ".guides.", options: .backwards) {
                return "\(body[..<r.lowerBound])'s \(String(body[r.upperBound...]).replacingOccurrences(of: "_", with: " ")) guide"
            }
        }
        if key.hasPrefix("guides.") {
            return "\(proto)'s \(key.dropFirst("guides.".count).replacingOccurrences(of: "_", with: " "))"
        }
        return "\(proto): \(key)"
    }
}

/// One guide or brief, as Settings lists it.
struct SettingsDoc: Hashable, Identifiable {
    let j: JSON
    var id: String { j["id"].str ?? "" }
    /// guide or brief.
    var kind: String { j["kind"].str ?? "guide" }
    /// review, build, or "" for a guide.
    var proto: String { j["protocol"].str ?? "" }
    var title: String { j["title"].str ?? id }
    var about: String? { j["about"].text }
    var builtin: Bool { j["builtin"].bool }
    var overridden: Bool { j["overridden"].bool }
    var path: String { j["path"].str ?? "" }
    var usedBy: [String] { j["used_by"].strings }
    var yours: Bool { !builtin }

    /// Built in, edited, or yours.
    var origin: String { !builtin ? "Yours" : overridden ? "Edited" : "Built in" }
}

/// Settings' state: the hub's view of it, and what each change said.
@Observable @MainActor
final class SettingsModel {
    private(set) var view: JSON?
    private(set) var loadError: String?
    /// A change's error, by what caused it: shown beside that control.
    var errors: [String: String] = [:]
    /// What a change did that's worth a word, by key.
    var notes: [String: String] = [:]
    private(set) var busy: Set<String> = []
    private let hub: HubClient

    init(hub: HubClient? = nil) {
        self.hub = hub ?? Store.shared.hub
    }

    var set: JSON { view?["set"] ?? .null }
    var build: JSON { view?["build"]["config"] ?? .null }
    var review: JSON { view?["review"]["config"] ?? .null }
    var orders: [JSON] { view?["orders"].array ?? [] }
    var docs: [SettingsDoc] { (view?["docs"].array ?? []).map(SettingsDoc.init) }
    func doc(_ id: String?) -> SettingsDoc? { docs.first { $0.id == id } }

    /// build.lua or review.lua, by name.
    func lua(_ part: String) -> String {
        (view?[part]["lua"].str).map { URL(fileURLWithPath: $0).lastPathComponent } ?? "\(part).lua"
    }

    func luaExists(_ part: String) -> Bool { view?[part]["lua_exists"].bool ?? false }

    /// Going back: to the Lua's value, or the default without one.
    func revertLabel(_ part: String) -> String { luaExists(part) ? "Use \(lua(part))'s" : "Use the default" }

    func isSet(_ part: String, _ path: [String]) -> Bool { SettingsPatch.isSet(set, part, path) }

    func load() async {
        do {
            view = try await hub.get("/api/settings")
            loadError = nil
        } catch {
            loadError = Store.words(error)
        }
    }

    /// Change settings; on refusal, keep the hub's sentence by `key`.
    @discardableResult
    func change(_ key: String, _ patch: [String: Any]) async -> Bool {
        busy.insert(key)
        defer { busy.remove(key) }
        do {
            let r = try await hub.post("/api/settings", patch)
            if r["orders"].exists { view = r } else { await load() }
            errors[key] = nil
            Store.shared.reload()
            return true
        } catch {
            errors[key] = Store.words(error)
            return false
        }
    }

    /// Review's and Build's own switches (posting, pausing), as before.
    func toggle(_ key: String, _ path: String, _ body: [String: Any]) async {
        busy.insert(key)
        defer { busy.remove(key) }
        do {
            try await hub.post(path, body)
            errors[key] = nil
        } catch {
            errors[key] = Store.words(error)
        }
        Store.shared.reload()
        await load()
    }

    func isBusy(_ key: String) -> Bool { busy.contains(key) }

    // ---- guides and briefs ----

    func text(of id: String) async throws -> JSON {
        try await hub.get("/api/settings/doc?id=\(id.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? id)")
    }

    func save(_ id: String, _ text: String) async throws -> JSON {
        let r = try await hub.post("/api/settings/doc", ["id": id, "text": text])
        await load()
        return r
    }

    func reset(_ id: String) async throws -> JSON {
        let r = try await hub.post("/api/settings/doc/reset", ["id": id])
        await load()
        return r
    }
}
