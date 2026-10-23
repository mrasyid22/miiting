import AppKit
import MiitingCore
import SwiftUI

struct LiveMeetingView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let r = model.recorder
        ZStack(alignment: .bottom) {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    header(r)
                    if let err = r.systemAudioError {
                        notice(icon: "speaker.slash", text: String(localized: "System audio can't be recorded: \(err). Only the microphone is being recorded."))
                    } else if r.capturingSystemAudio && !r.heardSystemAudio && r.elapsedMs > 20_000 {
                        systemAudioHint
                    }
                    if r.segments.isEmpty {
                        placeholder(r)
                    } else {
                        TranscriptList(segments: r.segments)
                    }
                    Color.clear.frame(height: 110)
                }
                .padding(.horizontal, 40)
                .padding(.top, 24)
                .frame(maxWidth: 880, alignment: .leading)
                .frame(maxWidth: .infinity)
            }
            .defaultScrollAnchor(.bottom, for: .sizeChanges)

            RecordingBar()
                .padding(.bottom, 22)
        }
        .navigationTitle("Recording")
    }

    private func header(_ r: RecordingController) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(r.meetingId.flatMap { model.meeting($0)?.title } ?? String(localized: "New meeting"))
                .font(.system(size: 28, weight: .bold))
            HStack(spacing: 8) {
                statusLabel(r)
                MetaPill(text: AppSettings.meetingLanguageName(r.language), systemImage: "globe")
                if !r.capturingSystemAudio {
                    MetaPill(text: String(localized: "Microphone only"), systemImage: "mic")
                }
            }
        }
    }

    @ViewBuilder
    private func statusLabel(_ r: RecordingController) -> some View {
        switch (r.state, r.liveStatus) {
        case (.starting, _):
            Label("Getting ready…", systemImage: "hourglass").foregroundStyle(.secondary)
        case (.stopping, _):
            Label("Saving the recording…", systemImage: "square.and.arrow.down").foregroundStyle(.secondary)
        case (.paused, _):
            Label("Paused", systemImage: "pause.circle").foregroundStyle(.orange)
        case (_, .loadingModel):
            Label("Loading the Whisper model…", systemImage: "cpu").foregroundStyle(.secondary)
        case (_, .recordingOnly):
            Label("Recording audio. The transcript is made after the meeting.", systemImage: "waveform")
                .foregroundStyle(.secondary)
        case (_, .behind(let q)):
            Label("The live transcript is \(String(q)) chunks behind. The final transcript will fill them in.",
                  systemImage: "tortoise")
                .foregroundStyle(.orange)
        default:
            Label("Listening", systemImage: "waveform")
                .symbolEffect(.variableColor.iterative, isActive: r.state == .recording)
                .foregroundStyle(Color.miiting)
        }
    }

    private func placeholder(_ r: RecordingController) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(r.liveTranscription
                 ? String(localized: "The transcript appears here when someone speaks.")
                 : String(localized: "The live transcript is off because no model is downloaded. The recording is still saved."))
                .foregroundStyle(.secondary)
            Text("Tip: wear headphones so other participants aren't recorded twice through your microphone.")
                .font(.callout)
                .foregroundStyle(.tertiary)
        }
        .padding(.top, 12)
    }

    private var systemAudioHint: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "speaker.wave.2").foregroundStyle(.orange)
            VStack(alignment: .leading, spacing: 6) {
                Text("No sound from the meeting app yet.")
                    .font(.callout.weight(.semibold))
                Text("If others are talking but the Them meter stays flat, allow Miiting in Privacy & Security › Screen & System Audio Recording, then start recording again.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
                Button("Open Privacy Settings") {
                    if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture") {
                        NSWorkspace.shared.open(url)
                    }
                }
                .controlSize(.small)
            }
        }
        .padding(14)
        .background(Color.orange.opacity(0.08), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
    }

    private func notice(icon: String, text: String) -> some View {
        Label(text, systemImage: icon)
            .font(.callout)
            .foregroundStyle(.secondary)
            .padding(12)
            .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
}

/// Floating Liquid Glass controls for the active recording.
struct RecordingBar: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let r = model.recorder
        GlassEffectContainer(spacing: 12) {
            HStack(spacing: 12) {
                HStack(spacing: 14) {
                    HStack(spacing: 8) {
                        RecordingDot(paused: r.state == .paused)
                        Text(formatTimestamp(r.elapsedMs))
                            .font(.system(.title3, design: .rounded).weight(.semibold).monospacedDigit())
                            .contentTransition(.numericText())
                    }
                    Divider().frame(height: 20)
                    LevelMeter(label: AudioSource.mic.label, level: r.micLevel, tint: .miiting, active: r.state == .recording)
                    LevelMeter(label: AudioSource.system.label, level: r.systemLevel, tint: .indigo,
                               active: r.capturingSystemAudio && r.state == .recording)
                }
                .padding(.horizontal, 18)
                .padding(.vertical, 12)
                .glassEffect(.regular, in: .capsule)

                Button {
                    r.togglePause()
                } label: {
                    Image(systemName: r.state == .paused ? "play.fill" : "pause.fill")
                        .frame(width: 22, height: 22)
                }
                .buttonStyle(.glass)
                .buttonBorderShape(.circle)
                .controlSize(.extraLarge)
                .disabled(!r.isRecording)
                .help(r.state == .paused ? String(localized: "Resume (⇧⌘P)") : String(localized: "Pause (⇧⌘P)"))

                Button {
                    Task { await model.stopRecording() }
                } label: {
                    Label("Stop", systemImage: "stop.fill")
                        .padding(.horizontal, 4)
                }
                .buttonStyle(.glassProminent)
                .tint(.red)
                .controlSize(.extraLarge)
                .disabled(!r.isRecording)
                .help("Stop recording (⇧⌘R)")
            }
        }
    }
}
