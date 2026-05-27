//  CompanionClient.swift
//
//  The only thing in FNDRKit that talks to the Mac. Encapsulates:
//   * URL composition from a `PairedMac` + `/v1/...` path
//   * Bearer-token attachment
//   * TLS pinning by sha256 fingerprint of the PEM bytes (matches the
//     Mac's `tls_cert_fingerprint()` in src-tauri/src/companion/mod.rs)
//   * JSON encode/decode of `Codable` request/response bodies
//   * Mapping `CompanionErrorBody` into typed `CompanionError`
//
//  Tests inject a custom `URLProtocol` via a configured `URLSession`, so we
//  exercise the full code path without hitting the network.

import Foundation

#if canImport(CryptoKit)
import CryptoKit
#endif

public protocol CompanionTransport: Sendable {
    func send(request: URLRequest) async throws -> (Data, URLResponse)
}

/// Default transport: a `URLSession` configured with a TLS pinning delegate
/// when the paired Mac has a `cert_fingerprint_sha256`.
public final class URLSessionTransport: NSObject, CompanionTransport, URLSessionDelegate, @unchecked Sendable {
    // IUO so we can install `self` as the URLSession delegate after super.init().
    // Stays effectively immutable — never reassigned outside this initializer.
    public private(set) var session: URLSession!
    private let pinnedFingerprint: String?

    public init(pinnedFingerprint: String?, configuration: URLSessionConfiguration = .ephemeral) {
        self.pinnedFingerprint = pinnedFingerprint?.lowercased()
        super.init()
        let config = configuration
        config.timeoutIntervalForRequest = 10
        config.timeoutIntervalForResource = 15
        config.waitsForConnectivity = false
        self.session = URLSession(configuration: config, delegate: self, delegateQueue: nil)
    }

    public func send(request: URLRequest) async throws -> (Data, URLResponse) {
        do {
            return try await session.data(for: request)
        } catch let urlError as URLError {
            throw CompanionError.transport(urlError)
        }
    }

    public func urlSession(
        _ session: URLSession,
        didReceive challenge: URLAuthenticationChallenge,
        completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void
    ) {
        guard
            challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
            let serverTrust = challenge.protectionSpace.serverTrust
        else {
            completionHandler(.performDefaultHandling, nil)
            return
        }

        // No pin yet (e.g. fingerprint absent in the QR payload) → accept by default.
        guard let expected = pinnedFingerprint else {
            completionHandler(.useCredential, URLCredential(trust: serverTrust))
            return
        }

        guard let actual = Self.computeFingerprint(trust: serverTrust) else {
            completionHandler(.cancelAuthenticationChallenge, nil)
            return
        }

        if actual == expected {
            completionHandler(.useCredential, URLCredential(trust: serverTrust))
        } else {
            completionHandler(.cancelAuthenticationChallenge, nil)
        }
    }

    /// SHA-256 over the leaf certificate's PEM-encoded representation,
    /// matching the Rust side's `tls_cert_fingerprint()` (which hashes the
    /// full PEM file). Returns nil if the cert chain is missing.
    public static func computeFingerprint(trust: SecTrust) -> String? {
        #if canImport(CryptoKit)
        guard SecTrustGetCertificateCount(trust) > 0 else { return nil }
        let chain = SecTrustCopyCertificateChain(trust) as? [SecCertificate]
        guard let leaf = chain?.first else { return nil }
        let der = SecCertificateCopyData(leaf) as Data
        let pem = Self.derToPEM(der)
        let digest = SHA256.hash(data: Data(pem.utf8))
        return digest.map { String(format: "%02x", $0) }.joined()
        #else
        return nil
        #endif
    }

    public static func derToPEM(_ der: Data) -> String {
        let b64 = der.base64EncodedString()
        var lines: [String] = ["-----BEGIN CERTIFICATE-----"]
        var index = b64.startIndex
        while index < b64.endIndex {
            let end = b64.index(index, offsetBy: 64, limitedBy: b64.endIndex) ?? b64.endIndex
            lines.append(String(b64[index..<end]))
            index = end
        }
        lines.append("-----END CERTIFICATE-----")
        // Mac side appends a trailing newline when writing the PEM file.
        return lines.joined(separator: "\n") + "\n"
    }
}

// MARK: - CompanionClient

