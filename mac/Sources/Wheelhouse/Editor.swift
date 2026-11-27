import AppKit
import SwiftUI

/// A native text box that grows with its text: ⌘↩ sends (or ↩, for chat,
/// where ⇧↩ or ⌥↩ is a new line), Esc cancels, ⌘B and ⌘I wrap the
/// selection in Markdown's bold and italic.
struct Editor: View {
    @Binding var text: String
    var placeholder: String = ""
    var returnSends = false
    var minHeight: CGFloat = 54
    var maxHeight: CGFloat = 320
    var font: NSFont = .systemFont(ofSize: 13)
    var focus = false
    var onSubmit: () -> Void = {}
    var onCancel: (() -> Void)?
    /// Images pasted or dropped in: their names and bytes, and how to put
    /// text where the cursor was when they came (the intent's Markdown).
    /// Nil: the box takes text only.
    var onImages: ImagesIn?
    @State private var height: CGFloat = 0
    @Environment(\.isEnabled) private var enabled

    var body: some View {
        TextViewBox(text: $text, height: $height, returnSends: returnSends, font: font, focus: focus, enabled: enabled, onSubmit: onSubmit, onCancel: onCancel, onImages: onImages)
            .frame(height: min(max(height, minHeight), maxHeight))
            .overlay(alignment: .topLeading) {
                if text.isEmpty {
                    Text(placeholder).font(Font(font)).foregroundStyle(K.faint).padding(.leading, 5).padding(.top, 1).allowsHitTesting(false)
                }
            }
    }
}

/// Images pasted or dropped into an Editor, and how to insert text where
/// they came in.
typealias ImagesIn = (_ images: [(name: String, data: Data)], _ insert: @escaping (String) -> Void) -> Void

/// Editable only while it has the keyboard. On macOS 26 an editable text
/// view that has never had the keyboard keeps its insertion point
/// animating in WindowServer: a docket's note box cost WindowServer about a
/// third of a core for as long as the docket was on screen. A click, Tab or
/// `focus` makes it the first responder, and that makes it editable.
private final class NoteTextView: NSTextView {
    /// Whether it may be edited at all (the Editor's `isEnabled`).
    var wantsEditable = true {
        didSet { isEditable = wantsEditable && window?.firstResponder === self }
    }

    var onImages: ImagesIn?

    /// Hand images to `onImages`, with a way to put text where they came in.
    private func take(_ pb: NSPasteboard, at range: NSRange) -> Bool {
        guard let onImages, isEditable || wantsEditable else { return false }
        let images = Images.read(pb)
        guard !images.isEmpty else { return false }
        onImages(images) { [weak self] s in
            guard let self else { return }
            let len = (self.string as NSString).length
            let r = NSRange(location: min(range.location, len), length: 0)
            if self.shouldChangeText(in: r, replacementString: s) {
                self.replaceCharacters(in: r, with: s)
                self.didChangeText()
            }
        }
        return true
    }

    override func paste(_ sender: Any?) {
        if take(NSPasteboard.general, at: selectedRange()) { return }
        super.paste(sender)
    }

    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        if onImages != nil, Images.has(sender.draggingPasteboard) { return .copy }
        return super.draggingEntered(sender)
    }

    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
        if onImages != nil, Images.has(sender.draggingPasteboard) { return .copy }
        return super.draggingUpdated(sender)
    }

    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        if onImages != nil {
            let at = characterIndexForInsertion(at: convert(sender.draggingLocation, from: nil))
            if take(sender.draggingPasteboard, at: NSRange(location: at, length: 0)) { return true }
        }
        return super.performDragOperation(sender)
    }

    override func becomeFirstResponder() -> Bool {
        isEditable = wantsEditable
        return super.becomeFirstResponder()
    }

    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        if ok { isEditable = false }
        return ok
    }
}

private struct TextViewBox: NSViewRepresentable {
    @Binding var text: String
    @Binding var height: CGFloat
    let returnSends: Bool
    let font: NSFont
    let focus: Bool
    let enabled: Bool
    let onSubmit: () -> Void
    let onCancel: (() -> Void)?
    let onImages: ImagesIn?

