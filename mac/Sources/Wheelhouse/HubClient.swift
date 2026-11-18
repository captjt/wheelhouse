import Foundation

/// The hub's HTTP API (src/server.rs), on this Mac by default.
struct HubError: LocalizedError {
    let message: String
    /// The HTTP status it came with (409: refused as things stand).
    var status = 0
    var errorDescription: String? { message }
}

final class HubClient: @unchecked Sendable {
    /// :7500, or the dev hub's for Wheelhouse Dev (Flavor).
    static let defaultAddress = Flavor.defaultHub

    /// Where the hub answers: Settings › Hub, or `WH_APP_HUB` (for a second hub).
    var base: URL {
        let s = ProcessInfo.processInfo.environment["WH_APP_HUB"] ?? UserDefaults.standard.string(forKey: "hubAddress") ?? Self.defaultAddress
        return URL(string: s.trimmingCharacters(in: CharacterSet(charactersIn: "/ "))) ?? URL(string: Self.defaultAddress)!
    }

    private let session: URLSession = {
        let c = URLSessionConfiguration.ephemeral
        c.timeoutIntervalForRequest = 20
        c.httpAdditionalHeaders = ["Accept": "application/json"]
        return URLSession(configuration: c)
    }()

    /// The event stream waits on the hub's keep-alives; it gets its own session.
    private let streaming: URLSession = {
        let c = URLSessionConfiguration.ephemeral
        c.timeoutIntervalForRequest = 45
        c.timeoutIntervalForResource = .infinity
        return URLSession(configuration: c)
    }()

    func url(_ path: String) -> URL {
        URL(string: base.absoluteString + path) ?? base
    }

    func data(_ path: String) async throws -> Data {
        let (data, res) = try await session.data(from: url(path))
        try check(res, data)
        return data
    }

    func get(_ path: String) async throws -> JSON {
        try JSON.parse(try await data(path))
    }

    @discardableResult
    func post(_ path: String, _ body: [String: Any] = [:]) async throws -> JSON {
        var req = URLRequest(url: url(path))
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        // Writes must carry this header; it's how the hub refuses other sites.
        req.setValue("mac", forHTTPHeaderField: "X-Wheelhouse")
        req.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (data, res) = try await session.data(for: req)
        try check(res, data)
        return (try? JSON.parse(data)) ?? .null
    }

    private func check(_ res: URLResponse, _ data: Data) throws {
        guard let h = res as? HTTPURLResponse, !(200 ..< 300).contains(h.statusCode) else { return }
        let msg = (try? JSON.parse(data))?["error"].text ?? String(decoding: data, as: UTF8.self)
        throw HubError(message: msg.isEmpty ? "The hub answered \(h.statusCode)" : msg, status: h.statusCode)
    }

    /// What changes, as it happens: the hub's server-sent events, one JSON
    /// value each. `notifies`: the app posts the Mac's notifications (macOS
    /// lets it, and they're on), so the hub posts none of its own.
    func events(notifies: Bool) -> AsyncThrowingStream<JSON, Error> {
        AsyncThrowingStream { cont in
            let task = Task {
                do {
                    var req = URLRequest(url: url(notifies ? "/api/events?client=mac" : "/api/events"))
                    req.setValue("text/event-stream", forHTTPHeaderField: "Accept")
                    let (bytes, res) = try await streaming.bytes(for: req)
                    if let h = res as? HTTPURLResponse, h.statusCode != 200 {
                        throw HubError(message: "The hub answered \(h.statusCode)")
                    }
                    cont.yield(.object(["type": .string("open")]))
                    for try await line in bytes.lines {
                        guard line.hasPrefix("data:") else { continue }
                        let payload = line.dropFirst(5).trimmingCharacters(in: .whitespaces)
                        cont.yield((try? JSON.parse(Data(payload.utf8))) ?? .null)
                    }
                    cont.finish()
                } catch {
                    cont.finish(throwing: error)
                }
            }
            cont.onTermination = { _ in task.cancel() }
        }
    }
}
