import CryptoKit
import Quartz
import SwiftUI

/// What a docket's work made, kept with it: each file pinned in the record by
/// its hash. Files open in Quick Look once their bytes match that hash.
enum Artifacts {
    static func kind(_ e: Entry) -> String {
        if e.data["url"].exists { return "link" }
        let t = e.data["media_type"].str ?? ""
        if t == "text/markdown" { return "markdown" }
        if t == "application/json" { return "json" }
        if t == "text/html" { return "html" }
        if t.hasPrefix("image/") { return "image" }
        if t == "application/pdf" { return "pdf" }
        if t.hasPrefix("text/") { return "text" }
        return "file"
    }

    static func symbol(_ e: Entry) -> String {
        switch kind(e) {
        case "link": "link"
        case "json": "curlybraces"
        case "html": "globe"
        case "image": "photo"
        case "pdf": "doc.richtext"
        case "markdown": "doc.text"
        default: "doc"
        }
    }

    static func size(_ n: Int?) -> String {
        guard let n else { return "" }
        return n < 1024 ? "\(n) B" : n < 1 << 20 ? "\(n / 1024) KB" : String(format: "%.1f MB", Double(n) / Double(1 << 20))
    }

    static func hex(_ e: Entry) -> String { (e.data["digest"].str ?? "").replacingOccurrences(of: "sha256:", with: "") }

    /// The file, fetched once into the app's cache, checked against the
    /// record's hash. Nil (and a toast) if it's missing or doesn't match.
    @MainActor static func fetch(_ e: Entry) async -> URL? {
        let store = Store.shared
        let hex = hex(e)
        let name = (e.data["name"].str ?? "file").replacingOccurrences(of: "/", with: "_")
        let dir = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("dev.wheelhouse.app/blobs/\(hex)", isDirectory: true)
        let file = dir.appendingPathComponent(name)
        do {
            let data: Data
            if let cached = try? Data(contentsOf: file) {
                data = cached
            } else {
                data = try await store.hub.data("/api/blobs/\(hex)?name=\(name.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? "file")")
            }
            let sum = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
            guard sum == hex else {
                store.toast("\(name) doesn't match the hash the record pinned: not opening it", error: true)
                return nil
            }
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            if !FileManager.default.fileExists(atPath: file.path) { try data.write(to: file) }
            return file
        } catch {
            store.toast(Store.words(error), error: true)
            return nil
        }
    }

    /// Open it: a link in the browser, a file in Quick Look.
    @MainActor static func open(_ e: Entry) {
        if let u = e.data["url"].str.flatMap(URL.init(string:)) {
            NSWorkspace.shared.open(u)
            return
        }
        Task {
            guard let file = await fetch(e) else { return }
            QuickLook.shared.show(file, title: e.body ?? e.data["name"].str ?? "File")
            Store.shared.toast("\(file.lastPathComponent) matches its hash in the record (#\(e.seq))")
        }
    }

    @MainActor static func openWithApp(_ e: Entry) {
        Task { if let f = await fetch(e) { NSWorkspace.shared.open(f) } }
    }

    @MainActor static func reveal(_ e: Entry) {
        Task { if let f = await fetch(e) { NSWorkspace.shared.activateFileViewerSelecting([f]) } }
    }

    @MainActor static func save(_ e: Entry) {
        Task {
            guard let f = await fetch(e) else { return }
            let panel = NSSavePanel()
            panel.nameFieldStringValue = f.lastPathComponent
            guard panel.runModal() == .OK, let dest = panel.url else { return }
            try? FileManager.default.removeItem(at: dest)
            do { try FileManager.default.copyItem(at: f, to: dest) } catch { Store.shared.toast(error.localizedDescription, error: true) }
        }
    }
}

/// The system's Quick Look panel, for one file at a time.
final class QuickLook: NSObject, QLPreviewPanelDataSource {
    static let shared = QuickLook()
    private var url: URL?
    private var title = ""

    func show(_ url: URL, title: String) {
        self.url = url
        self.title = title
        guard let panel = QLPreviewPanel.shared() else { return }
        panel.dataSource = self
        panel.reloadData()
        panel.makeKeyAndOrderFront(nil)
    }

    func numberOfPreviewItems(in panel: QLPreviewPanel!) -> Int { url == nil ? 0 : 1 }
    func previewPanel(_ panel: QLPreviewPanel!, previewItemAt index: Int) -> (any QLPreviewItem)! { url as NSURL? }
}

/// The docket's artifacts, grouped (Round 3, Plan, Attached), newest first.
struct ArtifactsSection: View {
    let d: Docket
    let entries: [Entry]
    @State private var all = false

