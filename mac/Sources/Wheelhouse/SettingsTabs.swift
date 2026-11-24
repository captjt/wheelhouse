import AppKit
import SwiftUI

// Settings' tabs for what Wheelhouse does on its own: standing orders and
// gates, agents, repos, and the guides and briefs agents are given.

// ---- small pieces ----

/// "Set here", with the way back to the Lua's value. Nothing when the value
/// comes from the Lua or the default.
struct SetHere: View {
    let part: String
    let path: [String]
    let key: String
    @Environment(SettingsModel.self) private var model

    var body: some View {
        if model.isSet(part, path) {
            HStack(spacing: 6) {
                Text("Set here").foregroundStyle(K.muted)
                Text("·").foregroundStyle(K.faint)
                Button(model.revertLabel(part)) {
                    Task { await model.change(key, SettingsPatch.make(part, path, nil)) }
                }
                .buttonStyle(.link)
                .disabled(model.isBusy(key))
                .help("Take this out of settings.json: \(model.luaExists(part) ? "\(model.lua(part))'s value" : "the default") applies again")
            }
            .font(.system(size: 11.5))
        }
    }
}

/// The hub's refusal, beside what caused it.
struct ErrorLine: View {
    let key: String
    @Environment(SettingsModel.self) private var model
    var body: some View {
        if let e = model.errors[key] {
            Label(e, systemImage: "exclamationmark.triangle.fill")
                .font(.system(size: 11.5))
                .foregroundStyle(K.error)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
        }
    }
}

/// A control's label with the sentence in force under it.
struct Described: View {
    let title: String
    let text: String
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
            Text(inlineMarkdown(text)).font(.system(size: 11.5)).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// Where these settings live, and what stays in the Lua.
struct WhereItLives: View {
    @Environment(SettingsModel.self) private var model
    var body: some View {
        Section {
            VStack(alignment: .leading, spacing: 6) {
                Text("Saved in \(model.view?["file"].str ?? "settings.json"), laid over \(model.lua("build")) and \(model.lua("review")). A value set here wins over theirs, and \(model.revertLabel("build")) takes it back out. Whatever a save replaces is kept in \(model.view?["history"].str ?? "the history").")
                Text("Agents' programs and arguments, PATH, caches, the wall and effort functions stay in the Lua: edit those there.")
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(["build", "review"], id: \.self) { p in
                        if let path = model.view?[p]["lua"].text {
                            Text("\(model.lua(p)): \(shortPath(path))\(model.luaExists(p) ? "" : " (not there)")").font(.system(size: 11, design: .monospaced))
                        }
                    }
                }
                .padding(.top, 2)
            }
            .font(.system(size: 11.5))
            .foregroundStyle(.secondary)
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// The configs' own errors, at the top of a tab: they stop what's below.
struct ConfigProblems: View {
    @Environment(SettingsModel.self) private var model
    var body: some View {
        let problems: [(String, String)] = [
            model.view?["set_error"].text.map { ("settings.json", $0) },
            model.view?["build"]["error"].text.map { (model.lua("build"), $0) },
            model.view?["review"]["error"].text.map { (model.lua("review"), $0) },
        ].compactMap { $0 }
        if let e = model.loadError {
            Section { Label("Can't read the settings: \(e)", systemImage: "exclamationmark.triangle.fill").foregroundStyle(K.error) }
        }
        if !problems.isEmpty {
            Section {
                ForEach(problems, id: \.0) { file, e in
                    Label { Text("\(file) doesn't load: \(e)") } icon: { Image(systemName: "exclamationmark.triangle.fill") }
                        .foregroundStyle(K.error).font(.system(size: 12)).textSelection(.enabled)
                }
            }
        }
    }
}

// ---- standing orders ----

struct OrdersSettings: View {
    @Environment(SettingsModel.self) private var model
    @Environment(Store.self) private var store

    var body: some View {
        let rv = store.state?.review
        let bs = store.state?.build
        Form {
            ConfigProblems()
            Section {
                // One cell, divided by hand: a grouped Form sets a row it
                // can't place (the last of a ForEach) out under the footer.
                VStack(alignment: .leading, spacing: 10) {
                    if model.orders.isEmpty {
                        Text(model.view == nil ? "Loading…" : "None: Review and Build aren't set up yet (review.lua, build.lua).").foregroundStyle(.secondary)
                    }
                    ForEach(Array(model.orders.enumerated()), id: \.offset) { i, o in
                        if i > 0 { Divider() }
                        OrderRow(o: o)
                    }
                }
            } header: {
                Text("Standing orders")
            } footer: {
                Text("What Wheelhouse does on its own. Each is worked out from the settings in these tabs, so it says what will happen.")
                    .font(.system(size: 11.5)).foregroundStyle(.secondary)
            }
            if model.review.exists {
                Section("Review") {
                    if rv?.enabled == false {
                        LabeledContent {
                            Button("Turn On Reviews") { Task { await model.toggle("review.enabled", "/api/review/settings", ["enabled": true]) } }
                                .disabled(rv?.rexReviewerRunning == true)
                        } label: {
                            Described(title: "Reviews are off", text: rv?.rexReviewerRunning == true ? "rex-reviewer's watcher is still running: stop it first." : "They start paused, until you resume them.")
                        }
                    } else {
                        Toggle(isOn: Binding(get: { !(rv?.paused ?? false) }, set: { on in
                            Task { await model.toggle("review.paused", "/api/review/settings", ["paused": !on]) }
                        })) {
                            Described(title: "Take on review requests", text: rv?.paused == true ? "Paused: no new rounds start." : "A round starts when a PR requests you.")
                        }
                        .disabled(model.isBusy("review.paused"))
                        ErrorLine(key: "review.paused")
                    }
                    // A rule is config: worth setting before Reviews are on, too.
                    PostingPicker(slug: nil)
                }
            }
            if model.build.exists {
                Section("Build") {
                    Toggle(isOn: Binding(get: { !(bs?.paused ?? false) }, set: { on in
                        Task { await model.toggle("build.paused", "/api/build/settings", ["paused": !on]) }
                    })) {
                        Described(title: "Start Builds", text: bs?.paused == true ? "Paused: queued Builds wait, and nothing new starts." : "Queued Builds start as there's room.")
                    }
                    .disabled(model.isBusy("build.paused"))
                    ErrorLine(key: "build.paused")
                }
                Section {
                    ForEach(Array(model.build["gates"].array.enumerated()), id: \.offset) { _, g in GateRow(g: g) }
                } header: {
                    Text("Gates")
                } footer: {
                    Text("At a gate set to Ask, a Build stops and asks you in the Inbox. Auto goes ahead, and the record says so.")
                        .font(.system(size: 11.5)).foregroundStyle(.secondary)
                }
            }
            WhereItLives()
        }
        .formStyle(.grouped)
        .frame(width: 640, height: 680)
        .task { await model.load() }
    }
}

struct OrderRow: View {
    let o: JSON
    var body: some View {
        let proto = o["protocol"].str ?? ""
        let state = !o["on"].bool ? "Off" : o["paused"].bool ? "Paused" : "On"
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: proto == "review" ? "arrow.triangle.pull" : proto == "build" ? "hammer" : "checkmark.shield")
                .foregroundStyle(K.muted).frame(width: 18).padding(.top, 1)
            Text(inlineMarkdown(o["text"].str ?? ""))
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
            Spacer(minLength: 8)
            Chip(text: state, color: state == "On" ? K.working : state == "Paused" ? K.blocked : K.faint)
                .help(state == "Paused" ? "Paused: resume it below" : state == "Off" ? "Turned off" : "In force")
        }
        .padding(.vertical, 2)
    }
}

struct GateRow: View {
    let g: JSON
    @Environment(SettingsModel.self) private var model

