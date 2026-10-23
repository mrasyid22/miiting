import MiitingCore
import SwiftUI

struct SettingsView: View {
    var body: some View {
        TabView {
            Tab("General", systemImage: "gearshape") { GeneralSettings() }
            Tab("Audio", systemImage: "mic") { AudioSettings() }
            Tab("Transcription", systemImage: "waveform") { TranscriptionSettings() }
            Tab("Models", systemImage: "cpu") { ModelSettings() }
            Tab("Notes", systemImage: "sparkles") { SummarySettings() }
            Tab("Glossary", systemImage: "character.book.closed") { GlossarySettings() }
            Tab("Privacy", systemImage: "lock.shield") { PrivacySettings() }
        }
        .frame(width: 680, height: 520)
    }
}

private struct GeneralSettings: View {
    @Environment(AppModel.self) private var model
    @State private var interfaceLanguage = InterfaceLanguage.system
    @State private var needsRelaunch = false

    var body: some View {
        @Bindable var s = model.settings
        Form {
            Section {
                Picker("Interface language", selection: $interfaceLanguage) {
                    ForEach(InterfaceLanguage.allCases) { Text(verbatim: $0.label).tag($0) }
                }
                .onChange(of: interfaceLanguage) { _, new in
                    s.interfaceLanguage = new
                    needsRelaunch = true
                }
                if needsRelaunch {
                    HStack {
                        Text("Restart Miiting to switch languages.").foregroundStyle(.secondary)
                        Spacer()
                        Button("Restart now") { relaunch() }
                            .disabled(model.recorder.isActive)
                    }
                }
            }
            Section {
                Picker("Meeting language", selection: $s.language) {
                    ForEach(AppSettings.meetingLanguages, id: \.code) { Text(verbatim: $0.name).tag($0.code) }
                }
            } footer: {
                Text("You can also pick the language next to the Record button before each meeting. It is fixed during the meeting so Whisper doesn't drift into another language. English terms are still written as spoken. Notes are written in this language (Indonesian for regional languages).")
            }
            Section("After each meeting") {
                Toggle("Re-transcribe with the accurate model", isOn: $s.finalPassEnabled)
                Toggle("Write notes automatically", isOn: $s.autoSummarize)
                Picker("Notes template", selection: $s.defaultTemplate) {
                    ForEach(model.templates, id: \.id) { Text($0.name).tag($0.id) }
                }
            }
            Section("About") {
                LabeledContent("Version", value: Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "dev")
                LabeledContent("License", value: "MIT")
                Link(destination: URL(string: "https://github.com/mrasyid22/miiting")!) { Text(verbatim: "github.com/mrasyid22/miiting") }
            }
        }
        .formStyle(.grouped)
        .onAppear { interfaceLanguage = model.settings.interfaceLanguage }
    }

    private func relaunch() {
        let url = Bundle.main.bundleURL
        let config = NSWorkspace.OpenConfiguration()
        config.createsNewApplicationInstance = true
        NSWorkspace.shared.openApplication(at: url, configuration: config) { _, _ in
            DispatchQueue.main.async { NSApp.terminate(nil) }
        }
    }
}

private struct AudioSettings: View {
    @Environment(AppModel.self) private var model
    @State private var devices: [AudioInputDevice] = []

    var body: some View {
        @Bindable var s = model.settings
        Form {
            Section {
                Picker("Microphone", selection: $s.micDeviceUID) {
                    Text("System default (\(AudioDevices.defaultInputName() ?? "–"))").tag("")
                    ForEach(devices) { Text($0.name).tag($0.uid) }
                }
                Toggle("Echo cancellation", isOn: $s.echoCancellation)
            } footer: {
                Text("Turn this on if you use your laptop speakers instead of headphones. Other participants picked up by the microphone are also removed automatically in the final transcript.")
            }
            Section {
                Toggle("Record system audio (the other participants)", isOn: $s.captureSystemAudio)
            } footer: {
                Text("Uses macOS Core Audio process taps. No virtual driver like BlackHole is needed, and no bot joins the meeting.")
            }
            Section {
                Button("Open audio privacy settings") {
                    NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")!)
                }
            }
        }
        .formStyle(.grouped)
        .onAppear { devices = AudioDevices.inputDevices() }
    }
}

