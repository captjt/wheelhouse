import AppKit
import Carbon
import SwiftUI

/// ⌥⌘K from any app: Wheelhouse's palette, floating over whatever you're
/// in. Approve a prompt, pick an agent's answer, find a docket, or start
/// one, without switching apps. The hot key is the system's own (Carbon),
/// so it needs no Accessibility permission. Settings › General turns it off.
@MainActor
enum QuickPanel {
    private static var panel: Panel?
    private static var hotKey: EventHotKeyRef?
    private static var handler: EventHandlerRef?

    static var enabled: Bool { UserDefaults.standard.object(forKey: "quickPanel") as? Bool ?? true }

    /// Register ⌥⌘K, or take it away, as Settings says.
    static func apply() {
        if enabled { register() } else { unregister() }
    }

    private static func register() {
        guard hotKey == nil else { return }
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, _, _ in
            DispatchQueue.main.async { QuickPanel.toggle() }
            return noErr
        }, 1, &spec, nil, &handler)
        let id = EventHotKeyID(signature: OSType(0x5748_4B31), id: 1) // "WHK1"
        RegisterEventHotKey(UInt32(kVK_ANSI_K), UInt32(cmdKey | optionKey), id, GetApplicationEventTarget(), 0, &hotKey)
    }

    private static func unregister() {
        if let hotKey { UnregisterEventHotKey(hotKey) }
        if let handler { RemoveEventHandler(handler) }
        hotKey = nil
        handler = nil
    }

    static func toggle() {
        show(key: true)
    }

    /// The panel itself, for snapshots: there, it never takes the keyboard.
    static var window: NSWindow? { panel }

    static func show(key: Bool) {
        if key, let p = panel, p.isVisible {
            p.orderOut(nil)
            return
        }
        let p = panel ?? make()
        panel = p
        // In the top third of the screen the mouse is on, as Spotlight sits.
        let screen = NSScreen.screens.first { NSMouseInRect(NSEvent.mouseLocation, $0.frame, false) } ?? NSScreen.main
        if let f = screen?.visibleFrame {
            p.setFrameOrigin(NSPoint(x: f.midX - p.frame.width / 2, y: f.maxY - f.height / 4 - p.frame.height + 60))
        }
        if key { p.makeKeyAndOrderFront(nil) } else { p.orderFront(nil) }
        NotificationCenter.default.post(name: .paletteShown, object: nil)
    }

    static func close() {
        panel?.orderOut(nil)
    }

    private static func make() -> Panel {
        let p = Panel(contentRect: NSRect(x: 0, y: 0, width: 648, height: 520), styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        p.isFloatingPanel = true
        p.level = .floating
        p.hidesOnDeactivate = false
        p.isOpaque = false
        p.backgroundColor = .clear
        p.hasShadow = false
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .transient]
        let root = PaletteOverlay(floating: true, onClose: { QuickPanel.close() })
            .environment(Store.shared)
            .environment(Nav.main)
            .frame(width: 648, height: 520, alignment: .top)
        p.contentView = NSHostingView(rootView: root)
        return p
    }

    /// A panel that takes the keyboard without bringing the app forward,
    /// and goes away when you click anywhere else.
    final class Panel: NSPanel {
        override var canBecomeKey: Bool { true }
        override func resignKey() {
            super.resignKey()
            orderOut(nil)
        }

        override func cancelOperation(_ sender: Any?) {
            orderOut(nil)
        }
    }
}