    var body: some View {
        let key = g["key"].str ?? ""
        let value = g["value"].str ?? "ask"
        VStack(alignment: .leading, spacing: 4) {
            Picker(selection: Binding(get: { value }, set: { v in
                guard v != value else { return }
                Task { await model.change("gates.\(key)", SettingsPatch.make("build", ["gates", key], v)) }
            })) {
                Text("Ask").tag("ask")
                Text("Auto").tag("auto")
            } label: {
                Described(title: g["label"].str ?? key, text: value == "auto" ? g["auto"].str ?? "" : g["ask"].str ?? "")
            }
            .pickerStyle(.segmented)
            .fixedSize()
            .disabled(model.isBusy("gates.\(key)"))
            SetHere(part: "build", path: ["gates", key], key: "gates.\(key)")
            ErrorLine(key: "gates.\(key)")
        }
    }
}

// ---- runtimes ----

/// Where new agents open (Rex or Herdr), and whether each runtime is
/// followed. The hub follows every one that's installed, whatever this says.
struct RuntimeSection: View {
    @Environment(SettingsModel.self) private var model

    var body: some View {
        let rt = model.view?["runtime"] ?? .null
        let all = rt["runtimes"].array.map(RuntimeLink.init)
        if !all.isEmpty {
            Section {
                VStack(alignment: .leading, spacing: 4) {
                    Picker(selection: Binding(get: { rt["value"].str ?? "rex" }, set: { v in
                        Task { await model.change("runtime", ["runtime": v]) }
                    })) {
                        ForEach(all, id: \.name) { r in Text(r.title).tag(r.name) }
                    } label: {
                        Described(title: "New agents open in", text: rt["env"].text.map { "WH_RUNTIME=\($0) decides this for now." } ?? "wh start, Build and Review open their panes here. Agents already running stay where they are.")
                    }
                    .pickerStyle(.segmented).fixedSize()
                    .disabled(rt["env"].text != nil || model.busy.contains("runtime"))
                    ErrorLine(key: "runtime")
                }
                ForEach(all, id: \.name) { r in
                    LabeledContent(r.title) {
                        Text(r.connected ? "Following, \(r.agents) agent\(r.agents == 1 ? "" : "s")" : !r.installed ? "Not installed" : "Not connected: \(r.error ?? "connecting")")
                            .foregroundStyle(r.connected || !r.installed ? .secondary : K.blocked)
                    }
                }
            } header: {
                Text("Runtimes")
            }
        }
    }
}

// ---- agents ----

struct AgentsSettings: View {
    @Environment(SettingsModel.self) private var model

    var body: some View {
        let b = model.build
        let r = model.review
        Form {
            ConfigProblems()
            RuntimeSection()
            if b.exists {
                Section {
                    agent("Builder", "builder", "Writes the plan, then builds it.")
                    agent("Checker", "checker", "Judges the branch against the intent and the plan.")
                    prereview(b)
                    if !b["prereview"].strings.isEmpty {
                        stepper("Pre-review rounds", "prereview_loops", value: b["prereview_loops"].int ?? 1, range: 1 ... 5, path: ["prereview", "loops"],
                                text: "How many times the blind reviewers look again after fixes.")
                    }
                    stepper("Fix rounds", "fix_loops", value: b["fix_loops"].int ?? 2, range: 0 ... 6, path: ["fix_loops"],
                            text: "How often the checker, or a failing check, can send the work back before it asks you.")
                    stepper("Builds at once", "max_tasks", value: b["max_tasks"].int ?? 2, range: 1 ... 8, path: ["max_tasks"],
                            text: "More wait in the queue.")
                } header: {
                    Text("Build")
                }
                Section("Pull requests") {
                    VStack(alignment: .leading, spacing: 4) {
                        Toggle(isOn: Binding(get: { b["pr"]["draft"].bool }, set: { v in
                            Task { await model.change("pr.draft", SettingsPatch.make("build", ["pr", "draft"], v)) }
                        })) {
                            Described(title: "Open PRs as drafts", text: b["pr"]["draft"].bool ? "You mark each ready for review on GitHub." : "Each opens ready for review.")
                        }
                        SetHere(part: "build", path: ["pr", "draft"], key: "pr.draft")
                        ErrorLine(key: "pr.draft")
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        Picker(selection: Binding(get: { b["update"].str ?? "merge" }, set: { v in
                            Task { await model.change("update", SettingsPatch.make("build", ["update"], v)) }
                        })) {
                            Text("Merge").tag("merge")
                            Text("Rebase").tag("rebase")
                        } label: {
                            Described(title: "Bring in the base", text: b["update"].str == "rebase" ? "Rebase the branch onto it, and force-push with lease." : "Merge it into the branch: no force-push.")
                        }
                        .pickerStyle(.segmented).fixedSize()
                        SetHere(part: "build", path: ["update"], key: "update")
                        ErrorLine(key: "update")
                    }
                }
            }
            if r.exists {
                Section {
                    LabeledContent {
                        Text(r["reviewers"].array.map { "\($0["name"].str ?? "") (\(appName($0["agent"].str)))" }.joined(separator: ", "))
                            .multilineTextAlignment(.trailing)
                    } label: {
                        Described(title: "Reviewers", text: "Each reviews blind, in its own pane. Their names, agents and effort are in \(model.lua("review")).")
                    }
                    agent("Checker", "checker", "Merges the reviewers' drafts into one review, and rates the merge risk.", part: "review")
                    stepper("Reviews at once", "max_rounds", value: r["max_rounds"].int ?? 3, range: 1 ... 8, path: ["max_rounds"], text: "More wait in the queue.", part: "review")
                } header: {
                    Text("Review")
                }
            }
            WhereItLives()
        }
        .formStyle(.grouped)
        .frame(width: 600, height: 680)
        .task { await model.load() }
    }

