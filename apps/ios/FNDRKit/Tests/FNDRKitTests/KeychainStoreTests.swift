#if canImport(XCTest)
//  KeychainStoreTests.swift — tests the in-memory keychain backend (the
//  same protocol the iOS app uses); the real Security-framework backend is
//  exercised on-device. The in-memory backend supports the integration
//  paths the real keychain serves: round-trip, overwrite, delete, missing.

import XCTest
@testable import FNDRKit

final class KeychainStoreTests: XCTestCase {
    func testInMemoryRoundTripString() throws {
        let store = InMemoryKeychainStore()
        try store.setString("token-abc", forKey: "k1")
        XCTAssertEqual(try store.stringForKey("k1"), "token-abc")
    }

    func testInMemoryOverwritesValueOnSetSameKey() throws {
        let store = InMemoryKeychainStore()
        try store.setString("one", forKey: "k1")
        try store.setString("two", forKey: "k1")
        XCTAssertEqual(try store.stringForKey("k1"), "two")
    }

    func testInMemoryMissingKeyReturnsNil() throws {
        let store = InMemoryKeychainStore()
        XCTAssertNil(try store.dataForKey("nope"))
        XCTAssertNil(try store.stringForKey("nope"))
    }

    func testInMemoryDeleteRemovesValue() throws {
        let store = InMemoryKeychainStore()
        try store.setString("x", forKey: "k1")
        try store.deleteKey("k1")
        XCTAssertNil(try store.stringForKey("k1"))
    }

    func testDeleteMissingKeyDoesNotThrow() throws {
        let store = InMemoryKeychainStore()
        try store.deleteKey("never-existed")
    }

    func testCodableRoundTripForPairedMac() throws {
        let store = InMemoryKeychainStore()
        let paired = PairedMac(
            deviceId: "dev_iphone_abc",
            macName: "Test Mac",
            host: "127.0.0.1",
            port: 47812,
            tls: true,
            certFingerprintSha256: "abc",
            permissions: ["ask", "search"],
            pairedAtMs: 1_700_000_000_000
        )
        try store.setCodable(paired, forKey: KeychainKeys.pairedMac)
        let loaded: PairedMac? = try store.codableForKey(KeychainKeys.pairedMac, as: PairedMac.self)
        XCTAssertEqual(loaded, paired)
    }

    func testPairedMacBaseURLBuildsHTTPS() {
        let paired = PairedMac(
            deviceId: "dev",
            macName: "Test",
            host: "127.0.0.1",
            port: 47812,
            tls: true,
            certFingerprintSha256: nil,
            permissions: [],
            pairedAtMs: 0
        )
        XCTAssertEqual(paired.baseURL.absoluteString, "https://127.0.0.1:47812")
    }
}
#endif
