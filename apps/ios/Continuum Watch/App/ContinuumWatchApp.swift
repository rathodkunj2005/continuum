import SwiftUI

@main
struct ContinuumWatchApp: App {
    @StateObject private var bridge = WatchBridgeClient.shared

    var body: some Scene {
        WindowGroup {
            WatchRootView()
                .onAppear {
                    bridge.activateIfPossible()
                }
        }
    }
}

struct WatchRootView: View {
    var body: some View {
        TabView {
            WatchAskView()
            WatchRememberView()
            WatchRecentView()
            WatchStatusView()
        }
    }
}
