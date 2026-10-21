// Native, PID-scoped driver for the packaged Abbey menu bar and window title.
//
// No application IPC and no window-server scripting: everything below is read
// through the macOS Accessibility API from the process that owns the bundle
// under test, so the observation belongs to that exact PID and to no other
// running copy of the app.
//
// Usage:
//   menu_icon-ax preflight          -> AX trust + interactive GUI probe
//   menu_icon-ax <pid> menu         -> AXMenuBar structure + AXWindow titles
//
// stdout carries a JSON envelope and never any application content; stderr
// carries a stable failure id and never observed data.
import AppKit
import ApplicationServices
import Foundation

var observedStrings: [String] = []
var observedBytes = 0
var discardedSnapshots = 0

struct DriverError: Error { let id: String }
func fail(_ id: String) throws -> Never { throw DriverError(id: id) }

func strings(_ value: Any) -> [String] {
    if let value = value as? String { return [value] }
    if let value = value as? NSAttributedString { return [value.string] }
    if let value = value as? [Any] { return value.flatMap(strings) }
    if let value = value as? [String: Any] { return value.values.flatMap(strings) }
    return []
}

func read(_ element: AXUIElement, _ name: String) throws -> CFTypeRef? {
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    if result == .noValue || result == .attributeUnsupported { return nil }
    guard result == .success else {
        let diagnosticName = name.range(of: "^AX[A-Za-z]{1,60}$", options: .regularExpression) == nil ? "attribute" : name
        try fail("ax_attribute_read_failed_\(result.rawValue)_\(diagnosticName)")
    }
    if let value {
        let found = strings(value)
        observedBytes += found.reduce(0) { $0 + $1.utf8.count }
        guard observedBytes <= 8_000_000 else { try fail("ax_traversal_truncated") }
        observedStrings.append(contentsOf: found)
    }
    return value
}

// AXUIElement is a distinct CF type, so a returned reference is checked by
// type id before it is used as an element rather than assumed.
func readElement(_ element: AXUIElement, _ name: String) throws -> AXUIElement? {
    guard let value = try read(element, name) else { return nil }
    guard CFGetTypeID(value) == AXUIElementGetTypeID() else { try fail("ax_element_type_invalid") }
    return (value as! AXUIElement)
}

struct MenuItemNode: Codable {
    let title: String
    let role: String
    let enabled: Bool?
}

struct MenuNode: Codable {
    let title: String
    let role: String
    let items: [MenuItemNode]
}

struct WindowNode: Codable {
    let title: String
    let role: String
    let subrole: String
}

struct Output: Codable {
    let complete: Bool
    let menus: [MenuNode]
    let windows: [WindowNode]
    let observedStrings: [String]
    let discardedSnapshots: Int
}

func emit(_ menus: [MenuNode], _ windows: [WindowNode]) throws {
    let data = try JSONEncoder().encode(Output(
        complete: true, menus: menus, windows: windows,
        observedStrings: observedStrings,
        discardedSnapshots: discardedSnapshots))
    FileHandle.standardOutput.write(data)
}

// The title of a menu, a menu item, or a window. AXTitle is the primary
// source; AXDescription is the documented fallback for elements that publish
// no title. The raw string is reported unchanged.
func label(_ element: AXUIElement) throws -> String {
    if let title = try read(element, kAXTitleAttribute) as? String { return title }
    if let description = try read(element, kAXDescriptionAttribute) as? String { return description }
    return ""
}

func role(_ element: AXUIElement) throws -> String {
    guard let value = try read(element, kAXRoleAttribute) as? String else {
        try fail("ax_role_missing")
    }
    return value
}

func readChildren(_ element: AXUIElement) throws -> [AXUIElement] {
    guard let value = try read(element, kAXChildrenAttribute) as? [AXUIElement] else {
        try fail("ax_children_missing")
    }
    return value
}

