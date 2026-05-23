import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var session: CompanionSession
    @AppStorage("fndr.cache.mode") private var cacheMode = "standard"
    @AppStorage("fndr.security.appLock") private var appLock = false
    @State private var queueDepth = 0

    var body: some View {
        NavigationStack {
            Form {
                Section("Pairing") {
                    if let paired = session.pairedMac {
                        Text("Paired with \(paired.macName)")
                        Text("\(paired.host):\(paired.port)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Button("Clear pairing", role: .destructive) {
                            session.clearPairing()
                        }
                    } else {
                        Text("No paired Mac")
                        NavigationLink("Pair now") {
                            PairingView(session: session)
                        }
                    }
                }

                Section("Privacy & Security") {
                    Toggle("App lock (Face ID)", isOn: $appLock)
                    Text("Draft setting for slice 7 hardening.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }

                Section("Cache") {
                    Picker("Cache mode", selection: $cacheMode) {
                        Text("Minimal").tag("minimal")
                        Text("Standard").tag("standard")
                        Text("Rich").tag("rich")
                    }
                    Text("Offline queue pending: \(queueDepth)")
                        .font(.footnote)
                }

                Section("Roadmap") {
                    Text("App Intents and watchOS bridge are scaffolded in this slice and will be finalized with full Xcode simulator/device validation.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Settings")
            .onAppear {
                Task {
                    let pending = await session.offlineQueue.pending()
                    await MainActor.run {
                        queueDepth = pending.count
                    }
                }
            }
        }
    }
}
