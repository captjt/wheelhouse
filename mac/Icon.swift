// Draws Wheelhouse.app's icon at every size macOS asks for, into an
// .iconset folder for iconutil: `swift Icon.swift build/AppIcon.iconset`.
// The mark is the web app's: a ship's wheel in Cloudflare orange, on the
// dark panel of the social preview card.
import AppKit

let out = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "AppIcon.iconset")
try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> NSColor {
    NSColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255, blue: CGFloat(hex & 0xff) / 255, alpha: a)
}

func draw(_ px: Int) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let ctx = NSGraphicsContext.current!.cgContext
    let s = CGFloat(px) / 1024

    // Apple's grid: an 824-point body on a 1024 canvas, with a soft shadow.
    let body = CGRect(x: 100 * s, y: 100 * s, width: 824 * s, height: 824 * s)
    let shape = NSBezierPath(roundedRect: body, xRadius: 185 * s, yRadius: 185 * s)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -10 * s), blur: 24 * s, color: NSColor.black.withAlphaComponent(0.35).cgColor)
    rgb(0x111214).setFill()
    shape.fill()
    ctx.restoreGState()

    // The panel: near-black, lit from the top, with the orange glow behind the mark.
    ctx.saveGState()
    shape.addClip()
    NSGradient(colors: [rgb(0x26272b), rgb(0x0c0c0e)])!.draw(in: body, angle: -90)
    let glow = NSGradient(colors: [rgb(0xf6821f, 0.28), rgb(0xf6821f, 0)])!
    glow.draw(fromCenter: NSPoint(x: body.midX, y: body.midY), radius: 0, toCenter: NSPoint(x: body.midX, y: body.midY), radius: 430 * s, options: [])
    // A hairline on the top edge, as glass catches light.
    let rim = NSBezierPath(roundedRect: body.insetBy(dx: 1.5 * s, dy: 1.5 * s), xRadius: 184 * s, yRadius: 184 * s)
    rim.lineWidth = 3 * s
    rgb(0xffffff, 0.08).setStroke()
    rim.stroke()
    ctx.restoreGState()

    // The wheel, on the web logo's 32-unit grid.
    let u = 824 * s * 0.66 / 32
    let c = CGPoint(x: body.midX, y: body.midY)
    let orange = NSGradient(colors: [rgb(0xfbad41), rgb(0xf6821f), rgb(0xe8650c)])!
    let mark = NSBezierPath()
    mark.appendOval(in: CGRect(x: c.x - 11 * u, y: c.y - 11 * u, width: 22 * u, height: 22 * u))
    let ring = mark.copy() as! NSBezierPath
    ring.lineWidth = 3.2 * u
    var parts = [strokedPath(ring), NSBezierPath(ovalIn: CGRect(x: c.x - 3.4 * u, y: c.y - 3.4 * u, width: 6.8 * u, height: 6.8 * u))]
    for i in 0 ..< 6 {
        let a = CGFloat(i) * .pi / 3 + .pi / 2
        let spoke = NSBezierPath()
        spoke.move(to: CGPoint(x: c.x + cos(a) * 11 * u, y: c.y + sin(a) * 11 * u))
        spoke.line(to: CGPoint(x: c.x + cos(a) * 14 * u, y: c.y + sin(a) * 14 * u))
        spoke.lineWidth = 2.6 * u
        spoke.lineCapStyle = .round
        parts.append(strokedPath(spoke))
    }
    // Each part on its own: overlapping outlines in one path leave seams.
    let span = CGRect(x: c.x - 16 * u, y: c.y - 16 * u, width: 32 * u, height: 32 * u)
    ctx.saveGState()
    ctx.setShadow(offset: CGSize(width: 0, height: -6 * s), blur: 18 * s, color: rgb(0x000000, 0.45).cgColor)
    ctx.beginTransparencyLayer(auxiliaryInfo: nil)
    rgb(0xf6821f).setFill()
    parts.forEach { $0.fill() }
    ctx.endTransparencyLayer()
    ctx.restoreGState()
    for p in parts {
        ctx.saveGState()
        p.addClip()
        orange.draw(in: span, angle: -90)
        ctx.restoreGState()
    }

    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

/// A stroked path as a fillable outline, so one gradient covers every part.
func strokedPath(_ p: NSBezierPath) -> NSBezierPath {
    let cg = p.cgPath.copy(strokingWithWidth: p.lineWidth, lineCap: p.lineCapStyle == .round ? .round : .butt, lineJoin: .round, miterLimit: 10)
    return NSBezierPath(cgPath: cg)
}

for (name, px) in [("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64), ("128x128", 128), ("128x128@2x", 256), ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024)] {
    try! draw(px).write(to: out.appendingPathComponent("icon_\(name).png"))
}
print(out.path)
