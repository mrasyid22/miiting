import MiitingCore
import SwiftUI

struct MeetingDetailView: View {
    @Environment(AppModel.self) private var model
    let detail: MeetingDetail

    enum Tab: String, CaseIterable, Identifiable {
        case summary, transcript
        var id: String { rawValue }
        var title: String {
            self == .summary ? String(localized: "Notes") : String(localized: "Transcript")
        }
    }

    @State private var tab: Tab = .summary
    @State private var title = ""
    @State private var editing = false
    @State private var draft = ""
    @State private var confirmDelete = false
    @State private var confirmRetranscribe = false
    @FocusState private var titleFocused: Bool

    private var meeting: MeetingItem { detail.meeting }
    private var job: JobProgress? { model.jobs[meeting.id] }
    private var hasSummary: Bool { !(detail.summaryMarkdown ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header
                if let job { JobBanner(job: job) { model.cancelJob(meeting.id) } }
                Picker("View", selection: $tab) {
                    ForEach(Tab.allCases) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .fixedSize()

                switch tab {
                case .summary: summaryPane
                case .transcript: transcriptPane
                }
            }
            .padding(.horizontal, 40)
            .padding(.top, 24)
            .padding(.bottom, 48)
            .frame(maxWidth: 880, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .navigationTitle(meeting.title)
        .toolbar { toolbar }
        .onAppear(perform: syncFromDetail)
        .onChange(of: meeting.title) { _, _ in if !titleFocused { title = meeting.title } }
        .onChange(of: hasSummary) { _, has in if has, !editing { tab = .summary } }
        .confirmationDialog("Delete this meeting?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Delete meeting and recording", role: .destructive) { model.delete(meeting.id) }
        } message: {
            Text("The transcript, notes and audio files will be permanently deleted from this Mac.")
        }
        .confirmationDialog("Re-transcribe in \(AppSettings.meetingLanguageName(meeting.language))?", isPresented: $confirmRetranscribe, titleVisibility: .visible) {
            Button("Re-transcribe") { Task { await model.retranscribe(meeting.id) } }
            Button("Not now", role: .cancel) {}
        } message: {
            Text("The current transcript was made in the previous language. Re-transcribing replaces it.")
        }
    }

    private func syncFromDetail() {
        title = meeting.title
        tab = hasSummary || job?.stage == .summary ? .summary : .transcript
    }

    // MARK: Header

    private var header: some View {
        VStack(alignment: .leading, spacing: 10) {
            TextField("Meeting title", text: $title)
                .font(.system(size: 28, weight: .bold))
                .textFieldStyle(.plain)
                .focused($titleFocused)
                .onSubmit { model.rename(meeting.id, to: title) }
                .onChange(of: titleFocused) { _, focused in
                    if !focused, title != meeting.title { model.rename(meeting.id, to: title) }
                }
            HStack(spacing: 8) {
                Text(meetingDateString(meeting.createdAtMs))
                    .font(.callout)
                    .foregroundStyle(.secondary)
                if meeting.durationMs > 0 { MetaPill(text: formatDuration(meeting.durationMs), systemImage: "clock") }
                MeetingLanguageMenu(code: Binding(get: { meeting.language }, set: { code in
                    guard code != meeting.language else { return }
                    model.setLanguage(meeting.id, to: code)
                    if !detail.audioFiles.isEmpty { confirmRetranscribe = true }
                })) {
                    MetaPill(text: AppSettings.meetingLanguageName(meeting.language), systemImage: "globe")
                }
                .menuStyle(.button)
                .buttonStyle(.plain)
                .menuIndicator(.hidden)
                .fixedSize()
                .disabled(job != nil)
                .help("Language spoken in this meeting. Change it, then re-transcribe.")
                if let engine = meeting.sttEngine {
                    MetaPill(text: engine, systemImage: "waveform")
                        .help(detail.shownPass == .final ? String(localized: "Final transcript") : String(localized: "Live transcript draft"))
                }
                PrivacyBadge(egress: meeting.egress)
            }
        }
    }

    // MARK: Summary

    @ViewBuilder
    private var summaryPane: some View {
        if editing {
            VStack(alignment: .trailing, spacing: 10) {
                TextEditor(text: $draft)
                    .font(.body.monospaced())
                    .scrollContentBackground(.hidden)
                    .padding(12)
                    .frame(minHeight: 420)
                    .background(.background.secondary, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                HStack {
                    Button("Cancel") { editing = false }
                    Button("Save") {
                        model.updateSummary(meeting.id, markdown: draft)
                        editing = false
                    }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut("s", modifiers: .command)
                }
            }
        } else if let md = detail.summaryMarkdown, hasSummary {
            VStack(alignment: .leading, spacing: 14) {
                MarkdownView(markdown: md)
                if let by = detail.summaryModel {
                    Text("Written by \(by). Review it before sharing.")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                        .padding(.top, 8)
                }
            }
        } else if job?.stage != .summary {
            emptySummary
        }
    }

    private var emptySummary: some View {
        Card {
            Label("No notes yet", systemImage: "text.document")
                .font(.headline)
            if detail.segments.isEmpty {
                Text(job == nil
                     ? String(localized: "The transcript is still empty. Transcribe the recording first, then write notes.")
                     : String(localized: "Notes can be written once transcription finishes."))
                    .foregroundStyle(.secondary)
            } else if model.summaryConfigured {
                Text("Write notes with a summary, decisions and action items from this transcript.")
                    .foregroundStyle(.secondary)
                HStack(spacing: 10) {
                    ForEach(model.templates.prefix(4), id: \.id) { t in
                        Button(t.name) { Task { await model.summarize(meeting.id, templateId: t.id) } }
                            .help(t.description)
                    }
                }
                .buttonStyle(.bordered)
                .controlSize(.large)
            } else {
                Text("Choose a notes provider first: Ollama to stay 100% local, or Claude/OpenAI with your API key.")
                    .foregroundStyle(.secondary)
                SettingsLink { Text("Open Notes Settings") }
                    .buttonStyle(.borderedProminent)
            }
        }
    }

    // MARK: Transcript

    @ViewBuilder
    private var transcriptPane: some View {
        if detail.segments.isEmpty {
            Card {
                Label(job == nil ? String(localized: "Empty transcript") : String(localized: "Transcribing…"), systemImage: "waveform")
                    .font(.headline)
                if job == nil {
                    Text(detail.audioFiles.isEmpty
                         ? String(localized: "No audio was recorded.")
                         : String(localized: "The audio is saved but hasn't been transcribed yet."))
                        .foregroundStyle(.secondary)
                    if !detail.audioFiles.isEmpty {
                        Button("Transcribe now") { Task { await model.retranscribe(meeting.id) } }
                            .buttonStyle(.borderedProminent)
                    }
                }
            }
        } else {
            VStack(alignment: .leading, spacing: 16) {
                if detail.shownPass == .live && job == nil {
                    HStack(alignment: .firstTextBaseline, spacing: 10) {
                        Image(systemName: "sparkles").foregroundStyle(Color.miiting)
                        Text("This is the live transcription draft. Re-transcribe with the accurate model for the best result.")
                            .font(.callout)
                            .foregroundStyle(.secondary)
                        Spacer()
                        Button("Re-transcribe") { Task { await model.retranscribe(meeting.id) } }
                    }
                    .padding(12)
                    .background(Color.miiting.opacity(0.08), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                }
                TranscriptList(segments: detail.segments)
            }
        }
    }

    // MARK: Toolbar

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            Menu {
                ForEach(model.templates, id: \.id) { t in
                    Button {
                        Task { await model.summarize(meeting.id, templateId: t.id) }
                    } label: {
                        Text(t.name)
                        Text(t.description)
                    }
                }
            } label: {
                Label(hasSummary ? String(localized: "Rewrite notes") : String(localized: "Write notes"), systemImage: "sparkles")
            }
            .disabled(job != nil || detail.segments.isEmpty || !model.summaryConfigured)
            .help(model.summaryConfigured ? String(localized: "Write notes with a template") : String(localized: "Set up a notes provider in Settings"))

            Menu {
                Button("Accurate model (from Settings)") { Task { await model.retranscribe(meeting.id) } }
                Divider()
                ForEach(model.whisperModels.filter(\.installed), id: \.id) { m in
                    Button(m.name) { Task { await model.retranscribe(meeting.id, modelId: m.id) } }
                }
            } label: {
                Label("Re-transcribe", systemImage: "arrow.trianglehead.2.clockwise")
            }
            .disabled(job != nil || detail.audioFiles.isEmpty)
            .help("Re-transcribe the audio with another model")

            if tab == .summary && hasSummary {
                Button {
                    draft = detail.summaryMarkdown ?? ""
                    editing.toggle()
                } label: {
                    Label("Edit notes", systemImage: editing ? "checkmark" : "pencil")
                }
                .help("Edit the notes (Markdown)")
            }

            Menu {
                Button("Copy notes (Markdown)") { model.copyMarkdown(meeting.id) }
                Button("Export as .md…") { model.exportMarkdown(meeting.id) }
                Divider()
                Button("Show recording in Finder") { model.revealInFinder(meeting.id) }
            } label: {
                Label("Share", systemImage: "square.and.arrow.up")
            }

            Button(role: .destructive) {
                confirmDelete = true
            } label: {
                Label("Delete", systemImage: "trash")
            }
            .help("Delete meeting")
        }
    }
}

struct JobBanner: View {
    let job: JobProgress
    let onCancel: () -> Void

    var body: some View {
        HStack(spacing: 14) {
            ProgressView(value: Double(max(job.fraction, 0.02)))
                .progressViewStyle(.circular)
                .controlSize(.small)
            VStack(alignment: .leading, spacing: 2) {
                Text(job.stage == .summary ? String(localized: "Writing notes") : String(localized: "Final transcript"))
                    .font(.callout.weight(.semibold))
                Text(job.message)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer()
            if job.cancel != nil {
                Button("Cancel", action: onCancel)
                    .controlSize(.small)
            }
        }
        .padding(14)
        .background(Color.miiting.opacity(0.08), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Color.miiting.opacity(0.2)))
    }
}
