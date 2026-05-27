//  KeychainStore.swift
//
//  Thin wrapper around the Security framework for storing the bearer token
//  and paired-Mac metadata. Uses kSecClassGenericPassword keyed by a
//  per-key service string. Tests on macOS hit the same Security framework
//  paths the iOS app will hit.
//
//  Note: a TestableKeychainStore protocol lets unit tests inject an
//  in-memory backend so they don't pollute the developer's keychain.

import Foundation
import Security

/// Storage backend abstraction. Two implementations live here: the real
/// `KeychainStore` (Security framework) and `InMemoryKeychainStore` for tests.
public protocol KeychainStorage: AnyObject {
    func setData(_ data: Data, forKey key: String) throws
    func dataForKey(_ key: String) throws -> Data?
    func deleteKey(_ key: String) throws
}

extension KeychainStorage {
    public func setString(_ value: String, forKey key: String) throws {
        try setData(Data(value.utf8), forKey: key)
    }

    public func stringForKey(_ key: String) throws -> String? {
        guard let data = try dataForKey(key) else { return nil }
        return String(data: data, encoding: .utf8)
    }

    public func setCodable<T: Encodable>(_ value: T, forKey key: String) throws {
        let data = try JSONEncoder().encode(value)
        try setData(data, forKey: key)
    }

    public func codableForKey<T: Decodable>(_ key: String, as _: T.Type = T.self) throws -> T? {
        guard let data = try dataForKey(key) else { return nil }
        return try JSONDecoder().decode(T.self, from: data)
    }
}

// MARK: - Real Security-framework backend

public final class KeychainStore: KeychainStorage {
    public let service: String

    public init(service: String = "com.fndr.companion") {
        self.service = service
    }

    public func setData(_ data: Data, forKey key: String) throws {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]

        let attributes: [String: Any] = [
            kSecValueData as String: data,
        ]

        let status = SecItemUpdate(query as CFDictionary, attributes as CFDictionary)
        switch status {
        case errSecSuccess:
            return
        case errSecItemNotFound:
            // Not present yet — add.
            query[kSecValueData as String] = data
            let addStatus = SecItemAdd(query as CFDictionary, nil)
            guard addStatus == errSecSuccess else {
                throw KeychainError.osStatus(addStatus, op: "add")
            }
        default:
            throw KeychainError.osStatus(status, op: "update")
        }
    }

    public func dataForKey(_ key: String) throws -> Data? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        switch status {
        case errSecSuccess:
            return result as? Data
        case errSecItemNotFound:
            return nil
        default:
            throw KeychainError.osStatus(status, op: "get")
        }
    }

    public func deleteKey(_ key: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: key,
        ]
        let status = SecItemDelete(query as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw KeychainError.osStatus(status, op: "delete")
        }
    }
}

public enum KeychainError: Error, LocalizedError {
    case osStatus(OSStatus, op: String)

    public var errorDescription: String? {
        switch self {
        case .osStatus(let code, let op):
            return "Keychain \(op) failed with OSStatus \(code)"
        }
    }
}

// MARK: - Standard keys + paired-Mac record

public enum KeychainKeys {
    public static let accessToken = "companion.accessToken"
    public static let pairedMac   = "companion.pairedMac"
}

/// What the iOS app persists after a successful pairing handshake.
/// Token lives separately under `KeychainKeys.accessToken` so it can be
/// cleared without touching the rest of the pairing.
public struct PairedMac: Codable, Equatable, Sendable {
    public let deviceId: String
    public let macName: String
    public let host: String
    public let port: Int
    public let tls: Bool
    public let certFingerprintSha256: String?
    public let permissions: [String]
    public let pairedAtMs: Int64

    public init(
        deviceId: String,
        macName: String,
        host: String,
        port: Int,
        tls: Bool,
        certFingerprintSha256: String?,
        permissions: [String],
        pairedAtMs: Int64
    ) {
        self.deviceId = deviceId
        self.macName = macName
        self.host = host
        self.port = port
        self.tls = tls
        self.certFingerprintSha256 = certFingerprintSha256
        self.permissions = permissions
        self.pairedAtMs = pairedAtMs
    }

    public var baseURL: URL {
        let scheme = tls ? "https" : "http"
        return URL(string: "\(scheme)://\(host):\(port)")!
    }
}

// MARK: - In-memory backend (for tests + previews)

public final class InMemoryKeychainStore: KeychainStorage, @unchecked Sendable {
    private var storage: [String: Data] = [:]
    private let lock = NSLock()

    public init() {}

    public func setData(_ data: Data, forKey key: String) throws {
        lock.lock(); defer { lock.unlock() }
        storage[key] = data
    }

    public func dataForKey(_ key: String) throws -> Data? {
        lock.lock(); defer { lock.unlock() }
        return storage[key]
    }

    public func deleteKey(_ key: String) throws {
        lock.lock(); defer { lock.unlock() }
        storage.removeValue(forKey: key)
    }
}