private struct TranscriptionSettings: View {
    @Environment(AppModel.self) private var model
    @State private var apiKey = ""

    var body: some View {
        @Bindable var s = model.settings
        let installed = model.whisperModels.filter(\.installed)
        Form {
            Section("Live transcript (during the meeting)") {
                Picker("Engine", selection: $s.liveLocation) {
                    ForEach(EngineLocation.allCases) { Text($0.label).tag($0) }
                }
                if s.liveLocation == .local {
                    modelPicker("Model", selection: $s.liveModelId, installed: installed)
                }
            }
            Section {
                Toggle("Make a final transcript", isOn: $s.finalPassEnabled)
                if s.finalPassEnabled {
                    Picker("Engine", selection: $s.finalLocation) {
                        ForEach(EngineLocation.allCases) { Text($0.label).tag($0) }
                    }
                    if s.finalLocation == .local {
                        modelPicker("Model", selection: $s.finalModelId, installed: installed)
                        Stepper("Beam search: \(String(s.beamSize))", value: $s.beamSize, in: 1...8)
                    }
                }
            } header: {
                Text("Final transcript (after the meeting)")
            } footer: {
                Text("The whole recording is transcribed again with the largest model, replacing the live draft. A larger beam is slightly more accurate but slower.")
            }
            if s.liveLocation == .cloud || (s.finalPassEnabled && s.finalLocation == .cloud) {
                Section {
                    Picker("Provider", selection: Binding(get: { s.cloudSttPreset }, set: {
                        s.applyCloudSttPreset($0)
                        apiKey = Keychain.get($0.keychainAccount) ?? ""
                    })) {
                        ForEach(CloudSttPreset.allCases) { Text($0.label).tag($0) }
                    }
                    TextField("Base URL", text: $s.cloudSttBaseURL)
                    TextField("Model", text: $s.cloudSttModel)
                    SecureField("API key", text: $apiKey)
                        .onChange(of: apiKey) { _, v in Keychain.set(v, for: s.cloudSttPreset.keychainAccount) }
                } header: {
                    Text("Cloud transcription")
                } footer: {
                    Label("Meeting audio is sent to \(URL(string: s.cloudSttBaseURL)?.host ?? String(localized: "this server")). The API key is stored in the Keychain.",
                          systemImage: "exclamationmark.icloud")
                }
            }
        }
        .formStyle(.grouped)
        .onAppear { apiKey = Keychain.get(model.settings.cloudSttPreset.keychainAccount) ?? "" }
        .safeAreaInset(edge: .top) {
            if let rec = model.recommendation, !model.settings.matches(rec) {
                HStack {
                    Label("These settings differ from the recommendation for your \(rec.device.chip).", systemImage: "sparkles")
                        .font(.callout)
                    Spacer()
                    Button("Apply recommendation") { model.applyRecommendation() }
                }
                .padding(.horizontal, 20)
                .padding(.vertical, 10)
                .background(Color.miiting.opacity(0.08))
            }
        }
    }

    @ViewBuilder
    private func modelPicker(_ title: LocalizedStringKey, selection: Binding<String>, installed: [ModelEntry]) -> some View {
        if installed.isEmpty {
            LabeledContent(title) {
                Text("No models yet. Download one in the Models tab.").foregroundStyle(.secondary)
            }
        } else {
            Picker(title, selection: selection) {
                ForEach(installed, id: \.id) { m in
                    Text(m.fit == .light || m.fit == .good ? m.name : "\(m.name) · \(m.fit.label.lowercased())").tag(m.id)
                }
                if !installed.contains(where: { $0.id == selection.wrappedValue }) {
                    Text("\(selection.wrappedValue) (not downloaded)").tag(selection.wrappedValue)
                }
            }
        }
    }
}

