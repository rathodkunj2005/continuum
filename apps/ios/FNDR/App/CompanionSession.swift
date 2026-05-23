import Foundation
import Combine
import FNDRKit

/// Session boundary for draft SwiftUI screens. Owns local pairing persistence
/// and creates FNDRKit clients/services from the current Keychain state.
@MainActor
final class CompanionSession: ObservableObject {
    @Published private(set) var pairedMac: PairedMac?

    let keychain: KeychainStorage
    let offlineQueue: OfflineCaptureQueue

    init(
        keychain: KeychainStorage = KeychainStore(),
        offlineQueue: OfflineCaptureQueue = OfflineCaptureQueue()
    ) {
        self.keychain = keychain
        self.offlineQueue = offlineQueue
        reloadPairingState()
    }

    func reloadPairingState() {
        do {
            pairedMac = try keychain.codableForKey(KeychainKeys.pairedMac, as: PairedMac.self)
        } catch {
            pairedMac = nil
        }
    }

    func clearPairing() {
        do {
            try keychain.deleteKey(KeychainKeys.accessToken)
            try keychain.deleteKey(KeychainKeys.pairedMac)
        } catch {
            // Best effort in draft UI.
        }
        pairedMac = nil
    }

    func makePairingFlow() -> PairingFlow {
        PairingFlow(keychain: keychain)
    }

    func makeClient() throws -> CompanionClient {
        guard let paired = pairedMac else {
            throw CompanionError.noEndpoint
        }
        let token = try keychain.stringForKey(KeychainKeys.accessToken)
        let transport = URLSessionTransport(pinnedFingerprint: paired.certFingerprintSha256)
        return CompanionClient(
            config: .init(baseURL: paired.baseURL, accessToken: token),
            transport: transport
        )
    }

    @discardableResult
    func captureNowOrQueue(
        text: String,
        captureType: String?,
        project: String?,
        topic: String?
    ) async -> Bool {
        let clientEventId = UUID().uuidString
        do {
            let client = try makeClient()
            _ = try await client.createManualMemory(
                request: ManualMemoryRequest(
                    text: text,
                    clientEventId: clientEventId,
                    captureType: captureType,
                    project: project,
                    topic: topic
                )
            )
            return true
        } catch {
            do {
                _ = try await offlineQueue.enqueue(
                    text: text,
                    clientEventId: clientEventId,
                    captureType: captureType,
                    project: project,
                    topic: topic
                )
            } catch {
                return false
            }
            return false
        }
    }

    func flushOfflineQueueIfPossible() async -> QueueFlushResult? {
        do {
            let client = try makeClient()
            return await offlineQueue.flush(using: client)
        } catch {
            return nil
        }
    }
}
