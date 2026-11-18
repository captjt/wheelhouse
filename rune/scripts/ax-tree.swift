// What VoiceOver reads from a running app: its accessibility tree through the
// AX API (the same NSAccessibility answers VoiceOver asks for), as indented
// lines. Needs Accessibility permission for the terminal.
//
//   swift scripts/ax-tree.swift $(pgrep -n wheelhouse-rune)
import ApplicationServices
import Foundation

guard CommandLine.arguments.count > 1, let pid = pid_t(CommandLine.arguments[1]) else {
    print("usage: ax-tree.swift PID"); exit(2)
}
func attr(_ e: AXUIElement, _ name: String) -> AnyObject? {
    var v: AnyObject?
    return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? v : nil
}
func str(_ e: AXUIElement, _ name: String) -> String? {
    guard let v = attr(e, name) else { return nil }
    if let s = v as? String { return s.isEmpty ? nil : s }
    if let n = v as? NSNumber { return n.stringValue }
    return nil
}
func walk(_ e: AXUIElement, _ depth: Int) {
    guard depth < 14 else { return }
    let role = str(e, kAXRoleAttribute) ?? "?"
    var parts = [role]
    if let s = str(e, kAXSubroleAttribute) { parts.append("(\(s))") }
    for (label, key) in [("title", kAXTitleAttribute), ("desc", kAXDescriptionAttribute), ("value", kAXValueAttribute), ("help", kAXHelpAttribute), ("id", kAXIdentifierAttribute)] {
        if let s = str(e, key) { parts.append("\(label)=\(s.prefix(70))") }
    }
    if let sel = attr(e, kAXSelectedAttribute) as? Bool, sel { parts.append("selected") }
    if let f = attr(e, kAXFocusedAttribute) as? Bool, f { parts.append("focused") }
    print(String(repeating: "  ", count: depth) + parts.joined(separator: " "))
    for c in (attr(e, kAXChildrenAttribute) as? [AXUIElement]) ?? [] { walk(c, depth + 1) }
}
let app = AXUIElementCreateApplication(pid)
AXUIElementSetMessagingTimeout(app, 3)
walk(app, 0)
