//  OfflineCaptureQueue.swift
//
//  Durable queue for manual iPhone/Watch captures when the Mac is unavailable.
//  Stores a compact JSON payload on disk and retries with idempotent
//  `client_event_id` values.

import Foundation

public struct QueuedManualCapture: Codable, Equatable, Sendable {
    public let id: String
    public let text: String
    public let clientEventId: String
    public let captureType: String?
    public let project: String?
    public let topic: String?
    public let sourceOverride: String?
    public let createdAtMs: Int64
    public var attempts: Int

    public init(
        id: String,
        text: String,
        clientEventId: String,
        captureType: String?,
        project: String?,
        topic: String?,
        sourceOverride: String?,
        createdAtMs: Int64,
        attempts: Int = 0
    ) {
        self.id = id
        self.text = text
        self.clientEventId = clientEventId
        self.captureType = captureType
        self.project = project
        self.topic = topic
        self.sourceOverride = sourceOverride
        self.createdAtMs = createdAtMs
        self.attempts = attempts
    }
}

public struct QueueFlushResult: Equatable, Sendable {
    public let attempted: Int
    public let succeeded: Int
    public let failed: Int
    public let remaining: Int
}

public actor OfflineCaptureQueue {
    private var items: [QueuedManualCapture]
    private let storageURL: URL
    private let now: @Sendable () -> Int64

    public init(
        storageURL: URL? = nil,
        now: @escaping @Sendable () -> Int64 = { Int64(Date().timeIntervalSince1970 * 1000) }
    ) {
        self.storageURL = storageURL ?? Self.defaultStorageURL()
        self.now = now
        self.items = Self.loadItems(from: self.storageURL)
    }

    public func pending() -> [QueuedManualCapture] {
        items
    }

    @discardableResult
    public func enqueue(
        text: String,
        clientEventId: String,
        captureType: String? = nil,
        project: String? = nil,
        topic: String? = nil,
        sourceOverride: String? = nil
    ) throws -> QueuedManualCapture {
        let trimmedText = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedEvent = clientEventId.trimmingCharacters(in: .whitespacesAndNewlines)

        guard !trimmedText.isEmpty else {
            throw CompanionError.http(status: 400, code: .badRequest, message: "queue text is empty")
        }
        guard !trimmedEvent.isEmpty else {
            throw CompanionError.http(status: 400, code: .badRequest, message: "client_event_id is empty")
        }

        if let existing = items.first(where: { $0.clientEventId == trimmedEvent }) {
            return existing
        }

        let queued = QueuedManualCapture(
            id: UUID().uuidString,
            text: trimmedText,
            clientEventId: trimmedEvent,
            captureType: captureType,
            project: project,
            topic: topic,
            sourceOverride: sourceOverride,
            createdAtMs: now(),
            attempts: 0
        )
        items.append(queued)
        try persist()
        return queued
    }

    public func removeAll() throws {
        items.removeAll()
        try persist()
    }

    public func flush(using client: CompanionClient, maxItems: Int = 25) async -> QueueFlushResult {
        guard !items.isEmpty else {
            return QueueFlushResult(attempted: 0, succeeded: 0, failed: 0, remaining: 0)
        }

        let limit = max(1, maxItems)
        var attempted = 0
        var succeeded = 0
        var failed = 0

        let candidates = Array(items.prefix(limit))
        for queued in candidates {
            attempted += 1
            do {
                _ = try await client.createManualMemory(
                    request: ManualMemoryRequest(
                        text: queued.text,
                        clientEventId: queued.clientEventId,
                        captureType: queued.captureType,
                        project: queued.project,
                        topic: queued.topic,
                        sourceOverride: queued.sourceOverride
                    )
                )
                items.removeAll { $0.id == queued.id }
                succeeded += 1
            } catch {
                failed += 1
                if let index = items.firstIndex(where: { $0.id == queued.id }) {
                    items[index].attempts += 1
                }
            }
        }

        do {
            try persist()
        } catch {
            // Queue stays in memory even if disk flush fails.
        }

        return QueueFlushResult(
            attempted: attempted,
            succeeded: succeeded,
            failed: failed,
            remaining: items.count
        )
    }

    public static func defaultStorageURL() -> URL {
        let base = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
        return base.appendingPathComponent("continuum_offline_capture_queue.json")
    }

    private func persist() throws {
        let data = try JSONEncoder().encode(items)
        let dir = storageURL.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try data.write(to: storageURL, options: .atomic)
    }

    private static func loadItems(from url: URL) -> [QueuedManualCapture] {
        guard let data = try? Data(contentsOf: url) else { return [] }
        return (try? JSONDecoder().decode([QueuedManualCapture].self, from: data)) ?? []
    }
}
