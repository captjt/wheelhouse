import AppKit
import Foundation
import Testing
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
    let art = Entry(j: JSON(any: ["seq": 7, "kind": "artifact", "body": "The header", "actor": ["type": "person", "id": "you"], "data": [
        "name": "shot.png", "media_type": "image/png", "digest": d1, "image": "reference", "caption": "The header", "width": 1440, "height": 900,
        "pins": [["n": 1, "note": "Bigger"]], "original": 6,
    ]]))
    let shot = Entry(j: JSON(any: ["seq": 9, "kind": "artifact", "actor": ["type": "agent", "id": "claude-code@b1"], "data": ["name": "built.png", "media_type": "image/png", "digest": d2, "image": "attached", "against": 7]]))
    let svg = Entry(j: JSON(any: ["seq": 10, "kind": "artifact", "data": ["name": "x.svg", "media_type": "image/svg+xml", "digest": d2]]))
    let all = DocketImage.all([art, shot, svg])
    #expect(all.map(\.seq) == [9, 7], "newest first, never SVG")
    #expect(all[1].pins == [ImagePin(n: 1, note: "Bigger")] && all[1].original == 6 && all[1].roleWords == "Reference")
    #expect(all[0].against == 7 && all[0].roleWords == "An agent's")
    // A note's `data.images`, filled in from its artifact when it's known.
    let carried = DocketImage(JSON(any: ["seq": 7, "digest": d1, "name": "shot.png"]), known: all)
    #expect(carried?.caption == "The header" && carried?.width == 1440)
    // Sent again with other words: the entry's stand, the artifact fills in the rest.
    let again = DocketImage(JSON(any: ["seq": 7, "digest": d1, "name": "shot.png", "caption": "Like this", "pins": [["n": 1, "note": "Primary"]]]), known: all)
    #expect(again?.caption == "Like this" && again?.pins == [ImagePin(n: 1, note: "Primary")] && again?.width == 1440 && again?.original == 6)
    let unknown = DocketImage(JSON(any: ["seq": 3, "digest": d2, "name": "y.png", "caption": "Inspiration"]), known: all)
    #expect(unknown?.label == "Inspiration")
    #expect(DocketImage(JSON(any: ["name": "z.png"]), known: all) == nil)
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

@Test func picturesAgentsCantOpenGoAsPNG() {
    let rep = NSBitmapImageRep(data: png(8, 6))!
    let tiff = rep.tiffRepresentation!
    let (name, data) = Images.normalized(name: "Screen Shot.tiff", data: tiff)
    #expect(name == "Screen Shot.png")
    #expect(data.starts(with: [0x89, 0x50, 0x4E, 0x47]))
    #expect(Images.pixelSize(data) == CGSize(width: 8, height: 6))
    let p = png(4, 4)
    #expect(Images.normalized(name: "a.png", data: p).data == p, "a PNG is sent as it is")
    #expect(Images.thumbnail(png(800, 400), max: 100).map { $0.size.width <= 100 } == true)
}
