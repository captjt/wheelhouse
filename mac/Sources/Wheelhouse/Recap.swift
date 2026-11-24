import Charts
import SwiftUI

/// What happened: today, this week, or 30 days, in your time zone. Reviews
/// and their verdicts, PRs merged, agents' turns, your decisions, and the
/// tokens all the work took, by provider and model; and the same as
/// Markdown, for a standup.
struct RecapScreen: View {
    @Environment(Store.self) private var store
    @Environment(Nav.self) private var nav
    @AppStorage("recap") private var period = "week"
    @State private var data: JSON?
    @State private var error: String?
    @State private var loadedAt = Date.distantPast

    private var words: (now: String, before: String) {
        switch period {
        case "today": ("today", "yesterday")
        case "30d": ("the last 30 days", "the 30 days before")
        default: ("this week", "the week before")
        }
    }

    var body: some View {
        ScrollView {
            if let d = data {
                content(d).padding(28).frame(maxWidth: 940).frame(maxWidth: .infinity)
            } else {
                Text(error.map { "Couldn't make the recap: \($0)" } ?? "").foregroundStyle(K.muted).padding(40)
            }
        }
        .task(id: period) { await load() }
        .task(id: store.version) { if Date().timeIntervalSince(loadedAt) > 15 { await load() } }
        .toolbar {
            ToolbarItemGroup {
                Picker("Period", selection: $period) {
                    Text("Today").tag("today")
                    Text("This week").tag("week")
                    Text("30 days").tag("30d")
                }
                .pickerStyle(.segmented)
                Button { copy() } label: { Label("Copy as Markdown", systemImage: "doc.on.clipboard") }
                    .disabled(data == nil)
                    .help("Copy the recap as a Markdown note, for a standup")
            }
        }
    }

