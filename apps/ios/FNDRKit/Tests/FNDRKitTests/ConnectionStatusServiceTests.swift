#if canImport(XCTest)
//  ConnectionStatusServiceTests.swift — covers the single-tick refresh
//  (not the polling loop, which is timing-sensitive).

import XCTest
@testable import FNDRKit

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
            let resp = HTTPURLResponse(
                url: request.url!,
                statusCode: 200,
                httpVersion: nil,
                headerFields: nil
            )!
            return (body, resp)
        case .failure(let err):
            throw err
        }
    }
}

final class ConnectionStatusServiceTests: XCTestCase {
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
            config: .init(
                baseURL: URL(string: "https://127.0.0.1:47812")!,
                accessToken: "tok"
            ),
            transport: transport
        )
        return ConnectionStatusService(client: client, now: { 1_000 })
    }

    func testRefreshOnceMarksReachableOnSuccess() async {
        let svc = makeService(responses: [.ok(sampleStatus())])
        let snapshot = await svc.refreshOnce()
        XCTAssertEqual(snapshot.reachability, .reachable)
        XCTAssertEqual(snapshot.status?.captureStatus, "running")
        XCTAssertEqual(snapshot.lastCheckedAtMs, 1_000)
    }

    func testRefreshOnceMarksUnreachableOnTransportFailure() async {
        let svc = makeService(responses: [.failure(URLError(.notConnectedToInternet))])
        let snapshot = await svc.refreshOnce()
        XCTAssertEqual(snapshot.reachability, .unreachable)
        XCTAssertNotNil(snapshot.lastErrorMessage)
    }

    func testRefreshOnceKeepsPriorStatusOnTransientError() async {
        let svc = makeService(responses: [
            .ok(sampleStatus(captureStatus: "paused")),
            .failure(URLError(.timedOut)),
        ])
        let first = await svc.refreshOnce()
        XCTAssertEqual(first.status?.captureStatus, "paused")

        let second = await svc.refreshOnce()
        XCTAssertEqual(second.reachability, .unreachable)
        // Keep last-known status so the UI doesn't blink to "empty" mid-flap.
        XCTAssertEqual(second.status?.captureStatus, "paused")
    }
}
#endif
