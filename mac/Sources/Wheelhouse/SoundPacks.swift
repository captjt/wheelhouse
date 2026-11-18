import AVFoundation
import AppKit
import CryptoKit
import Foundation
import Observation

// Notification sounds from peon-ping's packs (peonping.com, openpeon.com):
// voice lines from games and films, in the Coding Event Sound Pack format
// (CESP 1.0). The registry lists them; a pack installs from its GitHub
// source at the ref the registry pins, checked file by file, into
// Application Support. None ship with the app, and the macOS sound stays the
// default until you pick one in Settings › Notifications.

/// What a notification sounds like, as a pack's category. The hub picks it
/// (src/notify.rs): it knows a prompt from a failure, and a review ready to
/// post from a stalled one.
enum SoundCategory: String, CaseIterable, Identifiable, Sendable {
    case needs = "input.required"
    case done = "task.complete"
    case failed = "task.error"

    var id: String { rawValue }

    var label: String {
        switch self {
        case .needs: "Needs you"
        case .done: "Done"
        case .failed: "Failed"
        }
    }

    var detail: String {
        switch self {
        case .needs: "A prompt to approve, a question or message, blocked work"
        case .done: "A finished turn, a review ready to post"
        case .failed: "An agent failed, stalled or didn't start"
        }
    }

    /// Settings › Notifications: whether it plays (it does unless turned off).
    var defaultsKey: String { "sound.\(rawValue)" }
    var on: Bool { UserDefaults.standard.object(forKey: defaultsKey) as? Bool ?? true }

    /// A `notify` event's: what the hub said, or, from a hub that doesn't
    /// say, what its kind means.
    static func of(_ ev: JSON) -> SoundCategory? {
        if let s = ev["sound"].str { return SoundCategory(rawValue: s) }
        switch ev["kind"].str {
        case "needs", "message": return .needs
        case "turn": return .done
        default: return nil
        }
    }
}

enum SoundPackError: LocalizedError {
    case bad(String)

    var errorDescription: String? {
        if case let .bad(s) = self { return s }
        return nil
    }
}

/// What a pack's files may be, checked before anything is kept.
enum PackFiles {
    /// CESP caps a pack at 50 MB. Files go a little over its 1 MB in the
    /// registry, so a file may be up to 4 MB.
    static let maxFile = 4 << 20
    static let maxPack = 50_000_000

    /// A pack's name, as its folder: letters, digits, `-` and `_`.
    static func validName(_ s: String) -> Bool {
        !s.isEmpty && s.count <= 128 && s.unicodeScalars.allSatisfy { $0.isASCII && (CharacterSet.alphanumerics.contains($0) || $0 == "-" || $0 == "_") }
    }

    /// A path inside the pack: relative, and no step out of it.
    static func safe(_ path: String) -> Bool {
        guard !path.isEmpty, path.count <= 512, !path.hasPrefix("/"), !path.contains("\\") else { return false }
        return path.split(separator: "/", omittingEmptySubsequences: false).allSatisfy { seg in
            !seg.isEmpty && seg != "." && seg != ".." && !seg.unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) || $0 == ":" }
        }
    }

    /// Audio a player takes: WAV (RIFF…WAVE), MP3 (an ID3 tag or a frame
    /// sync), or Ogg. Anything else (an HTML error page, say) isn't kept.
    static func isAudio(_ d: Data) -> Bool {
        let b = [UInt8](d.prefix(12))
        if b.count >= 12, b[0 ..< 4] == [0x52, 0x49, 0x46, 0x46], b[8 ..< 12] == [0x57, 0x41, 0x56, 0x45] { return true }
        if b.starts(with: [0x49, 0x44, 0x33]) || b.starts(with: [0x4F, 0x67, 0x67, 0x53]) { return true }
        return b.count >= 2 && b[0] == 0xFF && b[1] & 0xE0 == 0xE0
    }

    static func sha256(_ d: Data) -> String {
        SHA256.hash(data: d).map { String(format: "%02x", $0) }.joined()
    }

    /// A path's steps, percent-encoded for a URL.
    static func encoded(_ path: String) -> String {
        path.split(separator: "/").map { String($0).addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? String($0) }.joined(separator: "/")
    }
}

