import Foundation
import FNDRKit
#if canImport(WatchConnectivity)
import WatchConnectivity
#endif

@MainActor
final class PhoneWatchBridge: NSObject, ObservableObject {
    private let companionSession: CompanionSession

    init(companionSession: CompanionSession) {
        self.companionSession = companionSession
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

    private func handle(request: WatchBridgeRequest) async -> WatchBridgeResponse {
        do {
            switch request.route {
            case .ask:
                let query = request.payload["query"]?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                guard !query.isEmpty else {
                    return WatchBridgeResponse(ok: false, message: "query is empty")
                }
                let client = try companionSession.makeClient()
                let response = try await client.ask(request: AskRequest(query: query, limit: 3, answerStyle: "short"))
                let answer = response.answer.trimmingCharacters(in: .whitespacesAndNewlines)
                let clipped = String(answer.prefix(280))
                return WatchBridgeResponse(
                    ok: true,
                    payload: [
                        "answer": clipped,
                        "sources": String(response.sourceCards.count),
                    ]
                )

            case .remember:
                let text = request.payload["text"]?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
                guard !text.isEmpty else {
                    return WatchBridgeResponse(ok: false, message: "note is empty")
                }
                let immediate = await companionSession.captureNowOrQueue(
                    text: text,
                    captureType: "note",
                    project: nil,
                    topic: nil
                )
                return WatchBridgeResponse(
                    ok: true,
                    payload: [
                        "result": immediate ? "saved" : "queued",
                    ]
                )

            case .status:
                let client = try companionSession.makeClient()
                let status = try await client.status()
                return WatchBridgeResponse(
                    ok: true,
                    payload: [
                        "capture_status": status.captureStatus,
                        "runtime_status": status.runtimeStatus,
                    ]
                )

            case .captureControl:
                let actionRaw = request.payload["action"]?.lowercased() ?? ""
                let action: CaptureAction
                switch actionRaw {
                case "pause":
                    action = .pause
                case "resume":
                    action = .resume
                default:
                    return WatchBridgeResponse(ok: false, message: "unsupported action")
                }
                let client = try companionSession.makeClient()
                let result = try await client.captureControl(
                    request: .init(action: action, durationMinutes: nil, reason: "watch_bridge")
                )
                return WatchBridgeResponse(
                    ok: true,
                    payload: [
                        "capture_status": result.captureStatus,
                    ]
                )

            case .recent:
                let limit = Int(request.payload["limit"] ?? "") ?? 5
                let boundedLimit = max(1, min(limit, 7))
                let client = try companionSession.makeClient()
                let result = try await client.searchMemories(
                    request: .init(query: "today", limit: boundedLimit, timeFilter: "last_24h")
                )
                let topTitles = result.cards.prefix(boundedLimit).map { $0.title }
                return WatchBridgeResponse(
                    ok: true,
                    payload: [
                        "count": String(topTitles.count),
                        "items": topTitles.joined(separator: "\n"),
                    ]
                )
            }
        } catch {
            return WatchBridgeResponse(ok: false, message: error.localizedDescription)
        }
    }
}

#if canImport(WatchConnectivity)
extension PhoneWatchBridge: WCSessionDelegate {
    nonisolated func session(
        _ session: WCSession,
        didReceiveMessage message: [String : Any],
        replyHandler: @escaping ([String : Any]) -> Void
    ) {
        Task { @MainActor in
            do {
                let request = try WatchBridgeRequest.fromDictionary(message)
                let response = await self.handle(request: request)
                let dictionary = try response.toDictionary()
                replyHandler(dictionary)
            } catch {
                let fallback = WatchBridgeResponse(ok: false, message: error.localizedDescription)
                let dictionary = (try? fallback.toDictionary()) ?? ["ok": false, "message": "bridge error"]
                replyHandler(dictionary)
            }
        }
    }

    nonisolated func session(
        _ session: WCSession,
        activationDidCompleteWith activationState: WCSessionActivationState,
        error: Error?
    ) {
    }

    nonisolated func sessionDidBecomeInactive(_ session: WCSession) {
    }

    nonisolated func sessionDidDeactivate(_ session: WCSession) {
        session.activate()
    }
}
#endif
