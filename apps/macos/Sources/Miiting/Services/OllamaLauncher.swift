import AppKit
import Foundation

/// Starts a local Ollama server when notes need one, so nobody has to open Terminal.
/// A server Miiting started itself is stopped when Miiting quits; one the user started is left alone.
@MainActor
final class OllamaLauncher {
    static let shared = OllamaLauncher()
    static let downloadURL = URL(string: "https://ollama.com/download")!

    enum Installation: Equatable {
        /// The Ollama app, which runs the server in the background.
        case app(URL)
        /// Only the command-line tool, for example from Homebrew.
        case cli(URL)
    }

    private var server: Process?

    /// Where Ollama is installed, if anywhere.
    static func installation() -> Installation? {
        let fm = FileManager.default
        let apps = [
            NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.electron.ollama"),
            URL(fileURLWithPath: "/Applications/Ollama.app"),
            fm.homeDirectoryForCurrentUser.appendingPathComponent("Applications/Ollama.app"),
        ]
        if let app = apps.compactMap({ $0 }).first(where: { fm.fileExists(atPath: $0.path) }) {
            return .app(app)
        }
        let tools = ["/opt/homebrew/bin/ollama", "/usr/local/bin/ollama"]
        if let tool = tools.first(where: { fm.isExecutableFile(atPath: $0) }) {
            return .cli(URL(fileURLWithPath: tool))
        }
        return nil
    }

    /// Starts the server that `baseURL` points to. Returns false when Ollama isn't installed.
    func launch(baseURL: String) async throws -> Bool {
        switch Self.installation() {
        case nil:
            return false
        case .app(let app):
            let config = NSWorkspace.OpenConfiguration()
            config.activates = false
            config.hides = true
            config.addsToRecentItems = false
            _ = try await NSWorkspace.shared.openApplication(at: app, configuration: config)
        case .cli(let tool):
            if server?.isRunning == true { return true }
            let p = Process()
            p.executableURL = tool
            p.arguments = ["serve"]
            var env = ProcessInfo.processInfo.environment
            if let url = URL(string: baseURL), let host = url.host(), let port = url.port {
                env["OLLAMA_HOST"] = "\(host):\(port)"
            }
            // A quantized KV cache (which needs flash attention) keeps long transcripts within RAM.
            env["OLLAMA_FLASH_ATTENTION"] = env["OLLAMA_FLASH_ATTENTION"] ?? "1"
            env["OLLAMA_KV_CACHE_TYPE"] = env["OLLAMA_KV_CACHE_TYPE"] ?? "q8_0"
            p.environment = env
            // The server log can include request details, so it stays out of Miiting's log.
            p.standardOutput = FileHandle.nullDevice
            p.standardError = FileHandle.nullDevice
            try p.run()
            server = p
        }
        return true
    }

    /// A server Miiting started has already exited, for example because the port is taken.
    var launchFailed: Bool { server.map { !$0.isRunning } ?? false }

    func stopOwnedServer() {
        if let server, server.isRunning { server.terminate() }
        server = nil
    }
}
