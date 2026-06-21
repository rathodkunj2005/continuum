//  DTOs.swift
//
//  Swift mirrors of the Rust types in src-tauri/src/companion/dto.rs.
//  Keep field names and casing identical to the JSON wire format — these
//  decode straight from the responses the Mac emits.

import Foundation

// MARK: - Device type

public enum DeviceType: String, Codable, Sendable {
    case iphone
    case watch
    case other

    /// What the Mac will tag manual memories from this device with.
    /// Mirrors `DeviceType::manual_capture_source` in the Rust crate.
    public var manualCaptureSource: String {
        switch self {
        case .iphone: return "iphone_manual_capture"
        case .watch:  return "watch_manual_capture"
        case .other:  return "mobile_manual_capture"
        }
    }
}

// MARK: - Pairing

/// JSON payload the Mac encodes into the pairing QR code. iOS decodes this
/// after a successful scan to learn the endpoint and code.
public struct QRPayload: Codable, Equatable, Sendable {
    public let version: Int
    public let macName: String
    public let host: String
    public let port: Int
    public let tls: Bool
    public let certFingerprintSha256: String?
    public let pairingCode: String
    public let expiresAtMs: Int64

    enum CodingKeys: String, CodingKey {
        case version
        case macName = "mac_name"
        case host
        case port
        case tls
        case certFingerprintSha256 = "cert_fingerprint_sha256"
        case pairingCode = "pairing_code"
        case expiresAtMs = "expires_at_ms"
    }
}

public struct PairCompleteRequest: Codable, Sendable {
    public let pairingCode: String
    public let deviceName: String
    public let deviceType: DeviceType
    public let publicKey: String?
    public let appVersion: String?

    enum CodingKeys: String, CodingKey {
        case pairingCode = "pairing_code"
        case deviceName  = "device_name"
        case deviceType  = "device_type"
        case publicKey   = "public_key"
        case appVersion  = "app_version"
    }

    public init(
        pairingCode: String,
        deviceName: String,
        deviceType: DeviceType,
        publicKey: String? = nil,
        appVersion: String? = nil
    ) {
        self.pairingCode = pairingCode
        self.deviceName = deviceName
        self.deviceType = deviceType
        self.publicKey = publicKey
        self.appVersion = appVersion
    }
}

public struct PairCompleteResponse: Codable, Sendable {
    public let deviceId: String
    public let accessToken: String
    public let macName: String
    public let permissions: [String]

    enum CodingKeys: String, CodingKey {
        case deviceId    = "device_id"
        case accessToken = "access_token"
        case macName     = "mac_name"
        case permissions
    }
}

// MARK: - Status

public struct StatusResponse: Codable, Equatable, Sendable {
    public let captureStatus: String
    public let runtimeStatus: String
    public let lastMemoryAtMs: Int64?
    public let storageStatus: String
    public let modelStatus: String
    public let activeProject: String?
    public let macName: String
    public let appVersion: String

    enum CodingKeys: String, CodingKey {
        case captureStatus  = "capture_status"
        case runtimeStatus  = "runtime_status"
        case lastMemoryAtMs = "last_memory_at_ms"
        case storageStatus  = "storage_status"
        case modelStatus    = "model_status"
        case activeProject  = "active_project"
        case macName        = "mac_name"
        case appVersion     = "app_version"
    }

    public init(
        captureStatus: String,
        runtimeStatus: String,
        lastMemoryAtMs: Int64?,
        storageStatus: String,
        modelStatus: String,
        activeProject: String?,
        macName: String,
        appVersion: String
    ) {
        self.captureStatus = captureStatus
        self.runtimeStatus = runtimeStatus
        self.lastMemoryAtMs = lastMemoryAtMs
        self.storageStatus = storageStatus
        self.modelStatus = modelStatus
        self.activeProject = activeProject
        self.macName = macName
        self.appVersion = appVersion
    }
}

// MARK: - Capture control

public enum CaptureAction: String, Codable, Sendable {
    case pause
    case resume
    case incognito
}

public struct CaptureControlRequest: Codable, Sendable {
    public let action: CaptureAction
    public let durationMinutes: Int?
    public let reason: String?

    enum CodingKeys: String, CodingKey {
        case action
        case durationMinutes = "duration_minutes"
        case reason
    }

    public init(action: CaptureAction, durationMinutes: Int? = nil, reason: String? = nil) {
        self.action = action
        self.durationMinutes = durationMinutes
        self.reason = reason
    }
}

public struct CaptureControlResponse: Codable, Sendable {
    public let captureStatus: String
    public let isPaused: Bool
    public let isIncognito: Bool
    public let until: String?

    enum CodingKeys: String, CodingKey {
        case captureStatus = "capture_status"
        case isPaused      = "is_paused"
        case isIncognito   = "is_incognito"
        case until
    }
}

// MARK: - Manual memory

public struct ManualMemoryRequest: Codable, Sendable {
    public let text: String
    public let clientEventId: String
    public let captureType: String?
    public let project: String?
    public let topic: String?
    public let sourceOverride: String?

