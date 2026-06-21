//  ConnectionStatusSuite.swift — refreshOnce + reachability tracking.

import Foundation
import ContinuumKit

private actor StubTransport: CompanionTransport {
    enum Response {
        case ok(StatusResponse)
        case failure(URLError)
    }

    private var queue: [Response]
    init(queue: [Response]) { self.queue = queue }

    func send(request: URLRequest) async throws -> (Data, URLResponse) {
        guard !queue.isEmpty else { throw URLError(.unknown) }
        let next = queue.removeFirst()
        switch next {
        case .ok(let status):
            let body = try JSONEncoder().encode(status)
            let resp = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (body, resp)
        case .failure(let err):
            throw err
        }
    }
}

private func sampleStatus(captureStatus: String = "running") -> StatusResponse {
    StatusResponse(
        captureStatus: captureStatus,
        runtimeStatus: "available",
        lastMemoryAtMs: 1234,
        storageStatus: "healthy",
        modelStatus: "available",
        activeProject: nil,
        macName: "Test Mac",
        appVersion: "0.2.11"
    )
}

private func makeService(responses: [StubTransport.Response]) -> ConnectionStatusService {
    let transport = StubTransport(queue: responses)
    let client = CompanionClient(
        config: .init(baseURL: URL(string: "https://127.0.0.1:47812")!, accessToken: "tok"),
        transport: transport
    )
    return ConnectionStatusService(client: client, now: { 1_000 })
}

let connectionStatusSuite = TestSuite("ConnectionStatusService", [
    TestCase("refreshOnce marks .reachable on a 200") {
        let svc = makeService(responses: [.ok(sampleStatus())])
        let snapshot = await svc.refreshOnce()
        try expectEqual(snapshot.reachability, .reachable)
        try expectEqual(snapshot.status?.captureStatus, "running")
        try expectEqual(snapshot.lastCheckedAtMs, 1_000)
    },

    TestCase("refreshOnce marks .unreachable on a transport failure") {
        let svc = makeService(responses: [.failure(URLError(.notConnectedToInternet))])
        let snapshot = await svc.refreshOnce()
        try expectEqual(snapshot.reachability, .unreachable)
        try expectNotNil(snapshot.lastErrorMessage)
    },

    TestCase("transient errors keep the last-known status snapshot so UI doesn't blink") {
        let svc = makeService(responses: [
            .ok(sampleStatus(captureStatus: "paused")),
            .failure(URLError(.timedOut)),
        ])
        let first = await svc.refreshOnce()
        try expectEqual(first.status?.captureStatus, "paused")

        let second = await svc.refreshOnce()
        try expectEqual(second.reachability, .unreachable)
        try expectEqual(second.status?.captureStatus, "paused")
    },
])
