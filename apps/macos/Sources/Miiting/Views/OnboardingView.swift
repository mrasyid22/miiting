import AppKit
import AVFoundation
import SwiftUI

struct OnboardingView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var step = 0
    @State private var micStatus = MicCapture.authorization()

    var body: some View {
        VStack(spacing: 0) {
            Group {
                switch step {
                case 0: welcome
                case 1: permissions
                case 2: models
                default: summary
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .padding(32)

            Divider()
            HStack {
                HStack(spacing: 6) {
                    ForEach(0..<4) { i in
                        Capsule().fill(i == step ? Color.miiting : Color.secondary.opacity(0.3))
                            .frame(width: i == step ? 18 : 6, height: 6)
                    }
                }
                .animation(.snappy, value: step)
                Spacer()
                if step > 0 {
                    Button("Back") { step -= 1 }
                }
                Button(step == 3 ? String(localized: "Start using Miiting") : String(localized: "Continue")) {
                    if step == 3 {
                        model.settings.onboardingDone = true
                        dismiss()
                    } else {
                        step += 1
                    }
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
            }
            .padding(20)
        }
        .frame(width: 620, height: 600)
    }

    private var welcome: some View {
        @Bindable var settings = model.settings
        return VStack(alignment: .leading, spacing: 18) {
            Image(systemName: "waveform.circle.fill")
                .font(.system(size: 54))
                .foregroundStyle(Color.miiting)
            Text("Welcome to Miiting")
                .font(.largeTitle.bold())
            Text("Miiting records Zoom, Google Meet and Teams calls or in-person meetings, then writes the transcript and meeting notes. It is tuned for Indonesian.")
                .font(.title3)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 12) {
                point("person.slash", "No bot", "Nobody extra joins the call. Miiting listens on your Mac.")
                point("lock.shield", "Local by default", "Audio, transcripts and notes are stored and processed on this Mac.")
                point("icloud", "Cloud if you want it", "Use Groq, OpenAI, Claude and others with your own API key. Miiting always marks data that leaves your Mac.")
            }
            .padding(.top, 6)
            Picker("Your meetings are mostly in", selection: $settings.language) {
                ForEach(AppSettings.meetingLanguages, id: \.code) { Text(verbatim: $0.name).tag($0.code) }
            }
            .fixedSize()
        }
    }

    private var permissions: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Permissions").font(.largeTitle.bold())
            Text("macOS asks for two permissions. Both are only used while you are recording.")
                .foregroundStyle(.secondary)
            Card {
                HStack {
                    Label("Microphone", systemImage: "mic").font(.headline)
                    Spacer()
                    switch micStatus {
                    case .authorized:
                        Label("Allowed", systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                    case .denied, .restricted:
                        Button("Open Settings") {
                            open("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
                        }
                    default:
                        Button("Allow") {
                            Task {
                                _ = await MicCapture.requestAccess()
                                micStatus = MicCapture.authorization()
                            }
                        }
                        .buttonStyle(.borderedProminent)
                    }
                }
                Text("To record your voice.").font(.callout).foregroundStyle(.secondary)
            }
            Card {
                Label("System audio", systemImage: "speaker.wave.2").font(.headline)
                Text("To record the other participants from your meeting app. macOS asks when you start your first recording; choose Allow.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var models: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Transcription models").font(.largeTitle.bold())
            Text("Whisper runs right on your Apple chip. Miiting picks the models that fit this Mac's RAM.")
                .foregroundStyle(.secondary)
            DeviceCard(compact: true)
            ModelSetupCard()
            Text("You can also skip this and use cloud transcription (Groq/OpenAI) from Settings › Transcription.")
                .font(.callout)
                .foregroundStyle(.tertiary)
        }
    }

    private var summary: some View {
        @Bindable var settings = model.settings
        return VStack(alignment: .leading, spacing: 18) {
            Text("Automatic notes").font(.largeTitle.bold())
            Text("After the meeting, a language model writes the summary, decisions and action items.")
                .foregroundStyle(.secondary)
            Card {
                Picker("Provider", selection: Binding(get: { settings.llmPreset }, set: { settings.applyLlmPreset($0) })) {
                    ForEach(LlmPreset.allCases) { Text($0.label).tag($0) }
                }
                if settings.llmPreset == .ollama {
                    let tag = settings.llmModel.isEmpty ? (model.recommendation?.llmTag ?? "gemma3:4b") : settings.llmModel
                    if model.ollamaInstalled {
                        Label("Ollama is installed. Miiting starts it and downloads \(tag) the first time it writes notes.",
                              systemImage: "checkmark.circle")
                            .font(.callout).foregroundStyle(.secondary)
                    } else {
                        Text("Ollama runs the notes model on your Mac. Install it for free; Miiting then starts it and downloads \(tag) for you.")
                            .font(.callout).foregroundStyle(.secondary)
                        Button("Download Ollama") { NSWorkspace.shared.open(OllamaLauncher.downloadURL) }
                    }
                } else if settings.llmPreset.needsKey {
                    Text("Add your API key in Settings › Notes. The transcript is sent to \(URL(string: settings.llmBaseURL)?.host ?? String(localized: "the provider")) when notes are written.")
                        .font(.callout).foregroundStyle(.secondary)
                }
            }
            Toggle("Write notes automatically after each meeting", isOn: $settings.autoSummarize)
        }
    }

    private func point(_ icon: String, _ title: LocalizedStringKey, _ text: LocalizedStringKey) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: icon)
                .font(.title3)
                .foregroundStyle(Color.miiting)
                .frame(width: 28)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.headline)
                Text(text).foregroundStyle(.secondary)
            }
        }
    }

    private func open(_ s: String) {
        if let url = URL(string: s) { NSWorkspace.shared.open(url) }
    }
}
