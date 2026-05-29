import SwiftUI

struct AppShellView: View {
    @EnvironmentObject private var session: CompanionSession

    var body: some View {
        TabView {
            StatusView()
                .tabItem {
                    Label("Status", systemImage: "waveform.path.ecg")
                }

            AskView()
                .tabItem {
                    Label("Ask", systemImage: "text.bubble")
                }

            MemoriesView()
                .tabItem {
                    Label("Memories", systemImage: "tray.full")
                }

            CaptureView()
                .tabItem {
                    Label("Capture", systemImage: "square.and.pencil")
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
        .environmentObject(CompanionSession())
}
