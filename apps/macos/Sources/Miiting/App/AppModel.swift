import AppKit
import Foundation
import Observation
import MiitingCore
import UniformTypeIdentifiers

final class ProgressRelay: ProgressListener, @unchecked Sendable {
    private let handler: @Sendable (Float, String) -> Void
    init(_ handler: @escaping @Sendable (Float, String) -> Void) { self.handler = handler }
    func onProgress(fraction: Float, message: String) { handler(fraction, message) }
}

struct JobProgress: Equatable {
    enum Stage: Equatable { case importing, finalPass, summary }
    var stage: Stage
    var fraction: Float
    var message: String
    var cancel: CancelToken?

    static func == (a: JobProgress, b: JobProgress) -> Bool {
        a.stage == b.stage && a.fraction == b.fraction && a.message == b.message
    }
}

struct DownloadProgress: Equatable {
    var fraction: Float
    var message: String
    var cancel: CancelToken
    static func == (a: DownloadProgress, b: DownloadProgress) -> Bool { a.fraction == b.fraction && a.message == b.message }
}

struct Toast: Identifiable, Equatable {
    enum Kind { case info, success, error }
    let id = UUID()
    let kind: Kind
    let message: String
}

@MainActor
@Observable
final class AppModel {
    let settings = AppSettings()
    let recorder = RecordingController()

    private(set) var engine: MiitingEngine?
    private(set) var startupError: String?
    private(set) var meetings: [MeetingItem] = []
    private(set) var detail: MeetingDetail?
    private(set) var models: [ModelEntry] = []
    private(set) var recommendation: Recommendation?
    private(set) var templates: [TemplateEntry] = []
    private(set) var downloads: [String: DownloadProgress] = [:]
    private(set) var jobs: [String: JobProgress] = [:]
    private(set) var toast: Toast?

    var selection: String? {
        didSet { if oldValue != selection { loadDetail() } }
    }
    var searchText = "" {
        didSet { scheduleSearch() }
    }
    var showOnboarding = false
    var showImporter = false

    @ObservationIgnored private var searchTask: Task<Void, Never>?
    @ObservationIgnored private var toastTask: Task<Void, Never>?

    init() {
        recorder.onError = { [weak self] message in self?.show(message, .error) }
    }

    // MARK: Lifecycle

    func bootstrap() async {
        guard engine == nil else { return }
        let base = DevHooks.dataDirOverride.map { URL(fileURLWithPath: $0, isDirectory: true) }
            ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("Miiting", isDirectory: true)
        do {
            try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
            let path = base.path
            let uiLanguage = AppSettings.interfaceLanguageCode
            engine = try await Task.detached { try MiitingEngine(dataDir: path, uiLanguage: uiLanguage) }.value
        } catch {
            startupError = userMessage(error)
            return
        }
        templates = engine?.templates() ?? []
        refreshModels()
        if let rec = recommendation, !settings.recommendationApplied {
            settings.apply(rec)
            settings.recommendationApplied = true
        }
        reloadMeetings()
        if !settings.onboardingDone && !DevHooks.skipOnboarding { showOnboarding = true }
        if let id = DevHooks.selectMeeting { selection = id }
    }

    var installedModelIds: Set<String> { Set(models.filter(\.installed).map(\.id)) }
    var hasLiveModel: Bool { settings.liveStt(installed: installedModelIds) != nil }
    var hasFinalModel: Bool { settings.finalStt(installed: installedModelIds) != nil }
    var summaryConfigured: Bool { settings.llm() != nil }
    var whisperModels: [ModelEntry] { models.filter { !$0.isVad } }

    // MARK: Meetings

    func reloadMeetings() {
        guard let engine else { return }
        let q = searchText.trimmingCharacters(in: .whitespaces)
        meetings = (try? engine.listMeetings(query: q.isEmpty ? nil : q)) ?? []
        loadDetail()
    }

