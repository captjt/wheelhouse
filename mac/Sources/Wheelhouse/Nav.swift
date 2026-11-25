import AppKit
import Observation
import SwiftUI

/// The sidebar's places.
enum Place: Hashable {
    case inbox, reviews, builds, active, all, done, agents, recap, team
    case session(String)

    var title: String {
        switch self {
        case .inbox: "Inbox"
        case .reviews: "Reviews"
        case .builds: "Builds"
        case .active: "Active"
        case .all: "All dockets"
        case .done: "Done"
        case .agents: "Agents"
        case .recap: "Recap"
        case .team: "Team"
        case let .session(s): s
        }
    }

    var symbol: String {
        switch self {
        case .inbox: "tray"
        case .reviews: "arrow.triangle.pull"
        case .builds: "hammer"
        case .active: "circle.dashed"
        case .all: "square.stack"
        case .done: "checkmark.circle"
        case .agents: "terminal"
        case .recap: "chart.bar.xaxis"
        case .team: "person.2"
        case .session: "rectangle.split.3x1"
        }
    }

    /// Its key in the Go menu (⌘1…⌘8) and after G (as in the web app).
    static let order: [(Place, String)] = [(.inbox, "i"), (.reviews, "r"), (.builds, "b"), (.active, "a"), (.all, "d"), (.done, "x"), (.agents, "n"), (.recap, "p")]

    /// Which dockets a list shows.
    func shows(_ d: Docket) -> Bool {
        switch self {
        case .active: d.isOpen
        case .all: true
        case .done: !d.isOpen
        case let .session(s): d.isOpen && d.agent?.rexSession == s
        default: false
        }
    }
}

/// How a list groups its dockets.
enum GroupBy: String, CaseIterable {
    case attention, status, session
    var label: String {
        switch self {
        case .attention: "Attention"
        case .status: "Status"
        case .session: "Session"
        }
    }
}

/// What a new docket starts as.
enum NewMode: String, Identifiable {
    case blank, linear, agent
    var id: String { rawValue }
}

/// One window's place: its section, what's selected in each list, and the
/// sheets it has open.
@Observable @MainActor
final class Nav {
    static let main = Nav()

    var section: Place = .inbox
    var selection: [Place: String] = [:]
    var palette = false
    var creating: NewMode?
    var pane: String?
    /// Which of the docket's agents the pane sheet shows; nil, its lead.
    var paneBlock: String?
    var record: String?
    var linking: String?
    /// Holding a docket: asking why.
    var holding: String?
    /// Starting an agent on a docket: asking where, and with what.
    var starting: String?
    /// What `/task` in the composer said the new agent should do.
    var startingPrompt: String?
    /// Done was refused for want of evidence: its checks, or your word.
    var closing: Closing?
    var inspector = true
    var shortcuts = false
    /// The docket the window is about now: the one selected and shown.
    var current: String?

    /// Peek at an agent's pane: that block's, or the docket's lead's.
    func peek(_ id: String, block: String? = nil) {
        paneBlock = block
        pane = id
    }

    func selected(_ s: Place) -> String? { selection[s] }

    func select(_ id: String?, in s: Place) {
        selection[s] = id
    }

    /// Show a docket in the main window: where it is in the list you're on,
    /// or in the Inbox if it needs you, or in All dockets.
    func show(_ id: String, store: Store) {
        if section == .inbox, let item = store.inbox.first(where: { $0.docketId == id }) {
            select(item.id, in: .inbox)
            return
        }
        if let d = store.docket(id), section.shows(d) {
            select(id, in: section)
            return
        }
        if let item = store.inbox.first(where: { $0.docketId == id }) {
            section = .inbox
            select(item.id, in: .inbox)
            return
        }
        section = .all
        select(id, in: .all)
    }

    func go(_ s: Place) {
        section = s
    }
}

/// Typing in a text box: single-letter keys (G, C, J, K…) are text then.
@MainActor
func typing() -> Bool {
    NSApp.keyWindow?.firstResponder is NSText
}

/// The docket a window shows, and that window's place: for the Docket menu,
/// which acts on the window you're in.
struct DocketFocus {
    let id: String
    let nav: Nav
}

struct DocketFocusKey: FocusedValueKey {
    typealias Value = DocketFocus
}

extension FocusedValues {
    var docket: DocketFocus? {
        get { self[DocketFocusKey.self] }
        set { self[DocketFocusKey.self] = newValue }
    }
}

/// Done, refused: the docket's intent has acceptance items, and nothing
/// witnessed or checkable says they're met.
struct Closing: Identifiable {
    let id: String
    /// The hub's words.
    let why: String
    /// Where it was, to stay there if you cancel.
    let from: Status
}

/// Which window shows what the Store has to ask: each window's Nav, by its
/// window, so a refusal appears where you acted.
extension Windows {
    private final class Weak {
        weak var nav: Nav?
        init(_ n: Nav) { nav = n }
    }

    private static var navs: [Int: Weak] = [:]

    static func register(_ window: NSWindow?, _ nav: Nav) {
        guard let window else { return }
        navs = navs.filter { $0.value.nav != nil }
        navs[window.windowNumber] = Weak(nav)
    }

    /// The key window's Nav, or the main window's (brought forward).
    static func presenting() -> Nav {
        if let w = NSApp.keyWindow, let n = navs[w.windowNumber]?.nav { return n }
        if Snapshot.requested == nil { showMain() }
        return Nav.main
    }

    /// Settings, from outside a view.
    static var openSettings: (() -> Void)?
}

/// Hands a view its window, once it has one.
struct WindowReader: NSViewRepresentable {
    let found: (NSWindow?) -> Void

    func makeNSView(context: Context) -> NSView {
        let v = Probe()
        v.found = found
        return v
    }

    func updateNSView(_ v: NSView, context: Context) {}

    private final class Probe: NSView {
        var found: ((NSWindow?) -> Void)?
        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            let w = window
            DispatchQueue.main.async { [found] in found?(w) }
        }
    }
}

/// When G was last pressed: the key after it goes somewhere (G then D is All
/// dockets). A focused list sees keys before the window does, so its own
/// keys check this, or G D in the Inbox would deny the prompt under the cursor.
@MainActor
enum Chord {
    static var gAt = Date.distantPast
    static var afterG: Bool { Date.now.timeIntervalSince(gAt) < 0.9 }
}
