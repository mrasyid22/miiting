import Foundation
import Observation
import MiitingCore

enum EngineLocation: String, CaseIterable, Identifiable {
    case local, cloud
    var id: String { rawValue }
    var label: String {
        self == .local
            ? String(localized: "Local (on this Mac)")
            : String(localized: "Cloud (third-party API)")
    }
}

/// Speech-to-text providers that speak OpenAI's `/audio/transcriptions`.
enum CloudSttPreset: String, CaseIterable, Identifiable {
    case groq, openai, custom
    var id: String { rawValue }

    var label: String {
        switch self {
        case .groq: "Groq"
        case .openai: "OpenAI"
        case .custom: String(localized: "Custom server (OpenAI-compatible)")
        }
    }

    var baseURL: String {
        switch self {
        case .groq: "https://api.groq.com/openai/v1"
        case .openai: "https://api.openai.com/v1"
        case .custom: "http://localhost:8000/v1"
        }
    }

    var defaultModel: String {
        switch self {
        case .groq: "whisper-large-v3"
        case .openai: "gpt-4o-transcribe"
        case .custom: "Systran/faster-whisper-large-v3"
        }
    }

    var keychainAccount: String { "stt.\(rawValue)" }
}

enum LlmPreset: String, CaseIterable, Identifiable {
    case ollama, lmStudio, anthropic, openai, openrouter, groq, gemini, custom
    var id: String { rawValue }

    var label: String {
        switch self {
        case .ollama: String(localized: "Ollama (local)")
        case .lmStudio: String(localized: "LM Studio (local)")
        case .anthropic: "Anthropic Claude"
        case .openai: "OpenAI"
        case .openrouter: "OpenRouter"
        case .groq: "Groq"
        case .gemini: "Google Gemini"
        case .custom: String(localized: "Custom server (OpenAI-compatible)")
        }
    }

    var isLocal: Bool { self == .ollama || self == .lmStudio }
    var needsKey: Bool { !isLocal && self != .custom }

    var baseURL: String {
        switch self {
        case .ollama: "http://localhost:11434/v1"
        case .lmStudio: "http://localhost:1234/v1"
        case .anthropic: "https://api.anthropic.com"
        case .openai: "https://api.openai.com/v1"
        case .openrouter: "https://openrouter.ai/api/v1"
        case .groq: "https://api.groq.com/openai/v1"
        case .gemini: "https://generativelanguage.googleapis.com/v1beta/openai"
        case .custom: "http://localhost:8080/v1"
        }
    }

    var defaultModel: String {
        switch self {
        case .ollama: "gemma3:12b"
        case .anthropic: "claude-opus-5-5"
        default: ""
        }
    }

    /// Suggestions shown under the model field. Users can type any model name.
    var suggestedModels: [String] {
        switch self {
        case .ollama: ["gemma3:4b", "qwen3:8b", "gemma3:12b", "qwen3:14b", "gemma3:27b"]
        case .anthropic: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"]
        default: []
        }
    }

    var modelPlaceholder: String {
        switch self {
        case .openai: String(localized: "e.g. \("gpt-5-mini")")
        case .openrouter: String(localized: "e.g. \("google/gemini-2.5-flash")")
        case .groq: String(localized: "e.g. \("llama-3.3-70b-versatile")")
        case .gemini: String(localized: "e.g. \("gemini-2.5-flash")")
        case .lmStudio: String(localized: "the model loaded in LM Studio")
        default: String(localized: "model name")
        }
    }

    var keychainAccount: String { "llm.\(rawValue)" }
}

/// Interface languages the app ships translations for.
enum InterfaceLanguage: String, CaseIterable, Identifiable {
    case system, en, id
    var id: String { rawValue }

    /// Language names are shown in their own language, so they are not translated.
    var label: String {
        switch self {
        case .system: String(localized: "System default")
        case .en: "English"
        case .id: "Bahasa Indonesia"
        }
    }
}

/// User preferences, persisted in UserDefaults. Secrets go through `Keychain`.
@MainActor
@Observable
final class AppSettings {
    private let d = UserDefaults.standard

    var language: String { didSet { d.set(language, forKey: "language") } }

    var liveLocation: EngineLocation { didSet { d.set(liveLocation.rawValue, forKey: "liveLocation") } }
    var liveModelId: String { didSet { d.set(liveModelId, forKey: "liveModelId") } }
    var finalPassEnabled: Bool { didSet { d.set(finalPassEnabled, forKey: "finalPassEnabled") } }
    var finalLocation: EngineLocation { didSet { d.set(finalLocation.rawValue, forKey: "finalLocation") } }
    var finalModelId: String { didSet { d.set(finalModelId, forKey: "finalModelId") } }
    var beamSize: Int { didSet { d.set(beamSize, forKey: "beamSize") } }