    private func load() async {
        var cal = Calendar.current
        cal.timeZone = .current
        let start = cal.startOfDay(for: Date())
        let until = cal.date(byAdding: .day, value: 1, to: start)!
        let since = cal.date(byAdding: .day, value: period == "week" ? -6 : period == "30d" ? -29 : 0, to: start)!
        let f = ISO8601DateFormatter()
        let q = "since=\(f.string(from: since))&until=\(f.string(from: until))&label=\(words.now)".addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) ?? ""
        do {
            data = try await store.hub.get("/api/recap?\(q.replacingOccurrences(of: "+", with: "%2B"))")
            error = nil
            loadedAt = Date()
        } catch {
            self.error = Store.words(error)
        }
    }

    private func copy() {
        guard let md = data?["markdown"].str else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(md, forType: .string)
        store.toast("Copied the recap for \(words.now) as Markdown")
    }

    @ViewBuilder
    private func content(_ d: JSON) -> some View {
        let r = d["reviews"], ag = d["agents"], you = d["you"]
        let posted = r["posted"].int ?? 0
        VStack(alignment: .leading, spacing: 28) {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 132), spacing: 12, alignment: .top)], spacing: 12) {
                Tile(label: "Reviews posted", value: posted, delta: (posted, r["prev_posted"].int ?? 0, words.before), spark: r["per_day"].array.compactMap(\.int), since: d["since"].str)
                Tile(label: "Approved", value: r["approved"].int ?? 0, sub: "\(pct(r["approved"].int ?? 0, posted)) of reviews")
                Tile(label: "Changes requested", value: r["changes"].int ?? 0, sub: "\(pct(r["changes"].int ?? 0, posted)) of reviews")
                Tile(label: "PRs merged", value: r["merged"].int ?? 0, delta: (r["merged"].int ?? 0, r["prev_merged"].int ?? 0, words.before))
                Tile(label: "Agent turns", value: ag["turns"].int ?? 0, sub: "\(plural(ag["done"].array.count, "docket")) done · \(plural(ag["prompts"].int ?? 0, "prompt"))", spark: ag["per_day"].array.compactMap(\.int), since: d["since"].str)
                if d["usage"]["tokens"].int != nil {
                    let u = d["usage"]
                    Tile(label: "Tokens", value: u["tokens"].int ?? 0, delta: (u["tokens"].int ?? 0, u["prev_tokens"].int ?? 0, words.before), spark: u["per_day"].array.compactMap(\.int), since: d["since"].str, format: compact, neutral: true)
                }
            }
            let extras = [r["median_to_review"].double.map { "Median time from request to review: \(dur($0))" }, (r["tokens"].int ?? 0) > 0 ? "\(compact(r["tokens"].int ?? 0)) review tokens\(r["cached"].double.map { ", \(Int($0 * 100))% cached" } ?? "")" : nil, (r["dismissed"].int ?? 0) > 0 ? "\(r["dismissed"].int!) dismissed" : nil].compactMap { $0 }
            if !extras.isEmpty { Text(extras.joined(separator: " · ")).font(.system(size: 12.5)).foregroundStyle(K.muted) }

            if d["usage"]["tokens"].int != nil {
                section("Tokens") { UsageView(u: d["usage"], words: words.now) { id in nav.show(id, store: store) } }
            }
            section("Reviews") {
                let merged = r["prs"]["merged"].array, open = r["prs"]["open"].array, closed = r["prs"]["closed"].array
                if merged.isEmpty && open.isEmpty && closed.isEmpty {
                    Text("No reviews \(words.now).").foregroundStyle(K.muted)
                } else {
                    prGroup("Merged", merged, kind: "merged")
                    prGroup("Waiting on their authors", open, kind: "open")
                    prGroup("Closed without merging", closed, kind: "closed")
                }
            }
            section("Agents") {
                Text("\(plural(ag["started"].int ?? 0, "docket")) started · \(ag["done"].array.count) done · \(ag["turns"].int ?? 0) turns · \(plural(ag["prompts"].int ?? 0, "prompt")) from you").font(.system(size: 12.5)).foregroundStyle(K.muted)
                ForEach(Array(ag["done"].array.enumerated()), id: \.offset) { _, x in
                    RecapRow(icon: AnyView(AppAvatar(app: x["app"].str, size: 16)), key: x["id"].str ?? "", title: x["title"].str ?? "", meta: "\(plural(x["turns"].int ?? 0, "turn")) \(words.now)", detail: x["summary"].text, at: x["at"].str) {
                        if let id = x["id"].str { nav.show(id, store: store) }
                    }
                }
            }
            let bs = d["builds"]
            if !bs["list"].array.isEmpty {
                section("Builds") {
                    Text("\(plural(bs["opened"].int ?? 0, "PR")) opened · \(plural(bs["rounds"].int ?? 0, "address round")) (\(bs["replies"].int ?? 0) \((bs["replies"].int ?? 0) == 1 ? "reply" : "replies")) · \(bs["merged"].int ?? 0) merged").font(.system(size: 12.5)).foregroundStyle(K.muted)
                    ForEach(Array(bs["list"].array.enumerated()), id: \.offset) { _, x in
                        let meta = [x["pr"].int.map { "#\($0)" }, x["opened"].bool ? "opened" : nil, (x["rounds"].int ?? 0) > 0 ? plural(x["rounds"].int ?? 0, "round") : nil, x["outcome"].str].compactMap { $0 }.joined(separator: " · ")
                        RecapRow(icon: AnyView(Image(systemName: "hammer").foregroundStyle(x["outcome"].str == "merged" ? K.working : K.accent)), key: x["id"].str ?? "", title: x["title"].str ?? "", meta: meta, detail: nil, at: x["at"].str) {
                            if let id = x["id"].str { nav.show(id, store: store) }
                        }
                    }
                }
            }
            section("You") {
                let decisions = you["decisions"].array
                Text("\(plural(decisions.count, "decision")) · \(plural(you["approved"].int ?? 0, "approval")) · \(you["denied"].int ?? 0) denied · \(plural(you["intents"].int ?? 0, "intent edit"))\((you["asked"].int ?? 0) > 0 ? " · agents asked you \(plural(you["asked"].int!, "time"))" : "")").font(.system(size: 12.5)).foregroundStyle(K.muted)
                ForEach(Array(decisions.enumerated()), id: \.offset) { _, x in
                    RecapRow(icon: AnyView(Image(systemName: "checkmark.circle").foregroundStyle(K.accent)), key: x["docket"].str ?? "", title: x["title"].str ?? "", meta: "“\(x["choice"].str ?? "")”", detail: nil, at: x["at"].str) {
                        if let id = x["docket"].str { nav.show(id, store: store) }
                    }
                }
            }
        }
    }

    private func section(_ title: String, @ViewBuilder _ body: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.system(size: 17, weight: .semibold)).foregroundStyle(K.text)
            body()
        }
    }

    @ViewBuilder
    private func prGroup(_ title: String, _ prs: [JSON], kind: String) -> some View {
        if !prs.isEmpty {
            Text("\(title) · \(prs.count)").sectionLabel().padding(.top, 4)
            ForEach(Array(prs.enumerated()), id: \.offset) { _, p in
                let verdicts = p["verdicts"].strings
                let last = verdicts.last
                let trail = verdicts.map { $0 == "APPROVED" ? "✓" : $0 == "CHANGES_REQUESTED" ? "✕" : "·" }.joined(separator: " → ")
                let state = kind == "open" ? (last == "APPROVED" ? "approved, not merged yet" : last == "CHANGES_REQUESTED" ? "changes requested" : p["state"].str == "reviewing" ? "in review" : "queued") : trail
                let lines = [p["additions"].int.map { "+\($0)" }, p["deletions"].int.map { "−\($0)" }].compactMap { $0 }.joined(separator: " ")
                RecapRow(icon: AnyView(Image(systemName: "arrow.triangle.pull").foregroundStyle(kind == "merged" ? K.working : kind == "closed" ? K.faint : K.accent)),
                         key: p["key"].str ?? "", title: p["title"].str ?? "",
                         meta: [state, plural(p["rounds"].int ?? 0, "round"), p["author"].str, lines.isEmpty ? nil : lines, p["merge_risk"].str.map { "risk \($0)" }].compactMap { $0 }.joined(separator: " · "),
                         detail: p["implementation"].text, at: p["ended_at"].str ?? p["last_at"].str) {
                    if let id = p["docket"].str { nav.show(id, store: store) } else if let u = p["url"].str.flatMap(URL.init(string:)) { NSWorkspace.shared.open(u) }
                }
            }
        }
    }
}