    private func agent(_ title: String, _ field: String, _ text: String, part: String = "build") -> some View {
        let cfg = part == "build" ? model.build : model.review
        let key = "\(part).\(field)"
        return VStack(alignment: .leading, spacing: 4) {
            Picker(selection: Binding(get: { cfg[field].str ?? cfg[field]["agent"].str ?? "claude" }, set: { v in
                // As {agent}: whatever else the Lua gives it (effort, args) stays.
                Task { await model.change(key, SettingsPatch.make(part, [field], ["agent": v])) }
            })) {
                Text("Claude Code").tag("claude")
                Text("Codex").tag("codex")
                Text("Antigravity").tag("agy")
            } label: {
                Described(title: title, text: text)
            }
            .fixedSize()
            .disabled(model.isBusy(key))
            SetHere(part: part, path: [field], key: key)
            ErrorLine(key: key)
        }
    }

    private func prereview(_ b: JSON) -> some View {
        let now = b["prereview"].strings
        let all = ["claude", "codex", "agy"]
        return VStack(alignment: .leading, spacing: 4) {
            LabeledContent {
                HStack(spacing: 6) {
                    ForEach(all, id: \.self) { a in
                        let on = now.contains(a)
                        Toggle(appName(a), isOn: Binding(get: { on }, set: { active in
                            let next = all.filter { $0 == a ? active : now.contains($0) }
                            Task { await model.change("prereview", SettingsPatch.make("build", ["prereview", "reviewers"], next)) }
                        }))
                        .toggleStyle(.button)
                        .disabled(model.isBusy("prereview"))
                    }
                }
            } label: {
                Described(title: "Pre-review", text: "Blind reviewers read the branch before the PR opens, and the builder fixes what they find.")
            }
            SetHere(part: "build", path: ["prereview"], key: "prereview")
            ErrorLine(key: "prereview")
        }
    }

    private func stepper(_ title: String, _ key: String, value: Int, range: ClosedRange<Int>, path: [String], text: String, part: String = "build") -> some View {
        let k = "\(part).\(key)"
        return VStack(alignment: .leading, spacing: 4) {
            LabeledContent {
                Stepper(value: Binding(get: { value }, set: { v in
                    Task { await model.change(k, SettingsPatch.make(part, path, v)) }
                }), in: range) {
                    Text("\(value)").monospacedDigit()
                }
                .fixedSize()
                .disabled(model.isBusy(k))
            } label: {
                Described(title: title, text: text)
            }
            SetHere(part: part, path: path, key: k)
            ErrorLine(key: k)
        }
    }
}

// ---- repos ----

struct ReposSettings: View {
    @Environment(SettingsModel.self) private var model
    @AppStorage("settingsRepo") private var selected = ""