    var cloudSttPreset: CloudSttPreset { didSet { d.set(cloudSttPreset.rawValue, forKey: "cloudSttPreset") } }
    var cloudSttBaseURL: String { didSet { d.set(cloudSttBaseURL, forKey: "cloudSttBaseURL") } }
    var cloudSttModel: String { didSet { d.set(cloudSttModel, forKey: "cloudSttModel") } }

    var autoSummarize: Bool { didSet { d.set(autoSummarize, forKey: "autoSummarize") } }
    var llmPreset: LlmPreset { didSet { d.set(llmPreset.rawValue, forKey: "llmPreset") } }
    var llmBaseURL: String { didSet { d.set(llmBaseURL, forKey: "llmBaseURL") } }
    var llmModel: String { didSet { d.set(llmModel, forKey: "llmModel") } }
    var defaultTemplate: String { didSet { d.set(defaultTemplate, forKey: "defaultTemplate") } }
    var extraInstructions: String { didSet { d.set(extraInstructions, forKey: "extraInstructions") } }

    var glossary: String { didSet { d.set(glossary, forKey: "glossary") } }

    var micDeviceUID: String { didSet { d.set(micDeviceUID, forKey: "micDeviceUID") } }
    var captureSystemAudio: Bool { didSet { d.set(captureSystemAudio, forKey: "captureSystemAudio") } }
    var echoCancellation: Bool { didSet { d.set(echoCancellation, forKey: "echoCancellation") } }

    var onboardingDone: Bool { didSet { d.set(onboardingDone, forKey: "onboardingDone") } }
    /// Set once the device recommendation has been applied on first launch.
    var recommendationApplied: Bool { didSet { d.set(recommendationApplied, forKey: "recommendationApplied") } }

    /// Meeting languages, named in their own language (so not translated).
    static let meetingLanguages: [(code: String, name: String)] = [
        ("id", "Bahasa Indonesia"),
        ("en", "English"),
        ("jw", "Basa Jawa"),
        ("su", "Basa Sunda"),
        ("ms", "Bahasa Melayu"),
    ]

    /// The language the interface is actually shown in ("en" or "id").
    static var interfaceLanguageCode: String { Bundle.main.preferredLocalizations.first ?? "en" }

    /// macOS picks the app language from `AppleLanguages`; it takes effect after a relaunch.
    var interfaceLanguage: InterfaceLanguage {
        get {
            guard let langs = d.persistentDomain(forName: Bundle.main.bundleIdentifier ?? "")?["AppleLanguages"] as? [String],
                  let first = langs.first
            else { return .system }
            return InterfaceLanguage(rawValue: String(first.prefix(2))) ?? .system
        }
        set {
            if newValue == .system {
                d.removeObject(forKey: "AppleLanguages")
            } else {
                d.set([newValue.rawValue], forKey: "AppleLanguages")
            }
        }
    }

    init() {
        d.register(defaults: [
            // Meetings default to the interface language; Indonesian users get "id".
            "language": Self.interfaceLanguageCode == "id" ? "id" : "en",
            "liveLocation": EngineLocation.local.rawValue,
            "liveModelId": "large-v3-turbo-q5_0",
            "finalPassEnabled": true,
            "finalLocation": EngineLocation.local.rawValue,
            "finalModelId": "large-v3",
            "beamSize": 5,
            "cloudSttPreset": CloudSttPreset.groq.rawValue,
            "cloudSttBaseURL": CloudSttPreset.groq.baseURL,
            "cloudSttModel": CloudSttPreset.groq.defaultModel,
            "autoSummarize": true,
            "llmPreset": LlmPreset.ollama.rawValue,
            "llmBaseURL": LlmPreset.ollama.baseURL,
            "llmModel": LlmPreset.ollama.defaultModel,
            "defaultTemplate": "general",
            "extraInstructions": "",
            "glossary": "",
            "micDeviceUID": "",
            "captureSystemAudio": true,
            "echoCancellation": false,
            "onboardingDone": false,
            "recommendationApplied": false,
        ])
        language = d.string(forKey: "language") ?? (Self.interfaceLanguageCode == "id" ? "id" : "en")
        liveLocation = EngineLocation(rawValue: d.string(forKey: "liveLocation") ?? "") ?? .local
        liveModelId = d.string(forKey: "liveModelId") ?? "large-v3-turbo-q5_0"
        finalPassEnabled = d.bool(forKey: "finalPassEnabled")
        finalLocation = EngineLocation(rawValue: d.string(forKey: "finalLocation") ?? "") ?? .local
        finalModelId = d.string(forKey: "finalModelId") ?? "large-v3"
        beamSize = d.integer(forKey: "beamSize")
        cloudSttPreset = CloudSttPreset(rawValue: d.string(forKey: "cloudSttPreset") ?? "") ?? .groq
        cloudSttBaseURL = d.string(forKey: "cloudSttBaseURL") ?? CloudSttPreset.groq.baseURL
        cloudSttModel = d.string(forKey: "cloudSttModel") ?? CloudSttPreset.groq.defaultModel
        autoSummarize = d.bool(forKey: "autoSummarize")
        llmPreset = LlmPreset(rawValue: d.string(forKey: "llmPreset") ?? "") ?? .ollama
        llmBaseURL = d.string(forKey: "llmBaseURL") ?? LlmPreset.ollama.baseURL
        llmModel = d.string(forKey: "llmModel") ?? LlmPreset.ollama.defaultModel
        defaultTemplate = d.string(forKey: "defaultTemplate") ?? "general"
        extraInstructions = d.string(forKey: "extraInstructions") ?? ""
        glossary = d.string(forKey: "glossary") ?? ""
        micDeviceUID = d.string(forKey: "micDeviceUID") ?? ""
        captureSystemAudio = d.bool(forKey: "captureSystemAudio")
        echoCancellation = d.bool(forKey: "echoCancellation")
        onboardingDone = d.bool(forKey: "onboardingDone")
        recommendationApplied = d.bool(forKey: "recommendationApplied")
    }