public actor CompanionClient {
    public struct Configuration: Sendable {
        public let baseURL: URL
        public let accessToken: String?

        public init(baseURL: URL, accessToken: String? = nil) {
            self.baseURL = baseURL
            self.accessToken = accessToken
        }
    }

    private var config: Configuration
    private let transport: CompanionTransport
    private let jsonDecoder: JSONDecoder
    private let jsonEncoder: JSONEncoder

    public init(config: Configuration, transport: CompanionTransport) {
        self.config = config
        self.transport = transport
        self.jsonDecoder = JSONDecoder()
        self.jsonEncoder = JSONEncoder()
    }

    public func updateAccessToken(_ token: String?) {
        config = Configuration(baseURL: config.baseURL, accessToken: token)
    }

    public var baseURL: URL { config.baseURL }
    public var accessToken: String? { config.accessToken }

    // MARK: API methods

    public func health() async throws -> Bool {
        let req = try makeRequest(method: "GET", path: "/v1/health", body: Optional<EmptyBody>.none, authenticated: false)
        let (data, response) = try await transport.send(request: req)
        try assertStatusOK(response: response, data: data)
        let decoded = try? jsonDecoder.decode([String: Bool].self, from: data)
        return decoded?["ok"] ?? false
    }

    public func completePairing(request: PairCompleteRequest) async throws -> PairCompleteResponse {
        let urlRequest = try makeRequest(
            method: "POST",
            path: "/v1/pair/complete",
            body: request,
            authenticated: false
        )
        let (data, response) = try await transport.send(request: urlRequest)
        try assertStatusOK(response: response, data: data)
        return try decode(PairCompleteResponse.self, from: data)
    }

    public func status() async throws -> StatusResponse {
        let req = try makeRequest(method: "GET", path: "/v1/status", body: Optional<EmptyBody>.none, authenticated: true)
        let (data, response) = try await transport.send(request: req)
        try assertStatusOK(response: response, data: data)
        return try decode(StatusResponse.self, from: data)
    }

    public func captureControl(request: CaptureControlRequest) async throws -> CaptureControlResponse {
        let req = try makeRequest(
            method: "POST",
            path: "/v1/capture/control",
            body: request,
            authenticated: true
        )
        let (data, response) = try await transport.send(request: req)
        try assertStatusOK(response: response, data: data)
        return try decode(CaptureControlResponse.self, from: data)
    }

    public func createManualMemory(request: ManualMemoryRequest) async throws -> ManualMemoryResponse {
        let req = try makeRequest(
            method: "POST",
            path: "/v1/memories/manual",
            body: request,
            authenticated: true
        )
        let (data, response) = try await transport.send(request: req)
        try assertStatusOK(response: response, data: data)
        return try decode(ManualMemoryResponse.self, from: data)
    }

    public func ask(request: AskRequest) async throws -> AskResponse {
        let req = try makeRequest(
            method: "POST",
            path: "/v1/ask",
            body: request,
            authenticated: true
        )
        let (data, response) = try await transport.send(request: req)
        try assertStatusOK(response: response, data: data)
        return try decode(AskResponse.self, from: data)
    }

    public func searchMemories(request: MemorySearchRequest) async throws -> MemorySearchResponse {
        let req = try makeRequest(
            method: "POST",
            path: "/v1/memories/search",
            body: request,
            authenticated: true
        )
        let (data, response) = try await transport.send(request: req)
        try assertStatusOK(response: response, data: data)
        return try decode(MemorySearchResponse.self, from: data)
    }

    public func memoryDetail(memoryId: String) async throws -> MemoryDetailResponse {
        let encodedId = memoryId.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? memoryId
        let req = try makeRequest(
            method: "GET",
            path: "/v1/memories/\(encodedId)",
            body: Optional<EmptyBody>.none,
            authenticated: true
        )
        let (data, response) = try await transport.send(request: req)
        try assertStatusOK(response: response, data: data)
        return try decode(MemoryDetailResponse.self, from: data)
    }

    public func submitFeedback(request: FeedbackRequest) async throws -> FeedbackResponse {
        let req = try makeRequest(
            method: "POST",
            path: "/v1/feedback",
            body: request,
            authenticated: true
        )
        let (data, response) = try await transport.send(request: req)
        try assertStatusOK(response: response, data: data)
        return try decode(FeedbackResponse.self, from: data)
    }

    // MARK: Internals

    private struct EmptyBody: Codable {}

    private func makeRequest<Body: Encodable>(
        method: String,
        path: String,
        body: Body?,
        authenticated: Bool
    ) throws -> URLRequest {
        let url = config.baseURL.appendingPathComponent(path.trimmingCharacters(in: CharacterSet(charactersIn: "/")))
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json", forHTTPHeaderField: "Accept")

        if authenticated {
            guard let token = config.accessToken, !token.isEmpty else {
                throw CompanionError.unauthenticated
            }
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }

        if let body = body, !(body is EmptyBody) {
            request.httpBody = try jsonEncoder.encode(body)
        }
        return request
    }

    private func assertStatusOK(response: URLResponse, data: Data) throws {
        guard let http = response as? HTTPURLResponse else {
            throw CompanionError.unexpectedResponse("non-HTTP response")
        }
        if (200..<300).contains(http.statusCode) { return }

        if let body = try? jsonDecoder.decode(CompanionErrorBody.self, from: data) {
            let code = CompanionErrorCode(raw: body.error)
            switch (http.statusCode, code) {
            case (401, _):
                throw CompanionError.unauthenticated
            case (403, .insufficientPermission):
                throw CompanionError.insufficientPermission
            case (403, _):
                throw CompanionError.forbidden
            case (400, .pairingCodeInvalid):
                throw CompanionError.pairingCodeInvalid
            case (409, .pairingCodeUsed):
                throw CompanionError.pairingCodeUsed
            default:
                throw CompanionError.http(status: http.statusCode, code: code, message: body.message)
            }
        }

        throw CompanionError.http(
            status: http.statusCode,
            code: .unknown,
            message: String(data: data, encoding: .utf8) ?? ""
        )
    }

    private func decode<T: Decodable>(_ type: T.Type, from data: Data) throws -> T {
        do {
            return try jsonDecoder.decode(type, from: data)
        } catch {
            throw CompanionError.decoding(String(describing: error))
        }
    }
}
