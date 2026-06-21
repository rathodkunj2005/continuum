//  main.swift — entry point for `swift run ContinuumKitCheck`.
//
//  Aggregates every suite into one run + exits with a nonzero status on
//  any failure so CI / `make test` can gate on it.

import Foundation

@main
struct Main {
    static func main() async {
        let exitCode = await runSuites([
            dtoSuite,
            keychainSuite,
            companionClientSuite,
            pairingFlowSuite,
            connectionStatusSuite,
            tlsPinningSuite,
            offlineQueueSuite,
            watchBridgeSuite,
        ])
        Foundation.exit(exitCode)
    }
}
