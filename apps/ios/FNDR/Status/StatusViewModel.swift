import Foundation
import Combine
import FNDRKit

@MainActor
final class StatusViewModel: ObservableObject {
    @Published private(set) var snapshot: ConnectionSnapshot = .unknown
    @Published private(set) var errorText: String?

    private var session: CompanionSession?
    private var service: ConnectionStatusService?

    init() {}

    func attach(session: CompanionSession) {
        if self.session !== session {
            self.session = session
            service = nil
        }
    }

    func refresh() {
        Task {
            do {
                guard let session else {
                    errorText = "No companion session bound."
                    return
                }
                if service == nil {
                    let client = try session.makeClient()
                    service = ConnectionStatusService(client: client)
                }
                guard let service else { return }
                let latest = await service.refreshOnce()
                await MainActor.run {
                    snapshot = latest
                    errorText = nil
                }
            } catch {
                await MainActor.run {
                    errorText = error.localizedDescription
                }
            }
        }
    }

    func pauseCapture() {
        runCaptureAction(.pause)
    }

    func resumeCapture() {
        runCaptureAction(.resume)
    }

    private func runCaptureAction(_ action: CaptureAction) {
        Task {
            do {
                guard let session else {
                    errorText = "No companion session bound."
                    return
                }
                let client = try session.makeClient()
                _ = try await client.captureControl(request: .init(action: action, durationMinutes: nil, reason: "ios_status_tab"))
                refresh()
            } catch {
                await MainActor.run {
                    errorText = error.localizedDescription
                }
            }
        }
    }
}
