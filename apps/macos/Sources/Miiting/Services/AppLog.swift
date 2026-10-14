import Foundation
import os
import MiitingCore

/// Pipeline events and errors, written to ~/Library/Logs/Miiting/miiting.log and the
/// unified log. Never log transcript or summary text here.
enum AppLog {
    private static let logger = Logger(subsystem: "app.miiting.mac", category: "app")
    private static let queue = DispatchQueue(label: "app.miiting.log", qos: .utility)

    static let fileURL = FileManager.default.urls(for: .libraryDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("Logs/Miiting/miiting.log")

    static func info(_ message: String) {
        logger.info("\(message, privacy: .public)")
        append("INFO", message)
    }

    static func error(_ message: String) {
        logger.error("\(message, privacy: .public)")
        append("ERROR", message)
    }

    private static func append(_ level: String, _ message: String) {
        let line = "\(Date().formatted(.iso8601)) [\(level)] \(message)\n"
        queue.async {
            let fm = FileManager.default
            try? fm.createDirectory(at: fileURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            if let handle = try? FileHandle(forWritingTo: fileURL) {
                defer { try? handle.close() }
                _ = try? handle.seekToEnd()
                try? handle.write(contentsOf: Data(line.utf8))
            } else {
                try? Data(line.utf8).write(to: fileURL)
            }
        }
    }
}

extension SttSettings {
    /// Engine and model only; never includes API keys.
    var logDescription: String {
        switch self {
        case .local(let modelId, let beamSize): "local:\(modelId) beam=\(beamSize)"
        case .openAiCompatible(let baseUrl, _, let model): "cloud:\(URL(string: baseUrl)?.host ?? "?"):\(model)"
        }
    }
}
