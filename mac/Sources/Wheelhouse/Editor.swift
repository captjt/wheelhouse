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
    @State private var height: CGFloat = 0
    @Environment(\.isEnabled) private var enabled

    var body: some View {
        TextViewBox(text: $text, height: $height, returnSends: returnSends, font: font, focus: focus, enabled: enabled, onSubmit: onSubmit, onCancel: onCancel)
            .frame(height: min(max(height, minHeight), maxHeight))
            .overlay(alignment: .topLeading) {
                if text.isEmpty {
                    Text(placeholder).font(Font(font)).foregroundStyle(K.faint).padding(.leading, 5).padding(.top, 1).allowsHitTesting(false)
                }
            }
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

    func makeCoordinator() -> Coord { Coord(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSTextView.scrollableTextView()
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        let tv = scroll.documentView as! NSTextView
        tv.delegate = context.coordinator
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
        guard let tv = scroll.documentView as? NSTextView else { return }
        tv.isEditable = enabled
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
