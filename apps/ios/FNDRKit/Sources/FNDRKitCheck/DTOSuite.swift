//  DTOSuite.swift — JSON shape parity with src-tauri/src/companion/dto.rs.

import Foundation
import FNDRKit

let dtoSuite = TestSuite("DTO", [
    TestCase("QRPayload decodes mac-shaped JSON") {
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
        try expectEqual(payload.macName, "Anurup MacBook Pro")
        try expectEqual(payload.port, 47812)
        try expect(payload.tls)
        try expectEqual(payload.pairingCode, "381729")
    },

    TestCase("DeviceType maps to provenance string") {
        try expectEqual(DeviceType.iphone.manualCaptureSource, "iphone_manual_capture")
        try expectEqual(DeviceType.watch.manualCaptureSource, "watch_manual_capture")
    },

    TestCase("PairCompleteRequest encodes snake_case keys") {
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
        try expect(str.contains("\"pairing_code\":\"123456\""))
        try expect(str.contains("\"device_name\":\"iPhone\""))
        try expect(str.contains("\"device_type\":\"iphone\""))
        try expect(str.contains("\"app_version\":\"0.1.0\""))
    },

    TestCase("StatusResponse decodes nullables") {
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
        try expectEqual(status.captureStatus, "running")
        try expectNil(status.lastMemoryAtMs)
        try expectNil(status.activeProject)
        try expectEqual(status.appVersion, "0.2.11")
    },

    TestCase("CaptureControl round-trips through JSON") {
        let req = CaptureControlRequest(action: .incognito, durationMinutes: 15, reason: "mobile_user_request")
        let body = try JSONEncoder().encode(req)
        let again = try JSONDecoder().decode(CaptureControlRequest.self, from: body)
        try expectEqual(again.action, .incognito)
        try expectEqual(again.durationMinutes, 15)
    },

    TestCase("ManualMemoryRequest encodes required fields") {
        let req = ManualMemoryRequest(
            text: "Remember to ship FNDRKit",
            clientEventId: "evt-1",
            captureType: "idea"
        )
        let body = try JSONEncoder().encode(req)
        let json = (try JSONSerialization.jsonObject(with: body) as? [String: Any]) ?? [:]
        try expectEqual(json["text"] as? String, "Remember to ship FNDRKit")
        try expectEqual(json["client_event_id"] as? String, "evt-1")
        try expectEqual(json["capture_type"] as? String, "idea")
    },

    TestCase("CompanionErrorBody decodes the wire shape") {
        let json = #"{"error":"pairing_code_invalid","message":"pairing code is invalid or expired"}"#
        let body = try JSONDecoder().decode(CompanionErrorBody.self, from: Data(json.utf8))
        try expectEqual(body.error, "pairing_code_invalid")
        try expectEqual(CompanionErrorCode(raw: body.error), .pairingCodeInvalid)
        try expectEqual(CompanionErrorCode(raw: "totally_unknown"), .unknown)
    },

    TestCase("AskResponse decodes source_cards shape") {
        let json = #"""
        {
          "query":"q",
          "answer":"a",
          "verify_outcome":"grounded",
          "source_cards":[
            {
              "memory_id":"m1",
              "title":"t",
              "summary":"s",
              "display_summary":"s",
              "internal_context":"ctx",
              "timestamp":1,
              "app_name":"Xcode",
              "window_title":"w",
              "url":null,
              "score":0.5,
              "source_count":1,
              "confidence":0.7,
              "project":"FNDR",
              "activity_type":"coding",
              "files_touched":[],
              "raw_snippets":[],
              "evidence_ids":[]
            }
          ],
          "latency_ms":9
        }
        """#
        let decoded = try JSONDecoder().decode(AskResponse.self, from: Data(json.utf8))
        try expectEqual(decoded.sourceCards.first?.memoryId, "m1")
        try expectEqual(decoded.verifyOutcome, "grounded")
    },

    TestCase("FeedbackRequest encodes memory_id key") {
        let req = FeedbackRequest(event: "thumbs_up", memoryId: "mem_1")
        let data = try JSONEncoder().encode(req)
        let json = String(data: data, encoding: .utf8) ?? ""
        try expect(json.contains("\"memory_id\":\"mem_1\""))
    },
])
