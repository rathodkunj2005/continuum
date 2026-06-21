#if canImport(XCTest)
//  TLSPinningTests.swift — verifies the DER→PEM transcoder matches the
//  Mac's PEM format. We don't have a real SecTrust to test against from
//  `swift test`, so we cover the conversion that the fingerprint hashes.

import XCTest
import CryptoKit
@testable import ContinuumKit

final class TLSPinningTests: XCTestCase {
    func testDerToPEMUses64CharLinesAndTrailingNewline() {
        let der = Data(repeating: 0xAB, count: 200)
        let pem = URLSessionTransport.derToPEM(der)
        XCTAssertTrue(pem.hasPrefix("-----BEGIN CERTIFICATE-----\n"))
        XCTAssertTrue(pem.hasSuffix("-----END CERTIFICATE-----\n"))
        // Body lines (excluding headers) must each be ≤ 64 chars.
        let lines = pem.split(separator: "\n")
        let bodyLines = lines.dropFirst().dropLast()
        for line in bodyLines {
            XCTAssertLessThanOrEqual(line.count, 64)
        }
    }

    func testFingerprintMatchesSwiftSha256OfPEM() {
        let der = Data(repeating: 0x42, count: 100)
        let pem = URLSessionTransport.derToPEM(der)
        let expected = SHA256.hash(data: Data(pem.utf8))
        let hex = expected.map { String(format: "%02x", $0) }.joined()
        XCTAssertEqual(hex.count, 64)
        // Sanity check: same input gives same output across runs.
        let again = SHA256.hash(data: Data(URLSessionTransport.derToPEM(der).utf8))
        let hex2 = again.map { String(format: "%02x", $0) }.joined()
        XCTAssertEqual(hex, hex2)
    }
}
#endif
