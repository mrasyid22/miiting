import MiitingCore
import SwiftUI

extension ModelFit {
    var label: String {
        switch self {
        case .light: String(localized: "Light", comment: "Model fit: barely noticeable on this Mac")
        case .good: String(localized: "Good fit", comment: "Model fit: suits this Mac's RAM")
        case .heavy: String(localized: "Heavy", comment: "Model fit: works but slows other apps")
        case .notRecommended: String(localized: "Not recommended", comment: "Model fit: likely to run out of RAM")
        }
    }

    var color: Color {
        switch self {
        case .light, .good: .green
        case .heavy: .orange
        case .notRecommended: .red
        }
    }

    var symbol: String {
        switch self {
        case .light: "leaf.fill"
        case .good: "checkmark.circle.fill"
        case .heavy: "exclamationmark.triangle.fill"
        case .notRecommended: "xmark.octagon.fill"
        }
    }

    var explanation: String {
        switch self {
        case .light: String(localized: "Barely noticeable, even during a video call.")
        case .good: String(localized: "A good fit for this Mac's RAM.")
        case .heavy: String(localized: "Works, but other apps slow down. Better for processing after the meeting.")
        case .notRecommended: String(localized: "Likely to run this Mac out of RAM and into swap.")
        }
    }
}

struct FitBadge: View {
    let fit: ModelFit
    var body: some View {
        Label(fit.label, systemImage: fit.symbol)
            .font(.caption2.weight(.semibold))
            .foregroundStyle(fit.color)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(fit.color.opacity(0.12), in: Capsule())
            .help(fit.explanation)
    }
}

/// Five-dot scale for the relative accuracy / speed estimates.
struct RatingDots: View {
    let label: String
    let value: UInt8
    var body: some View {
        HStack(spacing: 4) {
            Text(label).font(.caption2).foregroundStyle(.secondary)
            HStack(spacing: 2) {
                ForEach(1...5, id: \.self) { i in
                    Circle()
                        .fill(i <= Int(value) ? Color.miiting : Color.secondary.opacity(0.25))
                        .frame(width: 5, height: 5)
                }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(label) \(String(value)) of 5")
    }
}

extension DeviceInfo {
    var ramLabel: String {
        let gb = String(Int((Double(ramBytes) / 1_073_741_824).rounded()))
        return String(localized: "\(gb) GB RAM")
    }

    var coresLabel: String {
        let (p, e) = (String(performanceCores), String(efficiencyCores))
        return efficiencyCores > 0
            ? String(localized: "\(p) performance + \(e) efficiency cores")
            : String(localized: "\(p) cores")
    }

    var specLine: String {
        var parts = [ramLabel, coresLabel]
        if freeDiskBytes > 0 { parts.append(String(localized: "\(formatBytes(freeDiskBytes)) free")) }
        return parts.joined(separator: " · ")
    }
}

/// "Your Mac" card: detected specs plus the recommended setup.
struct DeviceCard: View {
    @Environment(AppModel.self) private var model
    var compact = false

    var body: some View {
        if let rec = model.recommendation {
            let applied = model.settings.matches(rec)
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: "laptopcomputer")
                        .font(.system(size: 26))
                        .foregroundStyle(Color.miiting)
                        .frame(width: 34)
                    VStack(alignment: .leading, spacing: 3) {
                        HStack(spacing: 8) {
                            Text(rec.device.chip).font(.headline)
                            Text("\(rec.device.tierLabel) tier")
                                .font(.caption.weight(.semibold))
                                .padding(.horizontal, 8)
                                .padding(.vertical, 2)
                                .background(Color.miiting.opacity(0.15), in: Capsule())
                                .foregroundStyle(Color.miiting)
                        }
                        Text(rec.device.specLine).font(.caption).foregroundStyle(.secondary)
                    }
                }
                Text(rec.headline).font(.callout)
                if !compact {
                    VStack(alignment: .leading, spacing: 6) {
                        ForEach(rec.reasons, id: \.self) { r in
                            Label(r, systemImage: "checkmark")
                                .font(.callout)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                ForEach(rec.warnings, id: \.self) { w in
                    Label(w, systemImage: "exclamationmark.triangle.fill")
                        .font(.callout)
                        .foregroundStyle(.orange)
                }
                HStack(spacing: 10) {
                    if applied {
                        Label("Using the recommended setup", systemImage: "checkmark.seal.fill")
                            .font(.callout)
                            .foregroundStyle(.green)
                    } else {
                        Button("Apply recommendation") { model.applyRecommendation() }
                            .buttonStyle(.borderedProminent)
                    }
                    if rec.downloadBytes > 0 {
                        Button("Download the models it needs (\(formatBytes(rec.downloadBytes)))") {
                            model.downloadRecommended()
                        }
                        .disabled(rec.modelIds.allSatisfy { model.downloads[$0] != nil || model.installedModelIds.contains($0) })
                    }
                }
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.miiting.opacity(0.06), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Color.miiting.opacity(0.18)))
        }
    }
}
