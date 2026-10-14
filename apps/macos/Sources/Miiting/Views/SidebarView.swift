import MiitingCore
import SwiftUI

struct SidebarView: View {
    @Environment(AppModel.self) private var model
    @State private var renameTarget: MeetingItem?
    @State private var renameText = ""
    @State private var deleteTarget: MeetingItem?

    var body: some View {
        @Bindable var model = model
        List(selection: $model.selection) {
            if let job = model.importJob {
                Section {
                    HStack(spacing: 10) {
                        ProgressView().controlSize(.small)
                        Text(job.message).font(.callout).lineLimit(2)
                    }
                    .padding(.vertical, 4)
                }
            }
            ForEach(groups, id: \.title) { group in
                Section(group.title) {
                    ForEach(group.items, id: \.id) { item in
                        MeetingRow(item: item, job: model.jobs[item.id], live: model.recorder.meetingId == item.id)
                            .tag(item.id)
                            .contextMenu { menu(for: item) }
                    }
                }
            }
        }
        .listStyle(.sidebar)
        .overlay {
            if model.meetings.isEmpty && model.importJob == nil && model.engine != nil {
                if model.searchText.isEmpty {
                    ContentUnavailableView("No meetings yet", systemImage: "waveform",
                                           description: Text("Your recorded meetings will appear here."))
                } else {
                    ContentUnavailableView.search(text: model.searchText)
                }
            }
        }
        .alert("Rename meeting", isPresented: Binding(get: { renameTarget != nil }, set: { if !$0 { renameTarget = nil } })) {
            TextField("Title", text: $renameText)
            Button("Save") {
                if let t = renameTarget { model.rename(t.id, to: renameText) }
                renameTarget = nil
            }
            Button("Cancel", role: .cancel) { renameTarget = nil }
        }
        .confirmationDialog("Delete “\(deleteTarget?.title ?? "")”?",
            isPresented: Binding(get: { deleteTarget != nil }, set: { if !$0 { deleteTarget = nil } }),
            titleVisibility: .visible
        ) {
            Button("Delete meeting and recording", role: .destructive) {
                if let t = deleteTarget { model.delete(t.id) }
                deleteTarget = nil
            }
        } message: {
            Text("The transcript, notes and audio files will be permanently deleted from this Mac.")
        }
    }

    @ViewBuilder
    private func menu(for item: MeetingItem) -> some View {
        Button("Rename…") {
            renameText = item.title
            renameTarget = item
        }
        Button("Copy notes") { model.copyMarkdown(item.id) }
        Button("Export Markdown…") { model.exportMarkdown(item.id) }
        Button("Show in Finder") { model.revealInFinder(item.id) }
        Divider()
        Button("Delete…", role: .destructive) { deleteTarget = item }
            .disabled(model.recorder.meetingId == item.id)
    }

    private struct MeetingGroup {
        let title: String
        var items: [MeetingItem]
    }

    private var groups: [MeetingGroup] {
        let cal = Calendar.current
        let now = Date()
        var result: [MeetingGroup] = []
        for item in model.meetings {
            let date = Date(timeIntervalSince1970: Double(item.createdAtMs) / 1000)
            let title: String
            if cal.isDateInToday(date) {
                title = String(localized: "Today")
            } else if cal.isDateInYesterday(date) {
                title = String(localized: "Yesterday")
            } else if let days = cal.dateComponents([.day], from: date, to: now).day, days < 7 {
                title = String(localized: "Previous 7 days")
            } else {
                title = date.formatted(.dateTime.month(.wide).year().locale(appLocale))
            }
            if result.last?.title == title {
                result[result.count - 1].items.append(item)
            } else {
                result.append(MeetingGroup(title: title, items: [item]))
            }
        }
        return result
    }
}

private struct MeetingRow: View {
    let item: MeetingItem
    let job: JobProgress?
    let live: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                Text(item.title)
                    .font(.body.weight(.medium))
                    .lineLimit(1)
                Spacer(minLength: 4)
                statusIcon
            }
            HStack(spacing: 5) {
                Text(Date(timeIntervalSince1970: Double(item.createdAtMs) / 1000)
                    .formatted(.dateTime.hour().minute().locale(appLocale)))
                if item.durationMs > 0 {
                    Text(verbatim: "·")
                    Text(formatDuration(item.durationMs))
                }
                if let job {
                    Text(verbatim: "·")
                    Text(job.stage == .summary ? String(localized: "Summarizing…") : String(localized: "Transcribing…"))
                        .foregroundStyle(Color.miiting)
                }
                Spacer(minLength: 0)
                if !item.egress.isEmpty {
                    Image(systemName: "icloud")
                        .help("Also processed by \(item.egress.joined(separator: ", "))")
                }
                if item.hasSummary {
                    Image(systemName: "text.document")
                        .help("Notes available")
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .padding(.vertical, 3)
    }

    @ViewBuilder
    private var statusIcon: some View {
        if live || item.status == .recording {
            RecordingDot()
        } else if job != nil || item.status == .processing {
            ProgressView().controlSize(.mini)
        }
    }
}
