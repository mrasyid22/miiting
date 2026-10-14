import MiitingCore
import SwiftUI

/// Consecutive segments from the same speaker, shown as one paragraph.
struct TranscriptBlock: Identifiable, Equatable {
    let id: Int
    let source: AudioSource
    let startMs: Int64
    var text: String

    static func group(_ segments: [TranscriptSegment], maxGapMs: Int64 = 2500) -> [TranscriptBlock] {
        var blocks: [TranscriptBlock] = []
        var lastEnd: Int64 = -1
        for s in segments {
            if var last = blocks.last, last.source == s.source, s.startMs - lastEnd <= maxGapMs {
                last.text += " " + s.text
                blocks[blocks.count - 1] = last
            } else {
                blocks.append(TranscriptBlock(id: blocks.count, source: s.source, startMs: s.startMs, text: s.text))
            }
            lastEnd = max(lastEnd, s.endMs)
        }
        return blocks
    }
}

struct TranscriptRow: View {
    let block: TranscriptBlock

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                SpeakerChip(source: block.source)
                Text(formatTimestamp(block.startMs))
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.tertiary)
            }
            Text(block.text)
                .font(.body)
                .lineSpacing(4)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, 6)
    }
}

struct TranscriptList: View {
    let segments: [TranscriptSegment]

    var body: some View {
        LazyVStack(alignment: .leading, spacing: 10) {
            ForEach(TranscriptBlock.group(segments)) { block in
                TranscriptRow(block: block)
            }
        }
    }
}
