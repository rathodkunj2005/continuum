import SwiftUI
import FNDRKit

struct AppShellView: View {
    @EnvironmentObject private var session: CompanionSession

    var body: some View {
        TabView {
            AskView()
                .tabItem {
                    Label("Ask", systemImage: "message")
                }

            MemoriesView()
                .tabItem {
                    Label("Memories", systemImage: "square.stack")
                }

            CaptureView()
                .tabItem {
                    Label("Capture", systemImage: "plus.bubble")
                }

            StatusView()
                .tabItem {
                    Label("Status", systemImage: "waveform.path.ecg")
                }

            SettingsView()
                .tabItem {
                    Label("Settings", systemImage: "gearshape")
                }
        }
        .overlay(alignment: .top) {
            if session.pairedMac == nil {
                Text("Not paired")
                    .font(.caption)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 4)
                    .background(.thinMaterial, in: Capsule())
                    .padding(.top, 8)
            }
        }
    }
}

#Preview {
    AppShellView()
        .environmentObject(CompanionSession(keychain: InMemoryKeychainStore()))
}