    var body: some View {
        let list = entries.filter { $0.kind == "artifact" }
        let groups = Dictionary(grouping: list) { $0.data["group"].str ?? "Kept" }
            .sorted { ($0.value.map(\.seq).max() ?? 0) > ($1.value.map(\.seq).max() ?? 0) }
        if !list.isEmpty {
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 8) {
                    Text("Artifacts").font(.system(size: 13, weight: .semibold)).foregroundStyle(K.text)
                    Text("\(list.count) kept · each pinned in the record by its hash").font(.system(size: 12)).foregroundStyle(K.muted)
                }
                ForEach(all ? groups : Array(groups.prefix(2)), id: \.key) { g, items in
                    if groups.count > 1 { Text(g).sectionLabel() }
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 220), spacing: 8)], alignment: .leading, spacing: 8) {
                        ForEach(items) { e in ArtifactCard(e: e) }
                    }
                }
                if groups.count > 2 {
                    Button(all ? "Show fewer" : "Show all \(groups.count) groups") { all.toggle() }.buttonStyle(.link).font(.system(size: 12))
                }
            }
        }
    }
}

struct ArtifactCard: View {
    let e: Entry
    @State private var hover = false
    @State private var viewing: DocketImage?
    @State private var comparing: (DocketImage, DocketImage)?
    @Environment(\.docketImages) private var docketImages

    var body: some View {
        let k = Artifacts.kind(e)
        let image = DocketImage(artifact: e)
        Button { if let image { viewing = image } else { Artifacts.open(e) } } label: {
            HStack(spacing: 10) {
                if let image {
                    ImageThumb(digest: image.digest, side: 44, height: 30)
                } else {
                    Image(systemName: Artifacts.symbol(e))
                        .font(.system(size: 14))
                        .foregroundStyle(k == "link" ? K.accent : k == "json" ? K.progress : k == "html" ? K.working : K.muted)
                        .frame(width: 30, height: 30)
                        .background(K.recessed, in: RoundedRectangle(cornerRadius: 7))
                }
                VStack(alignment: .leading, spacing: 2) {
                    Text(e.body ?? e.data["name"].str ?? "File").font(.system(size: 12.5, weight: .medium)).foregroundStyle(K.text).lineLimit(1)
                    Text(meta).font(.system(size: 11.5)).foregroundStyle(K.muted).lineLimit(1)
                }
                Spacer(minLength: 0)
            }
            .padding(9)
            .background(K.elev, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 9, style: .continuous).strokeBorder(hover ? K.line2 : K.line))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
        .help("\(e.body ?? e.data["name"].str ?? "")\n#\(e.seq) · \(e.data["digest"].str ?? e.data["url"].str ?? "")")
        .sheet(item: $viewing) { im in ImageViewer(image: im).environment(\.docketImages, docketImages) }
        .sheet(isPresented: Binding(get: { comparing != nil }, set: { if !$0 { comparing = nil } })) {
            if let (a, b) = comparing { CompareView(left: a, right: b) }
        }
        .contextMenu {
            if k == "link" {
                Button("Open Link") { Artifacts.open(e) }
                Button("Copy Link") { copy(e.data["url"].str ?? "") }
            } else {
                if let image {
                    Button("View") { viewing = image }
                    let others = docketImages.filter { $0.id != image.id && $0.role != "original" }
                    if let a = image.against, let ref = docketImages.first(where: { $0.seq == a && a > 0 }) {
                        Button("Compare With Its Reference (#\(a))") { comparing = (image, ref) }
                    }
                    if !others.isEmpty {
                        Menu("Compare With") {
                            ForEach(others) { o in Button("\(o.ref) · \(o.label)") { comparing = (image, o) } }
                        }
                    }
                    Divider()
                }
                Button("Quick Look") { Artifacts.open(e) }
                Button("Open With Default App") { Artifacts.openWithApp(e) }
                Button("Show in Finder") { Artifacts.reveal(e) }
                Button("Save As…") { Artifacts.save(e) }
                Divider()
                Button("Copy Hash") { copy(e.data["digest"].str ?? "") }
            }
        }
    }

    private var meta: String {
        if let im = DocketImage(artifact: e) {
            return [im.roleWords, im.against.map { "against #\($0)" }, im.width > 0 ? "\(im.width)×\(im.height)" : nil, e.data["size"].int.map(Artifacts.size)].compactMap { $0 }.joined(separator: " · ")
        }
        if let u = e.data["url"].str {
            return e.data["provider"].str == "claude" ? "Claude artifact" : URL(string: u)?.host ?? u
        }
        return [e.data["name"].str, e.data["size"].int.map(Artifacts.size)].compactMap { $0 }.joined(separator: " · ")
    }

    private func copy(_ s: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(s, forType: .string)
    }
}