private func pct(_ a: Int, _ b: Int) -> String { b == 0 ? "–" : "\(Int((Double(a) / Double(b) * 100).rounded()))%" }

private func dur(_ s: Double) -> String {
    s < 3600 ? "\(max(1, Int((s / 60).rounded())))m" : s < 86400 ? "\(Int(s / 3600))h \(Int((s.truncatingRemainder(dividingBy: 3600) / 60).rounded()))m" : "\(Int(s / 86400))d \(Int((s.truncatingRemainder(dividingBy: 86400) / 3600).rounded()))h"
}

private func compact(_ n: Int) -> String {
    n >= 1_000_000_000 ? String(format: "%.1fB", Double(n) / 1e9) : n >= 1_000_000 ? String(format: n >= 10_000_000 ? "%.0fM" : "%.1fM", Double(n) / 1e6) : n >= 10000 ? "\(n / 1000)k" : n.formatted()
}

/// A provider's color: its agent's avatar's.
private func providerColor(_ p: String?) -> Color {
    switch p {
    case "anthropic": K.claude
    case "openai": K.codexInk
    case "google": K.agy
    default: K.progress
    }
}

private func share(_ n: Int, _ of: Int) -> String {
    guard of > 0 else { return "–" }
    let p = Double(n) / Double(of) * 100
    return p >= 99.5 ? "100%" : p >= 1 ? "\(Int(p.rounded()))%" : "<1%"
}

/// Tokens across all the work: by provider and model, by kind of work, and
/// the dockets that took the most.
private struct UsageView: View {
    let u: JSON
    let words: String
    let open: (String) -> Void

