import AppKit
import Foundation
import ImageIO
import Testing
import UniformTypeIdentifiers
@testable import Wheelhouse

// Images shared with a docket's agents (Images.swift, Markup.swift): what
// the composer sends, what the intent gets, and the marks drawn on them,
// as src/images.rs reads them.

private func png(_ w: Int, _ h: Int) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    return rep.representation(using: .png, properties: [:])!
}

private let d1 = "sha256:" + String(repeating: "a", count: 64)
private let d2 = "sha256:" + String(repeating: "b", count: 64)

private func staged(_ digest: String, _ name: String = "shot.png") -> TrayImage.Stage {
    .staged(StagedImage(digest: digest, name: name, width: 10, height: 10))
}

@Test func theComposerSendsStagedImagesWithWhatYouSaidAboutThem() {
    var a = TrayImage(name: "shot.png", data: Data([1]))
    a.stage = staged(d2, "shot-marked.png")
    a.caption = "  The header, like this \n"
    a.baseDigest = d1
    a.pins = [ImagePin(n: 1, note: " Make this primary ")]
    var plain = TrayImage(name: "x.png", data: Data([2]))
    plain.stage = staged(d1, "x.png")
    plain.baseDigest = d1 // not marked up after all: no original
    let body = Images.composeBody(text: "@builder match this", tray: [a, plain])
    #expect(body["text"] as? String == "@builder match this")
    let images = body["images"] as! [[String: Any]]
    #expect(images.count == 2)
    #expect(images[0]["digest"] as? String == d2)
    #expect(images[0]["name"] as? String == "shot-marked.png")
    #expect(images[0]["caption"] as? String == "The header, like this")
    #expect(images[0]["original"] as? String == d1)
    let pins = images[0]["pins"] as! [[String: Any]]
    #expect(pins[0]["n"] as? Int == 1 && pins[0]["note"] as? String == "Make this primary")
    #expect(images[1]["original"] == nil && images[1]["caption"] == nil && images[1]["pins"] == nil)
    // It serializes as the hub reads it.
    #expect(JSONSerialization.isValidJSONObject(body))
    // Text alone has no images key.
    #expect(Images.composeBody(text: "hi", tray: [])["images"] == nil)
}

@Test func sendingWaitsForEveryImageToBeStaged() {
    var a = TrayImage(name: "a.png", data: Data())
    #expect(!Images.ready([a]))
    a.stage = .failed("too big")
    #expect(!Images.ready([a]) && a.failure == "too big")
    #expect(Images.composeBody(text: "x", tray: [a])["images"] == nil, "a failed one is never sent")
    a.stage = staged(d1)
    #expect(Images.ready([a]) && Images.ready([]))
}

@Test func anIntentsImageIsMarkdownTheHubKeeps() {
    let s = StagedImage(digest: d1, name: "settings mock.png", width: 1, height: 1)
    #expect(Images.markdown(s, alt: "The [settings]\npage") == "![The settings page](/api/blobs/\(d1)?name=settings%20mock.png)")
    #expect(Images.blobURL("/api/blobs/\(d1)?name=a.png")?.absoluteString.hasSuffix("/api/blobs/\(d1)?name=a.png") == true)
    #expect(Images.blobURL("/api/blobs/sha256:abc") == nil)
    #expect(Images.blobURL("/api/dockets/D-1") == nil)
    #expect(Images.blobURL("https://example.com/a.png") == nil)
    #expect(StagedImage(JSON(any: ["digest": d1, "name": "a.png", "width": 3, "height": 2]))?.width == 3)
    #expect(StagedImage(JSON(any: ["error": "nope"])) == nil)
}

