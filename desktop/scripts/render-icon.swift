// Abbey application-icon renderer (1024 master, CoreGraphics via AppKit).
//
// Renders the macOS/Windows application icon from vector geometry so the
// retina chain is sharp at every size. The shape is an abbey doorway: a
// Romanesque arch (white) opening onto depth inside a pre-rounded squircle,
// on a blue gradient in the product's existing palette. Big Sur convention:
// the icon body is 824x824 centered on a 1024 transparent canvas.
//
// Usage: xcrun swiftc render-icon.swift -o /tmp/abbey-icon-render && \
//        /tmp/abbey-icon-render <output-master.png>
// Then regenerate every asset from the master: bun run tauri icon <master>.
import AppKit

let size: CGFloat = 1024
let arguments = CommandLine.arguments
guard arguments.count == 2 else {
    fputs("usage: render-icon <master.png>\n", stderr)
    exit(2)
}
let output = URL(fileURLWithPath: arguments[1])

let rep = NSBitmapImageRep(
    bitmapDataPlanes: nil, pixelsWide: Int(size), pixelsHigh: Int(size),
    bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
    colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
let context = NSGraphicsContext(bitmapImageRep: rep)!
NSGraphicsContext.current = context

// Icon body: 824x824 squircle, Apple margin of 100 on every side.
let body = NSBezierPath(
    roundedRect: NSRect(x: 100, y: 100, width: 824, height: 824),
    xRadius: 186, yRadius: 186)
let top = NSColor(srgbRed: 0.318, green: 0.388, blue: 0.937, alpha: 1)   // #5163EF
let bottom = NSColor(srgbRed: 0.149, green: 0.176, blue: 0.580, alpha: 1) // #262D94
NSGradient(starting: top, ending: bottom)!.draw(in: body, angle: -90)

// Romanesque arch: vertical shafts capped by a semicircle. AppKit angles are
// CCW-in-math in the default bottom-left origin, so the cap runs 0° -> 180°
// (right spring point *up over the crown* to the left spring point).
func arch(_ x0: CGFloat, _ x1: CGFloat, _ baseY: CGFloat, _ springY: CGFloat) -> NSBezierPath {
    let path = NSBezierPath()
    let radius = (x1 - x0) / 2
    let center = NSPoint(x: (x0 + x1) / 2, y: springY)
    path.move(to: NSPoint(x: x1, y: baseY))
    path.line(to: NSPoint(x: x1, y: springY))
    path.appendArc(withCenter: center, radius: radius,
                   startAngle: 0, endAngle: 180, clockwise: false)
    path.line(to: NSPoint(x: x0, y: baseY))
    path.close()
    return path
}

// White doorway, centered horizontally on the body.
NSColor(white: 1, alpha: 1).setFill()
arch(342, 682, 236, 426).fill()
// Inner opening: depth, tinted toward the bottom of the gradient so the
// doorway reads at every size from 16 px up.
NSColor(srgbRed: 0.106, green: 0.125, blue: 0.420, alpha: 1).setFill() // #1B206B
arch(396, 628, 288, 406).fill()

NSGraphicsContext.restoreGraphicsState()
let data = rep.representation(using: .png, properties: [:])!
try! data.write(to: output)
print("rendered \(output.path)")
