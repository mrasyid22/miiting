@preconcurrency import AVFoundation
import Foundation

/// Converts any audio or video file AVFoundation can read (m4a, mp3, wav, mp4, mov, …)
/// into a temporary 16 kHz mono 16-bit WAV for the Rust core.
enum AudioImporter {
    static let supportedTypes = ["public.audio", "public.movie", "public.mpeg-4-audio", "com.microsoft.waveform-audio"]

    static func convertToWav(_ url: URL) async throws -> URL {
        let asset = AVURLAsset(url: url)
        guard let track = try await asset.loadTracks(withMediaType: .audio).first else {
            throw CoreAudioError(what: String(localized: "The file has no audio track"), status: -1)
        }
        let reader = try AVAssetReader(asset: asset)
        let settings: [String: Any] = [
            AVFormatIDKey: kAudioFormatLinearPCM,
            AVSampleRateKey: 16_000,
            AVNumberOfChannelsKey: 1,
            AVLinearPCMBitDepthKey: 16,
            AVLinearPCMIsFloatKey: false,
            AVLinearPCMIsBigEndianKey: false,
            AVLinearPCMIsNonInterleaved: false,
        ]
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: settings)
        reader.add(output)

        let dest = FileManager.default.temporaryDirectory.appendingPathComponent("miiting-import-\(UUID().uuidString).wav")
        FileManager.default.createFile(atPath: dest.path, contents: nil)
        let handle = try FileHandle(forWritingTo: dest)
        defer { try? handle.close() }
        try handle.write(contentsOf: Data(count: 44)) // header placeholder

        guard reader.startReading() else {
            throw reader.error ?? CoreAudioError(what: String(localized: "Couldn't read the audio file"), status: -1)
        }
        var dataBytes: UInt32 = 0
        while let sample = output.copyNextSampleBuffer() {
            guard let block = CMSampleBufferGetDataBuffer(sample) else { continue }
            let length = CMBlockBufferGetDataLength(block)
            var chunk = Data(count: length)
            let status = chunk.withUnsafeMutableBytes { raw in
                CMBlockBufferCopyDataBytes(block, atOffset: 0, dataLength: length, destination: raw.baseAddress!)
            }
            if status == kCMBlockBufferNoErr {
                try handle.write(contentsOf: chunk)
                dataBytes += UInt32(length)
            }
        }
        if reader.status == .failed {
            throw reader.error ?? CoreAudioError(what: String(localized: "Couldn't read the audio file"), status: -1)
        }
        try handle.seek(toOffset: 0)
        try handle.write(contentsOf: wavHeader(dataBytes: dataBytes))
        return dest
    }

    private static func wavHeader(dataBytes: UInt32) -> Data {
        var d = Data()
        func u32(_ v: UInt32) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        func u16(_ v: UInt16) { withUnsafeBytes(of: v.littleEndian) { d.append(contentsOf: $0) } }
        d.append(contentsOf: Array("RIFF".utf8)); u32(36 + dataBytes)
        d.append(contentsOf: Array("WAVE".utf8))
        d.append(contentsOf: Array("fmt ".utf8)); u32(16); u16(1); u16(1); u32(16_000); u32(32_000); u16(2); u16(16)
        d.append(contentsOf: Array("data".utf8)); u32(dataBytes)
        return d
    }
}
