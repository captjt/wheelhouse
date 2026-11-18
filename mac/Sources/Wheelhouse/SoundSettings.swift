import SwiftUI

/// Settings › Notifications › Sound: the macOS sound (the default), or a
/// peon-ping pack's voice lines, with its volume and which notifications
/// play one.
struct SoundSection: View {
    @AppStorage("soundPack") private var pack = ""
    @AppStorage("soundVolume") private var volume = 0.6
    @State private var browsing = false
    /// Snapshots: what Browse opens searching for.
    @State private var search = ""
    private let packs = SoundPacks.shared

    var body: some View {
        let chosen = packs.installed.first { $0.name == pack }
        Section {
            Picker("Sound", selection: Binding(get: { chosen?.name ?? "" }, set: { pack = $0 })) {
                Text("macOS Default").tag("")
                if !packs.installed.isEmpty {
                    Divider()
                    ForEach(packs.installed) { p in Text(p.manifest.displayName).tag(p.name) }
                }
            }
            if let chosen {
                Slider(value: $volume, in: 0 ... 1) {
                    Text("Volume")
                } minimumValueLabel: {
                    Image(systemName: "speaker.fill").foregroundStyle(.secondary)
                } maximumValueLabel: {
                    Image(systemName: "speaker.wave.3.fill").foregroundStyle(.secondary)
                }
                ForEach(SoundCategory.allCases) { c in SoundRow(c: c, pack: chosen) }
                Text(credit(chosen))
                    .font(.caption).foregroundStyle(.secondary)
            } else {
                Text("The sound macOS plays. Or pick a sound pack: voice lines from games and films, collected by peon-ping, with one for each kind of notification.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                Button("Browse Sound Packs…") { browsing = true }
                Spacer()
                Link("What's peon-ping?", destination: URL(string: "https://www.peonping.com/")!)
                    .font(.caption)
            }
        } header: {
            Text("Sound")
        }
        .sheet(isPresented: $browsing, onDismiss: { search = "" }) { PackBrowser(query: search) }
        .onReceive(NotificationCenter.default.publisher(for: .snapshotSounds)) { n in
            search = n.object as? String ?? ""
            browsing = n.object != nil
        }
    }

    private func credit(_ p: InstalledPack) -> String {
        let by = p.manifest.author.map { " by \($0)" } ?? ""
        let license = p.manifest.license.map { " · \($0)" } ?? ""
        return "\(p.manifest.displayName)\(by)\(license). It plays in place of the macOS sound, while you're in another app."
    }
}

/// One kind of notification: whether it plays, and a line to hear.
private struct SoundRow: View {
    let c: SoundCategory
    let pack: InstalledPack
    @AppStorage private var on: Bool

    init(c: SoundCategory, pack: InstalledPack) {
        self.c = c
        self.pack = pack
        _on = AppStorage(wrappedValue: true, c.defaultsKey)
    }

    var body: some View {
        let has = !pack.sounds(c).isEmpty
        LabeledContent {
            HStack(spacing: 10) {
                Button { SoundPacks.shared.preview(c, pack: pack) } label: {
                    Image(systemName: "play.circle").font(.system(size: 15))
                }
                .buttonStyle(.borderless)
                .help("Play one")
                .disabled(!has)
                Toggle(c.label, isOn: $on).labelsHidden().toggleStyle(.switch).controlSize(.small)
                    .disabled(!has)
            }
        } label: {
            Text(c.label)
            Text(has ? c.detail : "This pack has no sound for it")
        }
    }
}

/// The registry's packs: search them, hear one, install, use, remove.
struct PackBrowser: View {
    @Environment(\.dismiss) private var dismiss
    @AppStorage("soundPack") private var chosen = ""
    @State private var query: String
    @State private var installedOnly = false
    private let packs = SoundPacks.shared

    init(query: String = "") {
        _query = State(initialValue: query)
    }