    var body: some View {
        let slugs = Array(Set(Array((model.build["repos"].object ?? [:]).keys) + Array((model.review["repos"].object ?? [:]).keys))).sorted()
        let current = slugs.contains(selected) ? selected : slugs.first
        HStack(spacing: 0) {
            List(selection: Binding<String?>(get: { current }, set: { if let v = $0 { selected = v } })) {
                Section("Repos") {
                    ForEach(slugs, id: \.self) { s in
                        Label(s, systemImage: "folder").tag(s)
                    }
                }
            }
            .listStyle(.sidebar)
            .frame(width: 210)
            Divider()
            Group {
                if let current {
                    RepoForm(slug: current).id(current)
                } else if model.view != nil {
                    ContentUnavailableView("No repos yet", systemImage: "folder", description: Text("Add each repo to build.lua or review.lua with its checkout. Then its notes, checks and review guide are set here."))
                } else {
                    ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(width: 860, height: 680)
        .task { await model.load() }
    }
}

struct CheckSpec: Hashable, Identifiable {
    var id: String { name }
    var name: String
    var run: String
    var required: Bool
    var timeout: Int
}

struct RepoForm: View {
    let slug: String
    @Environment(SettingsModel.self) private var model
    @State private var notes = ""
    @State private var base = ""
    @State private var loaded = false
    @State private var editing: CheckSpec?
    @State private var adding = false
    @State private var selectedCheck: String?
    @FocusState private var baseFocused: Bool

    private var b: JSON { model.build["repos"][slug] }
    private var r: JSON { model.review["repos"][slug] }
    private var inBuild: Bool { b.exists }
    private var inReview: Bool { r.exists }
    private var savedNotes: String { b["notes"].str ?? r["notes"].str ?? "" }
    private var checks: [CheckSpec] {
        b["checks"].array.map { CheckSpec(name: $0["name"].str ?? "", run: $0["run"].str ?? "", required: $0["required"].exists ? $0["required"].bool : true, timeout: $0["timeout"].int ?? 1200) }
    }

    var body: some View {
        ScrollViewReader { proxy in
            form.onReceive(NotificationCenter.default.publisher(for: .snapshotScroll)) { n in
                if let to = n.object as? String { proxy.scrollTo(to, anchor: .top) }
            }
        }
    }

    private var form: some View {
        Form {
            ConfigProblems()
            Section {
                LabeledContent("Checkout") {
                    Text(b["checkout"].str ?? r["checkout"].str ?? "").font(.system(size: 12, design: .monospaced)).textSelection(.enabled)
                }
            } header: {
                Text(slug)
            } footer: {
                Text("Where Builds make their worktrees and reviewers read the code. It's set in the Lua.").font(.system(size: 11.5)).foregroundStyle(.secondary)
            }
            Section {
                VStack(alignment: .leading, spacing: 8) {
                    Editor(text: $notes, placeholder: "Rules agents follow in this repo. For example: Run `just check` before you commit. Don't touch the generated client.", minHeight: 76, maxHeight: 200, onSubmit: saveNotes)
                        .padding(7)
                        .background(K.main, in: RoundedRectangle(cornerRadius: 7))
                        .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(K.line2))
                    HStack(spacing: 8) {
                        if model.isSet("build", ["repos", slug, "notes"]) || model.isSet("review", ["repos", slug, "notes"]) {
                            HStack(spacing: 6) {
                                Text("Set here").foregroundStyle(K.muted)
                                Text("·").foregroundStyle(K.faint)
                                Button(model.luaExists("build") || model.luaExists("review") ? "Use the Lua's" : "Use the default") { revertNotes() }
                                    .buttonStyle(.link)
                                    .disabled(model.isBusy("notes"))
                            }
                            .font(.system(size: 11.5))
                        }
                        Spacer()
                        if notes != savedNotes {
                            Button("Revert") { notes = savedNotes }.controlSize(.small)
                        }
                        Button("Save Notes") { saveNotes() }
                            .controlSize(.small)
                            .keyboardShortcut("s", modifiers: .command)
                            .disabled(notes == savedNotes || model.isBusy("notes"))
                    }
                    ErrorLine(key: "notes")
                }
            } header: {
                Text("Notes for agents")
            } footer: {
                Text("This repo's rules. Builders, checkers and reviewers here read them before they start.").font(.system(size: 11.5)).foregroundStyle(.secondary)
            }
            if inBuild {
                Section("Pull requests") {
                    VStack(alignment: .leading, spacing: 4) {
                        LabeledContent {
                            TextField("Base branch", text: $base, prompt: Text("the default branch"))
                                .labelsHidden()
                                .textFieldStyle(.roundedBorder)
                                .font(.system(size: 12.5, design: .monospaced))
                                .frame(width: 200)
                                .focused($baseFocused)
                                .onSubmit(saveBase)
                                .onChange(of: baseFocused) { _, f in if !f { saveBase() } }
                        } label: {
                            Described(title: "Base branch", text: "Builds branch from it, and their PRs go into it.")
                        }
                        SetHere(part: "build", path: ["repos", slug, "base"], key: "base")
                        ErrorLine(key: "base")
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        let plan = b["plan"].str ?? "full"
                        Picker(selection: Binding(get: { plan }, set: { v in
                            Task { await model.change("plan", SettingsPatch.make("build", ["repos", slug, "plan"], v == "off" ? false : v)) }
                        })) {
                            Text("Full").tag("full")
                            Text("Summary").tag("summary")
                            Text("Off").tag("off")
                        } label: {
                            Described(title: "The plan on the PR", text: [
                                "full": "Its intent, acceptance, out of scope and notes go on the PR as they are.",
                                "summary": "In the builder's own words, without quoting the issue: for a public repo with a private tracker.",
                                "off": "No plan on the PR. The builder still writes one, for you and the checker.",
                            ][plan] ?? "")
                        }
                        .pickerStyle(.segmented).fixedSize()
                        SetHere(part: "build", path: ["repos", slug, "plan"], key: "plan")
                        ErrorLine(key: "plan")
                    }
                }
                Section {
                    ChecksTable(slug: slug, checks: checks, selected: $selectedCheck, edit: { editing = $0 }, add: { adding = true })
                } header: {
                    Text("Witnessed checks")
                } footer: {
                    Text("Wheelhouse runs these itself, at a Build's commit, before its PR opens or a round pushes, and when you press Run Checks on a docket in this repo. A required one that fails holds the PR; an optional one only informs.")
                        .font(.system(size: 11.5)).foregroundStyle(.secondary)
                }
            }
            if inReview {
                ReviewGuidePicker(slug: slug).id("posting")
            }
            WhereItLives()
        }
        .formStyle(.grouped)
        .onAppear {
            guard !loaded else { return }
            notes = savedNotes
            base = b["base"].str ?? ""
            loaded = true
        }
        .onChange(of: savedNotes) { old, new in if notes == old { notes = new } }
        .onChange(of: b["base"].str) { _, new in if !baseFocused { base = new ?? "" } }
        .sheet(item: $editing) { c in CheckEditor(slug: slug, existing: c, taken: checks.map(\.name)) }
        .sheet(isPresented: $adding) { CheckEditor(slug: slug, existing: nil, taken: checks.map(\.name)) }
        .onReceive(NotificationCenter.default.publisher(for: .snapshotCheck)) { n in
            editing = checks.first { $0.name == n.object as? String }
        }
    }

    private func saveNotes() {
        let t = notes.trimmingCharacters(in: .whitespacesAndNewlines)
        guard t != savedNotes.trimmingCharacters(in: .whitespacesAndNewlines) else { return }
        var patch: [String: Any] = [:]
        // Both protocols' agents read the repo's notes.
        if inBuild || model.luaExists("build") { patch = SettingsPatch.make("build", ["repos", slug, "notes"], t) }
        if inReview, model.luaExists("review") { patch = SettingsPatch.merge(patch, SettingsPatch.make("review", ["repos", slug, "notes"], t)) }
        Task { if await model.change("notes", patch) { notes = t } }
    }

    private func revertNotes() {
        var patch: [String: Any] = [:]
        for part in ["build", "review"] where model.isSet(part, ["repos", slug, "notes"]) {
            patch = SettingsPatch.merge(patch, SettingsPatch.make(part, ["repos", slug, "notes"], nil))
        }
        Task { await model.change("notes", patch) }
    }

    private func saveBase() {
        let t = base.trimmingCharacters(in: .whitespacesAndNewlines)
        guard t != (b["base"].str ?? "") else { return }
        Task { await model.change("base", SettingsPatch.make("build", ["repos", slug, "base"], t.isEmpty ? nil : t)) }
    }
}

/// A repo's checks: name, command, required, timeout; add, edit, remove.
struct ChecksTable: View {
    let slug: String
    let checks: [CheckSpec]
    @Binding var selected: String?
    let edit: (CheckSpec) -> Void
    let add: () -> Void
    @Environment(SettingsModel.self) private var model

    private func setHere(_ name: String) -> Bool { model.isSet("build", ["repos", slug, "checks", name]) }

    var body: some View {
        let key = "checks"
        let sel = checks.first { $0.name == selected }
        VStack(alignment: .leading, spacing: 8) {
            VStack(spacing: 0) {
                HStack(spacing: 10) {
                    Text("Name").frame(width: 90, alignment: .leading)
                    Text("Command").frame(maxWidth: .infinity, alignment: .leading)
                    Text("Required").frame(width: 62, alignment: .leading)
                    Text("Timeout").frame(width: 56, alignment: .trailing)
                }
                .font(.system(size: 11, weight: .medium)).foregroundStyle(.secondary)
                .padding(.horizontal, 10).padding(.vertical, 6)
                Divider()
                if checks.isEmpty {
                    Text("No checks yet. Add the commands that show this repo's work is sound: its tests, its linter.")
                        .font(.system(size: 12)).foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(10)
                }
                ForEach(checks) { c in
                    HStack(spacing: 10) {
                        Text(c.name).font(.system(size: 12, weight: .medium, design: .monospaced)).lineLimit(1).frame(width: 90, alignment: .leading)
                        HStack(spacing: 6) {
                            Text(c.run).font(.system(size: 12, design: .monospaced)).lineLimit(1).truncationMode(.middle).help(c.run)
                            if setHere(c.name) { Text("set here").font(.system(size: 10.5)).foregroundStyle(K.muted) }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)
                        Text(c.required ? "Required" : "Optional").font(.system(size: 12)).foregroundStyle(c.required ? K.text : K.muted).frame(width: 62, alignment: .leading)
                        Text(c.timeout % 60 == 0 ? "\(c.timeout / 60) min" : "\(c.timeout) s").font(.system(size: 12)).monospacedDigit().foregroundStyle(K.text2).frame(width: 56, alignment: .trailing)
                    }
                    .padding(.horizontal, 10).padding(.vertical, 6)
                    .background(selected == c.name ? K.accent.opacity(0.16) : .clear)
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2) { edit(c) }
                    .onTapGesture { selected = c.name }
                    .contextMenu {
                        Button("Edit…") { edit(c) }
                        Button(setHere(c.name) ? "Remove from Settings" : "Remove") { remove(c) }
                    }
                    if c.name != checks.last?.name { Divider().opacity(0.5) }
                }
            }
            .background(K.main, in: RoundedRectangle(cornerRadius: 7))
            .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(K.line2))
            HStack(spacing: 6) {
                ControlGroup {
                    Button { add() } label: { Image(systemName: "plus") }.help("Add a check")
                    Button { if let sel { remove(sel) } } label: { Image(systemName: "minus") }
                        .disabled(sel == nil)
                        .help(sel.map { setHere($0.name) ? "Remove \($0.name) from Settings" : "Turn \($0.name) off: \(model.lua("build")) lists it, and Settings keeps it off" } ?? "Select a check")
                }
                .controlSize(.small)
                .fixedSize()
                Button("Edit…") { if let sel { edit(sel) } }.controlSize(.small).disabled(sel == nil)
                Spacer()
                if let sel, !setHere(sel.name) {
                    Text("From \(model.lua("build")): editing it sets it here.").font(.system(size: 11.5)).foregroundStyle(.secondary)
                } else if let note = model.notes[key] {
                    Text(note).font(.system(size: 11.5)).foregroundStyle(.secondary)
                }
            }
            ErrorLine(key: key)
        }
    }

