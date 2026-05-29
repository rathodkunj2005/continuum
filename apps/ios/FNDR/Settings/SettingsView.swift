import SwiftUI

struct SettingsView: View {
    @EnvironmentObject private var session: CompanionSession

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

                Section("Device build") {
                    Text("Phone pairing, status, Ask, memory search, manual capture, offline queueing, and watch relay are enabled in this build.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Settings")
        }
    }
}