    func makeCoordinator() -> Coord { Coord(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NoteTextView.scrollableTextView()
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        let tv = scroll.documentView as! NoteTextView
        tv.delegate = context.coordinator
        tv.wantsEditable = enabled
        tv.onImages = onImages
        if onImages != nil { tv.registerForDraggedTypes([.png, .tiff, .fileURL]) }
        tv.drawsBackground = false
        tv.isRichText = false
        tv.allowsUndo = true
        tv.font = font
        tv.textColor = .labelColor
        tv.textContainerInset = NSSize(width: 0, height: 1)
        tv.isAutomaticQuoteSubstitutionEnabled = false
        tv.isAutomaticDashSubstitutionEnabled = false
        tv.isAutomaticTextReplacementEnabled = false
        tv.string = text
        context.coordinator.view = tv
        if focus { DispatchQueue.main.async { tv.window?.makeFirstResponder(tv) } }
        DispatchQueue.main.async { context.coordinator.measure() }
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.parent = self
        guard let tv = scroll.documentView as? NoteTextView else { return }
        tv.wantsEditable = enabled
        tv.onImages = onImages
        tv.isSelectable = enabled
        if tv.string != text {
            // Set from outside (sent, so cleared; or a new draft): the
            // box's undo is about text that's gone now.
            context.coordinator.undo.removeAllActions()
            tv.string = text
            context.coordinator.measure()
        }
    }

    final class Coord: NSObject, NSTextViewDelegate {
        var parent: TextViewBox
        weak var view: NSTextView?
        /// Each box's own undo, not the window's: clearing one box never
        /// leaves another's undo pointing at text that's gone.
        let undo = UndoManager()
        init(_ p: TextViewBox) { parent = p }

        func undoManager(for view: NSTextView) -> UndoManager? { undo }

        func textDidChange(_ n: Notification) {
            guard let tv = view else { return }
            parent.text = tv.string
            measure()
        }

        func measure() {
            guard let tv = view, let lm = tv.layoutManager, let tc = tv.textContainer else { return }
            lm.ensureLayout(for: tc)
            let h = ceil(lm.usedRect(for: tc).height + tv.textContainerInset.height * 2) + 2
            if abs(h - parent.height) > 0.5 { DispatchQueue.main.async { self.parent.height = h } }
        }

        func textView(_ tv: NSTextView, doCommandBy sel: Selector) -> Bool {
            let event = NSApp.currentEvent
            let flags = event?.modifierFlags ?? []
            // ⌘↩ comes as noop:, not insertNewline:.
            if sel == Selector(("noop:")), flags.contains(.command), event?.keyCode == 36 || event?.keyCode == 76 {
                parent.onSubmit()
                return true
            }
            if sel == #selector(NSResponder.insertNewline(_:)) {
                if flags.contains(.command) || (parent.returnSends && !flags.contains(.shift) && !flags.contains(.option)) {
                    parent.onSubmit()
                    return true
                }
                if parent.returnSends {
                    tv.insertNewlineIgnoringFieldEditor(nil)
                    return true
                }
            }
            if sel == #selector(NSResponder.cancelOperation(_:)), let cancel = parent.onCancel {
                cancel()
                return true
            }
            return false
        }
    }
}

/// ⌘B and ⌘I in any Editor: wraps the selection, as the web app's editor does.
final class MarkdownKeys {
    static func install() {
        NSEvent.addLocalMonitorForEvents(matching: .keyDown) { e in
            guard e.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command,
                  let mark = e.charactersIgnoringModifiers == "b" ? "**" : e.charactersIgnoringModifiers == "i" ? "_" : nil,
                  let tv = NSApp.keyWindow?.firstResponder as? NSTextView, tv.isEditable, !tv.isFieldEditor
            else { return e }
            let r = tv.selectedRange()
            let s = (tv.string as NSString).substring(with: r)
            tv.insertText(mark + s + mark, replacementRange: r)
            tv.setSelectedRange(NSRange(location: r.location + mark.count, length: r.length))
            return nil
        }
    }
}
