import Markdown
import SwiftUI

// Markdown as the web app renders it (web/src/markdown.ts): GitHub's
// flavor, a line break where the text has one (unless it's hard-wrapped
// prose), GitHub's alerts, task boxes you can tick, and `D-12` as a link to
// the docket. Raw HTML shows as text: nothing in an agent's words runs.

/// A block of rendered Markdown.
indirect enum MDBlock: Hashable {
    case paragraph(AttributedString)
    case heading(Int, AttributedString)
    case list(ordered: Bool, start: Int, items: [MDItem])
    case code(lang: String?, text: String)
    case quote([MDBlock])
    case alert(kind: String, [MDBlock])
    case table(head: [AttributedString], rows: [[AttributedString]], align: [Int])
    case rule
    case image(URL, alt: String)
}

struct MDItem: Hashable {
    /// The box's state, for a task item, and the source line it's on.
    var checked: Bool?
    var line: Int?
    var blocks: [MDBlock]
}

enum MD {
    static let alertTitles = ["NOTE": "Note", "TIP": "Tip", "IMPORTANT": "Important", "WARNING": "Warning", "CAUTION": "Caution"]
    nonisolated(unsafe) private static let cache = NSCache<NSString, Box>()
    final class Box { let blocks: [MDBlock]; init(_ b: [MDBlock]) { blocks = b } }

    static func blocks(_ text: String, size: CGFloat) -> [MDBlock] {
        let key = "\(size)\u{0}\(text)" as NSString
        if let hit = cache.object(forKey: key) { return hit.blocks }
        var conv = Converter(size: size, breaks: !hardWrapped(text))
        let out = conv.blocks(Document(parsing: text, options: [.disableSmartOpts]).children)
        cache.setObject(Box(out), forKey: key)
        return out
    }