    private func scheduleSearch() {
        searchTask?.cancel()
        searchTask = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(180))
            guard !Task.isCancelled else { return }
            self?.reloadMeetings()
        }
    }

    func loadDetail() {
        guard let engine, let id = selection else {
            detail = nil
            return
        }
        detail = try? engine.meetingDetail(meetingId: id)
    }

    func meeting(_ id: String) -> MeetingItem? { meetings.first { $0.id == id } }

    func rename(_ id: String, to title: String) {
        let t = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let engine, !t.isEmpty else { return }
        try? engine.renameMeeting(meetingId: id, title: t)
        reloadMeetings()
    }

    func updateSummary(_ id: String, markdown: String) {
        try? engine?.updateSummary(meetingId: id, markdown: markdown)
        loadDetail()
    }

    func delete(_ id: String) {
        guard let engine else { return }
        jobs[id]?.cancel?.cancel()
        do {
            try engine.deleteMeeting(meetingId: id)
            if selection == id { selection = nil }
            reloadMeetings()
        } catch {
            show(userMessage(error), .error)
        }
    }

    func revealInFinder(_ id: String) {
        guard let engine else { return }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: engine.meetingDir(meetingId: id))])
    }

    func copyMarkdown(_ id: String) {
        guard let md = try? engine?.exportMarkdown(meetingId: id) else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(md, forType: .string)
        show(String(localized: "Notes copied to the clipboard"), .success)
    }

    func exportMarkdown(_ id: String) {
        guard let engine, let md = try? engine.exportMarkdown(meetingId: id) else { return }
        let panel = NSSavePanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "md") ?? .plainText]
        panel.nameFieldStringValue = (meeting(id)?.title ?? String(localized: "Notes")).replacingOccurrences(of: "/", with: "-") + ".md"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            try md.write(to: url, atomically: true, encoding: .utf8)
            show(String(localized: "Exported to \(url.lastPathComponent)"), .success)
        } catch {
            show(userMessage(error), .error)
        }
    }

    // MARK: Recording

    func toggleRecording() async {
        if recorder.isRecording { await stopRecording() } else if !recorder.isActive { await startRecording() }
    }

    func startRecording() async {
        guard let engine else { return }
        do {
            let id = try await recorder.start(engine: engine, settings: settings, installed: installedModelIds)
            AppLog.info("recording started \(id) live=\(recorder.liveTranscription) systemAudio=\(recorder.capturingSystemAudio)")
            reloadMeetings()
            selection = id
            if !recorder.liveTranscription {
                show(String(localized: "No live model downloaded yet. Audio is still recorded and transcribed after the meeting."), .info)
            }
        } catch {
            show(userMessage(error), .error)
        }
    }

    func stopRecording() async {
        guard let engine else { return }
        let installed = installedModelIds
        let finalStt = settings.finalPassEnabled ? settings.finalStt(installed: installed) : nil
        let hadLive = recorder.liveTranscription
        // Without a live transcript, the final pass is the only transcript.
        let stt = finalStt ?? (hadLive ? nil : settings.liveStt(installed: installed))
        let summarize = settings.autoSummarize && settings.llm() != nil
        AppLog.info("stop recording: finalPass=\(stt != nil) summary=\(summarize) liveHadEngine=\(hadLive)")
        guard let id = await recorder.stop(engine: engine, finalPassFollows: stt != nil, processingFollows: stt != nil || summarize)
        else { return }
        reloadMeetings()
        selection = id
        await process(id, stt: stt, summarize: summarize)
    }

    /// Post-meeting pipeline: accurate re-transcription, then summary.
    private func process(_ id: String, stt: SttSettings?, summarize: Bool) async {
        if let stt { await runFinalPass(id, stt: stt) }
        if summarize, meeting(id) != nil { await self.summarize(id, templateId: settings.defaultTemplate) }
        try? engine?.markReady(meetingId: id)
        reloadMeetings()
    }

    /// `miiting://record/start|stop|toggle|pause` and `miiting://meeting/<id>[/retranscribe|/summarize]`, for Shortcuts,
    /// Raycast, Stream Deck and scripted tests.
    func handle(_ url: URL) async {
        guard url.scheme == "miiting" else { return }
        let action = url.pathComponents.dropFirst().first ?? ""
        switch (url.host, action) {
        case ("record", "start"): if !recorder.isActive { await startRecording() }
        case ("record", "stop"): if recorder.isRecording { await stopRecording() }
        case ("record", "toggle"): await toggleRecording()
        case ("record", "pause"): recorder.togglePause()
        case ("meeting", let id) where !id.isEmpty:
            selection = id
            switch url.pathComponents.dropFirst(2).first {
            case "retranscribe": await retranscribe(id)
            case "summarize": await summarize(id, templateId: settings.defaultTemplate)
            default: break
            }
        default: break
        }
    }

    // MARK: Processing

    func retranscribe(_ id: String, modelId: String? = nil) async {
        let stt: SttSettings? = if let modelId {
            .local(modelId: modelId, beamSize: Int32(settings.beamSize))
        } else {
            settings.finalStt(installed: installedModelIds) ?? settings.liveStt(installed: installedModelIds)
        }
        guard let stt else {
            show(String(localized: "Download a Whisper model first in Settings › Models."), .error)
            return
        }
        await runFinalPass(id, stt: stt)
        try? engine?.markReady(meetingId: id)
        reloadMeetings()
    }

    private func runFinalPass(_ id: String, stt: SttSettings) async {
        guard let engine, jobs[id] == nil else { return }
        let cancel = CancelToken()
        jobs[id] = JobProgress(stage: .finalPass, fraction: 0, message: String(localized: "Preparing the model…"), cancel: cancel)
        reloadMeetings()
        let relay = ProgressRelay { [weak self] f, _ in
            Task { @MainActor in
                guard let self, self.jobs[id]?.stage == .finalPass else { return }
                self.jobs[id]?.fraction = f
                let percent = Double(f).formatted(.percent.precision(.fractionLength(0)).locale(appLocale))
                self.jobs[id]?.message = String(localized: "Re-transcribing with the accurate model… \(percent)")
            }
        }
        let language = settings.language
        let glossary = settings.glossaryOrNil
        AppLog.info("final pass start \(id) \(stt.logDescription)")
        let started = Date()
        do {
            let n = try await Task.detached(priority: .userInitiated) {
                try engine.runFinalPass(meetingId: id, stt: stt, language: language, glossary: glossary, progress: relay, cancel: cancel)
            }.value
            AppLog.info("final pass done \(id): \(n) segments in \(Int(Date().timeIntervalSince(started)))s")
        } catch {
            if case MiitingError.Cancelled = error {
                AppLog.info("final pass cancelled \(id)")
            } else {
                show(userMessage(error), .error)
            }
        }
        jobs[id] = nil
        reloadMeetings()
    }

    func summarize(_ id: String, templateId: String) async {
        guard let engine, jobs[id] == nil else { return }
        guard let llm = settings.llm() else {
            show(String(localized: "Set up a notes provider first in Settings › Notes."), .error)
            return
        }
        jobs[id] = JobProgress(stage: .summary, fraction: 0, message: String(localized: "Writing notes…"), cancel: nil)
        let relay = ProgressRelay { [weak self] f, message in
            Task { @MainActor in
                guard let self, self.jobs[id]?.stage == .summary else { return }
                self.jobs[id]?.fraction = f
                self.jobs[id]?.message = message
            }
        }
        let glossary = settings.glossaryOrNil
        let extra = settings.extraInstructions.isEmpty ? nil : settings.extraInstructions
        AppLog.info("summary start \(id) template=\(templateId)")
        let started = Date()
        do {
            _ = try await Task.detached(priority: .userInitiated) {
                try engine.summarize(meetingId: id, llm: llm, templateId: templateId, glossary: glossary,
                                     extraInstructions: extra, progress: relay)
            }.value
            AppLog.info("summary done \(id) in \(Int(Date().timeIntervalSince(started)))s")
        } catch {
            show(userMessage(error), .error)
        }
        jobs[id] = nil
        reloadMeetings()
    }

    func cancelJob(_ id: String) {
        jobs[id]?.cancel?.cancel()
    }

    // MARK: Import

    func importFile(_ url: URL) async {
        guard let engine else { return }
        let placeholder = "import-\(UUID().uuidString)"
        jobs[placeholder] = JobProgress(stage: .importing, fraction: 0, message: String(localized: "Importing \(url.lastPathComponent)…"), cancel: nil)
        defer { jobs[placeholder] = nil }
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        do {
            let wav = try await AudioImporter.convertToWav(url)
            defer { try? FileManager.default.removeItem(at: wav) }
            let title = url.deletingPathExtension().lastPathComponent
            let language = settings.language
            let wavPath = wav.path
            let id = try await Task.detached {
                try engine.importWavFile(title: title, wavPath: wavPath, language: language)
            }.value
            jobs[placeholder] = nil
            reloadMeetings()
            selection = id
            let installed = installedModelIds
            let stt = settings.finalStt(installed: installed) ?? settings.liveStt(installed: installed)
            if stt == nil { show(String(localized: "Download a Whisper model, then choose Re-transcribe."), .info) }
            await process(id, stt: stt, summarize: settings.autoSummarize && settings.llm() != nil)
        } catch {
            show(userMessage(error), .error)
        }
    }

    var importJob: JobProgress? { jobs.first { $0.key.hasPrefix("import-") }?.value }

    // MARK: Models

    func refreshModels() {
        models = engine?.models() ?? []
        recommendation = engine?.recommendation()
    }

    func applyRecommendation() {
        guard let rec = recommendation else { return }
        settings.apply(rec)
        show(String(localized: "Model settings adjusted for your \(rec.device.chip)"), .success)
    }

    func model(_ id: String) -> ModelEntry? { models.first { $0.id == id } }

    func download(_ modelId: String) {
        guard let engine, downloads[modelId] == nil else { return }
        let cancel = CancelToken()
        downloads[modelId] = DownloadProgress(fraction: 0, message: String(localized: "Starting…"), cancel: cancel)
        let relay = ProgressRelay { [weak self] f, message in
            Task { @MainActor in
                guard let self, self.downloads[modelId] != nil else { return }
                self.downloads[modelId]?.fraction = f
                self.downloads[modelId]?.message = message
            }
        }
        Task {
            do {
                try await Task.detached(priority: .utility) {
                    try engine.downloadModel(modelId: modelId, progress: relay, cancel: cancel)
                }.value
                let name = models.first { $0.id == modelId }?.name ?? modelId
                show(String(localized: "\(name) is ready"), .success)
            } catch {
                if case MiitingError.Cancelled = error {} else { show(userMessage(error), .error) }
            }
            downloads[modelId] = nil
            refreshModels()
        }
    }

    /// The live, final and VAD models recommended for this Mac.
    var recommendedModelIds: [String] {
        recommendation?.modelIds ?? ["large-v3-turbo-q5_0", "large-v3-q5_0", "silero-vad"]
    }

    func downloadRecommended() {
        if let rec = recommendation, !settings.matches(rec) { settings.apply(rec) }
        for id in recommendedModelIds where !installedModelIds.contains(id) {
            download(id)
        }
    }

    func cancelDownload(_ modelId: String) {
        downloads[modelId]?.cancel.cancel()
    }

    func deleteModel(_ modelId: String) {
        try? engine?.deleteModel(modelId: modelId)
        refreshModels()
    }

    func revealModelsFolder() {
        guard let engine else { return }
        NSWorkspace.shared.open(URL(fileURLWithPath: engine.modelsDir()))
    }

    func revealDataFolder() {
        guard let engine else { return }
        NSWorkspace.shared.open(URL(fileURLWithPath: engine.dataDir()))
    }

    func testLlm() async -> Result<String, AppError> {
        guard let engine else { return .failure(AppError(String(localized: "The engine isn't ready yet."))) }
        guard let llm = settings.llm() else { return .failure(AppError(String(localized: "Fill in the model name and API key first."))) }
        do {
            let reply = try await Task.detached { try engine.testLlm(llm: llm) }.value
            return .success(reply)
        } catch {
            return .failure(AppError(userMessage(error)))
        }
    }

    // MARK: Toasts

    func show(_ message: String, _ kind: Toast.Kind = .info) {
        if kind == .error { AppLog.error(message) }
        toast = Toast(kind: kind, message: message)
        toastTask?.cancel()
        toastTask = Task { [weak self] in
            try? await Task.sleep(for: .seconds(kind == .error ? 7 : 4))
            guard !Task.isCancelled else { return }
            self?.toast = nil
        }
    }

    func dismissToast() {
        toast = nil
    }
}
