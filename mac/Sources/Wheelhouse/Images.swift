import AppKit
import CryptoKit
import ImageIO
import SwiftUI
import UniformTypeIdentifiers

// Images you share with a docket's agents (src/images.rs): staged with
// `POST /api/images`, sent with what you typed, kept as References. Here
// are their model, the cache that fetches them, and the views that show
// them: a thumbnail, the viewer, and Compare.

/// A numbered pin on a marked-up image, and what it says.
struct ImagePin: Hashable, Codable {
    var n: Int
    var note: String
}

/// What `POST /api/images` answers: a staged image, on no record yet.
struct StagedImage: Hashable {
    let digest: String
    let name: String
    let width: Int
    let height: Int

    init?(_ j: JSON) {
        guard let d = j["digest"].str, d.hasPrefix("sha256:") else { return nil }
        digest = d
        name = j["name"].str ?? "image.png"
        width = j["width"].int ?? 0
        height = j["height"].int ?? 0
    }

    init(digest: String, name: String, width: Int, height: Int) {
        self.digest = digest
        self.name = name
        self.width = width
        self.height = height
    }
}

/// An image waiting in the composer's tray: its bytes, staged or not, and
/// what you said about it. A marked-up one keeps the bytes it started from
/// and its marks, so Mark Up again starts where you left it.
struct TrayImage: Identifiable, Equatable {
    enum Stage: Equatable { case staging, staged(StagedImage), failed(String) }

    let id = UUID()
    var name: String
    /// What's sent: the original, or the marked-up render.
    var data: Data
    var stage: Stage = .staging
    var caption = ""
    /// Before markup: its bytes, and their digest once staged.
    var base: Data?
    var baseDigest: String?
    var baseName: String?
    var marks: [MarkupMark] = []
    var pins: [ImagePin] = []

    var staged: StagedImage? {
        if case let .staged(s) = stage { return s }
        return nil
    }

    var failure: String? {
        if case let .failed(why) = stage { return why }
        return nil
    }

    static func == (a: TrayImage, b: TrayImage) -> Bool {
        a.id == b.id && a.stage == b.stage && a.caption == b.caption && a.data == b.data && a.marks == b.marks && a.pins == b.pins
    }
}

enum Images {
    /// The most images sent at once (src/images.rs MAX_PER).
    static let maxPer = 10
    /// The largest image (src/images.rs MAX).
    static let maxBytes = 15 << 20

    /// The body of `POST /api/dockets/{id}/compose`: the text, and each
    /// staged image with its caption, its original and its pins. Images not
    /// staged yet (or failed) aren't in it; `ready` says whether all are.
    static func composeBody(text: String, tray: [TrayImage]) -> [String: Any] {
        var body: [String: Any] = ["text": text]
        let refs: [[String: Any]] = tray.compactMap { t in
            guard let s = t.staged else { return nil }
            var r: [String: Any] = ["digest": s.digest, "name": s.name]
            let cap = t.caption.trimmingCharacters(in: .whitespacesAndNewlines)
            if !cap.isEmpty { r["caption"] = cap }
            if let o = t.baseDigest, o != s.digest { r["original"] = o }
            let pins = t.pins.map { ["n": $0.n, "note": $0.note.trimmingCharacters(in: .whitespacesAndNewlines)] as [String: Any] }
            if !pins.isEmpty { r["pins"] = pins }
            return r
        }
        if !refs.isEmpty { body["images"] = refs }
        return body
    }

    /// Whether every image in the tray is staged: sending waits for that.
    static func ready(_ tray: [TrayImage]) -> Bool { tray.allSatisfy { $0.staged != nil } }

    /// The Markdown the intent gets for an image you pasted into it: the
    /// hub keeps it when you save (src/images.rs `in_markdown`).
    static func markdown(_ s: StagedImage, alt: String) -> String {
        let cleanAlt = alt.replacingOccurrences(of: "]", with: "").replacingOccurrences(of: "[", with: "").replacingOccurrences(of: "\n", with: " ")
        let name = s.name.addingPercentEncoding(withAllowedCharacters: .alphanumerics.union(CharacterSet(charactersIn: "-._"))) ?? "image.png"
        return "![\(cleanAlt)](/api/blobs/\(s.digest)?name=\(name))"
    }

