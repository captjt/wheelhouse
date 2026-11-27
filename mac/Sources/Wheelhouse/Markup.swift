import AppKit
import SwiftUI

// Mark up an image before you send it: a box, an arrow, a pen line, a
// label, or a numbered pin with a note. Marks are kept in the image's own
// pixels, so the flattened PNG is as sharp as the image; the pins' notes go
// to the agents beside it (src/images.rs `Pin`).

/// A point in the image's pixels.
struct MarkupPoint: Hashable, Codable {
    var x: Double
    var y: Double
    var cg: CGPoint { CGPoint(x: x, y: y) }
    init(_ p: CGPoint) {
        x = p.x
        y = p.y
    }
}

struct MarkupColor: Hashable, Codable {
    var rgb: Int
    var color: Color { Color(nsColor: nsColor) }
    var nsColor: NSColor {
        NSColor(srgbRed: CGFloat((rgb >> 16) & 0xFF) / 255, green: CGFloat((rgb >> 8) & 0xFF) / 255, blue: CGFloat(rgb & 0xFF) / 255, alpha: 1)
    }
}

enum MarkupTool: String, CaseIterable, Codable {
    case box, arrow, pen, text, pin

    var symbol: String {
        switch self {
        case .box: "rectangle"
        case .arrow: "arrow.up.right"
        case .pen: "scribble"
        case .text: "textformat"
        case .pin: "1.circle"
        }
    }

    var words: String {
        switch self {
        case .box: "Box"
        case .arrow: "Arrow"
        case .pen: "Pen"
        case .text: "Text"
        case .pin: "Numbered pin"
        }
    }
}

struct MarkupMark: Hashable, Codable, Identifiable {
    var id = UUID()
    var tool: MarkupTool
    var color: MarkupColor
    var points: [MarkupPoint]
    /// A label's words, or a pin's note.
    var text = ""
    /// A pin's number.
    var n = 0
}

enum ImageMarks {
    /// A strong red, then the palette's others (Theme.swift, light).
    static let colors: [MarkupColor] = [0xE7000B, 0xDB6809, 0xE39F08, 0x009966, 0x056DFF, 0x18181B, 0xFFFFFF].map { MarkupColor(rgb: $0) }
    static let defaultColor = colors[0]

    /// A line's width, in the image's pixels: thicker on a bigger image.
    static func lineWidth(_ size: CGSize) -> CGFloat { max(3, max(size.width, size.height) / 260) }

    /// The pins, numbered, with their notes: what goes to the agents.
    static func pins(_ marks: [MarkupMark]) -> [ImagePin] {
        marks.filter { $0.tool == .pin }.sorted { $0.n < $1.n }.map { ImagePin(n: $0.n, note: $0.text) }
    }

    /// The next pin's number.
    static func nextPin(_ marks: [MarkupMark]) -> Int { (marks.filter { $0.tool == .pin }.map(\.n).max() ?? 0) + 1 }

    /// Whether a mark is too small to keep (a click with the box tool).
    static func trivial(_ m: MarkupMark, lw: CGFloat) -> Bool {
        switch m.tool {
        case .text, .pin: return false
        case .pen: return m.points.count < 2
        case .box, .arrow:
            guard let a = m.points.first, let b = m.points.last else { return true }
            return hypot(b.x - a.x, b.y - a.y) < Double(lw * 2)
        }
    }

