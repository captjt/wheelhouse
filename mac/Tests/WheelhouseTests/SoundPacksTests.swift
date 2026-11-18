import AVFoundation
import Foundation
import Testing
@testable import Wheelhouse

// Sound packs: what a pack may hold, the registry's entries, installing one
// (every file checked, nothing left behind on a failure), and which sound a
// notification plays.

private let wav = Data([0x52, 0x49, 0x46, 0x46, 0x24, 0, 0, 0, 0x57, 0x41, 0x56, 0x45, 0x66, 0x6D, 0x74, 0x20])
private let mp3 = Data([0x49, 0x44, 0x33, 4, 0, 0, 0, 0, 0, 0])

private func sha(_ d: Data) -> String { PackFiles.sha256(d) }

/// A pack's manifest, and the registry's entry pinning it.
private func pack(_ sounds: [String: [(String, Data)]], name: String = "peon", path: String = "peon", hashes: Bool = true) -> (RegistryPack, Data) {
    var cats: [String: Any] = [:]
    for (cat, files) in sounds {
        cats[cat] = ["sounds": files.map { f, d -> [String: Any] in
            var s: [String: Any] = ["file": f, "label": f]
            if hashes { s["sha256"] = sha(d) }
            return s
        }]
    }
    let manifest = try! JSONSerialization.data(withJSONObject: ["cesp_version": "1.0", "name": name, "display_name": "Orc Peon", "version": "1.0.0", "license": "CC-BY-NC-4.0", "categories": cats])
    let entry = JSON(any: [
        "name": name, "display_name": "Orc Peon", "source_repo": "PeonPing/og-packs", "source_ref": "v1.0.0", "source_path": path,
        "manifest_sha256": sha(manifest), "license": "CC-BY-NC-4.0", "trust_tier": "official", "sound_count": 2, "total_size_bytes": 100,
        "categories": Array(sounds.keys),
    ])
    return (RegistryPack(entry)!, manifest)
}

/// A fetch that serves these files by URL, and fails for anything else.
private func serve(_ files: [String: Data]) -> PackInstaller.Fetch {
    { url in
        guard let d = files[url.absoluteString] else { throw SoundPackError.bad("not served: \(url)") }
        return d
    }
}

private func scratch() -> URL {
    let d = FileManager.default.temporaryDirectory.appendingPathComponent("wh-sounds-\(UUID().uuidString)")
    try? FileManager.default.createDirectory(at: d, withIntermediateDirectories: true)
    return d
}

@Test func aPackKeepsItsFilesInside() {
    #expect(PackFiles.safe("sounds/PeonYes1.wav"))
    #expect(PackFiles.safe("sounds/Ah ah ! Je suis vivant.mp3"))
    #expect(PackFiles.safe("PeonYes1.wav"))
    #expect(!PackFiles.safe("../PeonYes1.wav"))
    #expect(!PackFiles.safe("sounds/../../.zshrc"))
    #expect(!PackFiles.safe("/etc/passwd"))
    #expect(!PackFiles.safe("sounds//a.wav"))
    #expect(!PackFiles.safe("sounds/./a.wav"))
    #expect(!PackFiles.safe("sounds\\a.wav"))
    #expect(!PackFiles.safe(""))
    #expect(PackFiles.validName("sc_kerrigan"))
    #expect(PackFiles.validName("7yw2-chinese-farmer"))
    #expect(!PackFiles.validName(".installing-peon"))
    #expect(!PackFiles.validName("../peon"))
    #expect(!PackFiles.validName(""))
}

@Test func onlyAudioIsKept() {
    #expect(PackFiles.isAudio(wav))
    #expect(PackFiles.isAudio(mp3))
    #expect(PackFiles.isAudio(Data([0xFF, 0xFB, 0x90, 0x64])))
    #expect(PackFiles.isAudio(Data("OggS\0\u{2}".utf8)))
    #expect(!PackFiles.isAudio(Data("<!DOCTYPE html><html>".utf8)))
    #expect(!PackFiles.isAudio(Data("404: Not Found".utf8)))
    #expect(!PackFiles.isAudio(Data()))
}