    /// Adopt the models and beam size suggested for this Mac. Cloud choices are left alone.
    func apply(_ rec: Recommendation) {
        liveModelId = rec.liveModelId
        finalModelId = rec.finalModelId
        beamSize = Int(rec.beamSize)
        if llmPreset == .ollama { llmModel = rec.llmTag }
    }

    func matches(_ rec: Recommendation) -> Bool {
        liveModelId == rec.liveModelId && finalModelId == rec.finalModelId && beamSize == Int(rec.beamSize)
            && (llmPreset != .ollama || llmModel == rec.llmTag)
    }

    func applyCloudSttPreset(_ p: CloudSttPreset) {
        cloudSttPreset = p
        cloudSttBaseURL = p.baseURL
        cloudSttModel = p.defaultModel
    }

    func applyLlmPreset(_ p: LlmPreset) {
        llmPreset = p
        llmBaseURL = p.baseURL
        llmModel = p.defaultModel
    }

    var glossaryOrNil: String? {
        let g = glossary.trimmingCharacters(in: .whitespacesAndNewlines)
        return g.isEmpty ? nil : g
    }

    // MARK: Engine configuration

    func cloudStt() -> SttSettings {
        SttSettings.openAiCompatible(
            baseUrl: cloudSttBaseURL,
            apiKey: Keychain.get(cloudSttPreset.keychainAccount),
            model: cloudSttModel
        )
    }

    /// Live STT, or nil when the local model isn't downloaded (records audio only).
    func liveStt(installed: Set<String>) -> SttSettings? {
        switch liveLocation {
        case .cloud: return cloudStt()
        case .local:
            guard let id = Self.firstInstalled(preferred: liveModelId, fallbacks: Self.liveFallbacks, installed: installed)
            else { return nil }
            // Greedy decoding keeps live latency low; accuracy comes from the final pass.
            return .local(modelId: id, beamSize: 1)
        }
    }

    func finalStt(installed: Set<String>) -> SttSettings? {
        switch finalLocation {
        case .cloud: return cloudStt()
        case .local:
            guard let id = Self.firstInstalled(preferred: finalModelId, fallbacks: Self.finalFallbacks, installed: installed)
            else { return nil }
            return .local(modelId: id, beamSize: Int32(beamSize))
        }
    }

    func llm() -> LlmSettings? {
        let model = llmModel.trimmingCharacters(in: .whitespaces)
        guard !model.isEmpty else { return nil }
        let key = Keychain.get(llmPreset.keychainAccount)
        if llmPreset == .anthropic {
            guard let key else { return nil }
            return .anthropic(apiKey: key, model: model)
        }
        if llmPreset.needsKey && key == nil { return nil }
        return .openAiCompatible(baseUrl: llmBaseURL, apiKey: key, model: model)
    }

    static let liveFallbacks = ["large-v3-turbo-q5_0", "large-v3-turbo-q8_0", "large-v3-turbo", "small",
                                "large-v3-q5_0", "large-v3"]
    static let finalFallbacks = ["large-v3", "large-v3-q5_0", "large-v2-q5_0", "large-v3-turbo",
                                 "large-v3-turbo-q8_0", "large-v3-turbo-q5_0"]

    static func firstInstalled(preferred: String, fallbacks: [String], installed: Set<String>) -> String? {
        ([preferred] + fallbacks).first { installed.contains($0) }
    }
}