    /// Draw the marks: `scale` maps the image's pixels to the context's.
    static func draw(_ marks: [MarkupMark], in ctx: GraphicsContext, imageSize: CGSize, scale: CGFloat) {
        let lw = lineWidth(imageSize) * scale
        func pt(_ p: MarkupPoint) -> CGPoint { CGPoint(x: p.x * scale, y: p.y * scale) }
        for m in marks {
            let c = m.color.color
            switch m.tool {
            case .box:
                guard let a = m.points.first, let b = m.points.last else { continue }
                let r = CGRect(x: min(a.x, b.x) * scale, y: min(a.y, b.y) * scale, width: abs(b.x - a.x) * scale, height: abs(b.y - a.y) * scale)
                ctx.stroke(Path(roundedRect: r, cornerRadius: lw * 1.2), with: .color(c), style: StrokeStyle(lineWidth: lw, lineJoin: .round))
            case .arrow:
                guard let a = m.points.first.map(pt), let b = m.points.last.map(pt) else { continue }
                let angle = atan2(b.y - a.y, b.x - a.x)
                let head = lw * 4.5
                let back = CGPoint(x: b.x - cos(angle) * head * 0.8, y: b.y - sin(angle) * head * 0.8)
                var shaft = Path()
                shaft.move(to: a)
                shaft.addLine(to: back)
                ctx.stroke(shaft, with: .color(c), style: StrokeStyle(lineWidth: lw, lineCap: .round))
                var tip = Path()
                tip.move(to: b)
                tip.addLine(to: CGPoint(x: b.x - cos(angle - .pi / 7) * head, y: b.y - sin(angle - .pi / 7) * head))
                tip.addLine(to: CGPoint(x: b.x - cos(angle + .pi / 7) * head, y: b.y - sin(angle + .pi / 7) * head))
                tip.closeSubpath()
                ctx.fill(tip, with: .color(c))
            case .pen:
                guard let first = m.points.first else { continue }
                var p = Path()
                p.move(to: pt(first))
                for q in m.points.dropFirst() { p.addLine(to: pt(q)) }
                ctx.stroke(p, with: .color(c), style: StrokeStyle(lineWidth: lw, lineCap: .round, lineJoin: .round))
            case .text:
                guard let at = m.points.first.map(pt), !m.text.isEmpty else { continue }
                let fs = lw * 5.5
                let t = ctx.resolve(Text(m.text).font(.system(size: fs, weight: .semibold)).foregroundColor(m.color.rgb == 0xFFFFFF ? .black : .white))
                let size = t.measure(in: CGSize(width: 4000 * scale, height: 2000 * scale))
                let pad = fs * 0.3
                let box = CGRect(x: at.x, y: at.y, width: size.width + pad * 2, height: size.height + pad)
                ctx.fill(Path(roundedRect: box, cornerRadius: pad), with: .color(c))
                ctx.draw(t, at: CGPoint(x: at.x + pad, y: at.y + pad / 2), anchor: .topLeading)
            case .pin:
                guard let at = m.points.first.map(pt) else { continue }
                let r = lw * 4.2
                let circle = Path(ellipseIn: CGRect(x: at.x - r, y: at.y - r, width: r * 2, height: r * 2))
                ctx.fill(circle, with: .color(c))
                ctx.stroke(circle, with: .color(.white), lineWidth: max(1, lw * 0.6))
                let t = ctx.resolve(Text("\(m.n)").font(.system(size: r * 1.1, weight: .bold)).foregroundColor(m.color.rgb == 0xFFFFFF ? .black : .white))
                ctx.draw(t, at: at, anchor: .center)
            }
        }
    }

    /// The image with its marks, flattened to a PNG at the image's own
    /// pixel size. Nil when it can't be read or drawn.
    @MainActor static func render(_ data: Data, marks: [MarkupMark]) -> Data? {
        guard let src = CGImageSourceCreateWithData(data as CFData, nil), let cg = CGImageSourceCreateImageAtIndex(src, 0, nil) else { return nil }
        let size = CGSize(width: cg.width, height: cg.height)
        let flat = Canvas { ctx, _ in
            ctx.draw(Image(decorative: cg, scale: 1), in: CGRect(origin: .zero, size: size))
            draw(marks, in: ctx, imageSize: size, scale: 1)
        }
        .frame(width: size.width, height: size.height)
        let r = ImageRenderer(content: flat)
        r.scale = 1
        r.proposedSize = ProposedViewSize(size)
        guard let out = r.cgImage else { return nil }
        return NSBitmapImageRep(cgImage: out).representation(using: .png, properties: [:])
    }

