import AppKit
import SwiftUI

// Cloudflare's Kumo tokens, as the web app uses them (web/src/styles.css),
// each with its light and dark value. The sidebar and bars are the system's
// own materials; the content panel and the marks are Kumo's.

private func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255, blue: CGFloat(hex & 0xff) / 255, alpha: a)
}

private func dyn(_ light: UInt32, _ dark: UInt32, _ la: CGFloat = 1, _ da: CGFloat = 1) -> Color {
    Color(nsColor: NSColor(name: nil) { ap in
        ap.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? rgb(dark, da) : rgb(light, la)
    })
}

enum K {
    static let main = dyn(0xffffff, 0x111111)
    static let elev = dyn(0xffffff, 0x171717)
    static let recessed = dyn(0xf5f5f5, 0x0b0b0b)
    static let group = dyn(0xf9f9f9, 0x0d0d0d)
    static let hover = dyn(0xf3f3f3, 0x1a1a1a)
    static let sel = dyn(0xededed, 0x1e1e1e)
    static let line = dyn(0xe9e9e9, 0x232323)
    static let line2 = dyn(0xd9d9d9, 0x333333)
    static let text = dyn(0x18181b, 0xf5f5f5)
    static let text2 = dyn(0x3d3d3d, 0xd1d1d1)
    static let muted = dyn(0x737373, 0xa1a1a1)
    static let faint = dyn(0xa1a1a1, 0x737373)
    /// Cloudflare orange; Wheelhouse Dev's is blue, as a nightly build recolors its mark.
    static let brand = Color(nsColor: rgb(Flavor.isDev ? 0x2f7dff : 0xf6821f))
    static let brand2 = Color(nsColor: rgb(Flavor.isDev ? 0x8fb8ff : 0xfbad41))
    static let accent = dyn(0x056dff, 0x2f7dff)
    static let accentSoft = dyn(0xdbeafe, 0x1c398e, 0.65, 0.28)
    static let link = dyn(0x193cb8, 0x51a2ff)
    static let working = dyn(0x009966, 0x00d492)
    static let blocked = dyn(0xdb6809, 0xff8904)
    static let error = dyn(0xe7000b, 0xff6467)
    static let progress = dyn(0xe39f08, 0xf2bb3b)
    static let you = Color(nsColor: rgb(0x155dfc))
    static let successTint = dyn(0xdcfce7, 0x0c542b, 0.8, 0.22)
    static let warningTint = dyn(0xffebd4, 0x573100, 0.85, 0.37)
    static let dangerTint = dyn(0xffe2e2, 0x990003, 0.7, 0.2)
    static let claude = Color(nsColor: rgb(0xd97757))
    static let codex = Color(nsColor: rgb(0x3b3d44))
    static let agy = Color(nsColor: rgb(0x4f6ef7))

    static func live(_ s: LiveState?) -> Color {
        switch s {
        case .working: working
        case .blocked: blocked
        case .error: error
        case .done: accent
        default: faint
        }
    }

    static func status(_ s: Status) -> Color {
        switch s {
        case .in_progress: progress
        case .done: accent
        default: faint
        }
    }

    static func risk(_ level: String?) -> Color {
        switch level?.lowercased() {
        case "low": working
        case "medium": progress
        case "high", "critical": error
        default: muted
        }
    }
}

// ---- marks ----

/// A docket's status, as Linear draws it.
struct StatusIcon: View {
    let status: Status
    var size: CGFloat = 14

    var body: some View {
        Canvas { ctx, sz in
            let s = sz.width / 14
            let c = K.status(status)
            let full = Path(ellipseIn: CGRect(x: 0.5 * s, y: 0.5 * s, width: 13 * s, height: 13 * s))
            let ring = Path(ellipseIn: CGRect(x: 1 * s, y: 1 * s, width: 12 * s, height: 12 * s))
            switch status {
            case .todo:
                ctx.stroke(ring, with: .color(c), lineWidth: 1.5 * s)
            case .in_progress:
                ctx.stroke(ring, with: .color(c), lineWidth: 1.5 * s)
                var half = Path()
                half.move(to: CGPoint(x: 7 * s, y: 3.5 * s))
                half.addArc(center: CGPoint(x: 7 * s, y: 7 * s), radius: 3.5 * s, startAngle: .degrees(-90), endAngle: .degrees(90), clockwise: false)
                half.closeSubpath()
                ctx.fill(half, with: .color(c))
            case .done:
                ctx.fill(full, with: .color(c))
                var tick = Path()
                tick.move(to: CGPoint(x: 4.3 * s, y: 7.2 * s))
                tick.addLine(to: CGPoint(x: 6.2 * s, y: 9 * s))
                tick.addLine(to: CGPoint(x: 9.8 * s, y: 5.2 * s))
                ctx.stroke(tick, with: .color(.white), style: StrokeStyle(lineWidth: 1.6 * s, lineCap: .round, lineJoin: .round))
            case .dropped:
                ctx.fill(full, with: .color(c))
                var x = Path()
                x.move(to: CGPoint(x: 5 * s, y: 5 * s))
                x.addLine(to: CGPoint(x: 9 * s, y: 9 * s))
                x.move(to: CGPoint(x: 9 * s, y: 5 * s))
                x.addLine(to: CGPoint(x: 5 * s, y: 9 * s))
                ctx.stroke(x, with: .color(.white), style: StrokeStyle(lineWidth: 1.5 * s, lineCap: .round))
            }
        }
        .frame(width: size, height: size)
        .accessibilityLabel(status.label)
    }
}