@Test func aDocketsImagesComeFromItsArtifactsAndEntries() {
    let d0 = "sha256:" + String(repeating: "c", count: 64)
    let art = Entry(j: JSON(any: ["seq": 7, "kind": "artifact", "body": "The header", "actor": ["type": "person", "id": "you"], "data": [
        "name": "shot.png", "media_type": "image/png", "digest": d1, "image": "reference", "caption": "The header", "width": 1440, "height": 900,
        "pins": [["n": 1, "note": "Bigger"]], "original": d0,
    ]]))
    let before = Entry(j: JSON(any: ["seq": 6, "kind": "artifact", "data": ["name": "original-shot.png", "media_type": "image/png", "digest": d0, "image": "original"]]))
    let shot = Entry(j: JSON(any: ["seq": 9, "kind": "artifact", "actor": ["type": "agent", "id": "claude-code@b1"], "data": ["name": "built.png", "media_type": "image/png", "digest": d2, "image": "attached", "against": 7]]))
    let svg = Entry(j: JSON(any: ["seq": 10, "kind": "artifact", "data": ["name": "x.svg", "media_type": "image/svg+xml", "digest": d2]]))
    let all = DocketImage.all([before, art, shot, svg])
    #expect(all.map(\.seq) == [9, 7, 6], "newest first, never SVG")
    let ref = all[1]
    #expect(ref.pins == [ImagePin(n: 1, note: "Bigger")] && ref.original == d0 && ref.roleWords == "Reference")
    #expect(all[0].against == 7 && all[0].roleWords == "An agent's")
    // Before markup and marked up find each other by digest.
    #expect(ref.originalImage(in: all)?.seq == 6)
    #expect(all[2].markedUp(in: all)?.seq == 7)
    #expect(all[0].originalImage(in: all) == nil && ref.markedUp(in: all) == nil)
    // A note's `data.images`: the reference with the same digest and caption.
    let carried = DocketImage(JSON(any: ["digest": d1, "name": "shot.png", "caption": "The header"]), known: all)
    #expect(carried?.seq == 7 && carried?.width == 1440)
    // Sent again with other words, or its artifact not here yet: as the entry names it.
    let again = DocketImage(JSON(any: ["digest": d1, "name": "shot.png", "caption": "Like this", "pins": [["n": 1, "note": "Primary"]], "width": 10, "height": 5, "original": d0]), known: all)
    #expect(again?.seq == 0 && again?.ref == "shot.png" && again?.caption == "Like this" && again?.pins == [ImagePin(n: 1, note: "Primary")] && again?.original == d0)
    #expect(again?.originalImage(in: all)?.seq == 6)
    #expect(DocketImage(JSON(any: ["name": "z.png"]), known: all) == nil)
    #expect(DocketImage(JSON(any: ["digest": "md5:x", "name": "z.png"]), known: all) == nil)
}

@Test func theActivityShowsAnImageOnceWithWhatWasSaid() {
    let d0 = "sha256:" + String(repeating: "c", count: 64)
    let note = Entry(j: JSON(any: ["seq": 4, "kind": "note", "data": ["images": [["digest": d1, "name": "a.png", "original": d0]]]]))
    let answer = Entry(j: JSON(any: ["seq": 5, "kind": "decision", "data": ["replies_to": 3, "images": [["digest": d2, "name": "b.png"]]]]))
    let carried = DocketImage.carried([note, answer])
    #expect(carried == [d1, d0, d2])
    let ref = Entry(j: JSON(any: ["seq": 6, "kind": "artifact", "data": ["digest": d1, "image": "reference", "media_type": "image/png"]]))
    let orig = Entry(j: JSON(any: ["seq": 7, "kind": "artifact", "data": ["digest": d0, "image": "original", "media_type": "image/png"]]))
    let shot = Entry(j: JSON(any: ["seq": 8, "kind": "artifact", "data": ["digest": d1, "image": "attached", "against": 6, "media_type": "image/png"]]))
    let alone = Entry(j: JSON(any: ["seq": 9, "kind": "artifact", "data": ["digest": "sha256:" + String(repeating: "e", count: 64), "image": "reference", "media_type": "image/png"]]))
    #expect(DocketImage.hiddenInActivity(ref, carried: carried))
    #expect(DocketImage.hiddenInActivity(orig, carried: carried))
    #expect(!DocketImage.hiddenInActivity(shot, carried: carried), "a screenshot against one still shows")
    #expect(!DocketImage.hiddenInActivity(alone, carried: carried))
    #expect(!DocketImage.hiddenInActivity(note, carried: carried))
}

