@preconcurrency import AVFoundation
import AudioToolbox
import CoreAudio

/// Delivers Float32 PCM from the chosen microphone.
final class MicCapture: @unchecked Sendable {
    typealias Handler = @Sendable (_ samples: [Float], _ sampleRate: UInt32, _ channels: UInt32) -> Void

    private var engine: AVAudioEngine?

    static func authorization() -> AVAuthorizationStatus {
        AVCaptureDevice.authorizationStatus(for: .audio)
    }

    static func requestAccess() async -> Bool {
        await AVCaptureDevice.requestAccess(for: .audio)
    }

    /// - Parameters:
    ///   - deviceUID: empty for the system default input.
    ///   - echoCancellation: Apple's voice processing (AEC + noise suppression). Helps when
    ///     the user is on speakers; slightly changes the mic's sound.
    func start(deviceUID: String, echoCancellation: Bool, handler: @escaping Handler) throws {
        let engine = AVAudioEngine()
        let input = engine.inputNode

        if !deviceUID.isEmpty, let device = AudioDevices.deviceID(forUID: deviceUID), let unit = input.audioUnit {
            var id = device
            try check(
                AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0,
                                     &id, UInt32(MemoryLayout<AudioObjectID>.size)),
                String(localized: "Couldn't select the microphone")
            )
        }
        if echoCancellation {
            try input.setVoiceProcessingEnabled(true)
            // Don't turn the meeting app's audio down while we listen.
            input.voiceProcessingOtherAudioDuckingConfiguration =
                AVAudioVoiceProcessingOtherAudioDuckingConfiguration(enableAdvancedDucking: false, duckingLevel: .min)
        }

        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else {
            throw CoreAudioError(what: String(localized: "The microphone isn't available"), status: -1)
        }
        let rate = UInt32(format.sampleRate)
        input.installTap(onBus: 0, bufferSize: 4096, format: format) { buffer, _ in
            guard let data = buffer.floatChannelData else { return }
            let frames = Int(buffer.frameLength)
            // Mic arrays can expose several channels; the first carries the voice.
            let samples = Array(UnsafeBufferPointer(start: data[0], count: frames))
            handler(samples, rate, 1)
        }
        engine.prepare()
        try engine.start()
        self.engine = engine
    }

    func stop() {
        guard let engine else { return }
        engine.inputNode.removeTap(onBus: 0)
        engine.stop()
        self.engine = nil
    }
}
