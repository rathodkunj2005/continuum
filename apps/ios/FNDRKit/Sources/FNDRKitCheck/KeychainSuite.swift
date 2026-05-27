//  KeychainSuite.swift — in-memory KeychainStorage tests.

import Foundation
import FNDRKit

let keychainSuite = TestSuite("Keychain (in-memory backend)", [
    TestCase("Round-trip a string value") {
        let store = InMemoryKeychainStore()
        try store.setString("token-abc", forKey: "k1")
        try expectEqual(try store.stringForKey("k1"), "token-abc")
    },

    TestCase("setData overwrites an existing key") {
        let store = InMemoryKeychainStore()
        try store.setString("one", forKey: "k1")
        try store.setString("two", forKey: "k1")
        try expectEqual(try store.stringForKey("k1"), "two")
    },

    TestCase("Missing key reads as nil") {
        let store = InMemoryKeychainStore()
        try expectNil(try store.dataForKey("nope"))
        try expectNil(try store.stringForKey("nope"))
    },

    TestCase("deleteKey removes the value") {
        let store = InMemoryKeychainStore()
        try store.setString("x", forKey: "k1")
        try store.deleteKey("k1")
        try expectNil(try store.stringForKey("k1"))
    },

    TestCase("deleteKey on a missing entry does not throw") {
        let store = InMemoryKeychainStore()
        try store.deleteKey("never-existed")
    },

    TestCase("Codable round-trip preserves PairedMac fields") {
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
        try expectEqual(loaded, paired)
    },

    TestCase("PairedMac.baseURL builds https://host:port") {
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
        try expectEqual(paired.baseURL.absoluteString, "https://127.0.0.1:47812")
    },
])