private struct ModelSettings: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        Form {
            Section {
                DeviceCard()
                    .listRowInsets(EdgeInsets())
            }
            Section {
                ForEach(model.models.filter { !$0.isVad }, id: \.id) { m in
                    ModelRow(entry: m)
                }
            } header: {
                Text("Transcription models")
            } footer: {
                Text("Fit is based on this Mac's RAM. Accuracy and speed are relative estimates; run `miiting-cli bench` for real WER numbers.")
            }
            Section("Supporting models") {
                ForEach(model.models.filter(\.isVad), id: \.id) { m in
                    ModelRow(entry: m)
                }
            }
            Section {
                Button("Open models folder") { model.revealModelsFolder() }
            }
        }
        .formStyle(.grouped)
        .onAppear { model.refreshModels() }
    }
}

private struct ModelRow: View {
    @Environment(AppModel.self) private var model
    let entry: ModelEntry

    var body: some View {
        let s = model.settings
        let rec = model.recommendation
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(entry.name).font(.body.weight(.medium))
                    if !entry.isVad { FitBadge(fit: entry.fit) }
                    if entry.id == rec?.liveModelId || entry.id == rec?.finalModelId {
                        tag("Recommended", systemImage: "sparkles")
                    }
                    if s.liveModelId == entry.id { tag("Live") }
                    if s.finalModelId == entry.id { tag("Final") }
                }
                Text(entry.description).font(.caption).foregroundStyle(.secondary)
                if !entry.isVad {
                    HStack(spacing: 14) {
                        RatingDots(label: String(localized: "Accuracy"), value: entry.accuracy)
                        RatingDots(label: String(localized: "Speed"), value: entry.speed)
                        Text("About \(formatBytes(entry.ramBytes)) RAM").font(.caption2).foregroundStyle(.secondary)
                    }
                }
                if let d = model.downloads[entry.id] {
                    ProgressView(value: Double(d.fraction)) { EmptyView() } currentValueLabel: {
                        Text(d.message).font(.caption2)
                    }
                    .frame(maxWidth: 260)
                }
            }
            Spacer()
            Text(formatBytes(entry.sizeBytes)).font(.caption).foregroundStyle(.secondary)
            if model.downloads[entry.id] != nil {
                Button("Cancel") { model.cancelDownload(entry.id) }
            } else if entry.installed {
                Menu {
                    if !entry.isVad {
                        Button("Use for the live transcript") { s.liveModelId = entry.id }
                        Button("Use for the final transcript") { s.finalModelId = entry.id }
                        Divider()
                    }
                    Button("Delete model", role: .destructive) { model.deleteModel(entry.id) }
                } label: {
                    Label("Installed", systemImage: "checkmark.circle.fill")
                        .foregroundStyle(.green)
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
            } else {
                Button("Download") { model.download(entry.id) }
            }
        }
        .padding(.vertical, 3)
    }

    private func tag(_ t: LocalizedStringKey, systemImage: String? = nil) -> some View {
        Group {
            if let systemImage { Label(t, systemImage: systemImage) } else { Text(t) }
        }
        .font(.caption2.weight(.semibold))
        .padding(.horizontal, 6)
        .padding(.vertical, 1)
        .background(Color.miiting.opacity(0.15), in: Capsule())
        .foregroundStyle(Color.miiting)
    }
}

private struct SummarySettings: View {
    @Environment(AppModel.self) private var model
    @State private var apiKey = ""
    @State private var testResult: Result<String, AppError>?
    @State private var testing = false

