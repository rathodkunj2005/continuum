import SwiftUI

struct CaptureView: View {
    @EnvironmentObject private var session: CompanionSession
    @StateObject private var model = CaptureViewModel()

    var body: some View {
        NavigationStack {
            Form {
                Section("Remember this") {
                    TextEditor(text: $model.note)
                        .frame(minHeight: 140)
                }

                Section("Metadata") {
                    Picker("Type", selection: $model.captureType) {
                        Text("Note").tag("note")
                        Text("Idea").tag("idea")
                        Text("Todo").tag("todo")
                        Text("Decision").tag("decision")
                        Text("Question").tag("question")
                    }
                    TextField("Project", text: $model.project)
                    TextField("Topic", text: $model.topic)
                }

                Section("Queue") {
                    Text("Pending offline captures: \(model.queueDepth)")
                        .font(.footnote)
                    Button("Flush queue") {
                        model.flushQueue(session: session)
                    }
                }

                Section {
                    Button(model.isSaving ? "Saving…" : "Save to FNDR") {
                        model.save(session: session)
                    }
                    .disabled(model.note.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.isSaving)
                }

                if let statusText = model.statusText {
                    Section("Status") {
                        Text(statusText)
                            .font(.footnote)
                    }
                }
            }
            .navigationTitle("Capture")
            .onAppear {
                model.refreshQueueDepth(session: session)
            }
        }
    }
}
