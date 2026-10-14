import MiitingCore
import SwiftUI

extension Color {
    /// Miiting's accent: a calm teal that reads well on both light and dark glass.
    static let miiting = Color(red: 0.09, green: 0.56, blue: 0.53)
}

extension AudioSource {
    /// Who is speaking: the person recording, or everyone else on the call.
    var label: String {
        switch self {
        case .mic: String(localized: "Me", comment: "Speaker label for the microphone channel")
        case .system: String(localized: "Them", comment: "Speaker label for the other participants (system audio)")
        case .file: String(localized: "Speaker", comment: "Speaker label for an imported recording")
        }
    }

    var tint: Color {
        switch self {
        case .mic: .miiting
        case .system: .indigo
        case .file: .secondary
        }
    }
}

func formatTimestamp(_ ms: Int64) -> String {
    let s = max(ms, 0) / 1000
    let h = s / 3600, m = (s / 60) % 60, sec = s % 60
    return h > 0 ? String(format: "%d:%02d:%02d", h, m, sec) : String(format: "%02d:%02d", m, sec)
}

/// The locale for dates and numbers: the interface language plus the user's region.
let appLocale: Locale = {
    let language = Bundle.main.preferredLocalizations.first ?? "en"
    guard let region = Locale.current.region?.identifier else { return Locale(identifier: language) }
    return Locale(identifier: "\(language)_\(region)")
}()

/// "1 hr, 5 min" / "1 j 5 mnt", rounded to minutes ("< 1 min" for short recordings).
func formatDuration(_ ms: Int64) -> String {
    let formatter = DateComponentsFormatter()
    formatter.allowedUnits = ms >= 3_600_000 ? [.hour, .minute] : [.minute]
    formatter.unitsStyle = .abbreviated
    var calendar = Calendar.current
    calendar.locale = appLocale
    formatter.calendar = calendar
    if ms > 0 && ms < 60_000 {
        return String(localized: "< 1 min", comment: "Duration shorter than a minute")
    }
    return formatter.string(from: TimeInterval(ms / 1000)) ?? ""
}

func formatBytes(_ bytes: UInt64) -> String {
    ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
}

func meetingDateString(_ ms: Int64) -> String {
    Date(timeIntervalSince1970: Double(ms) / 1000)
        .formatted(.dateTime.weekday(.wide).day().month(.wide).year().hour().minute().locale(appLocale))
}

/// Small capsule naming who spoke.
struct SpeakerChip: View {
    let source: AudioSource
    var body: some View {
        Text(source.label)
            .font(.caption.weight(.semibold))
            .foregroundStyle(source.tint)
            .padding(.horizontal, 8)
            .padding(.vertical, 2)
            .background(source.tint.opacity(0.12), in: Capsule())
    }
}

/// Segmented level meter, driven by 0…1 values from the Rust core.
struct LevelMeter: View {
    let label: String
    let level: Float
    var tint: Color = .miiting
    var active = true

    var body: some View {
        HStack(spacing: 6) {
            Text(label)
                .font(.caption2.weight(.medium))
                .foregroundStyle(.secondary)
            HStack(spacing: 2) {
                ForEach(0..<10, id: \.self) { i in
                    Capsule()
                        .fill(Float(i) / 10 < level && active ? tint : Color.secondary.opacity(0.25))
                        .frame(width: 3, height: 6 + CGFloat(i) * 0.9)
                }
            }
            .animation(.easeOut(duration: 0.12), value: level)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Level \(label)")
        .accessibilityValue(Text(Double(level).formatted(.percent.precision(.fractionLength(0)).locale(appLocale))))
    }
}

/// States whether this meeting ever left the Mac.
struct PrivacyBadge: View {
    let egress: [String]
    var body: some View {
        if egress.isEmpty {
            Label("100% local", systemImage: "lock.shield")
                .font(.caption)
                .foregroundStyle(.green)
                .help("This meeting's audio and text never left your Mac.")
        } else {
            Label("Sent to \(egress.joined(separator: ", "))", systemImage: "icloud.and.arrow.up")
                .font(.caption)
                .foregroundStyle(.orange)
                .help("Part of this meeting was processed by a third-party service.")
        }
    }
}

struct MetaPill: View {
    let text: String
    var systemImage: String?
    var body: some View {
        Group {
            if let systemImage {
                Label(text, systemImage: systemImage)
            } else {
                Text(text)
            }
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(.quaternary.opacity(0.6), in: Capsule())
    }
}

/// Pulsing red dot used while recording.
struct RecordingDot: View {
    var paused = false
    @State private var pulse = false
    var body: some View {
        Circle()
            .fill(paused ? Color.orange : Color.red)
            .frame(width: 10, height: 10)
            .opacity(paused ? 1 : (pulse ? 0.35 : 1))
            .onAppear {
                withAnimation(.easeInOut(duration: 0.9).repeatForever(autoreverses: true)) { pulse = true }
            }
    }
}

/// A content card. Content stays on standard materials; Liquid Glass is reserved for
/// floating controls, per the HIG.
struct Card<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        VStack(alignment: .leading, spacing: 12) { content }
            .padding(18)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.background.secondary, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(.separator.opacity(0.5)))
    }
}