    /// A blob link in Markdown (`/api/blobs/sha256:…`), as a URL on the hub.
    static func blobURL(_ src: String) -> URL? {
        guard src.hasPrefix("/api/blobs/sha256:") else { return nil }
        let hex = src.dropFirst("/api/blobs/sha256:".count).prefix { $0.isHexDigit }
        guard hex.count == 64 else { return nil }
        return URL(string: HubClient.currentBase.absoluteString + src)
    }

    /// The image files and pictures on a pasteboard, each with a name: files
    /// first (they keep their names), else a picture (a screenshot copied to
    /// the clipboard), as PNG.
    static func read(_ pb: NSPasteboard) -> [(name: String, data: Data)] {
        var out: [(String, Data)] = []
        let urls = (pb.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]) ?? []
        for u in urls {
            guard let t = UTType(filenameExtension: u.pathExtension), t.conforms(to: .image), !t.conforms(to: .svg) else { continue }
            if let d = try? Data(contentsOf: u) { out.append((u.lastPathComponent, d)) }
        }
        if !out.isEmpty { return out }
        if let png = pb.data(forType: .png) { return [("pasted.png", png)] }
        if let img = NSImage(pasteboard: pb), let png = pngData(img) { return [("pasted.png", png)] }
        return []
    }

    /// Whether a pasteboard has an image this can take.
    static func has(_ pb: NSPasteboard) -> Bool {
        if pb.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true, .urlReadingContentsConformToTypes: [UTType.image.identifier]]) { return true }
        return pb.availableType(from: [.png, .tiff]) != nil
    }

    static func pngData(_ img: NSImage) -> Data? {
        guard let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil) else { return nil }
        return NSBitmapImageRep(cgImage: cg).representation(using: .png, properties: [:])
    }

    /// Pixel size from the bytes' header, without decoding them.
    static func pixelSize(_ data: Data) -> CGSize? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil),
              let p = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any],
              let w = p[kCGImagePropertyPixelWidth] as? Int, let h = p[kCGImagePropertyPixelHeight] as? Int else { return nil }
        return CGSize(width: w, height: h)
    }

    /// A thumbnail no bigger than `max` pixels on a side, decoded off the
    /// main thread's way (ImageIO, not NSImage's full decode).
    static func thumbnail(_ data: Data, max: Int) -> NSImage? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: max,
        ]
        guard let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) else { return nil }
        return NSImage(cgImage: cg, size: CGSize(width: cg.width, height: cg.height))
    }

    /// Open the file picker for images; their names and bytes.
    @MainActor static func pick() -> [(name: String, data: Data)] {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [.png, .jpeg, .gif, .webP, .heic, .tiff]
        panel.message = "Images to share with the agents on this docket"
        guard panel.runModal() == .OK else { return [] }
        return panel.urls.compactMap { u in (try? Data(contentsOf: u)).map { (u.lastPathComponent, $0) } }
    }

    /// The most pixels on a side, and in all, an image goes at: the web
    /// app's `fitLimits`. A full-page screenshot (1440×24000) comes out about
    /// 980×16330, still readable.
    static let fitSide = 16_384
    static let fitPixels = 16_000_000

    /// The size an image is fitted to: the same, or scaled so its longer
    /// side is at most `fitSide` and its area at most `fitPixels`.
    static func fitted(_ w: Int, _ h: Int) -> (w: Int, h: Int) {
        guard w > 0, h > 0 else { return (w, h) }
        let s = min(1, Double(fitSide) / Double(max(w, h)), (Double(fitPixels) / (Double(w) * Double(h))).squareRoot())
        if s >= 1 { return (w, h) }
        // ImageIO takes the longer side and rounds the other: step down
        // until what it makes fits.
        let (long, short) = (max(w, h), min(w, h))
        var l = min(fitSide, Int((Double(long) * s).rounded(.down)))
        var o = max(1, Int((Double(short) * Double(l) / Double(long)).rounded()))
        while l > 1, l * o > fitPixels {
            l -= 1
            o = max(1, Int((Double(short) * Double(l) / Double(long)).rounded()))
        }
        return w >= h ? (l, o) : (o, l)
    }

    /// An image as it's sent: left alone when it's already a PNG, JPEG, GIF
    /// or WebP within the limits; otherwise decoded, scaled to fit, and
    /// written again: a screenshot (a PNG) as PNG unless that's over
    /// `maxBytes`, a photo (anything else: HEIC, JPEG, TIFF) as JPEG at 0.9.
    /// Nil when it isn't an image. Slow on a big photo: call it off the main
    /// actor.
    static func fit(name: String, data: Data, maxBytes: Int = Images.maxBytes) -> (name: String, data: Data)? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil), let type = CGImageSourceGetType(src) as String?,
              let size = pixelSize(data) else { return nil }
        let (w, h) = (Int(size.width), Int(size.height))
        let ok: Set<String> = [UTType.png.identifier, UTType.jpeg.identifier, UTType.gif.identifier, UTType.webP.identifier]
        let to = fitted(w, h)
        if ok.contains(type), data.count <= maxBytes, to.w == w, to.h == h { return (name, data) }
        // Decoded at the fitted size (its orientation applied), never larger.
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: max(to.w, to.h),
        ]
        guard let cg = CGImageSourceCreateThumbnailAtIndex(src, 0, opts as CFDictionary) else { return nil }
        let rep = NSBitmapImageRep(cgImage: cg)
        let stem = { () -> String in
            let s = (name as NSString).deletingPathExtension
            return s.isEmpty ? "image" : s
        }()
        if type == UTType.png.identifier, let png = rep.representation(using: .png, properties: [:]), png.count <= maxBytes {
            return (stem + ".png", png)
        }
        guard let jpg = rep.representation(using: .jpeg, properties: [.compressionFactor: 0.9]) else { return nil }
        return (stem + ".jpg", jpg)
    }

    /// The tray after a send: what went is taken out; an image added while
    /// it was on its way stays, for the next one.
    static func afterSend(_ tray: [TrayImage], sent: Set<UUID>) -> [TrayImage] {
        tray.filter { !sent.contains($0.id) }
    }
}

