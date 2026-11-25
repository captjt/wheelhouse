import AppKit
import SwiftUI
import WebKit

/// A docket's code changes, counted (`GET /api/dockets/{id}/diff?stat=1`).
struct ChangeCount: Equatable {
    var added: Int
    var removed: Int
    var files: Int
    /// What it compares: "Base → Uncommitted".
    var range: String

    init(_ j: JSON) {
        added = j["added"].int ?? 0
        removed = j["removed"].int ?? 0
        files = j["files"].array.count
        range = "\(j["from"]["label"].str ?? "Base") → \(j["to"]["label"].str ?? "HEAD")"
    }
}

extension Store {
    /// The docket's changes, counted; nil when its code isn't on this Mac
    /// (a teammate's docket, or a checkout that's gone with nothing kept).
    func changeCount(_ id: String) async -> ChangeCount? {
        guard let j = try? await hub.get("/api/dockets/\(Self.pathPart(id))/diff?stat=1"), j["error"].str == nil else { return nil }
        return ChangeCount(j)
    }

    /// A docket id as one path segment.
    static func pathPart(_ s: String) -> String {
        s.addingPercentEncoding(withAllowedCharacters: .alphanumerics.union(CharacterSet(charactersIn: "-_."))) ?? s
    }

    /// The hub's Changes page for a docket, with no app around it
    /// (web/src/main.tsx's `embed` route). Its theme follows this app's.
    func changesURL(_ id: String) -> URL? {
        URL(string: hub.base.absoluteString + "/#/embed/changes/" + Self.pathPart(id))
    }
}

/// The chip on a docket: +12 −3 · 4 files, which opens its Changes window.
struct ChangesChip: View {
    let d: Docket
    @Environment(Store.self) private var store
    @Environment(\.openWindow) private var openWindow
    @State private var count: ChangeCount?

    var body: some View {
        Group {
            if let c = count {
                Button { openWindow(id: "changes", value: d.id) } label: {
                    HStack(spacing: 5) {
                        Image(systemName: "plus.forwardslash.minus")
                        if c.files == 0 {
                            Text("No changes")
                        } else {
                            Text("+\(c.added)").foregroundStyle(K.working)
                            Text("−\(c.removed)").foregroundStyle(K.error)
                            Text("· \(c.files) file\(c.files == 1 ? "" : "s")")
                        }
                    }
                    .monospacedDigit()
                }
                .buttonStyle(ChipButton())
                .help("\(c.range): see the changes (⇧⌘D)")
            } else {
                // Something for the task to run on: an empty Group has nothing.
                Color.clear.frame(width: 0, height: 0)
            }
        }
        // Read again when the record moves: a commit, a round, a check.
        .task(id: "\(d.id) \(d.updatedAt)") {
            guard d.sharedFrom == nil else { return }
            let c = await store.changeCount(d.id)
            if c != count { count = c }
        }
    }
}

/// A docket's Changes, in a window of its own: the hub's page in a web view,
/// so it's the same diff (Pierre's, highlighted, split or unified) as on the
/// web and the phone.
struct ChangesWindow: View {
    let id: String
    @Environment(Store.self) private var store

    var body: some View {
        Group {
            if let url = store.changesURL(id) {
                ChangesWeb(url: url)
            } else {
                Text("The hub's address in Settings isn't a URL.").foregroundStyle(K.muted).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(minWidth: 720, minHeight: 480)
        .background(K.main)
        .navigationTitle(store.docket(id).map { "\($0.id) · Changes · \($0.title)" } ?? "\(id) · Changes")
    }
}

/// The web view: it shows the hub's own pages and nothing else. A link off
/// the hub opens in your browser; nothing is kept between windows.
struct ChangesWeb: NSViewRepresentable {
    let url: URL

    func makeCoordinator() -> Coordinator { Coordinator(home: url) }

    func makeNSView(context: Context) -> WKWebView {
        let c = WKWebViewConfiguration()
        c.websiteDataStore = .nonPersistent()
        c.preferences.javaScriptCanOpenWindowsAutomatically = false
        let w = WKWebView(frame: .zero, configuration: c)
        w.navigationDelegate = context.coordinator
        w.uiDelegate = context.coordinator
        w.allowsBackForwardNavigationGestures = false
        // The app's own background behind the page, so dark mode doesn't
        // flash white before it paints.
        w.underPageBackgroundColor = NSColor(K.main)
        w.load(URLRequest(url: url))
        return w
    }

    func updateNSView(_ w: WKWebView, context: Context) {
        if context.coordinator.home != url {
            context.coordinator.home = url
            w.load(URLRequest(url: url))
        }
    }

    final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate {
        var home: URL
        init(home: URL) { self.home = home }

        /// The hub's own origin: scheme, host and port all the same.
        func ours(_ u: URL?) -> Bool {
            guard let u else { return false }
            return u.scheme == home.scheme && u.host == home.host && u.port == home.port
        }

        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void) {
            let u = action.request.url
            if ours(u) || u?.scheme == "about" {
                decisionHandler(.allow)
                return
            }
            if let u, ["http", "https"].contains(u.scheme ?? ""), action.navigationType == .linkActivated {
                NSWorkspace.shared.open(u)
            }
            decisionHandler(.cancel)
        }

        /// A link that asks for a new window (target=_blank): your browser.
        func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for action: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
            if let u = action.request.url, ["http", "https"].contains(u.scheme ?? "") { NSWorkspace.shared.open(u) }
            return nil
        }
    }
}
