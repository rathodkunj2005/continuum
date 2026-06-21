//  WatchBridge.swift
//
//  Shared message schema and thin service wrappers for iPhone <-> Watch
//  communication. The watch target calls into this envelope through
//  WatchConnectivity on iPhone.

import Foundation

public enum WatchBridgeRoute: String, Codable, Sendable {
    case ask
    case remember
    case status
    case captureControl = "capture_control"
    case recent
}

public struct WatchBridgeRequest: Codable, Equatable, Sendable {
    public let route: WatchBridgeRoute
    public let payload: [String: String]
    public let sentAtMs: Int64

    enum CodingKeys: String, CodingKey {
        case route
        case payload
        case sentAtMs = "sent_at_ms"
    }

    public init(route: WatchBridgeRoute, payload: [String: String], sentAtMs: Int64) {
        self.route = route
        self.payload = payload
        self.sentAtMs = sentAtMs
    }
}

public struct WatchBridgeResponse: Codable, Equatable, Sendable {
    public let ok: Bool
    public let message: String?
    public let payload: [String: String]

    public init(ok: Bool, message: String? = nil, payload: [String: String] = [:]) {
        self.ok = ok
        self.message = message
        self.payload = payload
    }
}

public enum WatchBridgeWireError: Error, LocalizedError, Sendable {
    case invalidPayload(String)

    public var errorDescription: String? {
        switch self {
        case .invalidPayload(let message):
            return "Watch bridge payload is invalid: \(message)"
        }
    }
}

extension WatchBridgeRequest {
    public func toDictionary() throws -> [String: Any] {
        let data = try JSONEncoder().encode(self)
        let object = try JSONSerialization.jsonObject(with: data)
        guard let dictionary = object as? [String: Any] else {
            throw WatchBridgeWireError.invalidPayload("request did not encode as dictionary")
        }
        return dictionary
    }

    public static func fromDictionary(_ dictionary: [String: Any]) throws -> WatchBridgeRequest {
        let data = try JSONSerialization.data(withJSONObject: dictionary)
        do {
            return try JSONDecoder().decode(WatchBridgeRequest.self, from: data)
        } catch {
            throw WatchBridgeWireError.invalidPayload("request decode failed: \(error.localizedDescription)")
        }
    }
}

extension WatchBridgeResponse {
    public func toDictionary() throws -> [String: Any] {
        let data = try JSONEncoder().encode(self)
        let object = try JSONSerialization.jsonObject(with: data)
        guard let dictionary = object as? [String: Any] else {
            throw WatchBridgeWireError.invalidPayload("response did not encode as dictionary")
        }
        return dictionary
    }

    public static func fromDictionary(_ dictionary: [String: Any]) throws -> WatchBridgeResponse {
        let data = try JSONSerialization.data(withJSONObject: dictionary)
        do {
            return try JSONDecoder().decode(WatchBridgeResponse.self, from: data)
        } catch {
            throw WatchBridgeWireError.invalidPayload("response decode failed: \(error.localizedDescription)")
        }
    }
}

public protocol WatchBridgeTransport: Sendable {
    func send(request: WatchBridgeRequest) async throws -> WatchBridgeResponse
}

public struct ClosureWatchBridgeTransport: WatchBridgeTransport {
    private let block: @Sendable (WatchBridgeRequest) async throws -> WatchBridgeResponse

    public init(_ block: @escaping @Sendable (WatchBridgeRequest) async throws -> WatchBridgeResponse) {
        self.block = block
    }

    public func send(request: WatchBridgeRequest) async throws -> WatchBridgeResponse {
        try await block(request)
    }
}

public actor WatchBridgeService {
    private let transport: WatchBridgeTransport
    private let now: @Sendable () -> Int64

    public init(
        transport: WatchBridgeTransport,
        now: @escaping @Sendable () -> Int64 = { Int64(Date().timeIntervalSince1970 * 1000) }
    ) {
        self.transport = transport
        self.now = now
    }

    public func ask(_ query: String) async throws -> WatchBridgeResponse {
        try await transport.send(
            request: WatchBridgeRequest(route: .ask, payload: ["query": query], sentAtMs: now())
        )
    }

    public func remember(_ text: String) async throws -> WatchBridgeResponse {
        try await transport.send(
            request: WatchBridgeRequest(route: .remember, payload: ["text": text], sentAtMs: now())
        )
    }

    public func status() async throws -> WatchBridgeResponse {
        try await transport.send(
            request: WatchBridgeRequest(route: .status, payload: [:], sentAtMs: now())
        )
    }

    public func captureControl(_ action: CaptureAction) async throws -> WatchBridgeResponse {
        try await transport.send(
            request: WatchBridgeRequest(
                route: .captureControl,
                payload: ["action": action.rawValue],
                sentAtMs: now()
            )
        )
    }

    public func recent(limit: Int = 5) async throws -> WatchBridgeResponse {
        try await transport.send(
            request: WatchBridgeRequest(
                route: .recent,
                payload: ["limit": String(max(1, limit))],
                sentAtMs: now()
            )
        )
    }
}