// The macOS menu bar is AXMenuBar -> AXMenuBarItem (one per top-level menu,
// carrying the visible title) -> AXMenu -> AXMenuItem (one per entry). The
// AXMenu itself publishes no title, so the top-level title is read from the
// menu bar item and the entries from the menu beneath it.
func readMenuBar(_ app: AXUIElement) throws -> [MenuNode] {
    guard let bar = try readElement(app, kAXMenuBarAttribute) else {
        try fail("ax_menu_bar_missing")
    }
    guard let childrenValue = try read(bar, kAXChildrenAttribute),
          let children = childrenValue as? [AXUIElement] else {
        try fail("ax_menu_bar_children_missing")
    }
    var menus: [MenuNode] = []
    for child in children {
        let childRole = try role(child)
        guard childRole == kAXMenuBarItemRole else { continue }
        var items: [MenuItemNode] = []
        let rawMenus = try readChildren(child)
        for menu in rawMenus {
            guard try role(menu) == kAXMenuRole else { continue }
            let rawItems = try readChildren(menu)
            for item in rawItems {
                let itemRole = try role(item)
                guard itemRole == kAXMenuItemRole else { continue }
                let enabled = (try read(item, kAXEnabledAttribute) as? Bool)
                items.append(MenuItemNode(title: try label(item), role: itemRole, enabled: enabled))
            }
        }
        menus.append(MenuNode(title: try label(child), role: childRole, items: items))
    }
    return menus
}

func readWindows(_ app: AXUIElement) throws -> [WindowNode] {
    guard let windowsValue = try read(app, kAXWindowsAttribute),
          let rawWindows = windowsValue as? [AXUIElement] else {
        try fail("ax_windows_missing")
    }
    return try rawWindows.map { window in
        WindowNode(title: try label(window), role: try role(window),
                   subrole: (try read(window, kAXSubroleAttribute) as? String) ?? "")
    }
}

do {
    guard AXIsProcessTrusted() else { try fail("accessibility_permission_required") }
    if CommandLine.arguments.count == 2 && CommandLine.arguments[1] == "preflight" {
        guard NSWorkspace.shared.frontmostApplication != nil else { try fail("interactive_gui_required") }
        print("{\"trusted\":true}")
        exit(0)
    }
    guard CommandLine.arguments.count >= 3, let pid = Int32(CommandLine.arguments[1]) else {
        try fail("owned_process_unavailable")
    }
    // A directly spawned bundle executable can precede LaunchServices' process
    // registration by a few hundred milliseconds. Retry that exact PID rather
    // than treating a normal startup race as process loss.
    var running: NSRunningApplication?
    let registrationDeadline = Date().addingTimeInterval(5)
    repeat {
        running = NSRunningApplication(processIdentifier: pid)
        if let running, !running.isTerminated { break }
        Thread.sleep(forTimeInterval: 0.05)
    } while Date() < registrationDeadline
    guard let running, !running.isTerminated else { try fail("owned_process_unavailable") }
    _ = running.activate(options: [.activateAllWindows])
    Thread.sleep(forTimeInterval: 0.15)
    let app = AXUIElementCreateApplication(pid)
    AXUIElementSetMessagingTimeout(app, 0.5)
    // The menu bar and the first window can arrive after the process does.
    // This readiness probe is not a snapshot and cannot satisfy any assertion.
    var menus: [MenuNode] = []
    var windows: [WindowNode] = []
    var lastError = "ax_menu_bar_missing"
    let deadline = Date().addingTimeInterval(10)
    repeat {
        observedStrings = []
        observedBytes = 0
        do {
            menus = try readMenuBar(app)
            windows = try readWindows(app)
            lastError = "ax_menu_bar_missing"
        } catch let error as DriverError {
            // Startup absence can be retried, but a failed or bounded-out
            // traversal must never be promoted into a complete observation.
            guard ["ax_menu_bar_missing", "ax_menu_bar_children_missing", "ax_windows_missing"].contains(error.id) else {
                throw error
            }
            lastError = error.id
            menus = []
            windows = []
        }
        if !menus.isEmpty && !windows.isEmpty { break }
        Thread.sleep(forTimeInterval: 0.1)
    } while Date() < deadline
    guard !menus.isEmpty else { try fail(lastError) }
    guard !windows.isEmpty else { try fail("ax_window_missing") }
    try emit(menus, windows)
} catch {
    // No success envelope is emitted for partial or truncated observations.
    // Never format AX data, application errors, or bearer values in diagnostics.
    let id = (error as? DriverError)?.id ?? "ax_driver_failed"
    FileHandle.standardError.write(Data((id + "\n").utf8))
    exit(1)
}