extension HubClient {
    /// The hub's address, read the way `base` reads it, from anywhere.
    static var currentBase: URL {
        let s = ProcessInfo.processInfo.environment["WH_APP_HUB"] ?? UserDefaults.standard.string(forKey: "hubAddress") ?? defaultAddress
        return URL(string: s.trimmingCharacters(in: CharacterSet(charactersIn: "/ "))) ?? URL(string: defaultAddress)!
    }

    /// Stage an image's bytes (`POST /api/images`).
    func stage(name: String, data: Data) async throws -> StagedImage {
        let q = name.addingPercentEncoding(withAllowedCharacters: .alphanumerics.union(CharacterSet(charactersIn: "-._"))) ?? "image"
        var req = URLRequest(url: url("/api/images?name=\(q)"))
        req.httpMethod = "POST"
        req.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
        req.setValue("mac", forHTTPHeaderField: "X-Wheelhouse")
        req.timeoutInterval = 120
        let (body, res) = try await URLSession.shared.upload(for: req, from: data)
        if let h = res as? HTTPURLResponse, !(200 ..< 300).contains(h.statusCode) {
            let msg = (try? JSON.parse(body))?["error"].text ?? "The hub answered \(h.statusCode)"
            throw HubError(message: msg, status: h.statusCode)
        }
        guard let s = StagedImage(try JSON.parse(body)) else { throw HubError(message: "The hub didn't say what it kept") }
        return s
    }
}