/// What an agent is doing: a pulse while it works, a mark when it needs you.
struct LiveIcon: View {
    let state: LiveState?
    var size: CGFloat = 14

    var body: some View {
        Group {
            switch state {
            case nil:
                Color.clear
            case .working?:
                Pulse(color: K.working)
            case .blocked?, .error?:
                Canvas { ctx, sz in
                    let s = sz.width / 14
                    let c = K.live(state)
                    ctx.fill(Path(roundedRect: CGRect(x: 1 * s, y: 1 * s, width: 12 * s, height: 12 * s), cornerRadius: 3.5 * s), with: .color(c))
                    var p = Path()
                    if state == .blocked {
                        p.move(to: CGPoint(x: 7 * s, y: 4 * s))
                        p.addLine(to: CGPoint(x: 7 * s, y: 7.6 * s))
                        ctx.stroke(p, with: .color(.white), style: StrokeStyle(lineWidth: 1.6 * s, lineCap: .round))
                        ctx.fill(Path(ellipseIn: CGRect(x: 6.05 * s, y: 9.05 * s, width: 1.9 * s, height: 1.9 * s)), with: .color(.white))
                    } else {
                        p.move(to: CGPoint(x: 5 * s, y: 5 * s))
                        p.addLine(to: CGPoint(x: 9 * s, y: 9 * s))
                        p.move(to: CGPoint(x: 9 * s, y: 5 * s))
                        p.addLine(to: CGPoint(x: 5 * s, y: 9 * s))
                        ctx.stroke(p, with: .color(.white), style: StrokeStyle(lineWidth: 1.5 * s, lineCap: .round))
                    }
                }
            default:
                Circle().fill(K.live(state)).padding(size * 0.28)
            }
        }
        .frame(width: size, height: size)
        .accessibilityLabel(state?.word ?? "")
    }
}

/// A working agent's pulse, drawn from the clock (an animation would also
/// move it whenever its row moves).
private struct Pulse: View {
    let color: Color
    var body: some View {
        TimelineView(.animation(minimumInterval: 1 / 24)) { ctx in
            let p = ctx.date.timeIntervalSinceReferenceDate.truncatingRemainder(dividingBy: 1.4) / 1.4
            ZStack {
                Circle().fill(color.opacity(0.35)).scaleEffect(0.45 + 0.55 * p).opacity(1 - p)
                Circle().fill(color).padding(3.5)
            }
        }
    }
}

/// An agent's app: Claude Code's coral C, Codex's slate X, Antigravity's blue A.
struct AppAvatar: View {
    let app: String?
    var size: CGFloat = 18
    var body: some View {
        let letter = app == "codex" ? "X" : app == "claude-code" || app == "claude" ? "C" : "A"
        let fill = app == "codex" ? K.codex : app == "claude-code" || app == "claude" ? K.claude : app == "agy" ? K.agy : K.faint
        RoundedRectangle(cornerRadius: size * 0.28, style: .continuous)
            .fill(fill)
            .overlay(Text(letter).font(.system(size: size * 0.55, weight: .semibold)).foregroundStyle(.white))
            .frame(width: size, height: size)
            .help(appName(app))
    }
}

struct PersonAvatar: View {
    let name: String
    var size: CGFloat = 18
    var body: some View {
        Circle().fill(K.you)
            .overlay(Text(String(name.first ?? "?").uppercased()).font(.system(size: size * 0.55, weight: .semibold)).foregroundStyle(.white))
            .frame(width: size, height: size)
    }
}

struct HubAvatar: View {
    var size: CGFloat = 18
    var body: some View {
        RoundedRectangle(cornerRadius: size * 0.28, style: .continuous)
            .fill(LinearGradient(colors: [K.brand2, K.brand], startPoint: .topLeading, endPoint: .bottomTrailing))
            .overlay(Logo(color: .white).padding(size * 0.19))
            .frame(width: size, height: size)
            .help(Flavor.name)
    }
}