/// A pack the registry lists.
struct RegistryPack: Identifiable, Hashable, Sendable {
    let name: String
    let displayName: String
    let description: String
    let author: String
    let license: String
    let language: String
    /// official (PeonPing's own), verified, or community.
    let trust: String
    let soundCount: Int
    let size: Int
    let categories: [String]
    let tags: [String]
    let repo: String
    let ref: String
    let path: String
    let manifestSHA: String

    var id: String { name }

    init?(_ j: JSON) {
        guard let name = j["name"].str, PackFiles.validName(name),
              let repo = j["source_repo"].str, repo.split(separator: "/", omittingEmptySubsequences: false).count == 2, PackFiles.safe(repo),
              let ref = j["source_ref"].text, PackFiles.safe(ref),
              let sha = j["manifest_sha256"].str?.lowercased(), sha.count == 64
        else { return nil }
        let path = (j["source_path"].str ?? "").trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard path.isEmpty || path == "." || PackFiles.safe(path) else { return nil }
        self.name = name
        self.repo = repo
        self.ref = ref
        self.path = path == "." ? "" : path
        manifestSHA = sha
        displayName = j["display_name"].text ?? name
        description = j["description"].str ?? ""
        author = j["author"]["name"].text ?? j["author"]["github"].text ?? ""
        license = j["license"].text ?? "No license given"
        language = j["language"].str ?? ""
        trust = j["trust_tier"].str ?? "community"
        soundCount = j["sound_count"].int ?? 0
        size = j["total_size_bytes"].int ?? 0
        categories = j["categories"].strings
        tags = j["tags"].strings
    }

    /// A file of it, in its GitHub repo at the ref the registry pins.
    func url(_ file: String) -> URL? {
        let parts = [repo, ref, path, file].filter { !$0.isEmpty }.map(PackFiles.encoded)
        return URL(string: "https://raw.githubusercontent.com/" + parts.joined(separator: "/"))
    }

    /// Whether it has a sound for every notification.
    var complete: Bool { SoundCategory.allCases.allSatisfy { categories.contains($0.rawValue) } }
}

/// A pack's manifest (openpeon.json), checked as CESP asks.
struct PackManifest: Sendable {
    struct Sound: Hashable, Sendable {
        let file: String
        let label: String
        let sha256: String?
    }

    let displayName: String
    let author: String?
    let license: String?
    let categories: [String: [Sound]]

    /// Every file it names, once.
    var files: [Sound] {
        var seen = Set<String>()
        return categories.keys.sorted().flatMap { categories[$0] ?? [] }.filter { seen.insert($0.file).inserted }
    }

    static func parse(_ data: Data) throws -> PackManifest {
        guard let j = try? JSON.parse(data), j.object != nil else { throw SoundPackError.bad("Its openpeon.json isn't JSON") }
        if let v = j["cesp_version"].str, !v.hasPrefix("1") { throw SoundPackError.bad("It's CESP \(v); Wheelhouse reads version 1") }
        var cats: [String: [Sound]] = [:]
        for (cat, v) in j["categories"].object ?? [:] {
            cats[cat] = try v["sounds"].array.map { s in
                guard let file = s["file"].str, PackFiles.safe(file) else { throw SoundPackError.bad("It names a file outside the pack: \(s["file"].str ?? "(none)")") }
                let sha = s["sha256"].str?.lowercased()
                if let sha, sha.count != 64 { throw SoundPackError.bad("Its hash for \(file) isn't a SHA-256") }
                return Sound(file: file, label: s["label"].str ?? "", sha256: sha)
            }
        }
        guard !cats.values.allSatisfy(\.isEmpty) else { throw SoundPackError.bad("It has no sounds") }
        return PackManifest(displayName: j["display_name"].text ?? j["name"].text ?? "Sound pack", author: j["author"]["name"].text, license: j["license"].text, categories: cats)
    }
}

