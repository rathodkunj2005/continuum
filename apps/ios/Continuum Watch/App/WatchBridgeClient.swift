import Foundation
import ContinuumKit
#if canImport(WatchConnectivity)
import WatchConnectivity
#endif

@MainActor
final class WatchBridgeClient: NSObject, ObservableObject {
    static let shared = WatchBridgeClient()

    private override init() {
        super.init()
        activateIfPossible()
    }

    func activateIfPossible() {
        #if canImport(WatchConnectivity)
        guard WCSession.isSupported() else { return }
        let wcSession = WCSession.default
        wcSession.delegate = self
        wcSession.activate()
        #endif
    }

    func ask(_ query: String) async -> WatchBridgeResponse {
        await send(route: .ask, payload: ["query": query])
    }

    func remember(_ text: String) async -> WatchBridgeResponse {
        await send(route: .remember, payload: ["text": text])
    }

    func status() async -> WatchBridgeResponse {
        await send(route: .status, payload: [:])
    }

    func pauseCapture() async -> WatchBridgeResponse {
        await send(route: .captureControl, payload: ["action": CaptureAction.pause.rawValue])
    }

    func recent(limit: Int = 5) async -> WatchBridgeResponse {
        await send(route: .recent, payload: ["limit": String(max(1, min(limit, 7)))])
    }

    private func send(route: WatchBridgeRoute, payload: [String: String]) async -> WatchBridgeResponse {
        #if canImport(WatchConnectivity)
        guard WCSession.isSupported() else {
            return WatchBridgeResponse(ok: false, message: "WatchConnectivity unsupported")
        }

        let request = WatchBridgeRequest(
            route: route,
            payload: payload,
            sentAtMs: Int64(Date().timeIntervalSince1970 * 1000)
        )
        do {
            let message = try request.toDictionary()
            let reply = try await sendMessage(message)
            return try WatchBridgeResponse.fromDictionary(reply)
        } catch {
            return WatchBridgeResponse(ok: false, message: error.localizedDescription)
        }
        #else
        return WatchBridgeResponse(ok: false, message: "WatchConnectivity unavailable")
        #endif
    }

    #if canImport(WatchConnectivity)
    private func sendMessage(_ message: [String: Any]) async throws -> [String: Any] {
        try await withCheckedThrowingContinuation { continuation in
            WCSession.default.sendMessage(
                message,
                replyHandler: { reply in
                    continuation.resume(returning: reply)
                },
                errorHandler: { error in
                    continuation.resume(throwing: error)
                }
            )
        }
    }
    #endif
}

#if canImport(WatchConnectivity)
extension WatchBridgeClient: WCSessionDelegate {
    nonisolated func session(
        _ session: WCSession,
        activationDidCompleteWith activationState: WCSessionActivationState,
        error: Error?
    ) {
    }
}
#endif