/// DEV, as a nightly build marks itself: at Wheelhouse Dev's sidebar foot, in
/// its quick panel and menu bar panel, with the build it is under the pointer.
/// Nothing on the daily app.
struct DevBadge: View {
    var body: some View {
        if Flavor.isDev {
            Text("DEV")
                .font(.system(size: 9.5, weight: .heavy))
                .tracking(0.6)
                .foregroundStyle(.white)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(LinearGradient(colors: [K.brand2, K.brand], startPoint: .topLeading, endPoint: .bottomTrailing), in: Capsule())
                .help("Wheelhouse Dev\(Flavor.build.map { ": \($0)" } ?? ""). A copy of your dockets, with nothing live.")
                .accessibilityLabel("Wheelhouse Dev")
        }
    }
}

/// A Rex session: its first letter, in a hue of its own.
struct SessionIcon: View {
    let name: String
    var size: CGFloat = 16
    @Environment(\.colorScheme) private var scheme

    static func hue(_ name: String) -> Double {
        var h = 0
        for c in name.unicodeScalars { h = (h * 31 + Int(c.value)) % 360 }
        return Double((h * 7 + 200) % 360) / 360
    }

    var body: some View {
        let h = Self.hue(name)
        let dark = scheme == .dark
        let letter = String((name.hasPrefix("rex") ? String(name.dropFirst(3).drop { $0 == "-" }) : name).first ?? name.first ?? "?").uppercased()
        RoundedRectangle(cornerRadius: 4, style: .continuous)
            .fill(Color(hue: h, saturation: dark ? 0.45 : 0.75, brightness: dark ? 0.42 : 0.97).opacity(dark ? 0.75 : 1))
            .overlay(RoundedRectangle(cornerRadius: 4, style: .continuous).strokeBorder(Color(hue: h, saturation: 0.55, brightness: dark ? 0.7 : 0.85).opacity(0.5), lineWidth: 1))
            .overlay(Text(letter).font(.system(size: size * 0.62, weight: .semibold)).foregroundStyle(Color(hue: h, saturation: dark ? 0.45 : 0.75, brightness: dark ? 0.98 : 0.52)))
            .frame(width: size, height: size)
    }
}

/// The wheel: Wheelhouse's mark.
struct Logo: View {
    var color: Color = K.brand
    var body: some View {
        Canvas { ctx, sz in
            let u = min(sz.width, sz.height) / 32
            let c = CGPoint(x: sz.width / 2, y: sz.height / 2)
            ctx.stroke(Path(ellipseIn: CGRect(x: c.x - 11 * u, y: c.y - 11 * u, width: 22 * u, height: 22 * u)), with: .color(color), lineWidth: 3.2 * u)
            ctx.fill(Path(ellipseIn: CGRect(x: c.x - 3.4 * u, y: c.y - 3.4 * u, width: 6.8 * u, height: 6.8 * u)), with: .color(color))
            var spokes = Path()
            for i in 0 ..< 6 {
                let a = Double(i) * .pi / 3 - .pi / 2
                spokes.move(to: CGPoint(x: c.x + cos(a) * 11 * u, y: c.y + sin(a) * 11 * u))
                spokes.addLine(to: CGPoint(x: c.x + cos(a) * 14 * u, y: c.y + sin(a) * 14 * u))
            }
            ctx.stroke(spokes, with: .color(color), style: StrokeStyle(lineWidth: 2.6 * u, lineCap: .round))
        }
    }
}

// ---- small pieces ----

/// A docket id, in the list's quiet monospace.
struct IdText: View {
    let id: String
    var body: some View {
        Text(id).font(.system(size: 11.5, design: .monospaced)).foregroundStyle(.secondary)
    }
}

/// A rounded tag: a verdict, a risk, a phase.
struct Chip: View {
    let text: String
    var color: Color = K.muted
    var filled = false
    var body: some View {
        Text(text)
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(filled ? .white : color)
            .padding(.horizontal, 6)
            .padding(.vertical, 1.5)
            .lineLimit(1)
            .fixedSize()
            .background(filled ? color : color.opacity(0.12), in: Capsule())
    }
}

/// A status line under a panel: tinted by what it's about.
struct Callout<Content: View>: View {
    let tone: LiveState
    @ViewBuilder var content: Content
    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            LiveIcon(state: tone, size: 16).padding(.top, 1)
            content
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tone == .error ? K.dangerTint : K.warningTint, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder((tone == .error ? K.error : K.blocked).opacity(0.25)))
    }
}

/// A card on the content panel.
struct Panel<Content: View>: View {
    var alert = false
    @ViewBuilder var content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 10) { content }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(K.elev, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 12, style: .continuous).strokeBorder(alert ? K.blocked.opacity(0.45) : K.line))
    }
}

extension View {
    /// A section's small caps label, as the web app's are.
    func sectionLabel() -> some View {
        font(.system(size: 11, weight: .semibold)).foregroundStyle(K.muted).textCase(.uppercase).kerning(0.4)
    }
}