/// A pack on this Mac, in Sound Packs/<name>/.
struct InstalledPack: Identifiable, Hashable {
    let name: String
    let dir: URL
    let manifest: PackManifest

    var id: String { name }

    static func == (a: InstalledPack, b: InstalledPack) -> Bool { a.name == b.name && a.dir == b.dir }
    func hash(into h: inout Hasher) { h.combine(name) }

    /// Its sounds for a notification, as files that are there.
    func sounds(_ c: SoundCategory) -> [URL] {
        (manifest.categories[c.rawValue] ?? []).map { dir.appendingPathComponent($0.file) }.filter { FileManager.default.fileExists(atPath: $0.path) }
    }
}

/// Fetching and checking a pack, apart from where it's kept, so a test can
/// hand it files.
enum PackInstaller {
    typealias Fetch = @Sendable (URL) async throws -> Data

    static let session: URLSession = {
        let c = URLSessionConfiguration.ephemeral
        c.timeoutIntervalForRequest = 20
        return URLSession(configuration: c)
    }()

    /// A file over HTTPS, or the registry from disk (a file: URL).
    static let fetch: Fetch = { url in
        if url.isFileURL { return try Data(contentsOf: url) }
        let (data, r) = try await session.data(from: url)
        if let h = r as? HTTPURLResponse, h.statusCode != 200 {
            throw SoundPackError.bad(h.statusCode == 404 ? "\(url.lastPathComponent) isn't where the registry says" : "GitHub answered \(h.statusCode) for \(url.lastPathComponent)")
        }
        return data
    }

    /// Its manifest, if it's the one the registry pinned.
    static func manifest(_ p: RegistryPack, fetch: Fetch) async throws -> (PackManifest, Data) {
        guard let url = p.url("openpeon.json") else { throw SoundPackError.bad("\(p.displayName) has no address") }
        let data = try await fetch(url)
        guard PackFiles.sha256(data) == p.manifestSHA else {
            throw SoundPackError.bad("\(p.displayName)'s openpeon.json isn't the one the registry lists, so it wasn't installed")
        }
        return (try PackManifest.parse(data), data)
    }

    /// One of its sounds, checked: its size, its hash when the manifest
    /// gives one, and that it's audio.
    static func sound(_ p: RegistryPack, _ s: PackManifest.Sound, fetch: Fetch) async throws -> Data {
        guard let url = p.url(s.file) else { throw SoundPackError.bad("\(s.file) has no address") }
        let data = try await fetch(url)
        if data.count > PackFiles.maxFile { throw SoundPackError.bad("\(s.file) is over \(PackFiles.maxFile >> 20) MB") }
        if let sha = s.sha256, PackFiles.sha256(data) != sha { throw SoundPackError.bad("\(s.file) isn't the file its manifest lists, so the pack wasn't installed") }
        if !PackFiles.isAudio(data) { throw SoundPackError.bad("\(s.file) isn't WAV, MP3 or Ogg audio") }
        return data
    }