    var body: some View {
        let total = u["tokens"].int ?? 0
        let models = u["models"].array, providers = u["providers"].array
        if total == 0 {
            Text("No model calls \(words).").foregroundStyle(K.muted)
        } else {
            let line = [
                "\(compact(total)) tokens \(words)",
                u["cached"].double.map { "\(Int(($0 * 100).rounded()))% of input from the cache" },
                "\(compact(u["split"]["output"].int ?? 0)) output",
                "\(plural(models.count, "model")) from \(plural(providers.count, "provider"))",
            ].compactMap { $0 }.joined(separator: " · ")
            Text(line).font(.system(size: 12.5)).foregroundStyle(K.muted)
            GeometryReader { g in
                let gaps = CGFloat(max(0, models.count - 1)) * 2
                HStack(spacing: 2) {
                    ForEach(Array(models.enumerated()), id: \.offset) { i, m in
                        Rectangle()
                            .fill(providerColor(m["provider"].str).opacity(1 - Double(min(i, 3)) * 0.18))
                            .frame(width: max(3, (g.size.width - gaps) * CGFloat(m["tokens"].int ?? 0) / CGFloat(total)))
                            .help("\(m["model"].str ?? "") (\(m["provider_name"].str ?? "")): \(compact(m["tokens"].int ?? 0)), \(share(m["tokens"].int ?? 0, total))")
                    }
                }
            }
            .frame(height: 10)
            .background(K.recessed)
            .clipShape(RoundedRectangle(cornerRadius: 5))
            HStack(spacing: 16) {
                ForEach(Array(providers.enumerated()), id: \.offset) { _, p in
                    HStack(spacing: 6) {
                        RoundedRectangle(cornerRadius: 2).fill(providerColor(p["provider"].str)).frame(width: 8, height: 8)
                        Text(p["name"].str ?? "").foregroundStyle(K.text)
                        Text("\(compact(p["tokens"].int ?? 0)) · \(share(p["tokens"].int ?? 0, total))").foregroundStyle(K.muted)
                    }
                    .font(.system(size: 12.5))
                }
            }
            HStack(alignment: .top, spacing: 28) {
                VStack(alignment: .leading, spacing: 0) {
                    Text("Models").sectionLabel().padding(.bottom, 4)
                    ForEach(Array(models.enumerated()), id: \.offset) { i, m in
                        let sp = m["split"]
                        let input = (sp["input"].int ?? 0) + (sp["cache_read"].int ?? 0) + (sp["cache_write"].int ?? 0)
                        let meta = [input > 0 ? "\(Int((Double(sp["cache_read"].int ?? 0) / Double(input) * 100).rounded()))% cached" : nil, "\(compact(sp["output"].int ?? 0)) out", plural(m["dockets"].int ?? 0, "docket")].compactMap { $0 }.joined(separator: " · ")
                        row(swatch: providerColor(m["provider"].str), name: m["model"].str ?? "", sub: m["provider_name"].str, meta: meta, n: m["tokens"].int ?? 0, total: total, last: i == models.count - 1)
                    }
                }
                .frame(maxWidth: .infinity)
                VStack(alignment: .leading, spacing: 0) {
                    Text("By kind of work").sectionLabel().padding(.bottom, 4)
                    let kinds = [("agents", "Agents"), ("builds", "Builds"), ("reviews", "Reviews")]
                    let top = max(1, kinds.map { u["work"][$0.0].int ?? 0 }.max() ?? 1)
                    ForEach(Array(kinds.enumerated()), id: \.offset) { i, k in
                        let n = u["work"][k.0].int ?? 0
                        HStack(spacing: 8) {
                            Text(k.1).foregroundStyle(K.text).frame(width: 64, alignment: .leading)
                            GeometryReader { g in
                                ZStack(alignment: .leading) {
                                    Capsule().fill(K.recessed)
                                    Capsule().fill(K.accent).frame(width: g.size.width * CGFloat(n) / CGFloat(top))
                                }
                            }
                            .frame(height: 6)
                            Text(compact(n)).monospacedDigit().foregroundStyle(K.text).frame(minWidth: 48, alignment: .trailing)
                            Text(share(n, total)).font(.system(size: 12)).monospacedDigit().foregroundStyle(K.muted).frame(width: 36, alignment: .trailing)
                        }
                        .font(.system(size: 13))
                        .padding(.vertical, 6)
                        .overlay(alignment: .bottom) { if i < kinds.count - 1 { Rectangle().fill(K.line).frame(height: 1) } }
                    }
                }
                .frame(width: 280)
            }
            .padding(.top, 4)
            let dockets = u["dockets"].array
            if !dockets.isEmpty {
                Text("Most tokens · \(dockets.count)").sectionLabel().padding(.top, 4)
                ForEach(Array(dockets.enumerated()), id: \.offset) { _, x in
                    let kind = x["kind"].str == "builds" ? "Build" : x["kind"].str == "reviews" ? "Review" : "Agent"
                    RecapRow(icon: AnyView(Image(systemName: x["kind"].str == "builds" ? "hammer" : x["kind"].str == "reviews" ? "arrow.triangle.pull" : "sparkles").foregroundStyle(K.accent)),
                             key: x["id"].str ?? "", title: x["title"].str ?? "",
                             meta: [kind, x["models"].strings.joined(separator: ", "), "\(share(x["tokens"].int ?? 0, total)) of the tokens"].filter { !$0.isEmpty }.joined(separator: " · "),
                             detail: nil, at: nil, trailing: compact(x["tokens"].int ?? 0)) {
                        if let id = x["id"].str { open(id) }
                    }
                }
            }
        }
    }

