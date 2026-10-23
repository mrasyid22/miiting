import Foundation
import Observation
import MiitingCore

enum RecorderState: Equatable {
    case idle, starting, recording, paused, stopping
}

enum LiveEvent: Sendable {
    case segment(TranscriptSegment)
    case level(AudioSource, Float)
    case status(LiveStatus)
    case error(String)
}

/// Receives callbacks from the Rust session on its worker threads.
final class LiveListener: SessionListener, @unchecked Sendable {
    private let deliver: @Sendable (LiveEvent) -> Void
    init(_ deliver: @escaping @Sendable (LiveEvent) -> Void) { self.deliver = deliver }
    func onSegment(segment: TranscriptSegment) { deliver(.segment(segment)) }
    func onLevel(source: AudioSource, level: Float) { deliver(.level(source, level)) }
    func onStatus(status: LiveStatus) { deliver(.status(status)) }
    func onError(message: String) { deliver(.error(message)) }
}

/// Owns the microphone + system-audio capture and the live Rust session.
@MainActor
@Observable
final class RecordingController {
    private(set) var state: RecorderState = .idle
    private(set) var meetingId: String?
    private(set) var segments: [TranscriptSegment] = []
    private(set) var micLevel: Float = 0
    private(set) var systemLevel: Float = 0
    private(set) var liveStatus: LiveStatus = .loadingModel
    private(set) var elapsedMs: Int64 = 0
    private(set) var systemAudioError: String?
    private(set) var capturingSystemAudio = false
    private(set) var heardSystemAudio = false
    private(set) var liveTranscription = true
    /// Language spoken in the meeting being recorded.
    private(set) var language = "id"

    @ObservationIgnored var onError: ((String) -> Void)?
    @ObservationIgnored private var session: RecordingSession?
    @ObservationIgnored private let mic = MicCapture()
    @ObservationIgnored private let system = SystemAudioCapture()
    @ObservationIgnored private let pushQueue = DispatchQueue(label: "app.miiting.push-audio", qos: .userInitiated)
    @ObservationIgnored private var ticker: Task<Void, Never>?

    var isActive: Bool { state != .idle }
    var isRecording: Bool { state == .recording || state == .paused }

    func start(engine: MiitingEngine, settings: AppSettings, installed: Set<String>) async throws -> String {
        guard state == .idle else { return meetingId ?? "" }
        state = .starting
        do {
            return try await startInner(engine: engine, settings: settings, installed: installed)
        } catch {
            mic.stop()
            system.stop()
            reset()
            throw error
        }
    }

    private func startInner(engine: MiitingEngine, settings: AppSettings, installed: Set<String>) async throws -> String {
        if MicCapture.authorization() == .notDetermined {
            _ = await MicCapture.requestAccess()
        }
        guard MicCapture.authorization() == .authorized else {
            throw AppError(String(localized: "Miiting isn't allowed to use the microphone. Open System Settings › Privacy & Security › Microphone."))
        }

        let live = settings.liveStt(installed: installed)
        liveTranscription = live != nil
        let listener = LiveListener { [weak self] event in
            Task { @MainActor in self?.handle(event) }
        }
        let language = settings.language
        self.language = language
        let glossary = settings.glossaryOrNil
        let session = try await Task.detached {
            try engine.startSession(title: nil, language: language, glossary: glossary, liveStt: live, listener: listener)
        }.value
        self.session = session
        meetingId = session.meetingId()

        let queue = pushQueue
        try mic.start(deviceUID: settings.micDeviceUID, echoCancellation: settings.echoCancellation) { samples, rate, channels in
            queue.async { session.pushAudio(source: .mic, samples: samples, sampleRate: rate, channels: channels) }
        }
        if settings.captureSystemAudio {
            do {
                try system.start { samples, rate, channels in
                    queue.async { session.pushAudio(source: .system, samples: samples, sampleRate: rate, channels: channels) }
                }
                capturingSystemAudio = true
            } catch {
                systemAudioError = userMessage(error)
            }
        }
        state = .recording
        startTicker()
        return session.meetingId()
    }

    func togglePause() {
        guard let session else { return }
        if state == .recording {
            session.pause()
            state = .paused
        } else if state == .paused {
            session.resume()
            state = .recording
        }
    }

    /// Stops capture and closes the session. Returns the meeting id.
    func stop(engine: MiitingEngine, finalPassFollows: Bool, processingFollows: Bool) async -> String? {
        guard let session, isRecording else { return nil }
        state = .stopping
        mic.stop()
        system.stop()
        ticker?.cancel()
        let queue = pushQueue
        do {
            try await Task.detached {
                queue.sync {} // let in-flight audio reach the session first
                try engine.finishSession(session: session, finalPassFollows: finalPassFollows, processingFollows: processingFollows)
            }.value
        } catch {
            onError?(userMessage(error))
        }
        let id = meetingId
        reset()
        return id
    }

    private func reset() {
        session = nil
        meetingId = nil
        segments = []
        micLevel = 0
        systemLevel = 0
        elapsedMs = 0
        liveStatus = .loadingModel
        systemAudioError = nil
        capturingSystemAudio = false
        heardSystemAudio = false
        state = .idle
    }

    private func startTicker() {
        ticker?.cancel()
        ticker = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(250))
                guard let self, let s = self.session else { return }
                self.elapsedMs = s.elapsedMs()
            }
        }
    }

    private func handle(_ event: LiveEvent) {
        guard isActive else { return }
        switch event {
        case .segment(let s):
            segments.append(s)
        case .level(let source, let level):
            if source == .mic {
                micLevel = level
            } else {
                systemLevel = level
                if level > 0.15 { heardSystemAudio = true }
            }
        case .status(let s):
            liveStatus = s
        case .error(let message):
            onError?(message)
        }
    }
}

struct AppError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

func userMessage(_ error: Error) -> String {
    if let e = error as? MiitingError {
        switch e {
        case .Failed(let m): return m
        case .ModelMissing(let m): return String(localized: "\(m) isn't downloaded. Download it in Settings › Models.")
        case .Cancelled: return String(localized: "Cancelled.")
        }
    }
    return (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
}