@Test func pinsAreNumberedAndGoToTheAgentsInOrder() {
    let red = ImageMarks.defaultColor
    let marks = [
        MarkupMark(tool: .pin, color: red, points: [MarkupPoint(.zero)], text: "Second", n: 2),
        MarkupMark(tool: .box, color: red, points: [MarkupPoint(.zero), MarkupPoint(CGPoint(x: 50, y: 50))]),
        MarkupMark(tool: .pin, color: red, points: [MarkupPoint(.zero)], text: "First", n: 1),
    ]
    #expect(ImageMarks.pins(marks) == [ImagePin(n: 1, note: "First"), ImagePin(n: 2, note: "Second")])
    #expect(ImageMarks.nextPin(marks) == 3 && ImageMarks.nextPin([]) == 1)
    // A click with the box or arrow is no mark; a pin or a label is.
    #expect(ImageMarks.trivial(MarkupMark(tool: .box, color: red, points: [MarkupPoint(.zero), MarkupPoint(CGPoint(x: 1, y: 1))]), lw: 3))
    #expect(!ImageMarks.trivial(marks[1], lw: 3))
    #expect(!ImageMarks.trivial(marks[0], lw: 3))
    #expect(ImageMarks.trivial(MarkupMark(tool: .pen, color: red, points: [MarkupPoint(.zero)]), lw: 3))
    #expect(ImageMarks.lineWidth(CGSize(width: 2880, height: 1800)) > ImageMarks.lineWidth(CGSize(width: 400, height: 300)))
}

@MainActor @Test func markedUpItsAsSharpAsTheImage() {
    let base = png(640, 400)
    let marks = [
        MarkupMark(tool: .box, color: ImageMarks.defaultColor, points: [MarkupPoint(CGPoint(x: 10, y: 10)), MarkupPoint(CGPoint(x: 200, y: 120))]),
        MarkupMark(tool: .arrow, color: ImageMarks.colors[4], points: [MarkupPoint(CGPoint(x: 300, y: 300)), MarkupPoint(CGPoint(x: 220, y: 130))]),
        MarkupMark(tool: .text, color: ImageMarks.defaultColor, points: [MarkupPoint(CGPoint(x: 20, y: 300))], text: "Too tight"),
        MarkupMark(tool: .pin, color: ImageMarks.defaultColor, points: [MarkupPoint(CGPoint(x: 400, y: 80))], n: 1),
    ]
    let out = ImageMarks.render(base, marks: marks)
    #expect(out != nil)
    #expect(Images.pixelSize(out!) == CGSize(width: 640, height: 400), "the image's own pixels, not the screen's")
    #expect(out!.starts(with: [0x89, 0x50, 0x4E, 0x47]), "a PNG")
    #expect(ImageMarks.render(Data("not an image".utf8), marks: marks) == nil)
}

@MainActor @Test func aMarkedUpImageIsFittedAsAnyImageIs() async {
    let marks = [MarkupMark(tool: .pin, color: ImageMarks.defaultColor, points: [MarkupPoint(CGPoint(x: 40, y: 40))], n: 1)]
    let out = await ImageMarks.flatten(png(640, 400), name: "shot-marked.png", marks: marks)
    #expect(out?.name == "shot-marked.png" && out.flatMap { Images.pixelSize($0.data) } == CGSize(width: 640, height: 400))
    // Over the limit as a PNG: a JPEG, the same size.
    let big = await ImageMarks.flatten(png(640, 400), name: "shot-marked.png", marks: marks, maxBytes: 100)
    #expect(big?.name == "shot-marked.jpg" && big?.data.starts(with: [0xFF, 0xD8, 0xFF]) == true)
    #expect(big.flatMap { Images.pixelSize($0.data) } == CGSize(width: 640, height: 400))
}

/// An image in `type`, w×h, of something a codec can't squash to nothing.
private func encoded(_ w: Int, _ h: Int, _ type: UTType) -> Data? {
    guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
    ctx.setFillColor(CGColor(srgbRed: 1, green: 1, blue: 1, alpha: 1))
    ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
    ctx.setFillColor(CGColor(srgbRed: 0.02, green: 0.43, blue: 1, alpha: 1))
    for y in stride(from: 0, to: h, by: 400) { ctx.fill(CGRect(x: 20, y: y, width: w / 2, height: 40)) }
    guard let cg = ctx.makeImage() else { return nil }
    let out = NSMutableData()
    guard let dest = CGImageDestinationCreateWithData(out, type.identifier as CFString, 1, nil) else { return nil }
    CGImageDestinationAddImage(dest, cg, [kCGImageDestinationLossyCompressionQuality: 0.9] as CFDictionary)
    return CGImageDestinationFinalize(dest) ? out as Data : nil
}