    private func row(swatch: Color, name: String, sub: String?, meta: String, n: Int, total: Int, last: Bool) -> some View {
        HStack(spacing: 8) {
            RoundedRectangle(cornerRadius: 2).fill(swatch).frame(width: 8, height: 8)
            HStack(spacing: 4) {
                Text(name).foregroundStyle(K.text)
                if let sub { Text(sub).font(.system(size: 12)).foregroundStyle(K.muted) }
            }
            .lineLimit(1)
            Spacer(minLength: 8)
            Text(meta).font(.system(size: 12)).foregroundStyle(K.muted).lineLimit(1)
            Text(compact(n)).monospacedDigit().foregroundStyle(K.text).frame(minWidth: 48, alignment: .trailing)
            Text(share(n, total)).font(.system(size: 12)).monospacedDigit().foregroundStyle(K.muted).frame(width: 36, alignment: .trailing)
        }
        .font(.system(size: 13))
        .padding(.vertical, 6)
        .overlay(alignment: .bottom) { if !last { Rectangle().fill(K.line).frame(height: 1) } }
    }
}

/// A number, what it's of, and how it moved.
private struct Tile: View {
    let label: String
    let value: Int
    var sub: String?
    var delta: (Int, Int, String)?
    var spark: [Int] = []
    var since: String?
    var format: (Int) -> String = { "\($0)" }
    /// More isn't better (tokens): the change in the muted ink.
    var neutral = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(label).font(.system(size: 12)).foregroundStyle(K.muted)
            Text(format(value)).font(.system(size: 28, weight: .semibold).monospacedDigit()).foregroundStyle(K.text)
            if let (now, before, words) = delta {
                let d = now - before
                Text("\(d > 0 ? "+" : d < 0 ? "−" : "±")\(format(abs(d))) vs \(words)").font(.system(size: 11.5)).foregroundStyle(neutral ? K.muted : d > 0 ? K.working : d < 0 ? K.error : K.faint)
            }
            if let sub { Text(sub).font(.system(size: 11.5)).foregroundStyle(K.muted) }
            Spacer(minLength: 0)
            if spark.count > 1 {
                let start = date(since) ?? Date()
                let top = max(1, spark.max() ?? 1)
                Chart(Array(spark.enumerated()), id: \.offset) { i, v in
                    let day = Calendar.current.date(byAdding: .day, value: i, to: start) ?? start
                    AreaMark(x: .value("Day", day), y: .value(label, v)).foregroundStyle(K.accent.opacity(0.08)).interpolationMethod(.monotone)
                    LineMark(x: .value("Day", day), y: .value(label, v)).foregroundStyle(K.faint).lineStyle(StrokeStyle(lineWidth: 1.5)).interpolationMethod(.monotone)
                    if i == spark.count - 1 {
                        PointMark(x: .value("Day", day), y: .value(label, v)).foregroundStyle(K.accent).symbolSize(28)
                    }
                }
                .chartYScale(domain: 0 ... Double(top))
                .chartXAxis(.hidden)
                .chartYAxis(.hidden)
                .frame(height: 30)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, minHeight: 150, alignment: .topLeading)
        .background(K.elev, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(K.line))
    }
}

private struct RecapRow: View {
    let icon: AnyView
    let key: String
    let title: String
    let meta: String
    let detail: String?
    let at: String?
    var trailing: String?
    let open: () -> Void
    @State private var hover = false

    var body: some View {
        Button(action: open) {
            HStack(alignment: .top, spacing: 10) {
                icon.frame(width: 18).padding(.top, 1)
                VStack(alignment: .leading, spacing: 3) {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text(key).font(.system(size: 11.5, design: .monospaced)).foregroundStyle(K.muted)
                        Text(title).font(.system(size: 13, weight: .medium)).foregroundStyle(K.text).lineLimit(1)
                        Spacer()
                        if let trailing {
                            Text(trailing).font(.system(size: 12.5).monospacedDigit()).foregroundStyle(K.text)
                        } else {
                            Text(ago(at)).font(.system(size: 11.5)).foregroundStyle(K.faint)
                        }
                    }
                    Text(meta).font(.system(size: 12)).foregroundStyle(K.muted)
                    if let detail { Text(detail).font(.system(size: 12)).foregroundStyle(K.text2).lineLimit(2) }
                }
            }
            .padding(10)
            .background(hover ? K.hover : .clear, in: RoundedRectangle(cornerRadius: 8))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hover = $0 }
    }
}
