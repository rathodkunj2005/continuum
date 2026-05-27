import Foundation
import FNDRKit

private actor QueueStubTransport: CompanionTransport {
    enum Reply {
        case success
        case failure(URLError)
    }

    private var replies: [Reply]

    init(replies: [Reply]) {
        self.replies = replies
    }

    func send(request: URLRequest) async throws -> (Data, URLResponse) {
        let next = replies.isEmpty ? .failure(URLError(.unknown)) : replies.removeFirst()
        switch next {
        case .success:
            let body = #"{"memory_id":"mem_1","status":"indexed","source_type":"iphone_manual_capture","duplicate":false}"#
            let resp = HTTPURLResponse(url: request.url!, statusCode: 200, httpVersion: nil, headerFields: nil)!
            return (Data(body.utf8), resp)
        case .failure(let err):
            throw err
        }
    }
}

private func tempQueueURL(_ name: String) -> URL {
    FileManager.default.temporaryDirectory
        .appendingPathComponent("fndrkitcheck")
        .appendingPathComponent(name)
}

let offlineQueueSuite = TestSuite("OfflineCaptureQueue", [
    TestCase("enqueue persists and is durable across re-init") {
        let url = tempQueueURL(UUID().uuidString)
        try? FileManager.default.removeItem(at: url)

        let queue1 = OfflineCaptureQueue(storageURL: url, now: { 1_000 })
        _ = try await queue1.enqueue(text: "Remember this", clientEventId: "evt_1", captureType: "idea")
        let pendingCount1 = await queue1.pending().count
        try expectEqual(pendingCount1, 1)

        let queue2 = OfflineCaptureQueue(storageURL: url, now: { 2_000 })
        let loaded = await queue2.pending()
        try expectEqual(loaded.count, 1)
        try expectEqual(loaded.first?.clientEventId, "evt_1")
    },

    TestCase("flush removes successful captures") {
        let url = tempQueueURL(UUID().uuidString)
        try? FileManager.default.removeItem(at: url)

        let queue = OfflineCaptureQueue(storageURL: url)
        _ = try await queue.enqueue(text: "one", clientEventId: "evt_1")
        _ = try await queue.enqueue(text: "two", clientEventId: "evt_2")

        let transport = QueueStubTransport(replies: [.success, .success])
        let client = CompanionClient(
            config: .init(baseURL: URL(string: "https://127.0.0.1:47812")!, accessToken: "tok"),
            transport: transport
        )

        let result = await queue.flush(using: client, maxItems: 10)
        try expectEqual(result.succeeded, 2)
        try expectEqual(result.remaining, 0)
        let pendingCount = await queue.pending().count
        try expectEqual(pendingCount, 0)
    },

    TestCase("flush keeps failures and increments attempts") {
        let url = tempQueueURL(UUID().uuidString)
        try? FileManager.default.removeItem(at: url)

        let queue = OfflineCaptureQueue(storageURL: url)
        _ = try await queue.enqueue(text: "one", clientEventId: "evt_1")

        let transport = QueueStubTransport(replies: [.failure(URLError(.notConnectedToInternet))])
        let client = CompanionClient(
            config: .init(baseURL: URL(string: "https://127.0.0.1:47812")!, accessToken: "tok"),
            transport: transport
        )

        let result = await queue.flush(using: client, maxItems: 10)
        try expectEqual(result.failed, 1)
        try expectEqual(result.remaining, 1)

        let pending = await queue.pending()
        try expectEqual(pending.first?.attempts, 1)
    },
])