private func size(_ d: Data?) -> (Int, Int)? {
    d.flatMap(Images.pixelSize).map { (Int($0.width), Int($0.height)) }
}

@Test func imagesFitWhatTheHubAndAgentsTake() {
    // The limits: a side, then the area.
    #expect(Images.fitted(1440, 900) == (1440, 900))
    #expect(Images.fitted(4000, 4000) == (4000, 4000), "16 MP exactly")
    let page = Images.fitted(1440, 24000)
    #expect(page.h <= Images.fitSide && page.w * page.h <= Images.fitPixels)
    #expect((975 ... 985).contains(page.w) && (16300 ... 16384).contains(page.h), "a full-page screenshot stays readable: \(page)")
    let wide = Images.fitted(20000, 400)
    #expect(wide == (16384, 328), "the short side rounded, as ImageIO does")
    let photo = Images.fitted(5712, 4284)
    #expect(photo.w * photo.h <= Images.fitPixels && photo.w > 4000, "\(photo)")
}

@Test func aScreenshotWithinTheLimitsGoesAsItIs() {
    let p = png(800, 500)
    #expect(Images.fit(name: "a.png", data: p)?.data == p)
    let j = encoded(800, 500, .jpeg)!
    #expect(Images.fit(name: "b.jpg", data: j)?.data == j)
    #expect(Images.fit(name: "x.png", data: Data("not an image".utf8)) == nil)
}

@Test func aLongScreenshotIsScaledAndStaysAPNG() throws {
    let tall = try #require(encoded(1440, 24000, .png))
    let out = try #require(Images.fit(name: "page.png", data: tall))
    #expect(out.name == "page.png" && out.data.starts(with: [0x89, 0x50, 0x4E, 0x47]))
    let (w, h) = try #require(size(out.data))
    #expect((975 ... 985).contains(w) && h <= Images.fitSide && w * h <= Images.fitPixels, "\(w)×\(h)")
    // Over the bytes as a PNG: a JPEG.
    let small = try #require(Images.fit(name: "page.png", data: png(300, 200), maxBytes: 100))
    #expect(small.name == "page.jpg" && small.data.starts(with: [0xFF, 0xD8, 0xFF]))
}

@Test func aPhotoGoesAsAJPEGThatFits() throws {
    // TIFF (or HEIC, where this Mac writes it): a JPEG at its size, or smaller.
    let tiff = try #require(encoded(800, 600, .tiff))
    let t = try #require(Images.fit(name: "Screen Shot.tiff", data: tiff))
    #expect(t.name == "Screen Shot.jpg" && t.data.starts(with: [0xFF, 0xD8, 0xFF]))
    #expect(size(t.data).map { $0 == (800, 600) } == true)
    if let heic = encoded(5712, 4284, .heic) {
        let h = try #require(Images.fit(name: "IMG_0042.HEIC", data: heic))
        let (w, hh) = try #require(size(h.data))
        #expect(h.name == "IMG_0042.jpg" && h.data.count <= Images.maxBytes && w * hh <= Images.fitPixels && w > 4000, "\(w)×\(hh)")
    }
    // A JPEG over the bytes: written again, smaller.
    let j = try #require(encoded(1200, 800, .jpeg))
    let again = try #require(Images.fit(name: "p.jpg", data: j, maxBytes: j.count - 1))
    #expect(again.name == "p.jpg" && size(again.data).map { $0 == (1200, 800) } == true)
}

@Test func anImageAddedWhileSendingStaysInTheTray() {
    var a = TrayImage(name: "a.png", data: Data([1]))
    a.stage = staged(d1)
    let b = TrayImage(name: "b.png", data: Data([2]))
    let c = TrayImage(name: "c.png", data: Data([3]))
    // a and b went; c was pasted while they were on their way.
    let after = Images.afterSend([a, b, c], sent: [a.id, b.id])
    #expect(after.map(\.id) == [c.id])
    #expect(Images.afterSend([a], sent: []).count == 1)
}