    /// The whole pack, into `root/<name>`: every file checked first, then
    /// put in place in one move, so a failure leaves nothing behind.
    static func install(_ p: RegistryPack, into root: URL, fetch: @escaping Fetch) async throws -> InstalledPack {
        let (manifest, raw) = try await manifest(p, fetch: fetch)
        let files = manifest.files
        let got = try await withThrowingTaskGroup(of: (String, Data).self) { group in
            var out: [String: Data] = [:]
            var next = 0
            // Six at a time.
            func add() {
                guard next < files.count else { return }
                let s = files[next]
                next += 1
                group.addTask { (s.file, try await sound(p, s, fetch: fetch)) }
            }
            for _ in 0 ..< 6 { add() }
            var total = 0
            while let got = try await group.next() {
                let (file, data) = got
                total += data.count
                if total > PackFiles.maxPack { throw SoundPackError.bad("\(p.displayName) is over 50 MB") }
                out[file] = data
                add()
            }
            return out
        }
        let fm = FileManager.default
        try fm.createDirectory(at: root, withIntermediateDirectories: true)
        let tmp = root.appendingPathComponent(".installing-\(p.name)-\(UUID().uuidString)")
        do {
            for (file, data) in got {
                let to = tmp.appendingPathComponent(file)
                try fm.createDirectory(at: to.deletingLastPathComponent(), withIntermediateDirectories: true)
                try data.write(to: to)
            }
            try raw.write(to: tmp.appendingPathComponent("openpeon.json"))
            let dest = root.appendingPathComponent(p.name)
            if fm.fileExists(atPath: dest.path) { try fm.removeItem(at: dest) }
            try fm.moveItem(at: tmp, to: dest)
            return InstalledPack(name: p.name, dir: dest, manifest: manifest)
        } catch {
            try? fm.removeItem(at: tmp)
            throw error
        }
    }
}

/// The packs: what's installed, the registry, and playing a notification's
/// sound from the one you picked.
@MainActor @Observable
final class SoundPacks {
    static let shared = SoundPacks()

    /// The registry's index (`WH_SOUND_REGISTRY` for another, a file: URL in tests).
    static let registryURL = URL(string: ProcessInfo.processInfo.environment["WH_SOUND_REGISTRY"] ?? "https://peonping.github.io/registry/index.json")!

    /// Where packs are kept (`WH_SOUND_PACKS` for another).
    static var dir: URL {
        if let d = ProcessInfo.processInfo.environment["WH_SOUND_PACKS"] { return URL(fileURLWithPath: d, isDirectory: true) }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support.appendingPathComponent(Flavor.name).appendingPathComponent("Sound Packs")
    }

    private(set) var installed: [InstalledPack] = []
    private(set) var registry: [RegistryPack]?
    private(set) var registryError: String?
    private(set) var loadingRegistry = false
    private var registryLoaded: Date?
    private(set) var installing: Set<String> = []
    private(set) var previewing: String?
    var errors: [String: String] = [:]

    private var player: AVAudioPlayer?
    private var last: [String: URL] = [:]
    private var manifests: [String: PackManifest] = [:]

    private init() { scan() }

    /// The pack Settings picked, if it's still here: nil is the macOS sound.
    var chosen: InstalledPack? {
        let name = UserDefaults.standard.string(forKey: "soundPack") ?? ""
        return installed.first { $0.name == name }
    }

    func scan() {
        let fm = FileManager.default
        let names = (try? fm.contentsOfDirectory(atPath: Self.dir.path)) ?? []
        installed = names.filter { PackFiles.validName($0) }.sorted().compactMap { name in
            let d = Self.dir.appendingPathComponent(name)
            guard let data = try? Data(contentsOf: d.appendingPathComponent("openpeon.json")), let m = try? PackManifest.parse(data) else { return nil }
            return InstalledPack(name: name, dir: d, manifest: m)
        }
    }

    func isInstalled(_ name: String) -> Bool { installed.contains { $0.name == name } }

    /// The registry, fetched when Browse opens (again after an hour).
    func loadRegistry(force: Bool = false) async {
        if loadingRegistry || (!force && registry != nil && registryLoaded.map { -$0.timeIntervalSinceNow < 3600 } == true) { return }
        loadingRegistry = true
        defer { loadingRegistry = false }
        do {
            let data = try await PackInstaller.fetch(Self.registryURL)
            let packs = try JSON.parse(data)["packs"].array.compactMap(RegistryPack.init)
            let rank = ["official": 0, "verified": 1]
            registry = packs.sorted { (rank[$0.trust] ?? 2, $0.displayName.lowercased()) < (rank[$1.trust] ?? 2, $1.displayName.lowercased()) }
            registryError = nil
            registryLoaded = Date()
        } catch {
            registryError = "Couldn't load the pack list: \(error.localizedDescription)"
        }
    }

