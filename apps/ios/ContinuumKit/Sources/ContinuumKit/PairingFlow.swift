//  PairingFlow.swift
//
//  Driver for the iPhone-side pairing handshake. Wraps the work of:
//   1. Parsing a scanned QR payload (or a hand-entered short code + host).
//   2. POSTing /v1/pair/complete via a fresh CompanionClient configured with
//      the QR-encoded fingerprint.
//   3. Persisting the resulting access token + PairedMac into the keychain.
//
//  The flow exposes a `state` enum the UI can switch on. All transitions
//  happen inside `complete(...)` — there are no hidden async-side-effects.

import Foundation

public enum PairingState: Equatable, Sendable {
    case idle
    case ready(QRPayload)
    case pairing(QRPayload)
    case paired(PairedMac)
    case failed(message: String)
}

public actor PairingFlow {
    private let keychain: KeychainStorage
    private let now: @Sendable () -> Int64
    private let transportFactory: @Sendable (QRPayload) -> CompanionTransport
    private let clientFactory: @Sendable (CompanionClient.Configuration, CompanionTransport) -> CompanionClient

    public private(set) var state: PairingState = .idle

    public init(
        keychain: KeychainStorage,
        now: @escaping @Sendable () -> Int64 = { Int64(Date().timeIntervalSince1970 * 1000) },
        transportFactory: @escaping @Sendable (QRPayload) -> CompanionTransport = { payload in
            URLSessionTransport(pinnedFingerprint: payload.certFingerprintSha256)
        },
        clientFactory: @escaping @Sendable (CompanionClient.Configuration, CompanionTransport) -> CompanionClient = {
            CompanionClient(config: $0, transport: $1)
        }
    ) {
        self.keychain = keychain
        self.now = now
        self.transportFactory = transportFactory
        self.clientFactory = clientFactory
    }

    /// Accept a QR scan and validate it. UI flips into a confirmation
    /// screen ("Pair with <macName>?") before calling `complete`.
    public func accept(payload: QRPayload) -> PairingState {
        let expired = payload.expiresAtMs <= now()
        if expired {
            state = .failed(message: "Pairing code expired — generate a new one on the Mac.")
        } else if payload.version != 1 {
            state = .failed(message: "Unsupported QR payload version (\(payload.version)).")
        } else if payload.pairingCode.count != 6
            || !payload.pairingCode.allSatisfy({ $0.isNumber }) {
            state = .failed(message: "Pairing code must be six digits.")
        } else {
            state = .ready(payload)
        }
        return state
    }

    /// Decode a JSON string that came from `AVCaptureMetadataOutput`.
    public static func parseQRPayload(_ raw: String) throws -> QRPayload {
        guard let data = raw.data(using: .utf8) else {
            throw CompanionError.decoding("QR payload is not valid UTF-8")
        }
        do {
            return try JSONDecoder().decode(QRPayload.self, from: data)
        } catch {
            throw CompanionError.decoding("QR payload JSON did not match schema: \(error)")
        }
    }

    /// Run the handshake. Idempotent on success — calling twice with the
    /// same code returns the second response or surfaces `pairingCodeUsed`.
    @discardableResult
    public func complete(deviceName: String, deviceType: DeviceType, appVersion: String?) async -> PairingState {
        guard case .ready(let payload) = state else {
            state = .failed(message: "No pairing payload to complete.")
            return state
        }
        state = .pairing(payload)

        let baseURL = baseURL(for: payload)
        let transport = transportFactory(payload)
        let client = clientFactory(.init(baseURL: baseURL, accessToken: nil), transport)
        let request = PairCompleteRequest(
            pairingCode: payload.pairingCode,
            deviceName: deviceName,
            deviceType: deviceType,
            appVersion: appVersion
        )

        do {
            let response = try await client.completePairing(request: request)
            let paired = PairedMac(
                deviceId: response.deviceId,
                macName: response.macName,
                host: payload.host,
                port: payload.port,
                tls: payload.tls,
                certFingerprintSha256: payload.certFingerprintSha256,
                permissions: response.permissions,
                pairedAtMs: now()
            )

            do {
                try keychain.setString(response.accessToken, forKey: KeychainKeys.accessToken)
                try keychain.setCodable(paired, forKey: KeychainKeys.pairedMac)
            } catch {
                state = .failed(message: "Pairing succeeded but token storage failed: \(error.localizedDescription)")
                return state
            }

            state = .paired(paired)
            return state
        } catch CompanionError.pairingCodeInvalid {
            state = .failed(message: "Pairing code is invalid or expired.")
            return state
        } catch CompanionError.pairingCodeUsed {
            state = .failed(message: "Pairing code was already used — generate a new one on the Mac.")
            return state
        } catch let CompanionError.tlsFingerprintMismatch(expected, _) {
            state = .failed(message: "TLS fingerprint mismatch against the paired Mac (\(expected.prefix(12))…). Re-pair.")
            return state
        } catch {
            state = .failed(message: error.localizedDescription)
            return state
        }
    }

    /// Test helper.
    public func reset() {
        state = .idle
    }

    private func baseURL(for payload: QRPayload) -> URL {
        let scheme = payload.tls ? "https" : "http"
        return URL(string: "\(scheme)://\(payload.host):\(payload.port)")!
    }
}