    var body: some View {
        @Bindable var s = model.settings
        Form {
            Section {
                Picker("Provider", selection: Binding(get: { s.llmPreset }, set: {
                    s.applyLlmPreset($0)
                    apiKey = Keychain.get($0.keychainAccount) ?? ""
                    testResult = nil
                })) {
                    ForEach(LlmPreset.allCases) { Text($0.label).tag($0) }
                }
                if s.llmPreset != .anthropic {
                    TextField("Base URL", text: $s.llmBaseURL)
                }
                HStack {
                    TextField("Model", text: $s.llmModel, prompt: Text(s.llmPreset.modelPlaceholder))
                    if !s.llmPreset.suggestedModels.isEmpty {
                        Menu("Suggestions") {
                            ForEach(s.llmPreset.suggestedModels, id: \.self) { m in
                                Button(m) { s.llmModel = m }
                            }
                        }
                        .fixedSize()
                    }
                }
                if s.llmPreset != .ollama && s.llmPreset != .lmStudio {
                    SecureField(s.llmPreset == .custom ? String(localized: "API key (optional)") : String(localized: "API key"), text: $apiKey)
                        .onChange(of: apiKey) { _, v in Keychain.set(v, for: s.llmPreset.keychainAccount) }
                }
                HStack {
                    Button(testing ? String(localized: "Testing…") : String(localized: "Test connection")) {
                        testing = true
                        Task {
                            testResult = await model.testLlm()
                            testing = false
                        }
                    }
                    .disabled(testing)
                    switch testResult {
                    case .success(let r):
                        Label("Connected: \(String(r.prefix(40)))", systemImage: "checkmark.circle.fill")
                            .foregroundStyle(.green).lineLimit(1)
                    case .failure(let e):
                        Label(e.message, systemImage: "xmark.octagon.fill")
                            .foregroundStyle(.red).lineLimit(3)
                    case nil:
                        EmptyView()
                    }
                }
            } header: {
                Text("Language model for notes")
            } footer: {
                if s.llmPreset.isLocal {
                    Label("Transcripts never leave this Mac.", systemImage: "lock.shield")
                } else {
                    Label("The transcript is sent to \(s.llmPreset == .anthropic ? "api.anthropic.com" : (URL(string: s.llmBaseURL)?.host ?? String(localized: "this server"))) when notes are written.",
                          systemImage: "exclamationmark.icloud")
                }
            }
            if s.llmPreset == .ollama {
                OllamaSection()
            }
            if s.llmPreset == .ollama, let rec = model.recommendation {
                Section {
                    ForEach(rec.localLlms, id: \.tag) { l in
                        HStack(spacing: 10) {
                            VStack(alignment: .leading, spacing: 3) {
                                HStack(spacing: 6) {
                                    Text(l.name).font(.body.weight(.medium))
                                    FitBadge(fit: l.fit)
                                    if model.ollamaStatus?.models.contains(l.tag) == true {
                                        Label("Downloaded", systemImage: "checkmark.circle.fill")
                                            .font(.caption2.weight(.semibold))
                                            .foregroundStyle(.green)
                                    }
                                    if l.recommended {
                                        Label("Recommended", systemImage: "sparkles")
                                            .font(.caption2.weight(.semibold))
                                            .foregroundStyle(Color.miiting)
                                    }
                                }
                                Text("\(l.note) \(formatBytes(l.sizeBytes)) download, needs about \(formatBytes(l.ramBytes)) of RAM.")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            if s.llmModel == l.tag {
                                Image(systemName: "checkmark").foregroundStyle(Color.miiting)
                            } else {
                                Button("Use") { s.llmModel = l.tag }
                                    .disabled(l.fit == .notRecommended)
                            }
                        }
                    }
                } header: {
                    Text("Local models for your \(rec.device.chip) · \(rec.device.ramLabel)")
                }
            }
            Section {
                TextEditor(text: $s.extraInstructions)
                    .font(.callout)
                    .frame(minHeight: 70)
            } header: {
                Text("Extra instructions (optional)")
            } footer: {
                Text("For example: “Use full names for action item owners” or “Write in a more formal tone”.")
            }
        }
        .formStyle(.grouped)
        .onAppear { apiKey = Keychain.get(model.settings.llmPreset.keychainAccount) ?? "" }
    }
}

/// Whether Ollama is installed and running, and whether the chosen model is downloaded.
private struct OllamaSection: View {
    @Environment(AppModel.self) private var model
    @State private var starting = false
    @State private var startError: String?

