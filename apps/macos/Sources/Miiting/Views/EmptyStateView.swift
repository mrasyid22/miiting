import MiitingCore
import SwiftUI

struct EmptyStateView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        VStack(spacing: 22) {
            ZStack {
                Circle()
                    .fill(Color.miiting.gradient)
                    .frame(width: 96, height: 96)
                    .shadow(color: Color.miiting.opacity(0.35), radius: 18, y: 8)
                Image(systemName: "waveform")
                    .font(.system(size: 42, weight: .semibold))
                    .foregroundStyle(.white)
            }
            VStack(spacing: 8) {
                Text(verbatim: "Miiting")
                    .font(.system(size: 34, weight: .bold))
                Text("Automatic meeting notes, tuned for Indonesian.\nNo bot joins your call, and everything runs on your Mac.")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
            }
            GlassEffectContainer(spacing: 12) {
                HStack(spacing: 12) {
                    Button {
                        Task { await model.startRecording() }
                    } label: {
                        Label("Start recording", systemImage: "record.circle")
                            .padding(.horizontal, 6)
                    }
                    .buttonStyle(.glassProminent)
                    .tint(.red)

                    Button {
                        model.showImporter = true
                    } label: {
                        Label("Import recording…", systemImage: "square.and.arrow.down")
                            .padding(.horizontal, 6)
                    }
                    .buttonStyle(.glass)
                }
                .controlSize(.extraLarge)
                .disabled(model.engine == nil)
            }

            if model.engine != nil && !model.hasLiveModel && !model.hasFinalModel {
                ModelSetupCard()
                    .frame(maxWidth: 460)
            }

            Text("Press ⇧⌘R to start or stop recording")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(40)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// Prompts for the models recommended for this Mac.
struct ModelSetupCard: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let rec = model.recommendation
        let entries = model.recommendedModelIds.compactMap { model.model($0) }
        let missing = entries.filter { !$0.installed }
        let total = missing.reduce(UInt64(0)) { $0 + $1.sizeBytes }
        Card {
            Label("Download transcription models", systemImage: "arrow.down.circle")
                .font(.headline)
            Text(rec.map { String(localized: "Picked for your \($0.device.chip) with \($0.device.ramLabel). Download once, then work offline.") }
                 ?? String(localized: "Download once, then work offline."))
                .font(.callout)
                .foregroundStyle(.secondary)
            ForEach(entries, id: \.id) { m in
                HStack(spacing: 8) {
                    Text(role(of: m.id, rec))
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                        .frame(width: 46, alignment: .leading)
                    Text(m.name).font(.callout)
                    Spacer()
                    if m.installed {
                        Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                    } else if let d = model.downloads[m.id] {
                        ProgressView(value: Double(d.fraction)).frame(width: 120)
                    } else {
                        Text(formatBytes(m.sizeBytes)).font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            if !missing.isEmpty {
                Button {
                    model.downloadRecommended()
                } label: {
                    Text("Download all (\(formatBytes(total)))")
                }
                .buttonStyle(.borderedProminent)
                .disabled(missing.allSatisfy { model.downloads[$0.id] != nil })
            }
        }
    }

    private func role(of id: String, _ rec: Recommendation?) -> String {
        if id == rec?.liveModelId { return "Live" }
        if id == rec?.finalModelId { return "Final" }
        return id == "silero-vad" ? "VAD" : ""
    }
}