    /// The marked-up image to send, fitted as any image is (`Images.fit`):
    /// drawn here, on the main actor, fitted off it. Its name and bytes.
    static func flatten(_ data: Data, name: String, marks: [MarkupMark], maxBytes: Int = Images.maxBytes) async -> (name: String, data: Data)? {
        guard let png = await render(data, marks: marks) else { return nil }
        return await Task.detached(priority: .userInitiated) { Images.fit(name: name, data: png, maxBytes: maxBytes) }.value
    }
}

/// The markup editor, as a sheet: tools across the top, the image in the
/// middle, the pins' notes and labels' words beside it.
struct MarkupSheet: View {
    let data: Data
    let name: String
    var initial: [MarkupMark] = []
    /// The marks, when you press Done; nil when you cancel.
    let onDone: ([MarkupMark]?) -> Void

    @State private var image: CGImage?
    @State private var marks: [MarkupMark] = []
    @State private var undone: [[MarkupMark]] = []
    @State private var past: [[MarkupMark]] = []
    @State private var tool: MarkupTool = .box
    @State private var color = ImageMarks.defaultColor
    @State private var drawing: MarkupMark?
    @FocusState private var focused: UUID?

    var body: some View {
        VStack(spacing: 0) {
            toolbar
            Divider()
            HStack(spacing: 0) {
                canvas
                Divider()
                notes.frame(width: 260)
            }
            Divider()
            HStack {
                Text("Marks are drawn into the image the agents get; the original is kept beside it.").font(.system(size: 11.5)).foregroundStyle(K.faint)
                Spacer()
                Button("Cancel") { onDone(nil) }.keyboardShortcut(.cancelAction)
                Button("Done") { onDone(marks) }.keyboardShortcut(.defaultAction).buttonStyle(.borderedProminent).tint(K.accent)
            }
            .padding(12)
        }
        .frame(minWidth: 900, idealWidth: 1180, minHeight: 600, idealHeight: 780)
        .background(K.main)
        .onAppear {
            marks = initial
            if let src = CGImageSourceCreateWithData(data as CFData, nil) { image = CGImageSourceCreateImageAtIndex(src, 0, nil) }
        }
        .background {
            // ⌘Z and ⇧⌘Z, while no field has the keyboard.
            Button("") { undo() }.keyboardShortcut("z", modifiers: .command).hidden()
            Button("") { redo() }.keyboardShortcut("z", modifiers: [.command, .shift]).hidden()
        }
    }

    private var toolbar: some View {
        HStack(spacing: 10) {
            Text("Mark Up").font(.system(size: 13, weight: .semibold))
            Text(name).font(.system(size: 12)).foregroundStyle(K.muted).lineLimit(1)
            Spacer()
            Picker("Tool", selection: $tool) {
                ForEach(MarkupTool.allCases, id: \.self) { t in Image(systemName: t.symbol).help(t.words).tag(t) }
            }
            .pickerStyle(.segmented).labelsHidden().fixedSize()
            HStack(spacing: 5) {
                ForEach(ImageMarks.colors, id: \.self) { c in
                    Button { color = c } label: {
                        Circle().fill(c.color).frame(width: 16, height: 16)
                            .overlay(Circle().strokeBorder(c == color ? K.text : K.line2, lineWidth: c == color ? 2 : 1))
                    }
                    .buttonStyle(.plain)
                    .help(String(format: "#%06X", c.rgb))
                }
            }
            Divider().frame(height: 18)
            Button { undo() } label: { Image(systemName: "arrow.uturn.backward") }.disabled(past.isEmpty).help("Undo (⌘Z)")
            Button { redo() } label: { Image(systemName: "arrow.uturn.forward") }.disabled(undone.isEmpty).help("Redo (⇧⌘Z)")
            Button("Clear") { change { $0.removeAll() } }.disabled(marks.isEmpty)
        }
        .controlSize(.small)
        .padding(12)
    }

