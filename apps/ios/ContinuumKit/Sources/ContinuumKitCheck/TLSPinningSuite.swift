//  TLSPinningSuite.swift — exercises the DER→PEM transcoder. We don't
//  hold a real SecTrust from the test process, so we cover the conversion
//  whose output the fingerprint hashes.

import Foundation
import CryptoKit
import ContinuumKit

let tlsPinningSuite = TestSuite("TLS pinning helpers", [
    TestCase("derToPEM emits header, ≤64-char body lines, trailing newline") {
        let der = Data(repeating: 0xAB, count: 200)
        let pem = URLSessionTransport.derToPEM(der)
        try expect(pem.hasPrefix("-----BEGIN CERTIFICATE-----\n"))
        try expect(pem.hasSuffix("-----END CERTIFICATE-----\n"))
        let lines = pem.split(separator: "\n")
        let bodyLines = lines.dropFirst().dropLast()
        for line in bodyLines {
            try expect(line.count <= 64, "line too long: \(line.count) chars")
        }
    },

    TestCase("SHA-256 over the PEM is stable across runs") {
        let der = Data(repeating: 0x42, count: 100)
        let pem = URLSessionTransport.derToPEM(der)
        let digest1 = SHA256.hash(data: Data(pem.utf8))
        let hex1 = digest1.map { String(format: "%02x", $0) }.joined()
        try expectEqual(hex1.count, 64)

        let pem2 = URLSessionTransport.derToPEM(der)
        let digest2 = SHA256.hash(data: Data(pem2.utf8))
        let hex2 = digest2.map { String(format: "%02x", $0) }.joined()
        try expectEqual(hex1, hex2)
    },
])
