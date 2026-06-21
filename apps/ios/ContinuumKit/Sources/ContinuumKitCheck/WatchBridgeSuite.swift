import Foundation
import ContinuumKit

private actor StubWatchTransport: WatchBridgeTransport {
    private(set) var lastRequest: WatchBridgeRequest?
    private let response: WatchBridgeResponse

    init(response: WatchBridgeResponse = .init(ok: true, payload: ["ok": "1"])) {
        self.response = response
    }

    func send(request: WatchBridgeRequest) async throws -> WatchBridgeResponse {
        lastRequest = request
        return response
    }
}

let watchBridgeSuite = TestSuite("WatchBridge", [
    TestCase("WatchBridgeRequest round-trips coding keys") {
        let req = WatchBridgeRequest(route: .ask, payload: ["query": "hello"], sentAtMs: 123)
        let data = try JSONEncoder().encode(req)
        let json = String(data: data, encoding: .utf8) ?? ""
        try expect(json.contains("\"sent_at_ms\":123"))

        let parsed = try JSONDecoder().decode(WatchBridgeRequest.self, from: data)
        try expectEqual(parsed.route, .ask)
        try expectEqual(parsed.payload["query"], "hello")
    },

    TestCase("wire dictionary conversions round-trip request and response") {
        let req = WatchBridgeRequest(route: .remember, payload: ["text": "ship it"], sentAtMs: 55)
        let reqDict = try req.toDictionary()
        let reqDecoded = try WatchBridgeRequest.fromDictionary(reqDict)
        try expectEqual(reqDecoded.route, .remember)
        try expectEqual(reqDecoded.payload["text"], "ship it")

        let response = WatchBridgeResponse(ok: true, message: "ok", payload: ["status": "running"])
        let respDict = try response.toDictionary()
        let respDecoded = try WatchBridgeResponse.fromDictionary(respDict)
        try expectEqual(respDecoded.ok, true)
        try expectEqual(respDecoded.payload["status"], "running")
    },

    TestCase("service methods map to expected routes") {
        let transport = StubWatchTransport()
        let service = WatchBridgeService(transport: transport, now: { 999 })

        _ = try await service.ask("what was I doing")
        var req = await transport.lastRequest
        try expectEqual(req?.route, .ask)
        try expectEqual(req?.payload["query"], "what was I doing")

        _ = try await service.captureControl(.pause)
        req = await transport.lastRequest
        try expectEqual(req?.route, .captureControl)
        try expectEqual(req?.payload["action"], "pause")

        _ = try await service.recent(limit: 7)
        req = await transport.lastRequest
        try expectEqual(req?.route, .recent)
        try expectEqual(req?.payload["limit"], "7")
    },
])
