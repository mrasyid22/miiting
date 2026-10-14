import AudioToolbox
import CoreAudio
import Foundation

/// Captures what the Mac is playing (the other meeting participants) with a Core Audio
/// process tap (macOS 14.2+). No virtual driver, no screen-recording permission, and
/// nothing joins the call. macOS asks once for "System Audio Recording" permission.
final class SystemAudioCapture: @unchecked Sendable {
    typealias Handler = MicCapture.Handler

    private var tapID = AudioObjectID(kAudioObjectUnknown)
    private var aggregateID = AudioObjectID(kAudioObjectUnknown)
    private var ioProcID: AudioDeviceIOProcID?
    private let queue = DispatchQueue(label: "app.miiting.system-audio", qos: .userInitiated)
    private var handler: Handler?
    private var outputListener: AudioObjectPropertyListenerBlock?

    var isRunning: Bool { ioProcID != nil }

    func start(handler: @escaping Handler) throws {
        self.handler = handler
        try startTap()
        watchDefaultOutput()
    }

    private func startTap() throws {
        // Every process's output mixed to stereo, excluding nothing (Miiting plays no audio).
        let description = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
        description.uuid = UUID()
        description.name = "Miiting"
        description.isPrivate = true
        description.muteBehavior = .unmuted

        var tap = AudioObjectID(kAudioObjectUnknown)
        try check(AudioHardwareCreateProcessTap(description, &tap), String(localized: "Couldn't create the system audio tap"))
        tapID = tap

        var formatAddr = AudioDevices.address(kAudioTapPropertyFormat)
        var asbd = AudioStreamBasicDescription()
        var size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
        try check(AudioObjectGetPropertyData(tap, &formatAddr, 0, nil, &size, &asbd), String(localized: "Couldn't read the system audio format"))

        guard let outputUID = AudioDevices.defaultOutputUID() else {
            throw CoreAudioError(what: String(localized: "No audio output device"), status: -1)
        }
        let aggregate: [String: Any] = [
            kAudioAggregateDeviceNameKey: "Miiting System Audio",
            kAudioAggregateDeviceUIDKey: "Miiting-\(UUID().uuidString)",
            kAudioAggregateDeviceMainSubDeviceKey: outputUID,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceIsStackedKey: false,
            kAudioAggregateDeviceTapAutoStartKey: true,
            kAudioAggregateDeviceSubDeviceListKey: [[kAudioSubDeviceUIDKey: outputUID]],
            kAudioAggregateDeviceTapListKey: [[
                kAudioSubTapDriftCompensationKey: true,
                kAudioSubTapUIDKey: description.uuid.uuidString,
            ]],
        ]
        var agg = AudioObjectID(kAudioObjectUnknown)
        try check(AudioHardwareCreateAggregateDevice(aggregate as CFDictionary, &agg), String(localized: "Couldn't create the aggregate device"))
        aggregateID = agg

        let rate = UInt32(asbd.mSampleRate)
        let channels = max(Int(asbd.mChannelsPerFrame), 1)
        let nonInterleaved = asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0
        let isFloat = asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0
        guard isFloat, asbd.mBitsPerChannel == 32 else {
            throw CoreAudioError(what: String(localized: "Unsupported system audio format"), status: -1)
        }
        let handler = self.handler

        var procID: AudioDeviceIOProcID?
        try check(AudioDeviceCreateIOProcIDWithBlock(&procID, agg, queue) { _, inputData, _, _, _ in
            let buffers = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: inputData))
            guard let first = buffers.first, let firstData = first.mData else { return }
            if nonInterleaved {
                let frames = Int(first.mDataByteSize) / MemoryLayout<Float>.size
                let n = min(channels, buffers.count)
                var interleaved = [Float](repeating: 0, count: frames * n)
                for (c, buf) in buffers.prefix(n).enumerated() {
                    guard let p = buf.mData?.assumingMemoryBound(to: Float.self) else { continue }
                    for i in 0..<frames { interleaved[i * n + c] = p[i] }
                }
                handler?(interleaved, rate, UInt32(n))
            } else {
                let count = Int(first.mDataByteSize) / MemoryLayout<Float>.size
                let samples = Array(UnsafeBufferPointer(start: firstData.assumingMemoryBound(to: Float.self), count: count))
                handler?(samples, rate, UInt32(first.mNumberChannels))
            }
        }, String(localized: "Couldn't attach the audio callback"))
        ioProcID = procID
        try check(AudioDeviceStart(agg, procID), String(localized: "Couldn't start system audio"))
    }

    /// Rebuild the tap when the output device changes (AirPods connected, etc.),
    /// because the aggregate is clocked by the old device.
    private func watchDefaultOutput() {
        var addr = AudioDevices.address(kAudioHardwarePropertyDefaultOutputDevice)
        let block: AudioObjectPropertyListenerBlock = { [weak self] _, _ in
            guard let self, self.isRunning else { return }
            self.teardownTap()
            try? self.startTap()
        }
        outputListener = block
        AudioObjectAddPropertyListenerBlock(AudioObjectID(kAudioObjectSystemObject), &addr, queue, block)
    }

    private func teardownTap() {
        if aggregateID != kAudioObjectUnknown {
            if let procID = ioProcID {
                AudioDeviceStop(aggregateID, procID)
                AudioDeviceDestroyIOProcID(aggregateID, procID)
            }
            AudioHardwareDestroyAggregateDevice(aggregateID)
        }
        if tapID != kAudioObjectUnknown {
            AudioHardwareDestroyProcessTap(tapID)
        }
        ioProcID = nil
        aggregateID = AudioObjectID(kAudioObjectUnknown)
        tapID = AudioObjectID(kAudioObjectUnknown)
    }

    func stop() {
        if let block = outputListener {
            var addr = AudioDevices.address(kAudioHardwarePropertyDefaultOutputDevice)
            AudioObjectRemovePropertyListenerBlock(AudioObjectID(kAudioObjectSystemObject), &addr, queue, block)
            outputListener = nil
        }
        queue.sync { teardownTap() }
        handler = nil
    }

    deinit {
        // Not via `queue.sync`: deinit may run on `queue` itself.
        if let block = outputListener {
            var addr = AudioDevices.address(kAudioHardwarePropertyDefaultOutputDevice)
            AudioObjectRemovePropertyListenerBlock(AudioObjectID(kAudioObjectSystemObject), &addr, queue, block)
        }
        teardownTap()
    }
}
