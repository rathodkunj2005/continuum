import Foundation
import Combine
import FNDRKit

/// Session boundary for draft SwiftUI screens. Owns local pairing persistence
/// and creates FNDRKit clients/services from the current Keychain state.
@MainActor
final class CompanionSession: ObservableObject {
    @Published private(set) var pairedMac: PairedMac?

    let keychain: KeychainStorage

    init(keychain: KeychainStorage = KeychainStore()) {
        self.keychain = keychain
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
}