    /// Prose someone wrapped by hand at ~80 columns reads as paragraphs, not
    /// as a line per line.
    static func hardWrapped(_ raw: String) -> Bool {
        let lines = raw.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        guard lines.count >= 2 else { return false }
        let broken = lines.dropLast()
        let lens = broken.map(\.count)
        guard lens.allSatisfy({ (55 ... 100).contains($0) }), (lens.max()! - lens.min()!) <= 30 else { return false }
        let mid = broken.filter { $0.range(of: #"[.!?:;]["')\]*]*$"#, options: .regularExpression) == nil }.count
        return lines.count >= 3 ? mid * 2 >= broken.count : mid == broken.count
    }

    /// Tick or untick the task box on `line` (1-based, as the parser
    /// numbers them) of `text`: a new version of it, or nil if the box
    /// isn't plainly there.
    static func toggleTask(_ text: String, line: Int, checked: Bool) -> String? {
        var lines = text.components(separatedBy: "\n")
        guard lines.indices.contains(line - 1) else { return nil }
        let l = lines[line - 1]
        guard let r = l.range(of: #"^(\s*(?:>\s*)*(?:[-*+]|\d+[.)])\s+)\[( |x|X)\]"#, options: .regularExpression) else { return nil }
        let box = l.index(r.upperBound, offsetBy: -2)
        lines[line - 1].replaceSubrange(box ..< l.index(after: box), with: checked ? "x" : " ")
        return lines.joined(separator: "\n")
    }
}

private struct Converter {
    let size: CGFloat
    let breaks: Bool
    /// How many quotes deep: cmark misses task boxes in lists inside them.
    var quotes = 0

    struct Style {
        var bold = false
        var italic = false
        var code = false
        var strike = false
        var link: URL?
    }

    mutating func blocks(_ children: some Sequence<Markup>) -> [MDBlock] {
        var out: [MDBlock] = []
        for c in children { if let b = block(c) { out.append(b) } }
        return out
    }

    mutating func block(_ m: Markup) -> MDBlock? {
        switch m {
        case let p as Paragraph:
            let kids = Array(p.children)
            if kids.count == 1, let img = kids[0] as? Markdown.Image, let src = img.source, let url = URL(string: src), url.scheme == "https" || url.scheme == "data" {
                return .image(url, alt: img.plainText)
            }
            return .paragraph(inline(kids, Style(), size))
        case let h as Heading:
            let sizes: [CGFloat] = [size + 7, size + 4, size + 2, size + 0.5, size, size - 1]
            return .heading(h.level, inline(Array(h.children), Style(bold: true), sizes[min(h.level, 6) - 1]))
        case let l as UnorderedList:
            return .list(ordered: false, start: 1, items: items(l.listItems))
        case let l as OrderedList:
            return .list(ordered: true, start: Int(l.startIndex), items: items(l.listItems))
        case let c as CodeBlock:
            return .code(lang: c.language?.isEmpty == false ? c.language : nil, text: c.code.hasSuffix("\n") ? String(c.code.dropLast()) : c.code)
        case let q as BlockQuote:
            quotes += 1
            var inner = blocks(q.children)
            quotes -= 1
            // GitHub's alerts: a quote whose first line is [!NOTE], [!TIP], …
            if case let .paragraph(first)? = inner.first {
                let s = String(first.characters)
                if let r = s.range(of: #"^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]\s*"#, options: .regularExpression) {
                    let kind = String(s[r]).trimmingCharacters(in: CharacterSet(charactersIn: "[]! \n"))
                    var rest = first
                    let cut = s.distance(from: s.startIndex, to: r.upperBound)
                    rest.removeSubrange(rest.startIndex ..< rest.index(rest.startIndex, offsetByCharacters: cut))
                    inner.removeFirst()
                    if !String(rest.characters).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { inner.insert(.paragraph(rest), at: 0) }
                    return .alert(kind: kind, inner)
                }
            }
            return .quote(inner)
        case let t as Markdown.Table:
            let me = self
            let head = t.head.cells.map { me.inline(Array($0.children), Style(bold: true), me.size) }
            let rows = t.body.rows.map { row in Array(row.cells.map { me.inline(Array($0.children), Style(), me.size) }) }
            let align = t.columnAlignments.map { a -> Int in
                switch a { case .center?: 1; case .right?: 2; default: 0 }
            }
            return .table(head: Array(head), rows: Array(rows), align: Array(align))
        case is ThematicBreak:
            return .rule
        case let h as HTMLBlock:
            let raw = h.rawHTML.trimmingCharacters(in: .whitespacesAndNewlines)
            if raw.isEmpty || raw.hasPrefix("<!--") { return nil }
            return .paragraph(plain(stripTags(raw), Style(), size))
        default:
            if let c = m as? (any InlineContainer) { return .paragraph(inline(Array(c.children), Style(), size)) }
            return nil
        }
    }

    mutating func items(_ list: some Sequence<ListItem>) -> [MDItem] {
        list.map { li in
            var item = MDItem(checked: nil, line: nil, blocks: [])
            // cmark misses a box in a list inside a quote; GitHub (and the
            // web app) render it: so do we.
            let raw = (li.children.first { _ in true } as? Paragraph)?.plainText ?? ""
            let missed = quotes > 0 && li.checkbox == nil && (raw.hasPrefix("[ ] ") || raw.hasPrefix("[x] ") || raw.hasPrefix("[X] "))
            if let box = li.checkbox {
                item.checked = box == .checked
            } else if missed {
                item.checked = !raw.hasPrefix("[ ]")
            }
            if item.checked != nil { item.line = li.range?.lowerBound.line }
            item.blocks = blocks(li.children)
            if missed, case let .paragraph(first)? = item.blocks.first, first.characters.count >= 4 {
                var rest = first
                rest.removeSubrange(rest.startIndex ..< rest.index(rest.startIndex, offsetByCharacters: 4))
                item.blocks[0] = .paragraph(rest)
            }
            return item
        }
    }

    func plain(_ s: String, _ st: Style, _ sz: CGFloat) -> AttributedString {
        var a = AttributedString(s)
        apply(&a, st, sz)
        return a
    }

    func apply(_ a: inout AttributedString, _ st: Style, _ sz: CGFloat) {
        var f = Font.system(size: st.code ? sz - 1 : sz, weight: st.bold ? .semibold : .regular, design: st.code ? .monospaced : .default)
        if st.italic { f = f.italic() }
        a.font = f
        if st.strike { a.strikethroughStyle = .single }
        if st.code { a.backgroundColor = K.recessed }
        if let link = st.link {
            a.link = link
            a.foregroundColor = K.link
        }
    }

    func inline(_ kids: [Markup], _ st: Style, _ sz: CGFloat) -> AttributedString {
        var out = AttributedString()
        for k in kids {
            switch k {
            case let t as Markdown.Text: out += st.link == nil && !st.code ? refs(t.string, st, sz) : plain(t.string, st, sz)
            case let e as Emphasis: var s = st; s.italic = true; out += inline(Array(e.children), s, sz)
            case let e as Strong: var s = st; s.bold = true; out += inline(Array(e.children), s, sz)
            case let e as Strikethrough: var s = st; s.strike = true; out += inline(Array(e.children), s, sz)
            case let c as InlineCode: var s = st; s.code = true; out += plain("\u{2009}\(c.code)\u{2009}", s, sz)
            case let l as Markdown.Link:
                var s = st
                // Only links that go somewhere safe: the web, mail, or a docket.
                if let d = l.destination, let u = URL(string: d), ["https", "http", "mailto", "wheelhouse"].contains(u.scheme?.lowercased() ?? "") { s.link = u }
                out += inline(Array(l.children), s, sz)
            case let i as Markdown.Image: out += plain(i.plainText.isEmpty ? "image" : i.plainText, st, sz)
            case is SoftBreak: out += plain(breaks ? "\n" : " ", st, sz)
            case is LineBreak: out += plain("\n", st, sz)
            case let h as InlineHTML:
                let raw = h.rawHTML.lowercased()
                if raw.hasPrefix("<br") { out += plain("\n", st, sz) } else if !raw.hasPrefix("<") || raw.hasPrefix("<!") { out += plain(h.rawHTML, st, sz) }
            case let c as any InlineContainer: out += inline(Array(c.children), st, sz)
            default: out += plain(k.format(), st, sz)
            }
        }
        return out
    }

    /// Text with each `D-12` as a link to its docket.
    func refs(_ s: String, _ st: Style, _ sz: CGFloat) -> AttributedString {
        guard s.contains("D-") else { return plain(s, st, sz) }
        var out = AttributedString()
        var rest = Substring(s)
        while let r = rest.range(of: #"(?<![\w/-])D-\d+\b"#, options: .regularExpression) {
            out += plain(String(rest[..<r.lowerBound]), st, sz)
            var linked = st
            linked.link = URL(string: "\(Flavor.scheme)://docket/\(rest[r])")
            out += plain(String(rest[r]), linked, sz)
            rest = rest[r.upperBound...]
        }
        out += plain(String(rest), st, sz)
        return out
    }

    func stripTags(_ s: String) -> String {
        s.replacingOccurrences(of: #"<br\s*/?>"#, with: "\n", options: .regularExpression)
            .replacingOccurrences(of: #"<[^>]+>"#, with: "", options: .regularExpression)
    }
}

// ---- the view ----

struct MarkdownView: View {
    let text: String
    var size: CGFloat = 13
    /// Tick a task box: its source line and its new state. Without it, boxes are read-only.
    var onTask: ((Int, Bool) -> Void)?
    /// Take the width it's given (a page), or only what the text needs (a bubble).
    var fill = true

    var body: some View {
        BlocksView(blocks: MD.blocks(text, size: size), size: size, onTask: onTask)
            .textSelection(.enabled)
            .frame(maxWidth: fill ? .infinity : nil, alignment: .leading)
    }
}

private struct BlocksView: View {
    let blocks: [MDBlock]
    let size: CGFloat
    let onTask: ((Int, Bool) -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: size * 0.7) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, b in
                BlockView(block: b, size: size, onTask: onTask)
            }
        }
    }
}

private struct BlockView: View {
    let block: MDBlock
    let size: CGFloat
    let onTask: ((Int, Bool) -> Void)?

    var body: some View {
        switch block {
        case let .paragraph(a):
            Text(a).foregroundStyle(K.text).lineSpacing(2.5).fixedSize(horizontal: false, vertical: true)
        case let .heading(level, a):
            Text(a).foregroundStyle(K.text).padding(.top, level <= 2 ? 4 : 2).fixedSize(horizontal: false, vertical: true)
        case let .list(ordered, start, items):
            VStack(alignment: .leading, spacing: size * 0.35) {
                ForEach(Array(items.enumerated()), id: \.offset) { i, item in
                    HStack(alignment: .firstTextBaseline, spacing: 7) {
                        if let checked = item.checked {
                            TaskBox(checked: checked, size: size) { if let l = item.line { onTask?(l, !checked) } }
                                .disabled(onTask == nil || item.line == nil)
                        } else {
                            Text(ordered ? "\(start + i)." : "•")
                                .font(.system(size: size, weight: ordered ? .regular : .bold).monospacedDigit())
                                .foregroundStyle(K.muted)
                                .frame(minWidth: ordered ? 16 : 8, alignment: .trailing)
                        }
                        BlocksView(blocks: item.blocks, size: size, onTask: onTask)
                    }
                }
            }
        case let .code(lang, text):
            CodeBlockView(lang: lang, text: text, size: size)
        case let .quote(inner):
            HStack(spacing: 10) {
                RoundedRectangle(cornerRadius: 1).fill(K.line2).frame(width: 3)
                BlocksView(blocks: inner, size: size, onTask: onTask).opacity(0.85)
            }
        case let .alert(kind, inner):
            let c = kind == "WARNING" ? K.blocked : kind == "CAUTION" ? K.error : kind == "TIP" ? K.working : kind == "IMPORTANT" ? Color.purple : K.accent
            HStack(spacing: 10) {
                RoundedRectangle(cornerRadius: 1).fill(c).frame(width: 3)
                VStack(alignment: .leading, spacing: 6) {
                    Label(MD.alertTitles[kind] ?? kind.capitalized, systemImage: kind == "WARNING" || kind == "CAUTION" ? "exclamationmark.triangle" : kind == "TIP" ? "lightbulb" : "info.circle")
                        .font(.system(size: size, weight: .semibold)).foregroundStyle(c)
                    BlocksView(blocks: inner, size: size, onTask: onTask)
                }
            }
        case let .table(head, rows, align):
            TableView(head: head, rows: rows, align: align)
        case .rule:
            Divider().padding(.vertical, 2)
        case let .image(url, alt):
            AsyncImage(url: url) { img in
                img.resizable().scaledToFit().frame(maxWidth: 560).clipShape(RoundedRectangle(cornerRadius: 6))
            } placeholder: {
                Text(alt.isEmpty ? "Image" : alt).foregroundStyle(K.muted)
            }
        }
    }
}

private struct TaskBox: View {
    let checked: Bool
    let size: CGFloat
    let toggle: () -> Void
    @Environment(\.isEnabled) private var enabled
    var body: some View {
        Button(action: toggle) {
            Image(systemName: checked ? "checkmark.square.fill" : "square")
                .font(.system(size: size + 1))
                .foregroundStyle(checked ? K.accent : K.faint)
        }
        .buttonStyle(.plain)
        .help(enabled ? (checked ? "Untick it: saves a new version of the intent" : "Tick it off: saves a new version of the intent") : "")
    }
}

private struct CodeBlockView: View {
    let lang: String?
    let text: String
    let size: CGFloat
    @State private var hover = false
    @State private var copied = false

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            Text(text)
                .font(.system(size: size - 1.5, design: .monospaced))
                .foregroundStyle(K.text)
                .lineSpacing(2)
                .padding(12)
                .fixedSize()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(K.recessed, in: RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(K.line))
        .overlay(alignment: .topTrailing) {
            HStack(spacing: 6) {
                if let lang { Text(lang).font(.system(size: 10.5, design: .monospaced)).foregroundStyle(K.faint) }
                if hover || copied {
                    Button {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(text, forType: .string)
                        copied = true
                        Task { try? await Task.sleep(for: .seconds(1.5)); copied = false }
                    } label: {
                        Image(systemName: copied ? "checkmark" : "doc.on.doc").font(.system(size: 11))
                    }
                    .buttonStyle(.borderless)
                    .help("Copy")
                }
            }
            .padding(7)
        }
        .onHover { hover = $0 }
    }
}

private struct TableView: View {
    let head: [AttributedString]
    let rows: [[AttributedString]]
    let align: [Int]

    private func a(_ i: Int) -> Alignment { i < align.count ? (align[i] == 1 ? .center : align[i] == 2 ? .trailing : .leading) : .leading }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    ForEach(Array(head.enumerated()), id: \.offset) { i, h in
                        Text(h).foregroundStyle(K.text).padding(.horizontal, 10).padding(.vertical, 6).frame(maxWidth: .infinity, alignment: a(i)).background(K.recessed)
                    }
                }
                ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                    Divider().gridCellUnsizedAxes(.horizontal)
                    GridRow {
                        ForEach(Array(row.enumerated()), id: \.offset) { i, c in
                            Text(c).foregroundStyle(K.text).padding(.horizontal, 10).padding(.vertical, 6).frame(maxWidth: .infinity, alignment: a(i)).fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
            }
            .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).strokeBorder(K.line))
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        }
    }
}
