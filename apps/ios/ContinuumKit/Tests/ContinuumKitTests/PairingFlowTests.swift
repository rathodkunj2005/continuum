#if canImport(XCTest)
//  PairingFlowTests.swift — exercises the iPhone-side pairing state
//  machine end-to-end with stubbed transport + in-memory keychain.

import XCTest
@testable import ContinuumKit

private actor StubTransport: CompanionTransport {
    let response: (status: Int, body: Data)
    private(set) var calls = 0

    init(status: Int, body: Data) {
        self.response = (status, body)
    }

    func send(request: URLRequest) async throws -> (Data, URLResponse) {
        calls += 1
        let resp = HTTPURLResponse(
            url: request.url!,
            statusCode: response.status,
            httpVersion: nil,
            headerFields: nil
        )!
        return (response.body, resp)
    }
}

final class PairingFlowTests: XCTestCase {
    private let validPayloadJSON = """
    {
      "version": 1,
      "mac_name": "Test Mac",
      "host": "127.0.0.1",
      "port": 47812,
      "tls": true,
      "cert_fingerprint_sha256": "abcdef",
      "pairing_code": "381729",
      "expires_at_ms": 9999999999999
    }
    """

    func testParseQRPayloadDecodes() throws {
        let payload = try PairingFlow.parseQRPayload(validPayloadJSON)
        XCTAssertEqual(payload.pairingCode, "381729")
        XCTAssertEqual(payload.port, 47812)
    }

    func testParseQRPayloadRejectsNonJSON() {
        XCTAssertThrowsError(try PairingFlow.parseQRPayload("not json"))
    }

    func testAcceptValidPayloadEntersReady() async throws {
        let payload = try PairingFlow.parseQRPayload(validPayloadJSON)
        let flow = PairingFlow(keychain: InMemoryKeychainStore(), now: { 1 })
        let state = await flow.accept(payload: payload)
        XCTAssertEqual(state, .ready(payload))
    }

    func testAcceptExpiredCodeFailsImmediately() async throws {
        let json = validPayloadJSON.replacingOccurrences(
            of: "9999999999999",
            with: "1"
        )
        let payload = try PairingFlow.parseQRPayload(json)
        let flow = PairingFlow(keychain: InMemoryKeychainStore(), now: { 2 })
        let state = await flow.accept(payload: payload)
        if case .failed(let msg) = state {
            XCTAssertTrue(msg.contains("expired"))
        } else {
            XCTFail("expected .failed, got \(state)")
        }
    }

    func testAcceptRejectsNonNumericCode() async throws {
        let bad = validPayloadJSON.replacingOccurrences(
            of: "\"381729\"",
            with: "\"abc123\""
        )
        let payload = try PairingFlow.parseQRPayload(bad)
        let flow = PairingFlow(keychain: InMemoryKeychainStore(), now: { 1 })
        let state = await flow.accept(payload: payload)
        if case .failed = state { /* ok */ } else { XCTFail("expected .failed") }
    }

    func testCompleteHappyPathStoresTokenAndPairedMac() async throws {
        let payload = try PairingFlow.parseQRPayload(validPayloadJSON)
        let pairResponse = """
        {
          "device_id": "dev_iphone_abc",
          "access_token": "tok-xyz",
          "mac_name": "Test Mac",
          "permissions": ["ask","search","manual_capture","capture_control"]
        }
        """
        let keychain = InMemoryKeychainStore()
        let flow = PairingFlow(
            keychain: keychain,
            now: { 1_700_000_000_000 },
            transportFactory: { _ in StubTransport(status: 200, body: Data(pairResponse.utf8)) }
        )
        _ = await flow.accept(payload: payload)
        let state = await flow.complete(deviceName: "Test iPhone", deviceType: .iphone, appVersion: "0.1.0")

        if case .paired(let mac) = state {
            XCTAssertEqual(mac.deviceId, "dev_iphone_abc")
            XCTAssertEqual(mac.permissions.count, 4)
        } else {
            XCTFail("expected .paired, got \(state)")
        }

        XCTAssertEqual(try keychain.stringForKey(KeychainKeys.accessToken), "tok-xyz")
        let stored: PairedMac? = try keychain.codableForKey(KeychainKeys.pairedMac, as: PairedMac.self)
        XCTAssertEqual(stored?.deviceId, "dev_iphone_abc")
        XCTAssertEqual(stored?.host, "127.0.0.1")
        XCTAssertEqual(stored?.certFingerprintSha256, "abcdef")
    }

    func testCompleteOn409SurfacesPairingCodeUsedMessage() async throws {
        let payload = try PairingFlow.parseQRPayload(validPayloadJSON)
        let errBody = #"{"error":"pairing_code_used","message":"already used"}"#
        let keychain = InMemoryKeychainStore()
        let flow = PairingFlow(
            keychain: keychain,
            transportFactory: { _ in StubTransport(status: 409, body: Data(errBody.utf8)) }
        )
        _ = await flow.accept(payload: payload)
        let state = await flow.complete(deviceName: "iPhone", deviceType: .iphone, appVersion: nil)
        if case .failed(let msg) = state {
            XCTAssertTrue(msg.lowercased().contains("used"))
        } else {
            XCTFail("expected .failed, got \(state)")
        }
        XCTAssertNil(try keychain.stringForKey(KeychainKeys.accessToken))
    }

    func testCompleteWithoutAcceptFails() async {
        let flow = PairingFlow(keychain: InMemoryKeychainStore())
        let state = await flow.complete(deviceName: "iPhone", deviceType: .iphone, appVersion: nil)
        if case .failed = state { /* ok */ } else { XCTFail("expected .failed") }
    }
}
#endif
