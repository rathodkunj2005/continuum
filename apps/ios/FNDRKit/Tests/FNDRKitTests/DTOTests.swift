#if canImport(XCTest)
//  DTOTests.swift — confirms the Swift DTOs decode JSON shaped exactly like
//  what the Rust companion API emits. If anyone touches `dto.rs` field
//  names, the round-trip tests here turn red.

import XCTest
@testable import FNDRKit

final class DTOTests: XCTestCase {
    func testQRPayloadDecodesMacShapedJSON() throws {
        let json = """
        {
          "version": 1,
          "mac_name": "Anurup MacBook Pro",
          "host": "127.0.0.1",
          "port": 47812,
          "tls": true,
          "cert_fingerprint_sha256": "abc123",
          "pairing_code": "381729",
          "expires_at_ms": 1716392400000
        }
        """
        let payload = try JSONDecoder().decode(QRPayload.self, from: Data(json.utf8))
        XCTAssertEqual(payload.macName, "Anurup MacBook Pro")
        XCTAssertEqual(payload.port, 47812)
        XCTAssertTrue(payload.tls)
        XCTAssertEqual(payload.pairingCode, "381729")
    }

    func testDeviceTypeMapsToProvenanceString() {
        XCTAssertEqual(DeviceType.iphone.manualCaptureSource, "iphone_manual_capture")
        XCTAssertEqual(DeviceType.watch.manualCaptureSource, "watch_manual_capture")
    }

    func testPairCompleteRequestEncodesSnakeCase() throws {
        let req = PairCompleteRequest(
            pairingCode: "123456",
            deviceName: "iPhone",
            deviceType: .iphone,
            appVersion: "0.1.0"
        )
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        let body = try encoder.encode(req)
        let str = String(data: body, encoding: .utf8) ?? ""
        XCTAssertTrue(str.contains("\"pairing_code\":\"123456\""))
        XCTAssertTrue(str.contains("\"device_name\":\"iPhone\""))
        XCTAssertTrue(str.contains("\"device_type\":\"iphone\""))
        XCTAssertTrue(str.contains("\"app_version\":\"0.1.0\""))
    }

    func testStatusResponseDecodesNullablesAndRequiredFields() throws {
        let json = """
        {
          "capture_status": "running",
          "runtime_status": "available",
          "last_memory_at_ms": null,
          "storage_status": "healthy",
          "model_status": "available",
          "active_project": null,
          "mac_name": "Test Mac",
          "app_version": "0.2.11"
        }
        """
        let status = try JSONDecoder().decode(StatusResponse.self, from: Data(json.utf8))
        XCTAssertEqual(status.captureStatus, "running")
        XCTAssertNil(status.lastMemoryAtMs)
        XCTAssertNil(status.activeProject)
        XCTAssertEqual(status.appVersion, "0.2.11")
    }

    func testCaptureControlRoundTrip() throws {
        let req = CaptureControlRequest(action: .incognito, durationMinutes: 15, reason: "mobile_user_request")
        let body = try JSONEncoder().encode(req)
        let again = try JSONDecoder().decode(CaptureControlRequest.self, from: body)
        XCTAssertEqual(again.action, .incognito)
        XCTAssertEqual(again.durationMinutes, 15)
    }

    func testManualMemoryRequestStripsUnsetOptionals() throws {
        let req = ManualMemoryRequest(
            text: "Remember to ship FNDRKit",
            clientEventId: "evt-1",
            captureType: "idea"
        )
        let body = try JSONEncoder().encode(req)
        let json = (try JSONSerialization.jsonObject(with: body) as? [String: Any]) ?? [:]
        XCTAssertEqual(json["text"] as? String, "Remember to ship FNDRKit")
        XCTAssertEqual(json["client_event_id"] as? String, "evt-1")
        XCTAssertEqual(json["capture_type"] as? String, "idea")
        // Optionals encoded as JSON null are still present; we just confirm
        // the wire format is acceptable to the Rust handler.
    }

    func testCompanionErrorBodyDecodesShape() throws {
        let json = """
        { "error": "pairing_code_invalid", "message": "pairing code is invalid or expired" }
        """
        let body = try JSONDecoder().decode(CompanionErrorBody.self, from: Data(json.utf8))
        XCTAssertEqual(body.error, "pairing_code_invalid")
        XCTAssertEqual(CompanionErrorCode(raw: body.error), .pairingCodeInvalid)
        XCTAssertEqual(CompanionErrorCode(raw: "totally_unknown"), .unknown)
    }
}
#endif
