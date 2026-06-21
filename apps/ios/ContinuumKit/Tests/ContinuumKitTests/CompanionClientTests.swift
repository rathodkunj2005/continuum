#if canImport(XCTest)
//  CompanionClientTests.swift — exercises the HTTP shapes by injecting a
//  scripted in-memory CompanionTransport. We don't use URLProtocol because
//  the client takes a transport interface — much easier to stub.

import XCTest
@testable import ContinuumKit

private actor StubTransport: CompanionTransport {
    struct Stub {
        let status: Int
        let body: Data
        let headers: [String: String]
    }

    private(set) var lastRequest: URLRequest?
    private(set) var lastBodyString: String?
    private var responses: [Stub]

    init(responses: [Stub]) { self.responses = responses }

    func send(request: URLRequest) async throws -> (Data, URLResponse) {
        lastRequest = request
        lastBodyString = request.httpBody.flatMap { String(data: $0, encoding: .utf8) }
        guard !responses.isEmpty else {
            throw URLError(.unknown)
        }
        let next = responses.removeFirst()
        let url = request.url ?? URL(string: "https://example.invalid/x")!
        let resp = HTTPURLResponse(
            url: url,
            statusCode: next.status,
            httpVersion: "HTTP/1.1",
            headerFields: next.headers
        )!
        return (next.body, resp)
    }
}

final class CompanionClientTests: XCTestCase {
    private func makeClient(
        token: String? = "tok-abc",
        responses: [StubTransport.Stub]
    ) -> (CompanionClient, StubTransport) {
        let transport = StubTransport(responses: responses)
        let base = URL(string: "https://127.0.0.1:47812")!
        let client = CompanionClient(
            config: .init(baseURL: base, accessToken: token),
            transport: transport
        )
        return (client, transport)
    }

    func testStatusOKDecodes() async throws {
        let body = #"""
        {"capture_status":"running","runtime_status":"available","last_memory_at_ms":1234,"storage_status":"healthy","model_status":"available","active_project":null,"mac_name":"Mac","app_version":"0.2.11"}
        """#
        let (client, transport) = makeClient(responses: [
            .init(status: 200, body: Data(body.utf8), headers: [:])
        ])
        let status = try await client.status()
        XCTAssertEqual(status.captureStatus, "running")
        XCTAssertEqual(status.lastMemoryAtMs, 1234)

        let lastReq = await transport.lastRequest
        XCTAssertEqual(lastReq?.value(forHTTPHeaderField: "Authorization"), "Bearer tok-abc")
        XCTAssertEqual(lastReq?.url?.path, "/v1/status")
    }

    func testStatusWithoutTokenThrowsUnauthenticated() async {
        let (client, _) = makeClient(token: nil, responses: [])
        do {
            _ = try await client.status()
            XCTFail("expected unauthenticated")
        } catch CompanionError.unauthenticated {
            // ok
        } catch {
            XCTFail("got \(error)")
        }
    }

    func testCompletePairingMapsConflictToPairingCodeUsed() async {
        let errorBody = #"{"error":"pairing_code_used","message":"pairing code already consumed"}"#
        let (client, _) = makeClient(token: nil, responses: [
            .init(status: 409, body: Data(errorBody.utf8), headers: [:])
        ])
        let req = PairCompleteRequest(
            pairingCode: "111111",
            deviceName: "iPhone",
            deviceType: .iphone,
            appVersion: nil
        )
        do {
            _ = try await client.completePairing(request: req)
            XCTFail("expected pairing_code_used")
        } catch CompanionError.pairingCodeUsed {
            // ok
        } catch {
            XCTFail("got \(error)")
        }
    }

    func testCompletePairingMapsBadRequestToPairingCodeInvalid() async {
        let errorBody = #"{"error":"pairing_code_invalid","message":"pairing code is invalid or expired"}"#
        let (client, _) = makeClient(token: nil, responses: [
            .init(status: 400, body: Data(errorBody.utf8), headers: [:])
        ])
        let req = PairCompleteRequest(
            pairingCode: "000000",
            deviceName: "iPhone",
            deviceType: .iphone,
            appVersion: nil
        )
        do {
            _ = try await client.completePairing(request: req)
            XCTFail("expected pairing_code_invalid")
        } catch CompanionError.pairingCodeInvalid {
            // ok
        } catch {
            XCTFail("got \(error)")
        }
    }

    func testForbiddenMapsRegardlessOfBody() async {
        let (client, _) = makeClient(responses: [
            .init(status: 403, body: Data(#"{"error":"forbidden","message":"go away"}"#.utf8), headers: [:])
        ])
        do {
            _ = try await client.status()
            XCTFail("expected forbidden")
        } catch CompanionError.forbidden {
            // ok
        } catch {
            XCTFail("got \(error)")
        }
    }

    func testManualMemoryPostsBodyAndDecodesResponse() async throws {
        let resp = #"{"memory_id":"mem-1","status":"indexed","source_type":"iphone_manual_capture","duplicate":false}"#
        let (client, transport) = makeClient(responses: [
            .init(status: 200, body: Data(resp.utf8), headers: [:])
        ])
        let req = ManualMemoryRequest(
            text: "Hello Continuum",
            clientEventId: "evt-1",
            captureType: "idea"
        )
        let result = try await client.createManualMemory(request: req)
        XCTAssertEqual(result.memoryId, "mem-1")
        XCTAssertEqual(result.sourceType, "iphone_manual_capture")

        let sent = await transport.lastBodyString ?? ""
        XCTAssertTrue(sent.contains("\"client_event_id\":\"evt-1\""))
        XCTAssertTrue(sent.contains("\"text\":\"Hello Continuum\""))
    }

    func testNon2xxWithUnknownErrorBodyFallsBackToHTTP() async {
        let (client, _) = makeClient(responses: [
            .init(status: 500, body: Data("not even json".utf8), headers: [:])
        ])
        do {
            _ = try await client.status()
            XCTFail("expected http error")
        } catch CompanionError.http(let status, let code, _) {
            XCTAssertEqual(status, 500)
            XCTAssertEqual(code, .unknown)
        } catch {
            XCTFail("got \(error)")
        }
    }
}
#endif
