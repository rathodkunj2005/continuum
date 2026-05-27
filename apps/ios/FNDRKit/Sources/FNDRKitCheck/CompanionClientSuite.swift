//  CompanionClientSuite.swift — request shaping + error mapping for the
//  HTTP client. Uses a scripted in-memory transport so no real network.

import Foundation
import FNDRKit

private actor StubTransport: CompanionTransport {
    struct Stub {
        let status: Int
        let body: Data
    }

    private(set) var lastRequest: URLRequest?
    private(set) var lastBodyString: String?
    private var responses: [Stub]

    init(responses: [Stub]) { self.responses = responses }

    func send(request: URLRequest) async throws -> (Data, URLResponse) {
        lastRequest = request
        lastBodyString = request.httpBody.flatMap { String(data: $0, encoding: .utf8) }
        guard !responses.isEmpty else { throw URLError(.unknown) }
        let next = responses.removeFirst()
        let url = request.url ?? URL(string: "https://example.invalid/x")!
        let resp = HTTPURLResponse(url: url, statusCode: next.status, httpVersion: "HTTP/1.1", headerFields: nil)!
        return (next.body, resp)
    }
}

private func makeClient(
    token: String? = "tok-abc",
    responses: [StubTransport.Stub]
) -> (CompanionClient, StubTransport) {
    let transport = StubTransport(responses: responses)
    let base = URL(string: "https://127.0.0.1:47812")!
    let client = CompanionClient(config: .init(baseURL: base, accessToken: token), transport: transport)
    return (client, transport)
}