/// Images fetched from the hub by their hash, each checked against it,
/// kept in memory (thumbnails) and on disk (the bytes).
actor ImageCache {
    static let shared = ImageCache()
    private var thumbs: [String: NSImage] = [:]
    private var order: [String] = []

    private func dir() -> URL {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("dev.wheelhouse.app/images", isDirectory: true)
    }

    static func hex(_ digest: String) -> String { digest.replacingOccurrences(of: "sha256:", with: "").lowercased() }

    /// The image's bytes, matching its hash, or an error that says why not.
    func data(_ digest: String) async throws -> Data {
        let hex = Self.hex(digest)
        guard hex.count == 64, hex.allSatisfy(\.isHexDigit) else { throw HubError(message: "Not an image's hash") }
        let file = dir().appendingPathComponent(hex)
        if let d = try? Data(contentsOf: file), Self.sum(d) == hex { return d }
        let (d, res) = try await URLSession.shared.data(from: URL(string: HubClient.currentBase.absoluteString + "/api/blobs/\(hex)?name=image")!)
        if let h = res as? HTTPURLResponse, h.statusCode != 200 { throw HubError(message: h.statusCode == 404 ? "This hub doesn't have the image" : "The hub answered \(h.statusCode)", status: h.statusCode) }
        guard Self.sum(d) == hex else { throw HubError(message: "The image doesn't match the hash the record pinned") }
        try? FileManager.default.createDirectory(at: dir(), withIntermediateDirectories: true)
        try? d.write(to: file, options: .atomic)
        return d
    }

    /// A thumbnail at most `max` pixels on a side.
    func thumb(_ digest: String, max: Int) async throws -> NSImage {
        let key = "\(Self.hex(digest))@\(max)"
        if let t = thumbs[key] { return t }
        let d = try await data(digest)
        guard let t = Images.thumbnail(d, max: max) else { throw HubError(message: "Couldn't read the image") }
        thumbs[key] = t
        order.append(key)
        if order.count > 300 { thumbs[order.removeFirst()] = nil }
        return t
    }

    static func sum(_ d: Data) -> String { SHA256.hash(data: d).map { String(format: "%02x", $0) }.joined() }
}

/// An image on a docket, as the viewer and Compare show it: from its
/// artifact entry, or from an entry's `data.images`. Images are matched by
/// their digest: a shared docket's entries have no number until its
/// workspace orders them.
struct DocketImage: Identifiable, Hashable {
    /// Its artifact's entry; 0 when it isn't known (yet).
    let seq: Int
    let digest: String
    let name: String
    var caption: String
    var title: String
    var pins: [ImagePin]
    let width: Int
    let height: Int
    /// "reference", "original", "intent", "attached".
    let role: String
    /// The entry of the reference a screenshot is compared with.
    let against: Int?
    /// The digest of the image before it was marked up.
    let original: String?
    let byAgent: Bool
    var id: String { "\(seq)|\(digest)|\(role)|\(caption)" }

    init?(artifact e: Entry) {
        guard e.kind == "artifact", let t = e.data["media_type"].str, t.hasPrefix("image/"), t != "image/svg+xml", let d = e.data["digest"].str else { return nil }
        seq = e.seq
        digest = d
        name = e.data["name"].str ?? "image"
        caption = e.data["caption"].str ?? ""
        title = e.body ?? name
        pins = DocketImage.pins(e.data["pins"])
        width = e.data["width"].int ?? 0
        height = e.data["height"].int ?? 0
        role = e.data["image"].str ?? "attached"
        against = e.data["against"].int
        original = e.data["original"].str.flatMap { $0.hasPrefix("sha256:") ? $0 : nil }
        byAgent = e.actorType == "agent"
    }

    /// One of an entry's `data.images` (`{digest, name, caption, width,
    /// height, pins, original}`): the reference artifact kept with the same
    /// digest and caption when it's there, else the image as the entry
    /// names it (the artifact comes after the entry, or hasn't arrived).
    init?(_ j: JSON, known: [DocketImage]) {
        guard let d = j["digest"].str, d.hasPrefix("sha256:") else { return nil }
        let caption = j["caption"].str ?? ""
        if let k = known.first(where: { $0.digest == d && $0.role == "reference" && $0.caption == caption }) {
            self = k
            return
        }
        seq = 0
        digest = d
        name = j["name"].str ?? "image"
        self.caption = caption
        title = caption.isEmpty ? name : caption
        pins = DocketImage.pins(j["pins"])
        width = j["width"].int ?? 0
        height = j["height"].int ?? 0
        role = "reference"
        against = nil
        original = j["original"].str.flatMap { $0.hasPrefix("sha256:") ? $0 : nil }
        byAgent = false
    }

