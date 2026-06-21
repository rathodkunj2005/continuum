import SwiftUI

struct StatusView: View {
    @EnvironmentObject private var session: CompanionSession
    @StateObject private var model = StatusViewModel()

    var body: some View {
        NavigationStack {
            List {
                if let paired = session.pairedMac {
                    Section("Paired Mac") {
                        Text(paired.macName)
                        Text("\(paired.host):\(paired.port)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                Section("Runtime") {
                    row("Reachability", model.snapshot.reachability.rawValue)
                    row("Capture", model.snapshot.status?.captureStatus ?? "unknown")
                    row("Storage", model.snapshot.status?.storageStatus ?? "unknown")
                    row("Model", model.snapshot.status?.modelStatus ?? "unknown")
                    row("Version", model.snapshot.status?.appVersion ?? "unknown")
                }

                if let errorText = model.errorText {
                    Section("Error") {
                        Text(errorText)
                            .font(.footnote)
                            .foregroundStyle(.red)
                    }
                }

                Section("Actions") {
                    Button("Refresh status") {
                        model.refresh()
                    }
                    Button("Pause capture") {
                        model.pauseCapture()
                    }
                    Button("Resume capture") {
                        model.resumeCapture()
                    }
                }
            }
            .navigationTitle("Status")
            .toolbar {
                NavigationLink("Pair") {
                    PairingView(session: session)
                }
            }
            .onAppear {
                model.attach(session: session)
                model.refresh()
            }
        }
    }

    private func row(_ title: String, _ value: String) -> some View {
        HStack {
            Text(title)
            Spacer()
            Text(value)
                .foregroundStyle(.secondary)
        }
    }
}
