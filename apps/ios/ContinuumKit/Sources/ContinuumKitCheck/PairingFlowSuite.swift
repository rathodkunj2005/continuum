//  PairingFlowSuite.swift — happy-path + error-path tests for PairingFlow.

import Foundation
import ContinuumKit

private actor StubTransport: CompanionTransport {
    let status: Int
    let body: Data
    init(status: Int, body: Data) { self.status = status; self.body = body }

    func send(request: URLRequest) async throws -> (Data, URLResponse) {
        let resp = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: nil)!
        return (body, resp)
    }
}

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

let pairingFlowSuite = TestSuite("PairingFlow", [
    TestCase("parseQRPayload decodes a well-formed payload") {
        let payload = try PairingFlow.parseQRPayload(validPayloadJSON)
        try expectEqual(payload.pairingCode, "381729")
        try expectEqual(payload.port, 47812)
    },

    TestCase("parseQRPayload rejects non-JSON input") {
        do {
            _ = try PairingFlow.parseQRPayload("not json")
            try expect(false, "expected throw")
        } catch {
            // ok
        }
    },

    TestCase("accept on valid payload moves to .ready") {
        let payload = try PairingFlow.parseQRPayload(validPayloadJSON)
        let flow = PairingFlow(keychain: InMemoryKeychainStore(), now: { 1 })
        let state = await flow.accept(payload: payload)
        try expectEqual(state, .ready(payload))
    },

    TestCase("accept on expired payload moves to .failed with 'expired'") {
        let json = validPayloadJSON.replacingOccurrences(of: "9999999999999", with: "1")
        let payload = try PairingFlow.parseQRPayload(json)
        let flow = PairingFlow(keychain: InMemoryKeychainStore(), now: { 2 })
        let state = await flow.accept(payload: payload)
        guard case .failed(let msg) = state else {
            try expect(false, "expected .failed, got \(state)")
            return
        }
        try expect(msg.lowercased().contains("expired"))
    },

    TestCase("accept rejects non-numeric pairing code") {
        let bad = validPayloadJSON.replacingOccurrences(of: "\"381729\"", with: "\"abc123\"")
        let payload = try PairingFlow.parseQRPayload(bad)
        let flow = PairingFlow(keychain: InMemoryKeychainStore(), now: { 1 })
        let state = await flow.accept(payload: payload)
        guard case .failed = state else {
            try expect(false, "expected .failed")
            return
        }
    },

    TestCase("complete on 200 persists token + PairedMac into the keychain") {
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

        guard case .paired(let mac) = state else {
            try expect(false, "expected .paired, got \(state)")
            return
        }
        try expectEqual(mac.deviceId, "dev_iphone_abc")
        try expectEqual(mac.permissions.count, 4)
        try expectEqual(try keychain.stringForKey(KeychainKeys.accessToken), "tok-xyz")
        let stored: PairedMac? = try keychain.codableForKey(KeychainKeys.pairedMac, as: PairedMac.self)
        try expectEqual(stored?.deviceId, "dev_iphone_abc")
        try expectEqual(stored?.host, "127.0.0.1")
        try expectEqual(stored?.certFingerprintSha256, "abcdef")
    },

    TestCase("complete on 409 surfaces a 'used' failure and does not store a token") {
        let payload = try PairingFlow.parseQRPayload(validPayloadJSON)
        let errBody = #"{"error":"pairing_code_used","message":"already used"}"#
        let keychain = InMemoryKeychainStore()
        let flow = PairingFlow(
            keychain: keychain,
            transportFactory: { _ in StubTransport(status: 409, body: Data(errBody.utf8)) }
        )
        _ = await flow.accept(payload: payload)
        let state = await flow.complete(deviceName: "iPhone", deviceType: .iphone, appVersion: nil)
        guard case .failed(let msg) = state else {
            try expect(false, "expected .failed, got \(state)")
            return
        }
        try expect(msg.lowercased().contains("used"))
        try expectNil(try keychain.stringForKey(KeychainKeys.accessToken))
    },

    TestCase("complete without accept fails immediately") {
        let flow = PairingFlow(keychain: InMemoryKeychainStore())
        let state = await flow.complete(deviceName: "iPhone", deviceType: .iphone, appVersion: nil)
        guard case .failed = state else {
            try expect(false, "expected .failed")
            return
        }
    },
])