let companionClientSuite = TestSuite("CompanionClient", [
    TestCase("status() decodes 200 + sends Bearer header") {
        let body = #"""
        {"capture_status":"running","runtime_status":"available","last_memory_at_ms":1234,"storage_status":"healthy","model_status":"available","active_project":null,"mac_name":"Mac","app_version":"0.2.11"}
        """#
        let (client, transport) = makeClient(responses: [.init(status: 200, body: Data(body.utf8))])
        let status = try await client.status()
        try expectEqual(status.captureStatus, "running")
        try expectEqual(status.lastMemoryAtMs, 1234)

        let lastReq = await transport.lastRequest
        try expectEqual(lastReq?.value(forHTTPHeaderField: "Authorization"), "Bearer tok-abc")
        try expectEqual(lastReq?.url?.path, "/v1/status")
    },

    TestCase("status() with no token throws .unauthenticated before reaching the transport") {
        let (client, _) = makeClient(token: nil, responses: [])
        do {
            _ = try await client.status()
            try expect(false, "expected throw")
        } catch CompanionError.unauthenticated {
            // ok
        } catch {
            try expect(false, "wrong error: \(error)")
        }
    },

    TestCase("completePairing maps 409 → .pairingCodeUsed") {
        let errorBody = #"{"error":"pairing_code_used","message":"already used"}"#
        let (client, _) = makeClient(token: nil, responses: [.init(status: 409, body: Data(errorBody.utf8))])
        let req = PairCompleteRequest(pairingCode: "111111", deviceName: "iPhone", deviceType: .iphone)
        do {
            _ = try await client.completePairing(request: req)
            try expect(false, "expected throw")
        } catch CompanionError.pairingCodeUsed {
            // ok
        } catch {
            try expect(false, "wrong error: \(error)")
        }
    },

    TestCase("completePairing maps 400 + pairing_code_invalid → .pairingCodeInvalid") {
        let errorBody = #"{"error":"pairing_code_invalid","message":"expired"}"#
        let (client, _) = makeClient(token: nil, responses: [.init(status: 400, body: Data(errorBody.utf8))])
        let req = PairCompleteRequest(pairingCode: "000000", deviceName: "iPhone", deviceType: .iphone)
        do {
            _ = try await client.completePairing(request: req)
            try expect(false, "expected throw")
        } catch CompanionError.pairingCodeInvalid {
            // ok
        } catch {
            try expect(false, "wrong error: \(error)")
        }
    },

    TestCase("403 maps to .forbidden regardless of body") {
        let (client, _) = makeClient(responses: [.init(status: 403, body: Data(#"{"error":"forbidden","message":"go away"}"#.utf8))])
        do {
            _ = try await client.status()
            try expect(false, "expected throw")
        } catch CompanionError.forbidden {
            // ok
        } catch {
            try expect(false, "wrong error: \(error)")
        }
    },

    TestCase("403 + insufficient_permission maps to .insufficientPermission") {
        let errorBody = #"{"error":"insufficient_permission","message":"missing permission"}"#
        let (client, _) = makeClient(responses: [.init(status: 403, body: Data(errorBody.utf8))])
        do {
            _ = try await client.status()
            try expect(false, "expected throw")
        } catch CompanionError.insufficientPermission {
            // ok
        } catch {
            try expect(false, "wrong error: \(error)")
        }
    },

    TestCase("createManualMemory posts body + decodes response") {
        let resp = #"{"memory_id":"mem-1","status":"indexed","source_type":"iphone_manual_capture","duplicate":false}"#
        let (client, transport) = makeClient(responses: [.init(status: 200, body: Data(resp.utf8))])
        let req = ManualMemoryRequest(text: "Hello FNDR", clientEventId: "evt-1", captureType: "idea")
        let result = try await client.createManualMemory(request: req)
        try expectEqual(result.memoryId, "mem-1")
        try expectEqual(result.sourceType, "iphone_manual_capture")

        let sent = await transport.lastBodyString ?? ""
        try expect(sent.contains("\"client_event_id\":\"evt-1\""))
        try expect(sent.contains("\"text\":\"Hello FNDR\""))
    },

    TestCase("Unknown-shaped 500 body falls back to .http") {
        let (client, _) = makeClient(responses: [.init(status: 500, body: Data("not even json".utf8))])
        do {
            _ = try await client.status()
            try expect(false, "expected throw")
        } catch CompanionError.http(let status, let code, _) {
            try expectEqual(status, 500)
            try expectEqual(code, .unknown)
        } catch {
            try expect(false, "wrong error: \(error)")
        }
    },

    TestCase("ask() posts query and decodes source cards") {
        let body = #"""
        {
          "query":"what was I working on",
          "answer":"You were shipping FNDRKit.",
          "verify_outcome":"grounded",
          "source_cards":[
            {
              "memory_id":"mem_1",
              "title":"FNDRKit",
              "summary":"Worked on FNDRKit.",
              "display_summary":"Worked on FNDRKit.",
              "internal_context":"ctx",
              "timestamp":1700000000000,
              "app_name":"Xcode",
              "window_title":"CompanionClient.swift",
              "url":null,
              "score":0.9,
              "source_count":2,
              "confidence":0.8,
              "project":"FNDR",
              "activity_type":"coding",
              "files_touched":["CompanionClient.swift"],
              "raw_snippets":["snippet"],
              "evidence_ids":["mem_1"]
            }
          ],
          "latency_ms":42
        }
        """#
        let (client, transport) = makeClient(responses: [.init(status: 200, body: Data(body.utf8))])
        let response = try await client.ask(request: AskRequest(query: "what was I working on", limit: 5, answerStyle: "short"))
        try expectEqual(response.answer, "You were shipping FNDRKit.")
        try expectEqual(response.sourceCards.first?.memoryId, "mem_1")

        let reqBody = await transport.lastBodyString ?? ""
        try expect(reqBody.contains("\"answer_style\":\"short\""))
        try expect(reqBody.contains("\"query\":\"what was I working on\""))
    },

    TestCase("searchMemories() decodes cards and total") {
        let body = #"""
        {
          "query":"fndr",
          "cards":[],
          "total":0,
          "latency_ms":12
        }
        """#
        let (client, _) = makeClient(responses: [.init(status: 200, body: Data(body.utf8))])
        let response = try await client.searchMemories(request: MemorySearchRequest(query: "fndr", limit: 10))
        try expectEqual(response.query, "fndr")
        try expectEqual(response.total, 0)
    },

    TestCase("memoryDetail() uses /v1/memories/:id route") {
        let body = #"""
        {
          "card":{
            "memory_id":"mem_42",
            "title":"Title",
            "summary":"Summary",
            "display_summary":"Summary",
            "internal_context":"ctx",
            "timestamp":1700000000000,
            "app_name":"Terminal",
            "window_title":"zsh",
            "url":null,
            "score":0.5,
            "source_count":1,
            "confidence":0.5,
            "project":"",
            "activity_type":"coding",
            "files_touched":[],
            "raw_snippets":[],
            "evidence_ids":[]
          }
        }
        """#
        let (client, transport) = makeClient(responses: [.init(status: 200, body: Data(body.utf8))])
        let response = try await client.memoryDetail(memoryId: "mem_42")
        try expectEqual(response.card.memoryId, "mem_42")
        let req = await transport.lastRequest
        try expectEqual(req?.url?.path, "/v1/memories/mem_42")
    },

    TestCase("submitFeedback() posts event and decodes ok response") {
        let body = #"{"status":"ok"}"#
        let (client, transport) = makeClient(responses: [.init(status: 200, body: Data(body.utf8))])
        let response = try await client.submitFeedback(request: FeedbackRequest(event: "thumbs_up", query: "hello", memoryId: "mem_1"))
        try expectEqual(response.status, "ok")

        let reqBody = await transport.lastBodyString ?? ""
        try expect(reqBody.contains("\"event\":\"thumbs_up\""))
        try expect(reqBody.contains("\"memory_id\":\"mem_1\""))
    },
])