    static func pins(_ j: JSON) -> [ImagePin] {
        j.array.compactMap { p in p["n"].int.map { ImagePin(n: $0, note: p["note"].str ?? "") } }
    }

    /// A docket's images, newest first.
    static func all(_ entries: [Entry]) -> [DocketImage] {
        entries.compactMap(DocketImage.init(artifact:)).reversed()
    }

    /// The digests the docket's entries carry with what was said (each
    /// `data.images[]`'s, and its original's): their artifacts aren't
    /// shown again in the Activity.
    static func carried(_ entries: [Entry]) -> Set<String> {
        var out = Set<String>()
        for e in entries where e.kind != "artifact" {
            for i in e.data["images"].array {
                if let d = i["digest"].str { out.insert(d) }
                if let o = i["original"].str { out.insert(o) }
            }
        }
        return out
    }

    /// Whether an artifact's row is left out of the Activity: a reference
    /// or its original that an entry already shows. A screenshot attached
    /// against one still shows.
    static func hiddenInActivity(_ e: Entry, carried: Set<String>) -> Bool {
        guard e.kind == "artifact", ["reference", "original"].contains(e.data["image"].str ?? ""), let d = e.data["digest"].str else { return false }
        return carried.contains(d)
    }

    /// Before markup: the original kept for this one.
    func originalImage(in all: [DocketImage]) -> DocketImage? {
        guard let o = original else { return nil }
        return all.first { $0.digest == o && $0.role == "original" } ?? all.first { $0.digest == o }
    }

    /// Marked up: the reference drawn from this original.
    func markedUp(in all: [DocketImage]) -> DocketImage? {
        guard role == "original" else { return nil }
        return all.first { $0.role == "reference" && $0.original == digest }
    }

    /// How it's named: its entry's number, or its name until it has one.
    var ref: String { seq > 0 ? "#\(seq)" : name }
    var label: String { caption.isEmpty ? title : caption }
    var roleWords: String {
        switch role {
        case "reference": "Reference"
        case "original": "Before markup"
        case "intent": "In the intent"
        default: byAgent ? "An agent's" : "Attached"
        }
    }
}

/// The docket's images, for any view under its page: the viewer and
/// Compare pick from them.
private struct DocketImagesKey: EnvironmentKey {
    static let defaultValue: [DocketImage] = []
}

/// The digests the docket's entries carry (`DocketImage.carried`).
private struct CarriedImagesKey: EnvironmentKey {
    static let defaultValue: Set<String> = []
}

extension EnvironmentValues {
    var docketImages: [DocketImage] {
        get { self[DocketImagesKey.self] }
        set { self[DocketImagesKey.self] = newValue }
    }

    var carriedImages: Set<String> {
        get { self[CarriedImagesKey.self] }
        set { self[CarriedImagesKey.self] = newValue }
    }
}

/// A thumbnail fetched by its hash: a spinner while it comes, the reason
/// when it doesn't.
struct ImageThumb: View {
    let digest: String
    var side: CGFloat = 96
    var height: CGFloat?
    @State private var img: NSImage?
    @State private var failed: String?

    var body: some View {
        ZStack {
            K.recessed
            if let img {
                Image(nsImage: img).resizable().scaledToFill()
            } else if let failed {
                Image(systemName: "photo.badge.exclamationmark").foregroundStyle(K.error).help(failed)
            } else {
                ProgressView().controlSize(.small)
            }
        }
        .frame(width: side, height: height ?? side * 0.68)
        .clipShape(RoundedRectangle(cornerRadius: 7, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 7, style: .continuous).strokeBorder(K.line))
        .task(id: digest) {
            do { img = try await ImageCache.shared.thumb(digest, max: Int(side * 2 * 2)) } catch { failed = Store.words(error) }
        }
    }
}

