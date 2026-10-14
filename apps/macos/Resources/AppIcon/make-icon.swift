// Renders the Miiting app icon. Run: swift apps/macos/Resources/AppIcon/make-icon.swift <out.iconset>
import AppKit

let out = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)
try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

func render(_ px: Int) -> Data {
    let s = CGFloat(px)
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                               bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    // macOS icon grid: 824/1024 rounded square, centered, with a soft shadow.
    let inset = s * 100 / 1024
    let rect = NSRect(x: inset, y: inset, width: s - 2 * inset, height: s - 2 * inset)
    let radius = rect.width * 0.225
    let shadow = NSShadow()
    shadow.shadowColor = NSColor.black.withAlphaComponent(0.28)
    shadow.shadowBlurRadius = s * 0.025
    shadow.shadowOffset = NSSize(width: 0, height: -s * 0.012)
    shadow.set()
    let path = NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius)
    NSGradient(colors: [NSColor(red: 0.16, green: 0.68, blue: 0.62, alpha: 1),
                        NSColor(red: 0.04, green: 0.42, blue: 0.43, alpha: 1)])!.draw(in: path, angle: -90)
    NSShadow().set()
    // Subtle top highlight.
    NSGradient(colors: [NSColor.white.withAlphaComponent(0.22), NSColor.white.withAlphaComponent(0)])!
        .draw(in: NSBezierPath(roundedRect: rect.insetBy(dx: s * 0.004, dy: s * 0.004), xRadius: radius, yRadius: radius), angle: -90)
    // Waveform bars.
    let heights: [CGFloat] = [0.22, 0.42, 0.66, 0.42, 0.82, 0.5, 0.3]
    let barW = rect.width * 0.07, gap = rect.width * 0.045
    let total = CGFloat(heights.count) * barW + CGFloat(heights.count - 1) * gap
    var x = rect.midX - total / 2
    NSColor.white.setFill()
    for h in heights {
        let bh = rect.height * h * 0.62
        NSBezierPath(roundedRect: NSRect(x: x, y: rect.midY - bh / 2, width: barW, height: bh),
                     xRadius: barW / 2, yRadius: barW / 2).fill()
        x += barW + gap
    }
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

for (name, px) in [("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64), ("128x128", 128),
                   ("128x128@2x", 256), ("256x256", 256), ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024)] {
    try! render(px).write(to: out.appendingPathComponent("icon_\(name).png"))
}
print("✓ \(out.path)")