    private var canvas: some View {
        GeometryReader { g in
            if let image {
                let size = CGSize(width: image.width, height: image.height)
                let scale = max(min(g.size.width / size.width, g.size.height / size.height), 0.01)
                let shown = CGSize(width: size.width * scale, height: size.height * scale)
                let lw = ImageMarks.lineWidth(size)
                Canvas { ctx, _ in
                    ctx.draw(Image(decorative: image, scale: 1), in: CGRect(origin: .zero, size: shown))
                    ImageMarks.draw(marks + (drawing.map { [$0] } ?? []), in: ctx, imageSize: size, scale: scale)
                }
                .frame(width: shown.width, height: shown.height)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { v in
                            let p = MarkupPoint(CGPoint(x: clamp(v.location.x / scale, size.width), y: clamp(v.location.y / scale, size.height)))
                            if drawing == nil {
                                let start = MarkupPoint(CGPoint(x: clamp(v.startLocation.x / scale, size.width), y: clamp(v.startLocation.y / scale, size.height)))
                                drawing = MarkupMark(tool: tool, color: color, points: [start])
                            }
                            switch tool {
                            case .pen: drawing?.points.append(p)
                            case .box, .arrow: drawing?.points = [drawing?.points.first ?? p, p]
                            case .text, .pin: drawing?.points = [p]
                            }
                        }
                        .onEnded { _ in
                            guard var m = drawing else { return }
                            drawing = nil
                            if m.tool == .pin { m.n = ImageMarks.nextPin(marks) }
                            if m.tool == .text { m.text = "Label" }
                            guard !ImageMarks.trivial(m, lw: lw) else { return }
                            change { $0.append(m) }
                            if m.tool == .text || m.tool == .pin { focused = m.id }
                        }
                )
                .frame(width: g.size.width, height: g.size.height)
            } else {
                ProgressView().frame(width: g.size.width, height: g.size.height)
            }
        }
        .padding(12)
        .background(K.recessed)
    }

    private func clamp(_ v: CGFloat, _ max: CGFloat) -> Double { Double(min(Swift.max(v, 0), max)) }

    private var notes: some View {
        let listed = marks.filter { $0.tool == .pin || $0.tool == .text }
        return VStack(alignment: .leading, spacing: 8) {
            Text("Pins and labels").sectionLabel()
            if listed.isEmpty {
                Text("Pick the numbered pin and click the image: each pin's note goes to the agents with the image. A label is drawn into it.")
                    .font(.system(size: 12)).foregroundStyle(K.muted).fixedSize(horizontal: false, vertical: true)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(listed) { m in
                        HStack(alignment: .top, spacing: 8) {
                            if m.tool == .pin {
                                PinBadge(n: m.n, color: m.color.color, size: 20)
                            } else {
                                Image(systemName: "textformat").frame(width: 20, height: 20).foregroundStyle(m.color.color)
                            }
                            TextField(m.tool == .pin ? "What about it?" : "Label", text: binding(m.id), axis: .vertical)
                                .textFieldStyle(.roundedBorder)
                                .font(.system(size: 12.5))
                                .lineLimit(1 ... 4)
                                .focused($focused, equals: m.id)
                            Button { change { $0.removeAll { $0.id == m.id } } } label: { Image(systemName: "trash") }
                                .buttonStyle(.borderless).help("Remove it")
                        }
                    }
                }
            }
            Spacer(minLength: 0)
        }
        .padding(12)
    }

    private func binding(_ id: UUID) -> Binding<String> {
        Binding(
            get: { marks.first { $0.id == id }?.text ?? "" },
            set: { v in if let i = marks.firstIndex(where: { $0.id == id }) { marks[i].text = v } }
        )
    }

    private func change(_ f: (inout [MarkupMark]) -> Void) {
        past.append(marks)
        undone.removeAll()
        f(&marks)
    }

    private func undo() {
        guard let p = past.popLast() else { return }
        undone.append(marks)
        marks = p
    }

    private func redo() {
        guard let n = undone.popLast() else { return }
        past.append(marks)
        marks = n
    }
}