/// A row of an entry's images: click one to view it.
struct ImageStrip: View {
    let images: [DocketImage]
    var side: CGFloat = 120
    @State private var open: DocketImage?

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(images) { im in
                    Button { open = im } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            ImageThumb(digest: im.digest, side: side)
                            if !im.caption.isEmpty || !im.pins.isEmpty {
                                Text([im.caption, im.pins.isEmpty ? "" : "\(im.pins.count) pin\(im.pins.count == 1 ? "" : "s")"].filter { !$0.isEmpty }.joined(separator: " · "))
                                    .font(.system(size: 11.5)).foregroundStyle(K.muted).lineLimit(2).frame(width: side, alignment: .leading)
                            }
                        }
                    }
                    .buttonStyle(.plain)
                    .help(im.label)
                }
            }
        }
        .sheet(item: $open) { im in ImageViewer(image: im) }
    }
}

/// One image, large, with its caption and pins: Compare it with another
/// on the docket, or open it in Quick Look.
struct ImageViewer: View {
    let image: DocketImage
    @Environment(\.docketImages) private var all
    @Environment(\.dismiss) private var dismiss
    @State private var full: NSImage?
    @State private var failed: String?
    @State private var comparing: DocketImage?
    @State private var shown: DocketImage?

    var body: some View {
        let im = shown ?? image
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                Text(im.label).font(.system(size: 14, weight: .semibold)).foregroundStyle(K.text).lineLimit(2)
                Text("\(im.ref) · \(im.roleWords)\(im.width > 0 ? " · \(im.width)×\(im.height)" : "")").font(.system(size: 12)).foregroundStyle(K.muted)
                Spacer()
                if let orig = image.originalImage(in: all) {
                    Button(shown == nil ? "Show Original" : "Show Marked Up") { shown = shown == nil ? orig : nil }.controlSize(.small)
                } else if let marked = image.markedUp(in: all) {
                    Button(shown == nil ? "Show Marked Up" : "Show Original") { shown = shown == nil ? marked : nil }.controlSize(.small)
                }
                compareMenu(im)
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction).controlSize(.small)
            }
            ZStack {
                K.recessed
                if let full {
                    Image(nsImage: full).resizable().scaledToFit()
                } else if let failed {
                    Text(failed).foregroundStyle(K.error)
                } else {
                    ProgressView()
                }
            }
            .frame(minWidth: 520, idealWidth: 900, maxWidth: .infinity, minHeight: 320, idealHeight: 560, maxHeight: .infinity)
            .clipShape(RoundedRectangle(cornerRadius: 8))
            if !im.pins.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(im.pins, id: \.n) { p in
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            PinBadge(n: p.n, color: ImageMarks.defaultColor.color, size: 18)
                            Text(p.note.isEmpty ? "(no note)" : p.note).font(.system(size: 12.5)).foregroundStyle(p.note.isEmpty ? K.faint : K.text2).textSelection(.enabled)
                        }
                    }
                }
            }
        }
        .padding(16)
        .frame(minWidth: 560, minHeight: 420)
        .background(K.main)
        .task(id: im.digest) {
            full = nil
            failed = nil
            do {
                let d = try await ImageCache.shared.data(im.digest)
                full = NSImage(data: d)
                if full == nil { failed = "Couldn't read the image" }
            } catch { failed = Store.words(error) }
        }
        .sheet(item: $comparing) { other in CompareView(left: im, right: other) }
    }

    @ViewBuilder private func compareMenu(_ im: DocketImage) -> some View {
        let others = all.filter { $0.id != im.id && $0.role != "original" }
        if !others.isEmpty {
            Menu("Compare") {
                if let a = im.against, let ref = all.first(where: { $0.seq == a && a > 0 }) {
                    Button("With Its Reference (#\(a))") { comparing = ref }
                    Divider()
                }
                ForEach(others) { o in Button("\(o.ref) · \(o.label)") { comparing = o } }
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .controlSize(.small)
        }
    }
}