    private func remove(_ c: CheckSpec) {
        guard setHere(c.name) else {
            // build.lua lists it: `false` here keeps it off (settings.json wins).
            Task {
                if await model.change("checks", SettingsPatch.make("build", ["repos", slug, "checks", c.name], false)) {
                    model.notes["checks"] = "Removed \(c.name). \(model.lua("build")) still lists it; Settings keeps it off."
                    selected = nil
                }
            }
            return
        }
        Task {
            if await model.change("checks", SettingsPatch.make("build", ["repos", slug, "checks", c.name], nil)) {
                // Removing what's set here brings back the Lua's check of that name, if it has one.
                let back = model.build["repos"][slug]["checks"].array.contains { $0["name"].str == c.name }
                model.notes["checks"] = back ? "\(model.lua("build"))'s \(c.name) applies again." : "Removed \(c.name)."
                if !back { selected = nil }
            }
        }
    }
}

/// Add or edit a check: its command, whether it's required, and how long it may run.
struct CheckEditor: View {
    let slug: String
    let existing: CheckSpec?
    let taken: [String]
    @Environment(SettingsModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var run = ""
    @State private var required = true
    @State private var minutes = 20

    var body: some View {
        let n = name.trimmingCharacters(in: .whitespaces)
        let nameProblem: String? = n.isEmpty ? nil : !SettingsPatch.validCheckName(n) ? "Letters, digits, dashes and underscores" : existing == nil && taken.contains(n) ? "There's a check by that name already" : nil
        VStack(alignment: .leading, spacing: 0) {
            Text(existing == nil ? "Add a check to \(slug)" : "Edit \(existing?.name ?? "")").font(.system(size: 14, weight: .semibold)).padding([.horizontal, .top], 20)
            Form {
                TextField("Name", text: $name, prompt: Text("test"))
                    .disabled(existing != nil)
                    .font(.system(size: 13, design: .monospaced))
                if let nameProblem { Text(nameProblem).font(.system(size: 11.5)).foregroundStyle(K.error) }
                TextField("Command", text: $run, prompt: Text("pnpm test"))
                    .font(.system(size: 13, design: .monospaced))
                Toggle(isOn: $required) {
                    Described(title: "Required", text: required ? "If it fails, the PR doesn't open and a round doesn't push." : "It only informs: a failure holds nothing.")
                }
                LabeledContent("Timeout") {
                    Stepper(value: $minutes, in: 1 ... 120) { Text("\(minutes) min").monospacedDigit() }.fixedSize()
                }
                ErrorLine(key: "check.edit")
            }
            .formStyle(.grouped)
            .scrollDisabled(true)
            Text("It runs in the docket's worktree or folder, with your login shell, behind the same wall as the agents.")
                .font(.system(size: 11.5)).foregroundStyle(.secondary).padding(.horizontal, 20)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(existing == nil ? "Add Check" : "Save") { save() }
                    .buttonStyle(.borderedProminent).tint(K.accent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(n.isEmpty || nameProblem != nil || run.trimmingCharacters(in: .whitespaces).isEmpty || model.isBusy("check.edit"))
            }
            .padding(20)
        }
        .frame(width: 480)
        .onAppear {
            model.errors["check.edit"] = nil
            if let e = existing {
                name = e.name
                run = e.run
                required = e.required
                minutes = max(1, Int((Double(e.timeout) / 60).rounded(.up)))
            }
        }
    }

    private func save() {
        let n = name.trimmingCharacters(in: .whitespaces)
        Task {
            if await model.change("check.edit", SettingsPatch.make("build", ["repos", slug, "checks", n], SettingsPatch.check(run: run, required: required, timeout: minutes * 60))) {
                model.notes["checks"] = nil
                dismiss()
            }
        }
    }
}

// ---- who posts a checked review ----

/// Who posts a checked review, for every repo or (with a slug) one: Ask,
/// Auto, or When… (a rule: it posts itself when every condition holds,
/// and otherwise waits for you, saying why). docs/review.md#who-posts.
struct PostingPicker: View {
    let slug: String?
    @Environment(SettingsModel.self) private var model
    @State private var editing = false

    private var rule: JSON { slug.map { model.review["repos"][$0]["post"] } ?? model.review["post_rule"] }
    private var path: [String] { slug.map { ["repos", $0, "post"] } ?? ["post"] }
    private var key: String { slug.map { "repo.\($0).post" } ?? "review.post" }

