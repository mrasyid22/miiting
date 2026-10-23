import AppKit
import SwiftUI

@main
struct MiitingApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var model = AppModel()

    var body: some Scene {
        Window("Miiting", id: "main") {
            ContentView()
                .environment(model)
                .tint(.miiting)
                .frame(minWidth: 860, minHeight: 560)
                .task {
                    appDelegate.model = model
                    await model.bootstrap()
                }
        }
        .defaultSize(width: 1180, height: 760)
        .windowToolbarStyle(.unified)
        .commands { MiitingCommands(model: model) }

        Settings {
            SettingsView()
                .environment(model)
                .tint(.miiting)
        }

        MenuBarExtra {
            MenuBarContent()
                .environment(model)
        } label: {
            Image(systemName: model.recorder.isRecording ? "record.circle.fill" : "waveform")
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    @MainActor weak var model: AppModel?

    /// Close an in-progress recording cleanly so its WAV files are finalized.
    @MainActor
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let model, model.recorder.isRecording, let engine = model.engine else { return .terminateNow }
        Task { @MainActor in
            _ = await model.recorder.stop(engine: engine, finalPassFollows: false, processingFollows: false)
            sender.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }

    @MainActor
    func applicationWillTerminate(_ notification: Notification) {
        OllamaLauncher.shared.stopOwnedServer()
    }
}

struct MiitingCommands: Commands {
    let model: AppModel

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button(model.recorder.isRecording ? String(localized: "Stop Recording") : String(localized: "Start Recording")) {
                Task { await model.toggleRecording() }
            }
            .keyboardShortcut("r", modifiers: [.command, .shift])
            .disabled(model.engine == nil || model.recorder.state == .starting || model.recorder.state == .stopping)

            Button(model.recorder.state == .paused ? String(localized: "Resume Recording") : String(localized: "Pause Recording")) {
                model.recorder.togglePause()
            }
            .keyboardShortcut("p", modifiers: [.command, .shift])
            .disabled(!model.recorder.isRecording)

            Divider()

            Button("Import Recording…") { model.showImporter = true }
                .keyboardShortcut("i", modifiers: .command)
                .disabled(model.engine == nil)
        }
    }
}

struct MenuBarContent: View {
    @Environment(AppModel.self) private var model
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        let r = model.recorder
        if r.isRecording {
            let state = r.state == .paused ? String(localized: "Paused") : String(localized: "Recording")
            Text(verbatim: "\(state) · \(formatTimestamp(r.elapsedMs))")
            Button(r.state == .paused ? String(localized: "Resume") : String(localized: "Pause")) { r.togglePause() }
            Button("Stop") { Task { await model.stopRecording() } }
        } else {
            @Bindable var settings = model.settings
            Button("Start Recording") { Task { await model.startRecording() } }
                .disabled(model.engine == nil || r.isActive)
            Picker("Meeting Language", selection: $settings.language) {
                ForEach(AppSettings.meetingLanguages, id: \.code) { Text(verbatim: $0.name).tag($0.code) }
            }
        }
        Divider()
        Button("Open Miiting") {
            openWindow(id: "main")
            NSApp.activate()
        }
        Button("Quit Miiting") { NSApp.terminate(nil) }
            .keyboardShortcut("q")
    }
}