    private var shown: [RegistryPack] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return (packs.registry ?? []).filter { p in
            (!installedOnly || packs.isInstalled(p.name))
                && (q.isEmpty || ([p.displayName, p.name, p.description, p.author, p.language, language(p) ?? ""] + p.tags).contains { $0.lowercased().contains(q) })
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 10) {
                Text("Sound Packs").font(.system(size: 14, weight: .semibold))
                Text("Voice lines from games and films, collected by peon-ping. A pack plays one of its lines when something needs you, when work is done, and when it fails. The sounds belong to their owners: each pack's license says how you may use them.")
                    .font(.system(size: 12.5)).foregroundStyle(K.text2).fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 10) {
                    TextField("Search", text: $query, prompt: Text("Search packs, games, languages"))
                        .textFieldStyle(.roundedBorder)
                    Picker("Show", selection: $installedOnly) {
                        Text("All").tag(false)
                        Text("Installed").tag(true)
                    }
                    .labelsHidden().pickerStyle(.segmented).fixedSize()
                }
            }
            .padding(20)
            Divider()
            Group {
                if packs.registry == nil, let e = packs.registryError {
                    VStack(spacing: 10) {
                        Text(e).font(.system(size: 12.5)).foregroundStyle(K.error).multilineTextAlignment(.center)
                        Button("Try Again") { Task { await packs.loadRegistry(force: true) } }
                    }
                    .padding(30).frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if packs.registry == nil {
                    ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if shown.isEmpty {
                    Text(installedOnly && query.isEmpty ? "No packs installed yet." : "No pack matches “\(query)”.")
                        .font(.system(size: 12.5)).foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ScrollView {
                        LazyVStack(spacing: 0) {
                            ForEach(shown) { p in
                                PackRow(p: p, chosen: $chosen, language: language(p))
                                Divider().padding(.leading, 58)
                            }
                        }
                    }
                }
            }
            .background(K.main)
            Divider()
            HStack {
                Text(packs.registry.map { _ in "\(shown.count) of \(packs.registry?.count ?? 0) packs" } ?? " ")
                    .font(.system(size: 11.5)).foregroundStyle(.secondary)
                Spacer()
                Link("openpeon.com/packs", destination: URL(string: "https://openpeon.com/packs")!).font(.system(size: 12))
                Button("Done") { dismiss() }
                    .buttonStyle(.borderedProminent).tint(K.accent)
                    .keyboardShortcut(.defaultAction)
            }
            .padding(.horizontal, 20).padding(.vertical, 12)
        }
        .frame(width: 680, height: 600)
        .task { await packs.loadRegistry() }
    }

    private func language(_ p: RegistryPack) -> String? {
        p.language.isEmpty ? nil : Locale.current.localizedString(forIdentifier: p.language)
    }
}

private struct PackRow: View {
    let p: RegistryPack
    @Binding var chosen: String
    let language: String?
    private let packs = SoundPacks.shared

    var body: some View {
        let installed = packs.isInstalled(p.name)
        let missing = SoundCategory.allCases.filter { !p.categories.contains($0.rawValue) }.map(\.label)
        HStack(alignment: .top, spacing: 12) {
            Button { Task { await packs.preview(p) } } label: {
                Group {
                    if packs.previewing == p.name {
                        ProgressView().controlSize(.small)
                    } else {
                        Image(systemName: "play.circle.fill").font(.system(size: 22)).foregroundStyle(K.muted)
                    }
                }
                .frame(width: 26, height: 26)
            }
            .buttonStyle(.borderless)
            .help("Hear one")
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(p.displayName).font(.system(size: 13, weight: .medium)).foregroundStyle(K.text)
                    if p.trust == "official" || p.trust == "verified" { PackTag(text: p.trust == "official" ? "Official" : "Verified", color: K.muted) }
                    if installed && chosen == p.name { PackTag(text: "In use", color: K.working) }
                }
                if !p.description.isEmpty {
                    Text(p.description).font(.system(size: 12)).foregroundStyle(K.text2).lineLimit(2)
                }
                Text(facts(language)).font(.system(size: 11.5)).foregroundStyle(K.muted).lineLimit(1)
                if !missing.isEmpty {
                    Text("Silent for \(missing.joined(separator: " and "))").font(.system(size: 11.5)).foregroundStyle(K.blocked)
                }
                if let e = packs.errors[p.name] {
                    Text(e).font(.system(size: 11.5)).foregroundStyle(K.error).fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 8)
            HStack(spacing: 8) {
                if packs.installing.contains(p.name) {
                    ProgressView().controlSize(.small)
                    Text("Installing…").font(.system(size: 12)).foregroundStyle(.secondary)
                } else if installed {
                    if chosen != p.name { Button("Use") { chosen = p.name } }
                    Button("Remove") { packs.remove(p.name) }
                } else {
                    Button("Install") { Task { await packs.install(p) } }
                }
            }
            .controlSize(.small)
            .padding(.top, 2)
        }
        .padding(.horizontal, 20).padding(.vertical, 10)
    }

    private func facts(_ language: String?) -> String {
        var f: [String] = []
        if !p.author.isEmpty { f.append(p.author) }
        if let language { f.append(language) }
        if p.soundCount > 0 { f.append("\(p.soundCount) sounds") }
        if p.size > 0 { f.append(ByteCountFormatter.string(fromByteCount: Int64(p.size), countStyle: .file)) }
        f.append(p.license)
        return f.joined(separator: " · ")
    }
}

private struct PackTag: View {
    let text: String
    let color: Color

    var body: some View {
        Text(text)
            .font(.system(size: 10.5, weight: .semibold))
            .foregroundStyle(color)
            .padding(.horizontal, 5).padding(.vertical, 1)
            .overlay(Capsule().strokeBorder(color.opacity(0.5), lineWidth: 1))
    }
}

extension Notification.Name {
    static let snapshotSounds = Notification.Name("wheelhouse.snapshotSounds")
}
