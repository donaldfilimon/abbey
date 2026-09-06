// Native, PID-scoped driver for the ordinary Tauri window. No application IPC.
import AppKit
import ApplicationServices
import Foundation

struct DriverError: Error { let id: String }
func fail(_ id: String) throws -> Never { throw DriverError(id: id) }
func read(_ element: AXUIElement, _ name: String) throws -> CFTypeRef? {
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    if result == .noValue || result == .attributeUnsupported { return nil }
    guard result == .success else { try fail("ax_attribute_read_failed") }
    return value
}
func strings(_ value: Any) -> [String] {
    if let value = value as? String { return [value] }
    if let value = value as? NSAttributedString { return [value.string] }
    if let value = value as? [Any] { return value.flatMap(strings) }
    if let value = value as? [String: Any] { return value.values.flatMap(strings) }
    return []
}
struct Node: Codable {
    let role: String
    let attributes: [String: [String]]
}
struct Snapshot: Codable {
    let complete: Bool
    let nodes: [Node]
}
struct Tree {
    var nodes: [Node] = []
    var elements: [AXUIElement] = []
    var textBytes = 0
    mutating func walk(_ element: AXUIElement, depth: Int = 0) throws {
        guard depth < 128, nodes.count < 15000 else { try fail("ax_traversal_truncated") }
        if elements.contains(where: { CFEqual($0, element) }) { return }
        var names: CFArray?
        let inventoryResult = AXUIElementCopyAttributeNames(element, &names)
        guard inventoryResult == .success, let names = names as? [String] else {
            try fail("ax_attribute_inventory_failed_\(inventoryResult.rawValue)_depth_\(depth)")
        }
        var attributes: [String: [String]] = [:]
        for name in names {
            if let value = try read(element, name) {
                let found = strings(value)
                textBytes += found.reduce(0) { $0 + $1.utf8.count }
                guard textBytes <= 2_000_000 else { try fail("ax_traversal_truncated") }
                if !found.isEmpty { attributes[name] = found }
            }
        }
        let role = attributes["AXRole"]?.first ?? ""
        elements.append(element)
        nodes.append(Node(role: role, attributes: attributes))
        if let value = try read(element, kAXChildrenAttribute) {
            guard let children = value as? [AXUIElement] else { try fail("ax_children_invalid") }
            for child in children { try walk(child, depth: depth + 1) }
        }
    }
    var snapshot: Snapshot { Snapshot(complete: true, nodes: nodes) }
    func matching(_ roles: [String], _ title: String) -> [AXUIElement] {
        zip(nodes, elements).compactMap { node, element in
            let labels = ["AXTitle", "AXDescription", "AXValue"].flatMap { node.attributes[$0] ?? [] }
            return roles.contains(node.role) && labels.contains(title) ? element : nil
        }
    }
}
func tree(_ app: AXUIElement) throws -> Tree {
    var result = Tree()
    try result.walk(app)
    guard result.nodes.contains(where: { $0.role == "AXWindow" }) else { try fail("ax_window_missing") }
    return result
}
func press(_ elements: [AXUIElement]) throws {
    guard elements.count == 1 else { try fail("ax_control_missing_or_ambiguous") }
    guard AXUIElementPerformAction(elements[0], kAXPressAction as CFString) == .success else {
        try fail("ax_press_failed")
    }
}

do {
    guard AXIsProcessTrusted() else { try fail("accessibility_permission_required") }
    if CommandLine.arguments.count == 2 && CommandLine.arguments[1] == "preflight" {
        guard NSWorkspace.shared.frontmostApplication != nil else { try fail("interactive_gui_required") }
        print("{\"trusted\":true}")
        exit(0)
    }
    guard CommandLine.arguments.count >= 3, let pid = Int32(CommandLine.arguments[1]),
          let running = NSRunningApplication(processIdentifier: pid), !running.isTerminated else {
        try fail("owned_process_unavailable")
    }
    let app = AXUIElementCreateApplication(pid)
    AXUIElementSetMessagingTimeout(app, 0.5)
    // A launched PID can precede the first registered AppKit window. This
    // readiness probe is not a snapshot and cannot satisfy any assertion.
    var windows: CFTypeRef?
    guard AXUIElementCopyAttributeValue(app, kAXWindowsAttribute as CFString, &windows) == .success,
          let windows = windows as? [AXUIElement], !windows.isEmpty else {
        try fail("ax_window_missing")
    }
    // Ask WebKit to expose its ordinary accessibility tree to assistive technology.
    _ = AXUIElementSetAttributeValue(app, "AXManualAccessibility" as CFString, kCFBooleanTrue)
    if CommandLine.arguments[2] != "snapshot" { _ = running.activate(options: []) }
    let initial = try tree(app)
    var snapshots = [initial.snapshot]
    switch CommandLine.arguments[2] {
    case "snapshot": break
    case "routes":
        try press(initial.matching(["AXButton"], "Routes"))
    case "limit":
        guard CommandLine.arguments.count == 4,
              ["10", "25", "50"].contains(CommandLine.arguments[3]) else { try fail("invalid_limit") }
        let popups = zip(initial.nodes, initial.elements).compactMap { node, element in
            node.role == "AXPopUpButton" ? element : nil
        }
        try press(popups)
        let deadline = Date().addingTimeInterval(5)
        var selected = false
        repeat {
            Thread.sleep(forTimeInterval: 0.1)
            let menu = try tree(app)
            snapshots.append(menu.snapshot)
            let items = menu.matching(["AXMenuItem"], "last " + CommandLine.arguments[3])
            if !items.isEmpty { try press(items); selected = true; break }
        } while Date() < deadline
        guard selected else { try fail("ax_limit_option_missing") }
    default: try fail("invalid_action")
    }
    let data = try JSONEncoder().encode(snapshots)
    FileHandle.standardOutput.write(data)
} catch {
    // Never format AX data, application errors, or bearer values in diagnostics.
    let id = (error as? DriverError)?.id ?? "ax_driver_failed"
    FileHandle.standardError.write(Data((id + "\n").utf8))
    exit(1)
}