    var body: some View {
        let s = model.settings
        let status = model.ollamaStatus
        let tag = s.llmModel.trimmingCharacters(in: .whitespaces)
        Section {
            LabeledContent("Ollama") {
                if status?.running == true {
                    Label("Running", systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                } else if starting {
                    HStack(spacing: 6) {
                        ProgressView().controlSize(.small)
                        Text("Starting…").foregroundStyle(.secondary)
                    }
                } else if !model.ollamaInstalled {
                    HStack(spacing: 8) {
                        Text("Not installed").foregroundStyle(.secondary)
                        Button("Download Ollama") { NSWorkspace.shared.open(OllamaLauncher.downloadURL) }
                    }
                } else {
                    HStack(spacing: 8) {
                        Text("Not running").foregroundStyle(.secondary)
                        Button("Start") {
                            starting = true
                            Task {
                                startError = await model.startOllama()
                                starting = false
                            }
                        }
                    }
                }
            }
            if let startError {
                Text(startError).font(.caption).foregroundStyle(.red)
            }
            if !tag.isEmpty {
                LabeledContent {
                    if let d = model.downloads[AppModel.ollamaDownloadKey(tag)] {
                        HStack(spacing: 8) {
                            ProgressView(value: d.fraction).frame(width: 120)
                            Text(d.message).font(.caption).monospacedDigit().foregroundStyle(.secondary)
                            Button {
                                d.cancel.cancel()
                            } label: {
                                Image(systemName: "xmark.circle.fill")
                            }
                            .buttonStyle(.borderless)
                            .help("Cancel download")
                        }
                    } else if status?.modelInstalled == true {
                        Label("Downloaded", systemImage: "checkmark.circle.fill").foregroundStyle(.green)
                    } else {
                        Button("Download") { model.downloadOllamaModel(tag) }
                            .disabled(!model.ollamaInstalled)
                    }
                } label: {
                    Text(verbatim: tag)
                }
            }
        } header: {
            Text("Ollama")
        } footer: {
            Text("Miiting starts Ollama and downloads the model by itself when it writes notes. No Terminal needed.")
        }
        .task(id: "\(s.llmBaseURL)|\(tag)") { await model.refreshOllama() }
    }
}

private struct GlossarySettings: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var s = model.settings
        Form {
            Section {
                TextEditor(text: $s.glossary)
                    .font(.body.monospaced())
                    .frame(minHeight: 260)
            } header: {
                Text("Names and terms")
            } footer: {
                Text("List names, products and acronyms that come up often, separated by commas or new lines. For example: Rahma, SmartMCU, Nextcloud, OKR, sprint review. The glossary helps Whisper spell them right and is used when writing notes. Whisper only reads about the first 200 tokens, so put the most important ones first.")
            }
        }
        .formStyle(.grouped)
    }
}

private struct PrivacySettings: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let s = model.settings
        Form {
            Section("Where meeting data goes") {
                flow("Live transcript", local: s.liveLocation == .local, host: URL(string: s.cloudSttBaseURL)?.host)
                if s.finalPassEnabled {
                    flow("Final transcript", local: s.finalLocation == .local, host: URL(string: s.cloudSttBaseURL)?.host)
                }
                flow("Notes", local: s.llmPreset.isLocal,
                     host: s.llmPreset == .anthropic ? "api.anthropic.com" : URL(string: s.llmBaseURL)?.host)
            }
            Section {
                LabeledContent("Telemetry") { Text("None") }
                LabeledContent("Storage") { Text("SQLite and WAV files on this Mac") }
                Button("Open data folder") { model.revealDataFolder() }
            } footer: {
                Text("Every meeting records which third-party services received its data. Meetings that stayed 100% local show a green lock.")
            }
        }
        .formStyle(.grouped)
    }

    private func flow(_ title: LocalizedStringKey, local: Bool, host: String?) -> some View {
        LabeledContent(title) {
            if local {
                Label("Local", systemImage: "lock.shield").foregroundStyle(.green)
            } else {
                Label(host ?? String(localized: "Cloud"), systemImage: "icloud.and.arrow.up").foregroundStyle(.orange)
            }
        }
    }
}
