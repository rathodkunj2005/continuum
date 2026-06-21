//  Errors.swift
//
//  Typed errors emitted by ContinuumKit. CompanionError captures every shape of
//  failure the iOS/Watch apps care about: transport (no route to host),
//  TLS (cert mismatch), HTTP (4xx/5xx with stable error code), decoding.

import Foundation

public enum CompanionError: LocalizedError, Sendable {
    case noEndpoint
    case transport(URLError)
    case tlsFingerprintMismatch(expected: String, actual: String)
    case http(status: Int, code: CompanionErrorCode, message: String)
    case decoding(String)
    case unexpectedResponse(String)
    case pairingCodeInvalid
    case pairingCodeUsed
    case unauthenticated
    case forbidden
    case insufficientPermission

    public var errorDescription: String? {
        switch self {
        case .noEndpoint:
            return "No paired Mac. Tap Pair on the Mac and scan the QR code."
        case .transport(let urlError):
            return "Network: \(urlError.localizedDescription)"
        case .tlsFingerprintMismatch(let expected, let actual):
            return "TLS certificate does not match the paired Mac (expected \(expected.prefix(12))…, got \(actual.prefix(12))…). Re-pair to refresh trust."
        case .http(let status, let code, let message):
            return "HTTP \(status) (\(code.rawValue)): \(message)"
        case .decoding(let detail):
            return "Decoding failed: \(detail)"
        case .unexpectedResponse(let detail):
            return "Unexpected response: \(detail)"
        case .pairingCodeInvalid:
            return "Pairing code is invalid or expired. Generate a fresh code on the Mac."
        case .pairingCodeUsed:
            return "Pairing code has already been used."
        case .unauthenticated:
            return "Missing or malformed Authorization header."
        case .forbidden:
            return "This device has been revoked on the Mac. Re-pair to continue."
        case .insufficientPermission:
            return "This device does not have permission for that action. Re-pair from the Mac settings."
        }
    }
}