    var body: some View {
        let mode = rule.exists ? (rule["mode"].str ?? "ask") : "same"
        let sentence = rule["fn"].text.map { "`\($0)` in review.lua decides, for each checked review. Pick Ask, Auto or When… to set it here instead." }
            ?? (rule.exists ? "A checked review \(rule["text"].str ?? "")." : "As every repo: a checked review \(model.review["post_rule"]["text"].str ?? "waits for you").")
        VStack(alignment: .leading, spacing: 4) {
            Picker(selection: Binding(get: { mode }, set: { pick($0) })) {
                if slug != nil { Text("Same as All").tag("same") }
                Text("Ask").tag("ask")
                Text("Auto").tag("auto")
                Text("When…").tag("rule")
            } label: {
                Described(title: "Posting", text: sentence)
            }
            .pickerStyle(.segmented)
            .fixedSize()
            .disabled(model.isBusy(key))
            if mode == "rule", rule["auto"].exists {
                Button("Edit the Rule…") { editing = true }.buttonStyle(.link).font(.system(size: 11.5))
            }
            if slug == nil, let o = model.review["post_override"].text {
                HStack(spacing: 6) {
                    Text("`wh review post-mode \(o)` is on: every repo \(o == "auto" ? "posts itself" : "waits for you") until you change posting here.")
                        .foregroundStyle(.secondary)
                    Button("Follow These Settings") { Task { await model.toggle(key, "/api/review/settings", ["post": "config"]) } }
                        .buttonStyle(.link)
                        .disabled(model.isBusy(key))
                }
                .font(.system(size: 11.5))
            }
            SetHere(part: "review", path: path, key: key)
            ErrorLine(key: key)
        }
        .sheet(isPresented: $editing) { RuleEditor(slug: slug, rule: rule["mode"].str == "rule" ? rule["auto"] : .null) }
        .onReceive(NotificationCenter.default.publisher(for: .snapshotRule)) { n in
            if (n.object as? String) == (slug ?? "") { editing = true }
        }
    }

    private func pick(_ v: String) {
        switch v {
        case "rule": editing = true
        case "same": Task { await model.change(key, SettingsPatch.make("review", path, nil)) }
        default: Task { await model.change(key, SettingsPatch.make("review", path, v)) }
        }
    }
}

/// A posting rule's conditions: every one set must hold for a checked
/// review to post itself.
struct RuleEditor: View {
    let slug: String?
    let rule: JSON
    @Environment(SettingsModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var approve = true
    @State private var risk = "low"
    @State private var maxLines = ""
    @State private var authors = ""
    @State private var labels = ""
    @State private var notTouching = ""
    @State private var problem: String?

    private var key: String { "rule.edit" }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(slug.map { "When a review in \($0) posts itself" } ?? "When a review posts itself").font(.system(size: 14, weight: .semibold)).padding([.horizontal, .top], 20)
            Form {
                Picker("Verdict", selection: $approve) {
                    Text("Approves").tag(true)
                    Text("Any").tag(false)
                }
                .pickerStyle(.segmented)
                Picker("Merge risk", selection: $risk) {
                    Text("Low").tag("low")
                    Text("Medium or Lower").tag("medium")
                    Text("Any").tag("")
                }
                .pickerStyle(.segmented)
                TextField("At most, lines", text: $maxLines, prompt: Text("any"))
                TextField("By one of", text: $authors, prompt: Text("anyone"))
                TextField("Labeled one of", text: $labels, prompt: Text("any labels"))
                TextField("Not touching", text: $notTouching, prompt: Text("db/migrations/, auth/"))
                    .font(.system(size: 13, design: .monospaced))
                if let problem { Text(problem).font(.system(size: 11.5)).foregroundStyle(K.error) }
                ErrorLine(key: key)
            }
            .formStyle(.grouped)
            .scrollDisabled(true)
            Text("It posts itself only when every condition holds. Otherwise it waits for you, and the Inbox says why. Lists are comma-separated.")
                .font(.system(size: 11.5)).foregroundStyle(.secondary).padding(.horizontal, 20)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save") { save() }
                    .buttonStyle(.borderedProminent).tint(K.accent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(model.isBusy(key))
            }
            .padding(20)
        }
        .frame(width: 480)
        .onAppear {
            model.errors[key] = nil
            guard rule.exists else { return }
            approve = rule["verdict"].str == "approve"
            risk = rule["merge_risk"].str ?? ""
            maxLines = rule["max_lines"].int.map(String.init) ?? ""
            authors = rule["authors"].strings.joined(separator: ", ")
            labels = rule["labels"].strings.joined(separator: ", ")
            notTouching = rule["not_touching"].strings.joined(separator: ", ")
        }
    }

    private func save() {
        let r = SettingsPatch.rule(approve: approve, risk: risk, maxLines: maxLines, authors: authors, labels: labels, notTouching: notTouching)
        guard let v = r.value else {
            problem = r.error
            return
        }
        problem = nil
        let path = slug.map { ["repos", $0, "post"] } ?? ["post"]
        Task {
            if await model.change(key, SettingsPatch.make("review", path, v)) { dismiss() }
        }
    }
}

/// The repo's Review section: who posts its checked reviews, and its
/// review guide (one of your guides, or none).
struct ReviewGuidePicker: View {
    let slug: String
    @Environment(SettingsModel.self) private var model
    @AppStorage("settingsTab") private var tab = "general"
    @AppStorage("settingsGuide") private var guide = ""

    var body: some View {
        let mine = model.docs.filter { $0.kind == "guide" && $0.yours }
        let current = model.review["repos"][slug]["guides"]["review"].text
        let known = mine.contains { $0.path == current }
        let doc = mine.first { $0.path == current }
        Section {
            PostingPicker(slug: slug)
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Picker(selection: Binding(get: { current ?? "" }, set: { v in
                        Task { await model.change("guide", SettingsPatch.make("review", ["repos", slug, "guides", "review"], v.isEmpty ? nil : v)) }
                    })) {
                        Text("None").tag("")
                        if !mine.isEmpty { Divider() }
                        ForEach(mine) { d in Text(d.title).tag(d.path) }
                        if let current, !known { Text(shortPath(current)).tag(current) }
                    } label: {
                        Described(title: "Review guide", text: mine.isEmpty ? "Write a guide of your own in Guides & Briefs, then pick it here." : "What reviewers in this repo look for, beyond what they always do.")
                    }
                    .fixedSize()
                    if let doc {
                        Button("Edit…") {
                            guide = doc.id
                            tab = "guides"
                        }
                        .controlSize(.small)
                    }
                }
                SetHere(part: "review", path: ["repos", slug, "guides", "review"], key: "guide")
                ErrorLine(key: "guide")
            }
        } header: {
            Text("Review")
        }
    }
}

