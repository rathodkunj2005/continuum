import SwiftUI

@main
struct FNDRDraftApp: App {
    @StateObject private var session: CompanionSession

    init() {
        let companionSession = CompanionSession()
        _session = StateObject(wrappedValue: companionSession)
    }

    var body: some Scene {
        WindowGroup {
            AppShellView()
                .environmentObject(session)
        }
    }
}