    /// Install it, and use it if you were on the macOS sound.
    func install(_ p: RegistryPack) async {
        guard !installing.contains(p.name) else { return }
        installing.insert(p.name)
        errors[p.name] = nil
        defer { installing.remove(p.name) }
        do {
            _ = try await PackInstaller.install(p, into: Self.dir, fetch: PackInstaller.fetch)
            scan()
            if chosen == nil { UserDefaults.standard.set(p.name, forKey: "soundPack") }
        } catch {
            errors[p.name] = error.localizedDescription
        }
    }

    /// Remove it; if it was the one in use, back to the macOS sound.
    func remove(_ name: String) {
        guard PackFiles.validName(name) else { return }
        try? FileManager.default.removeItem(at: Self.dir.appendingPathComponent(name))
        if UserDefaults.standard.string(forKey: "soundPack") == name { UserDefaults.standard.set("", forKey: "soundPack") }
        scan()
    }

    // ---- playing ----

    /// A notification's sound from the chosen pack, unless one is playing:
    /// a burst doesn't cut each line off.
    func play(_ c: SoundCategory) {
        guard let pack = chosen, c.on, player?.isPlaying != true else { return }
        play(pack.sounds(c), key: "\(pack.name)/\(c.rawValue)")
    }

    /// Settings' preview: one of the chosen pack's, now.
    func preview(_ c: SoundCategory, pack: InstalledPack) {
        player?.stop()
        play(pack.sounds(c), key: "\(pack.name)/\(c.rawValue)")
    }

    /// Browse's preview: a "Done" line, fetched (checked) into the cache.
    func preview(_ p: RegistryPack) async {
        player?.stop()
        if let i = installed.first(where: { $0.name == p.name }) {
            preview(.done, pack: i)
            return
        }
        previewing = p.name
        errors[p.name] = nil
        defer { if previewing == p.name { previewing = nil } }
        do {
            let m: PackManifest
            if let cached = manifests[p.name] {
                m = cached
            } else {
                m = try await PackInstaller.manifest(p, fetch: PackInstaller.fetch).0
                manifests[p.name] = m
            }
            let pool = m.categories[SoundCategory.done.rawValue].flatMap { $0.isEmpty ? nil : $0 } ?? m.files
            guard let s = pool.randomElement() else { return }
            let cache = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
                .appendingPathComponent(Flavor.name).appendingPathComponent("Sound Previews").appendingPathComponent(p.name)
            let file = cache.appendingPathComponent(s.file)
            if !FileManager.default.fileExists(atPath: file.path) {
                let data = try await PackInstaller.sound(p, s, fetch: PackInstaller.fetch)
                try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
                try data.write(to: file)
            }
            guard previewing == p.name else { return }
            player?.stop()
            if !start(file) { errors[p.name] = "This Mac can't play \(s.file)" }
        } catch {
            errors[p.name] = error.localizedDescription
        }
    }

    private func play(_ files: [URL], key: String) {
        var pool = files
        // One this Mac can't play is passed over for another.
        while let f = Self.pick(pool, last: last[key]) {
            if start(f) {
                last[key] = f
                return
            }
            pool.removeAll { $0 == f }
        }
    }

    private func start(_ file: URL) -> Bool {
        guard let p = try? AVAudioPlayer(contentsOf: file) else { return false }
        p.volume = Float(UserDefaults.standard.object(forKey: "soundVolume") as? Double ?? 0.6)
        player = p
        return p.play()
    }

    /// A random one, not the last one played when there's another (CESP).
    nonisolated static func pick<T: Equatable>(_ items: [T], last: T?) -> T? {
        items.filter { $0 != last }.randomElement() ?? items.first
    }
}
