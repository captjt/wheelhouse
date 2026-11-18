import Foundation

/// A JSON value from the hub, read leniently: a missing or mistyped field
/// reads as nil (or empty), never as a failure that loses the whole answer.
enum JSON: Hashable, @unchecked Sendable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSON])
    case object([String: JSON])

    init(any: Any?) {
        switch any {
        case nil, is NSNull: self = .null
        case let n as NSNumber:
            self = CFGetTypeID(n) == CFBooleanGetTypeID() ? .bool(n.boolValue) : .number(n.doubleValue)
        case let s as String: self = .string(s)
        case let a as [Any]: self = .array(a.map { JSON(any: $0) })
        case let o as [String: Any]: self = .object(o.mapValues { JSON(any: $0) })
        default: self = .null
        }
    }

    static func parse(_ data: Data) throws -> JSON {
        JSON(any: try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]))
    }

    subscript(key: String) -> JSON {
        if case let .object(o) = self { return o[key] ?? .null }
        return .null
    }

    subscript(i: Int) -> JSON {
        if case let .array(a) = self, a.indices.contains(i) { return a[i] }
        return .null
    }

    var str: String? {
        switch self {
        case let .string(s): s
        case let .number(n): n == n.rounded() ? String(Int(n)) : String(n)
        default: nil
        }
    }

    /// A string that's there and not blank.
    var text: String? { str.flatMap { $0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : $0 } }

    var int: Int? {
        switch self {
        case let .number(n): Int(n)
        case let .string(s): Int(s)
        default: nil
        }
    }

    var double: Double? {
        if case let .number(n) = self { return n }
        return nil
    }

    var bool: Bool {
        if case let .bool(b) = self { return b }
        return false
    }

    var array: [JSON] {
        if case let .array(a) = self { return a }
        return []
    }

    var object: [String: JSON]? {
        if case let .object(o) = self { return o }
        return nil
    }

    var isNull: Bool { self == .null }
    var exists: Bool { !isNull }
    var strings: [String] { array.compactMap(\.str) }

    /// Back to Foundation's types, to serialize.
    var any: Any {
        switch self {
        case .null: NSNull()
        case let .bool(b): b
        case let .number(n): n == n.rounded() && abs(n) < 1e15 ? Int(n) as Any : n
        case let .string(s): s
        case let .array(a): a.map(\.any)
        case let .object(o): o.mapValues(\.any)
        }
    }

    /// Indented, keys sorted: for reading.
    var pretty: String {
        guard JSONSerialization.isValidJSONObject(any) else { return str ?? "" }
        let data = (try? JSONSerialization.data(withJSONObject: any, options: [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes])) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }
}