// ---- guides and briefs ----

struct GuidesSettings: View {
    @Environment(SettingsModel.self) private var model
    @AppStorage("settingsGuide") private var selected = ""
    /// Unsaved edits, by doc: switching doesn't lose them.
    @State private var drafts: [String: String] = [:]
    @State private var creating = false

    var body: some View {
        let docs = model.docs
        let groups: [(String, [SettingsDoc])] = [
            ("Your guides", docs.filter { $0.kind == "guide" && $0.yours }),
            ("Built-in guides", docs.filter { $0.kind == "guide" && !$0.yours }),
            ("Build's briefs", docs.filter { $0.kind == "brief" && $0.proto == "build" }),
            ("Review's briefs", docs.filter { $0.kind == "brief" && $0.proto == "review" }),
        ].filter { !$0.1.isEmpty }
        let current = docs.first { $0.id == selected } ?? docs.first
        HStack(spacing: 0) {
            VStack(spacing: 0) {
                List(selection: Binding<String?>(get: { current?.id }, set: { if let v = $0 { selected = v } })) {
                    ForEach(groups, id: \.0) { title, items in
                        Section(title) {
                            ForEach(items) { d in DocRow(d: d, unsaved: drafts[d.id] != nil).tag(d.id) }
                        }
                    }
                }
                .listStyle(.sidebar)
                Divider()
                HStack(spacing: 6) {
                    Button { creating = true } label: { Label("New Guide…", systemImage: "plus") }
                        .buttonStyle(.borderless)
                        .help("Write a guide of your own: for a repo's reviews, say")
                    Spacer()
                }
                .font(.system(size: 12))
                .padding(.horizontal, 12).padding(.vertical, 8)
            }
            .frame(width: 300)
            Divider()
            Group {
                if let current {
                    DocEditor(doc: current, draft: Binding(get: { drafts[current.id] }, set: { drafts[current.id] = $0 }))
                        .id(current.id)
                } else if model.view != nil {
                    ContentUnavailableView("No guides", systemImage: "text.book.closed")
                } else {
                    ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(width: 1040, height: 700)
        .task { await model.load() }
        .sheet(isPresented: $creating) { NewGuideSheet { id in selected = id } }
    }
}

struct DocRow: View {
    let d: SettingsDoc
    let unsaved: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Text(d.title).font(.system(size: 13, weight: .medium)).lineLimit(1)
                if unsaved { Circle().fill(K.accent).frame(width: 6, height: 6).help("Unsaved changes") }
                Spacer(minLength: 4)
                if d.kind == "brief" || !d.yours {
                    Text(d.origin).font(.system(size: 10.5, weight: .medium)).foregroundStyle(d.overridden ? K.blocked : .secondary)
                }
            }
            if let a = d.about {
                Text(inlineMarkdown(a)).font(.system(size: 11.5)).foregroundStyle(.secondary).lineLimit(2)
            }
            if !d.usedBy.isEmpty {
                Text("Used by \(d.usedBy.map(SettingsPatch.usedBy).joined(separator: ", "))").font(.system(size: 11)).foregroundStyle(K.accent).lineLimit(1)
            }
        }
        .padding(.vertical, 3)
    }
}

/// One guide or brief: edit it, compare it with the built-in, save, reset.
struct DocEditor: View {
    let doc: SettingsDoc
    @Binding var draft: String?
    @Environment(SettingsModel.self) private var model
    @State private var loaded: JSON?
    @State private var error: String?
    @State private var busy = false
    @State private var compare = false
    @State private var confirmReset = false

