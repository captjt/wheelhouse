// The outside of an app's window, the same for both: wait for it on screen,
// place it, send it keys and scrolling. Events go to the process only
// (CGEvent.postToPid), never the cursor or the frontmost app.
//
//   wintool wait PID SECONDS          the wall clock (ns) when its main window is on screen
//   wintool frame PID X Y W H         move and size its largest window
//   wintool key PID cmd+5             bring it forward, then send ⌘5
//   wintool scroll PID X Y SECS HZ PX scroll at screen point X,Y, down then up
import ApplicationServices
import Cocoa

func mainWindow(_ pid: pid_t) -> [String: Any]? {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    return list.first { w in
        guard (w[kCGWindowOwnerPID as String] as? pid_t) == pid, (w[kCGWindowLayer as String] as? Int) == 0 else { return false }
        let b = w[kCGWindowBounds as String] as? [String: CGFloat] ?? [:]
        return (b["Width"] ?? 0) >= 600 && (b["Height"] ?? 0) >= 400
    }
}

func now() -> UInt64 { clock_gettime_nsec_np(CLOCK_REALTIME) }

/// Bring it to the front through Accessibility: macOS's cooperative
/// activation ignores a request from an app that isn't active itself.
func front(_ pid: pid_t) {
    let app = AXUIElementCreateApplication(pid)
    AXUIElementSetAttributeValue(app, kAXFrontmostAttribute as CFString, kCFBooleanTrue)
    var v: CFTypeRef?
    AXUIElementCopyAttributeValue(app, kAXMainWindowAttribute as CFString, &v)
    if let v { AXUIElementPerformAction(v as! AXUIElement, kAXRaiseAction as CFString) }
}

func frontmost() -> pid_t { NSWorkspace.shared.frontmostApplication?.processIdentifier ?? 0 }

let a = CommandLine.arguments
let pid = pid_t(a[2])!
switch a[1] {
case "wait":
    let deadline = Date().addingTimeInterval(Double(a[3])!)
    while Date() < deadline {
        if mainWindow(pid) != nil { print(now()); exit(0) }
        usleep(1000)
    }
    exit(1)
case "frame":
    let app = AXUIElementCreateApplication(pid)
    var v: CFTypeRef?
    AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &v)
    let wins = (v as? [AXUIElement]) ?? []
    func area(_ w: AXUIElement) -> CGFloat {
        var s: CFTypeRef?
        AXUIElementCopyAttributeValue(w, kAXSizeAttribute as CFString, &s)
        var size = CGSize.zero
        if let s { AXValueGetValue(s as! AXValue, .cgSize, &size) }
        return size.width * size.height
    }
    guard let w = wins.max(by: { area($0) < area($1) }) else { print("no window"); exit(1) }
    var p = CGPoint(x: Double(a[3])!, y: Double(a[4])!)
    var s = CGSize(width: Double(a[5])!, height: Double(a[6])!)
    AXUIElementSetAttributeValue(w, kAXSizeAttribute as CFString, AXValueCreate(.cgSize, &s)!)
    AXUIElementSetAttributeValue(w, kAXPositionAttribute as CFString, AXValueCreate(.cgPoint, &p)!)
    AXUIElementSetAttributeValue(w, kAXSizeAttribute as CFString, AXValueCreate(.cgSize, &s)!)
    print("ok")
case "front":
    front(pid)
    usleep(300_000)
    print(frontmost() == pid ? "front" : "not front")
case "key":
    front(pid)
    usleep(300_000)
    let codes: [String: CGKeyCode] = ["1": 18, "2": 19, "3": 20, "4": 21, "5": 23, "6": 22, "7": 26, "8": 28]
    let key = String(a[3].split(separator: "+").last!)
    let src = CGEventSource(stateID: .hidSystemState)
    for down in [true, false] {
        let e = CGEvent(keyboardEventSource: src, virtualKey: codes[key]!, keyDown: down)!
        e.flags = .maskCommand
        e.postToPid(pid)
        usleep(30_000)
    }
    print("ok")
case "scroll":
    let at = CGPoint(x: Double(a[3])!, y: Double(a[4])!)
    let secs = Double(a[5])!, hz = Double(a[6])!, px = Int32(a[7])!
    let src = CGEventSource(stateID: .hidSystemState)
    let win = (mainWindow(pid)?[kCGWindowNumber as String] as? Int64) ?? 0
    let move = CGEvent(mouseEventSource: src, mouseType: .mouseMoved, mouseCursorPosition: at, mouseButton: .left)!
    move.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: win)
    move.postToPid(pid)
    usleep(100_000)
    let n = Int(secs * hz)
    var dir: Int32 = -1
    for i in 0..<n {
        if i > 0 && i % Int(hz) == 0 { dir = -dir }
        let lines = ProcessInfo.processInfo.environment["WINTOOL_LINES"] != nil
        let e = CGEvent(scrollWheelEvent2Source: src, units: lines ? .line : .pixel, wheelCount: 1, wheel1: dir * px, wheel2: 0, wheel3: 0)!
        if ProcessInfo.processInfo.environment["WINTOOL_CONTINUOUS"] != nil {
            e.setIntegerValueField(.scrollWheelEventIsContinuous, value: 1)
        }
        e.location = at
        e.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: win)
        e.setIntegerValueField(.mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: win)
        // Scrolling is routed by window position, not by process: post it
        // where the window is (the session tap leaves the cursor alone).
        if ProcessInfo.processInfo.environment["WINTOOL_PID_ONLY"] != nil { e.postToPid(pid) } else { e.post(tap: .cgSessionEventTap) }
        usleep(UInt32(1_000_000 / hz))
    }
    print("ok", n)
default:
    exit(2)
}