/// Two images, to compare: side by side, a swipe between them, or one over
/// the other.
struct CompareView: View {
    enum Mode: String, CaseIterable { case side = "Side by Side", swipe = "Swipe", onion = "Onion Skin" }
    let left: DocketImage
    let right: DocketImage
    @Environment(\.dismiss) private var dismiss
    @State private var mode: Mode = .side
    @State private var split: CGFloat = 0.5
    @State private var opacity: Double = 0.5
    @State private var a: NSImage?
    @State private var b: NSImage?
    @State private var failed: String?

    /// The reference on the left, what was built on the right.
    private var pair: (DocketImage, DocketImage) {
        if (right.seq > 0 && left.against == right.seq) || (right.role == "reference" && left.role != "reference") { return (right, left) }
        return (left, right)
    }

    var body: some View {
        let (l, r) = pair
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Text("Compare").font(.system(size: 14, weight: .semibold))
                Picker("", selection: $mode) { ForEach(Mode.allCases, id: \.self) { Text($0.rawValue).tag($0) } }
                    .pickerStyle(.segmented).labelsHidden().fixedSize()
                if mode == .onion {
                    Slider(value: $opacity, in: 0 ... 1).frame(width: 160).help("How much of \(r.ref) shows over \(l.ref)")
                }
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction).controlSize(.small)
            }
            HStack {
                Text("\(l.ref) · \(l.label)").lineLimit(1)
                Spacer()
                Text("\(r.ref) · \(r.label)").lineLimit(1)
            }
            .font(.system(size: 12)).foregroundStyle(K.muted)
            ZStack {
                K.recessed
                if let failed {
                    Text(failed).foregroundStyle(K.error)
                } else if let a, let b {
                    stage(a, b)
                } else {
                    ProgressView()
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .frame(minWidth: 720, idealWidth: 1100, maxWidth: .infinity, minHeight: 420, idealHeight: 640, maxHeight: .infinity)
        }
        .padding(16)
        .background(K.main)
        .task {
            do {
                async let da = ImageCache.shared.data(l.digest)
                async let db = ImageCache.shared.data(r.digest)
                let (x, y) = try await (da, db)
                a = NSImage(data: x)
                b = NSImage(data: y)
                if a == nil || b == nil { failed = "Couldn't read one of the images" }
            } catch { failed = Store.words(error) }
        }
    }

    @ViewBuilder private func stage(_ a: NSImage, _ b: NSImage) -> some View {
        switch mode {
        case .side:
            HStack(spacing: 8) {
                Image(nsImage: a).resizable().scaledToFit()
                Image(nsImage: b).resizable().scaledToFit()
            }
            .padding(8)
        case .onion:
            ZStack {
                Image(nsImage: a).resizable().scaledToFit()
                Image(nsImage: b).resizable().scaledToFit().opacity(opacity)
            }
        case .swipe:
            GeometryReader { g in
                ZStack(alignment: .leading) {
                    Image(nsImage: a).resizable().scaledToFit().frame(width: g.size.width, height: g.size.height)
                    Image(nsImage: b).resizable().scaledToFit().frame(width: g.size.width, height: g.size.height)
                        .mask(alignment: .leading) { Rectangle().frame(width: g.size.width * split) }
                    Rectangle().fill(K.accent).frame(width: 2).offset(x: g.size.width * split - 1)
                    Circle().fill(K.accent).frame(width: 16, height: 16).offset(x: g.size.width * split - 8)
                }
                .contentShape(Rectangle())
                .gesture(DragGesture(minimumDistance: 0).onChanged { v in split = min(max(v.location.x / max(g.size.width, 1), 0), 1) })
            }
        }
    }
}

/// A numbered pin, as drawn on an image and listed beside it.
struct PinBadge: View {
    let n: Int
    let color: Color
    var size: CGFloat = 22

    var body: some View {
        Text("\(n)")
            .font(.system(size: size * 0.55, weight: .bold))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(Circle().fill(color))
            .overlay(Circle().strokeBorder(.white, lineWidth: max(1, size / 14)))
    }
}