    private var saved: String { loaded?["text"].str ?? "" }
    private var original: String? { loaded?["original"].str }
    private var text: String { draft ?? saved }
    private var dirty: Bool { draft != nil && draft != saved }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(doc.title).font(.system(size: 15, weight: .semibold))
                    Chip(text: doc.origin, color: doc.overridden ? K.blocked : doc.yours ? K.accent : K.muted)
                    Spacer()
                    if original != nil {
                        Picker("", selection: $compare) {
                            Text("Edit").tag(false)
                            Text("Compare with Built-in").tag(true)
                        }
                        .pickerStyle(.segmented).labelsHidden().fixedSize()
                    }
                }
                if let a = doc.about { Text(inlineMarkdown(a)).font(.system(size: 12.5)).foregroundStyle(K.text2).fixedSize(horizontal: false, vertical: true) }
                Text(doc.path).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary).textSelection(.enabled)
                if !doc.usedBy.isEmpty {
                    Text("Used by \(doc.usedBy.map(SettingsPatch.usedBy).joined(separator: ", ")).").font(.system(size: 11.5)).foregroundStyle(K.muted)
                }
                if doc.kind == "brief" {
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: "curlybraces").foregroundStyle(K.blocked).padding(.top, 1)
                        Text("A brief is what agents are told. Its `{{VARIABLES}}` are filled in by Wheelhouse when an agent starts, so keep them. A broken brief changes what every agent is told: compare with the built-in before you save.")
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .font(.system(size: 12))
                    .foregroundStyle(K.text2)
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(K.warningTint.opacity(0.7), in: RoundedRectangle(cornerRadius: 8))
                }
            }
            .padding(.horizontal, 18).padding(.top, 16).padding(.bottom, 12)
            Divider()
            Group {
                if loaded == nil, error == nil {
                    ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if compare, let original {
                    DiffView(lines: lineDiff(original, text), same: original == text)
                } else {
                    GeometryReader { g in
                        Editor(text: Binding(get: { text }, set: { draft = $0 == saved ? nil : $0 }),
                               minHeight: g.size.height - 20, maxHeight: g.size.height - 20,
                               font: .monospacedSystemFont(ofSize: 12.5, weight: .regular), onSubmit: save)
                            .padding(.horizontal, 14).padding(.vertical, 10)
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            Divider()
            VStack(alignment: .leading, spacing: 6) {
                if let original, doc.kind == "brief" {
                    let gone = SettingsPatch.dropped(from: original, in: text)
                    if !gone.isEmpty {
                        Label("This leaves out \(gone.joined(separator: ", ")), which the built-in uses: agents won't be told what it fills in.", systemImage: "exclamationmark.triangle.fill")
                            .font(.system(size: 11.5)).foregroundStyle(K.blocked)
                    }
                }
                if let error {
                    Label(error, systemImage: "exclamationmark.triangle.fill").font(.system(size: 11.5)).foregroundStyle(K.error).textSelection(.enabled)
                }
                HStack(spacing: 8) {
                    Text(status).font(.system(size: 11.5)).foregroundStyle(.secondary).lineLimit(1)
                    Spacer()
                    if doc.builtin, doc.overridden {
                        Button("Reset to Built-in…") { confirmReset = true }.disabled(busy)
                    } else if doc.yours {
                        Button("Set Aside…") { confirmReset = true }
                            .disabled(busy || !doc.usedBy.isEmpty)
                            .help(doc.usedBy.isEmpty ? "Move it out of your guides; it's kept in the history" : "It's still used by \(doc.usedBy.map(SettingsPatch.usedBy).joined(separator: ", ")): point those elsewhere first")
                    }
                    if dirty { Button("Revert") { draft = nil } }
                    Button(busy ? "Saving…" : "Save") { save() }
                        .buttonStyle(.borderedProminent).tint(K.accent)
                        .keyboardShortcut("s", modifiers: .command)
                        .disabled(!dirty || busy)
                }
            }
            .controlSize(.small)
            .padding(.horizontal, 18).padding(.vertical, 12)
        }
        .task { await load() }
        .confirmationDialog(doc.yours ? "Set aside “\(doc.title)”?" : "Reset “\(doc.title)” to the built-in?", isPresented: $confirmReset) {
            Button(doc.yours ? "Set Aside" : "Reset", role: .destructive) { reset() }
        } message: {
            Text("Your text is kept in \(model.view?["history"].str ?? "the history").\(doc.yours ? "" : " Agents get the built-in from now on.")")
        }
        .onReceive(NotificationCenter.default.publisher(for: .snapshotGuide)) { n in
            // A made-up edit, to show the comparison.
            guard let s = n.object as? String else { return }
            draft = s.isEmpty ? nil : saved.replacingOccurrences(of: s, with: "")
            compare = !s.isEmpty
        }
    }

    private var status: String {
        if dirty { return "Unsaved changes" }
        if doc.yours { return "Your guide." }
        if doc.overridden { return "Edited: yours replaces the built-in." }
        return "Built in. Saving makes it yours, at \(doc.path)."
    }

    private func load() async {
        do {
            loaded = try await model.text(of: doc.id)
            error = nil
        } catch { self.error = Store.words(error) }
    }

    private func save() {
        guard dirty, !busy else { return }
        busy = true
        Task {
            do {
                loaded = try await model.save(doc.id, text)
                draft = nil
                error = nil
                Store.shared.toast("Saved \(doc.title): agents get it from their next brief")
            } catch { self.error = Store.words(error) }
            busy = false
        }
    }

    private func reset() {
        busy = true
        Task {
            do {
                let r = try await model.reset(doc.id)
                draft = nil
                error = nil
                if r["removed"].bool { loaded = nil } else { loaded = r }
                Store.shared.toast(doc.yours ? "Set aside \(doc.title)" : "\(doc.title) is the built-in again")
            } catch { self.error = Store.words(error) }
            busy = false
        }
    }
}

/// A line diff, wrapped: lines only, nothing sideways.
struct DiffView: View {
    let lines: [DiffLine]
    let same: Bool

    var body: some View {
        ScrollViewReader { proxy in
        ScrollView {
            if same {
                Text("Same as the built-in.").font(.system(size: 12.5)).foregroundStyle(.secondary).padding(18).frame(maxWidth: .infinity, alignment: .leading)
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(lines.enumerated()), id: \.offset) { i, l in
                        HStack(alignment: .top, spacing: 8) {
                            Text(l.op == .add ? "+" : l.op == .del ? "−" : " ").foregroundStyle(l.op == .add ? K.working : l.op == .del ? K.error : K.faint)
                            Text(l.text.isEmpty ? " " : l.text).foregroundStyle(l.op == .same ? K.muted : K.text)
                                .fixedSize(horizontal: false, vertical: true)
                            Spacer(minLength: 0)
                        }
                        .font(.system(size: 12, design: .monospaced))
                        .padding(.horizontal, 10).padding(.vertical, 1.5)
                        .background(l.op == .add ? K.successTint : l.op == .del ? K.dangerTint : .clear)
                        .id(i)
                    }
                }
                .textSelection(.enabled)
                .padding(14)
            }
        }
        .onAppear {
            // To the first change, with a few lines before it.
            if let first = lines.firstIndex(where: { $0.op != .same }), first > 6 {
                DispatchQueue.main.async { proxy.scrollTo(first - 4, anchor: .top) }
            }
        }
        }
    }
}

/// A guide of your own: its name, and a first line to start from.
struct NewGuideSheet: View {
    let created: (String) -> Void
    @Environment(SettingsModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var title = ""
    @State private var error: String?
    @State private var busy = false

    var body: some View {
        let file = SettingsPatch.guideFile(title)
        let taken = file.map { f in model.docs.contains { $0.id == "guide/\(f)" } } ?? false
        VStack(alignment: .leading, spacing: 12) {
            Text("New guide").font(.system(size: 14, weight: .semibold))
            Text("A guide is Markdown agents read and follow: how to review a repo, say. Pick it as a repo's review guide in Repos.")
                .font(.system(size: 12.5)).foregroundStyle(K.text2).fixedSize(horizontal: false, vertical: true)
            TextField("Name", text: $title, prompt: Text("API review"))
                .textFieldStyle(.roundedBorder)
                .onSubmit(create)
            Text(file.map { taken ? "There's a guide called \($0) already." : "Saved as \(((model.view?["file"].str ?? "~/.config/wheelhouse/settings.json") as NSString).deletingLastPathComponent)/guides/\($0)" } ?? "Letters and digits; the file name is made from them.")
                .font(.system(size: 11.5, design: file == nil ? .default : .monospaced))
                .foregroundStyle(taken ? K.error : .secondary)
            if let error { Text(error).font(.system(size: 11.5)).foregroundStyle(K.error) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(busy ? "Creating…" : "Create Guide") { create() }
                    .buttonStyle(.borderedProminent).tint(K.accent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(file == nil || taken || busy)
            }
        }
        .padding(20)
        .frame(width: 460)
    }

    private func create() {
        guard let file = SettingsPatch.guideFile(title), !busy else { return }
        let name = title.trimmingCharacters(in: .whitespacesAndNewlines)
        busy = true
        Task {
            do {
                let r = try await model.save("guide/\(file)", "# \(name)\n\nWhat agents should look for, and how.\n")
                created(r["id"].str ?? "guide/\(file)")
                dismiss()
            } catch { self.error = Store.words(error) }
            busy = false
        }
    }
}

extension Notification.Name {
    static let snapshotGuide = Notification.Name("wheelhouse.snapshotGuide")
    static let snapshotCheck = Notification.Name("wheelhouse.snapshotCheck")
}
