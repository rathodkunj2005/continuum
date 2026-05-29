import SwiftUI

@main
struct FNDRDraftApp: App {
    @StateObject private var session: CompanionSession
    @StateObject private var watchBridge: PhoneWatchBridge

    init() {
        let companionSession = CompanionSession()
        _session = StateObject(wrappedValue: companionSession)
        _watchBridge = StateObject(wrappedValue: PhoneWatchBridge(companionSession: companionSession))
    }

    var body: some Scene {
        WindowGroup {
            AppShellView()
                .environmentObject(session)
                .environmentObject(watchBridge)
        }
    }
}