    enum CodingKeys: String, CodingKey {
        case text
        case clientEventId  = "client_event_id"
        case captureType    = "capture_type"
        case project
        case topic
        case sourceOverride = "source_override"
    }

    public init(
        text: String,
        clientEventId: String,
        captureType: String? = nil,
        project: String? = nil,
        topic: String? = nil,
        sourceOverride: String? = nil
    ) {
        self.text = text
        self.clientEventId = clientEventId
        self.captureType = captureType
        self.project = project
        self.topic = topic
        self.sourceOverride = sourceOverride
    }
}

public struct ManualMemoryResponse: Codable, Sendable {
    public let memoryId: String
    public let status: String
    public let sourceType: String
    public let duplicate: Bool

    enum CodingKeys: String, CodingKey {
        case memoryId   = "memory_id"
        case status
        case sourceType = "source_type"
        case duplicate
    }
}

// MARK: - Ask / Search / Detail

public struct CompanionMemoryCard: Codable, Equatable, Sendable {
    public let memoryId: String
    public let title: String
    public let summary: String
    public let displaySummary: String
    public let internalContext: String
    public let timestamp: Int64
    public let appName: String
    public let windowTitle: String
    public let url: String?
    public let score: Double
    public let sourceCount: Int
    public let confidence: Double
    public let project: String
    public let topic: String?
    public let activityType: String
    public let filesTouched: [String]
    public let rawSnippets: [String]
    public let evidenceIds: [String]

    enum CodingKeys: String, CodingKey {
        case memoryId = "memory_id"
        case title
        case summary
        case displaySummary = "display_summary"
        case internalContext = "internal_context"
        case timestamp
        case appName = "app_name"
        case windowTitle = "window_title"
        case url
        case score
        case sourceCount = "source_count"
        case confidence
        case project
        case topic
        case activityType = "activity_type"
        case filesTouched = "files_touched"
        case rawSnippets = "raw_snippets"
        case evidenceIds = "evidence_ids"
    }
}

public struct AskRequest: Codable, Sendable {
    public let query: String
    public let limit: Int?
    public let answerStyle: String?

    enum CodingKeys: String, CodingKey {
        case query
        case limit
        case answerStyle = "answer_style"
    }

    public init(query: String, limit: Int? = nil, answerStyle: String? = nil) {
        self.query = query
        self.limit = limit
        self.answerStyle = answerStyle
    }
}

public struct AskResponse: Codable, Sendable {
    public let query: String
    public let answer: String
    public let verifyOutcome: String
    public let sourceCards: [CompanionMemoryCard]
    public let latencyMs: Int64

    enum CodingKeys: String, CodingKey {
        case query
        case answer
        case verifyOutcome = "verify_outcome"
        case sourceCards = "source_cards"
        case latencyMs = "latency_ms"
    }
}

public struct MemorySearchRequest: Codable, Sendable {
    public let query: String
    public let limit: Int?
    public let timeFilter: String?
    public let appFilter: String?
    public let projectFilter: String?

    enum CodingKeys: String, CodingKey {
        case query
        case limit
        case timeFilter = "time_filter"
        case appFilter = "app_filter"
        case projectFilter = "project_filter"
    }

    public init(
        query: String,
        limit: Int? = nil,
        timeFilter: String? = nil,
        appFilter: String? = nil,
        projectFilter: String? = nil
    ) {
        self.query = query
        self.limit = limit
        self.timeFilter = timeFilter
        self.appFilter = appFilter
        self.projectFilter = projectFilter
    }
}

public struct MemorySearchResponse: Codable, Sendable {
    public let query: String
    public let cards: [CompanionMemoryCard]
    public let total: Int
    public let latencyMs: Int64

    enum CodingKeys: String, CodingKey {
        case query
        case cards
        case total
        case latencyMs = "latency_ms"
    }
}

public struct MemoryDetailResponse: Codable, Sendable {
    public let card: CompanionMemoryCard
}

// MARK: - Feedback

public struct FeedbackRequest: Codable, Sendable {
    public let event: String
    public let query: String?
    public let memoryId: String?
    public let note: String?

    enum CodingKeys: String, CodingKey {
        case event
        case query
        case memoryId = "memory_id"
        case note
    }

    public init(event: String, query: String? = nil, memoryId: String? = nil, note: String? = nil) {
        self.event = event
        self.query = query
        self.memoryId = memoryId
        self.note = note
    }
}

public struct FeedbackResponse: Codable, Sendable {
    public let status: String
}

// MARK: - Error envelope (mirrors src-tauri/src/companion/errors.rs)

public struct CompanionErrorBody: Codable, Equatable, Sendable {
    public let error: String
    public let message: String
}

public enum CompanionErrorCode: String, Sendable {
    case unauthenticated
    case forbidden
    case insufficientPermission = "insufficient_permission"
    case pairingCodeInvalid = "pairing_code_invalid"
    case pairingCodeUsed    = "pairing_code_used"
    case badRequest         = "bad_request"
    case notFound           = "not_found"
    case `internal`
    case unknown

    public init(raw: String) {
        self = CompanionErrorCode(rawValue: raw) ?? .unknown
    }
}
