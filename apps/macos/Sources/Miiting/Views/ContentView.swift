import SwiftUI

struct ContentView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        @Bindable var settings = model.settings
        NavigationSplitView {
            SidebarView()
                .navigationSplitViewColumnWidth(min: 240, ideal: 290, max: 380)
        } detail: {
            DetailRouter()
        }
        .searchable(text: $model.searchText, placement: .sidebar, prompt: "Search titles, transcripts and notes")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                if !model.recorder.isActive {
                    MeetingLanguageMenu(code: $settings.language) {
                        Label(AppSettings.meetingLanguageName(settings.language), systemImage: "globe")
                            .labelStyle(.titleAndIcon)
                    }
                    .help("Language spoken in the next meeting")
                }
            }
            ToolbarItem(placement: .primaryAction) {
                RecordToolbarButton()
            }
            ToolbarItem(placement: .primaryAction) {
                Button {
                    model.showImporter = true
                } label: {
                    Label("Import recording", systemImage: "square.and.arrow.down")
                }
                .help("Import an audio or video recording (⌘I)")
                .disabled(model.engine == nil)
            }
        }
        .fileImporter(
            isPresented: $model.showImporter,
            allowedContentTypes: [.audio, .movie, .mpeg4Audio, .wav, .mp3],
            allowsMultipleSelection: false
        ) { result in
            if case .success(let urls) = result, let url = urls.first {
                Task { await model.importFile(url) }
            }
        }
        .onOpenURL { url in
            Task { await model.handle(url) }
        }
        .sheet(isPresented: $model.showOnboarding) {
            OnboardingView()
        }
        .overlay(alignment: .bottom) {
            ToastView()
                .padding(.bottom, model.recorder.isActive ? 96 : 24)
        }
        .overlay {
            if let error = model.startupError {
                ContentUnavailableView("Miiting couldn't start", systemImage: "exclamationmark.triangle", description: Text(error))
                    .background(.background)
            }
        }
    }
}

private struct DetailRouter: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let recorder = model.recorder
        Group {
            if let id = model.selection {
                if recorder.isActive, recorder.meetingId == id {
                    LiveMeetingView()
                } else if let detail = model.detail, detail.meeting.id == id {
                    MeetingDetailView(detail: detail)
                        .id(id)
                } else {
                    ContentUnavailableView("Meeting not found", systemImage: "questionmark.folder")
                }
            } else if recorder.isActive {
                LiveMeetingView()
            } else {
                EmptyStateView()
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct RecordToolbarButton: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        let r = model.recorder
        if r.isActive {
            Button {
                if let id = r.meetingId { model.selection = id }
            } label: {
                HStack(spacing: 6) {
                    RecordingDot(paused: r.state == .paused)
                    Text(formatTimestamp(r.elapsedMs)).monospacedDigit()
                }
            }
            .help("Show the meeting being recorded")
        } else {
            Button {
                Task { await model.startRecording() }
            } label: {
                Label("Record", systemImage: "record.circle")
            }
            .buttonStyle(.glassProminent)
            .tint(.red)
            .help("Start recording a meeting (⇧⌘R)")
            .disabled(model.engine == nil)
        }
    }
}

private struct ToastView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        if let toast = model.toast {
            HStack(spacing: 10) {
                Image(systemName: icon(toast.kind))
                    .foregroundStyle(color(toast.kind))
                Text(toast.message)
                    .font(.callout)
                    .fixedSize(horizontal: false, vertical: true)
                if let action = toast.action {
                    Button(action.title) {
                        action.perform()
                        model.dismissToast()
                    }
                    .buttonStyle(.glassProminent)
                    .controlSize(.small)
                }
                Button {
                    model.dismissToast()
                } label: {
                    Image(systemName: "xmark").font(.caption.weight(.semibold))
                }
                .buttonStyle(.borderless)
                .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .frame(maxWidth: 520)
            .glassEffect(.regular, in: .capsule)
            .transition(.move(edge: .bottom).combined(with: .opacity))
            .id(toast.id)
        }
    }

    private func icon(_ k: Toast.Kind) -> String {
        switch k {
        case .info: "info.circle.fill"
        case .success: "checkmark.circle.fill"
        case .error: "exclamationmark.triangle.fill"
        }
    }

    private func color(_ k: Toast.Kind) -> Color {
        switch k {
        case .info: .blue
        case .success: .green
        case .error: .orange
        }
    }
}