@Test func aRegistryEntryPointsAtItsPinnedSource() {
    let (p, _) = pack(["task.complete": [("sounds/PeonYes1.wav", wav)]])
    #expect(p.url("openpeon.json")?.absoluteString == "https://raw.githubusercontent.com/PeonPing/og-packs/v1.0.0/peon/openpeon.json")
    #expect(p.url("sounds/Je suis prêt.mp3")?.absoluteString == "https://raw.githubusercontent.com/PeonPing/og-packs/v1.0.0/peon/sounds/Je%20suis%20pr%C3%AAt.mp3")
    // A pack at the repo's root.
    let (root, _) = pack(["task.complete": [("a.wav", wav)]], path: ".")
    #expect(root.url("openpeon.json")?.absoluteString == "https://raw.githubusercontent.com/PeonPing/og-packs/v1.0.0/openpeon.json")
    let (empty, _) = pack(["task.complete": [("a.wav", wav)]], path: "")
    #expect(empty.url("a.wav")?.absoluteString == "https://raw.githubusercontent.com/PeonPing/og-packs/v1.0.0/a.wav")
    #expect(!p.complete)
    // An entry that doesn't pin its manifest, or reaches out of its repo, isn't listed.
    #expect(RegistryPack(JSON(any: ["name": "x", "source_repo": "a/b", "source_ref": "v1"])) == nil)
    #expect(RegistryPack(JSON(any: ["name": "x", "source_repo": "a/b", "source_ref": "v1", "source_path": "../c", "manifest_sha256": String(repeating: "a", count: 64)])) == nil)
    #expect(RegistryPack(JSON(any: ["name": "x", "source_repo": "a/../b", "source_ref": "v1", "manifest_sha256": String(repeating: "a", count: 64)])) == nil)
}

@Test func aManifestThatReachesOutIsRefused() throws {
    let ok = try PackManifest.parse(Data(#"{"cesp_version":"1.0","name":"p","display_name":"P","categories":{"task.complete":{"sounds":[{"file":"sounds/a.wav","label":"A"},{"file":"sounds/a.wav","label":"A again"}]},"task.error":{"sounds":[]}}}"#.utf8))
    #expect(ok.displayName == "P")
    #expect(ok.files.map(\.file) == ["sounds/a.wav"])
    #expect(throws: SoundPackError.self) { try PackManifest.parse(Data(#"{"cesp_version":"1.0","categories":{"task.complete":{"sounds":[{"file":"../../.zshrc"}]}}}"#.utf8)) }
    #expect(throws: SoundPackError.self) { try PackManifest.parse(Data(#"{"cesp_version":"2.0","categories":{"task.complete":{"sounds":[{"file":"a.wav"}]}}}"#.utf8)) }
    #expect(throws: SoundPackError.self) { try PackManifest.parse(Data(#"{"cesp_version":"1.0","categories":{}}"#.utf8)) }
    #expect(throws: SoundPackError.self) { try PackManifest.parse(Data("<html>".utf8)) }
}

@Test func installsWhatTheRegistryPinned() async throws {
    let (p, manifest) = pack(["task.complete": [("sounds/PeonYes1.wav", wav)], "input.required": [("sounds/PeonWhat1.mp3", mp3)]])
    let base = "https://raw.githubusercontent.com/PeonPing/og-packs/v1.0.0/peon/"
    let root = scratch()
    defer { try? FileManager.default.removeItem(at: root) }
    let got = try await PackInstaller.install(p, into: root, fetch: serve([
        base + "openpeon.json": manifest, base + "sounds/PeonYes1.wav": wav, base + "sounds/PeonWhat1.mp3": mp3,
    ]))
    #expect(got.dir.standardizedFileURL.path == root.appendingPathComponent("peon").standardizedFileURL.path)
    #expect(try Data(contentsOf: got.dir.appendingPathComponent("sounds/PeonYes1.wav")) == wav)
    #expect(try Data(contentsOf: got.dir.appendingPathComponent("openpeon.json")) == manifest)
    #expect(got.sounds(.done).map(\.lastPathComponent) == ["PeonYes1.wav"])
    #expect(got.sounds(.needs).map(\.lastPathComponent) == ["PeonWhat1.mp3"])
    #expect(got.sounds(.failed).isEmpty)
    #expect(try FileManager.default.contentsOfDirectory(atPath: root.path) == ["peon"])
}

@Test func refusesAPackThatIsntWhatWasPinned() async throws {
    let (p, manifest) = pack(["task.complete": [("sounds/PeonYes1.wav", wav)]])
    let base = "https://raw.githubusercontent.com/PeonPing/og-packs/v1.0.0/peon/"
    let root = scratch()
    defer { try? FileManager.default.removeItem(at: root) }
    // A manifest changed since the registry listed it.
    var changed = manifest
    changed.append(0x20)
    await #expect(throws: SoundPackError.self) {
        try await PackInstaller.install(p, into: root, fetch: serve([base + "openpeon.json": changed, base + "sounds/PeonYes1.wav": wav]))
    }
    // A sound that isn't the one the manifest hashed.
    var other = wav
    other.append(0)
    await #expect(throws: SoundPackError.self) {
        try await PackInstaller.install(p, into: root, fetch: serve([base + "openpeon.json": manifest, base + "sounds/PeonYes1.wav": other]))
    }
    // A sound that's missing.
    await #expect(throws: SoundPackError.self) {
        try await PackInstaller.install(p, into: root, fetch: serve([base + "openpeon.json": manifest]))
    }
    // Nothing kept from any of them.
    #expect(((try? FileManager.default.contentsOfDirectory(atPath: root.path)) ?? []).isEmpty)
}

@Test func refusesWhatIsntAudio() async throws {
    let page = Data("<!DOCTYPE html><title>Moved</title>".utf8)
    let (p, manifest) = pack(["task.complete": [("sounds/PeonYes1.wav", page)]], hashes: false)
    let base = "https://raw.githubusercontent.com/PeonPing/og-packs/v1.0.0/peon/"
    let root = scratch()
    defer { try? FileManager.default.removeItem(at: root) }
    await #expect(throws: SoundPackError.self) {
        try await PackInstaller.install(p, into: root, fetch: serve([base + "openpeon.json": manifest, base + "sounds/PeonYes1.wav": page]))
    }
    #expect(((try? FileManager.default.contentsOfDirectory(atPath: root.path)) ?? []).isEmpty)
}

@Test func aNotificationPlaysWhatTheHubSays() {
    #expect(SoundCategory.of(JSON(any: ["kind": "review", "sound": "task.complete"])) == .done)
    #expect(SoundCategory.of(JSON(any: ["kind": "needs", "sound": "task.error"])) == .failed)
    #expect(SoundCategory.of(JSON(any: ["kind": "build", "sound": "input.required"])) == .needs)
    // A category Wheelhouse doesn't play.
    #expect(SoundCategory.of(JSON(any: ["kind": "needs", "sound": "user.spam"])) == nil)
    // From a hub that doesn't say: what the kind means.
    #expect(SoundCategory.of(JSON(any: ["kind": "message"])) == .needs)
    #expect(SoundCategory.of(JSON(any: ["kind": "turn"])) == .done)
    #expect(SoundCategory.of(JSON(any: ["kind": "review"])) == nil)
}

@Test func neverTheSameLineTwiceInARow() {
    for _ in 0 ..< 50 {
        #expect(SoundPacks.pick(["a", "b"], last: "a") == "b")
        #expect(["b", "c"].contains(SoundPacks.pick(["a", "b", "c"], last: "a")!))
    }
    #expect(SoundPacks.pick(["a"], last: "a") == "a")
    #expect(SoundPacks.pick([String](), last: nil) == nil)
}

/// The real registry and GitHub: `WH_SOUND_NET=1 swift test --filter installsPeon`.
/// `WH_SOUND_NET_DIR` keeps what it installed there.
@Test(.enabled(if: ProcessInfo.processInfo.environment["WH_SOUND_NET"] == "1"))
func installsPeonFromTheRealRegistry() async throws {
    let index = try await PackInstaller.fetch(URL(string: "https://peonping.github.io/registry/index.json")!)
    let peon = try #require(try JSON.parse(index)["packs"].array.compactMap(RegistryPack.init).first { $0.name == "peon" })
    let root = ProcessInfo.processInfo.environment["WH_SOUND_NET_DIR"].map { URL(fileURLWithPath: $0) } ?? scratch()
    let got = try await PackInstaller.install(peon, into: root, fetch: PackInstaller.fetch)
    #expect(!got.sounds(.done).isEmpty)
    #expect(!got.sounds(.needs).isEmpty)
    #expect(!got.sounds(.failed).isEmpty)
    // Every one of them plays on this Mac.
    for c in SoundCategory.allCases {
        for f in got.sounds(c) { #expect((try? AVAudioPlayer(contentsOf: f))?.prepareToPlay() == true, "\(f.lastPathComponent)") }
    }
    if ProcessInfo.processInfo.environment["WH_SOUND_NET_DIR"] == nil { try? FileManager.default.removeItem(at: root) }
}
