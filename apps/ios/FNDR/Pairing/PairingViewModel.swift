import Foundation
import Combine
import FNDRKit

@MainActor
final class PairingViewModel: ObservableObject {
    @Published var qrPayloadJSON: String = ""
    @Published private(set) var stateText: String = "Paste FNDR pairing QR JSON payload"
    @Published private(set) var isPairing = false

    private let session: CompanionSession
    private let flow: PairingFlow

    init(session: CompanionSession) {
        self.session = session
        self.flow = session.makePairingFlow()
    }

    func acceptPayload() {
        accept(rawPayload: qrPayloadJSON)
    }

    func accept(rawPayload: String) {
        qrPayloadJSON = rawPayload
        do {
            let payload = try PairingFlow.parseQRPayload(rawPayload)
            Task {
                let state = await flow.accept(payload: payload)
                await MainActor.run {
                    stateText = describe(state)
                }
            }
        } catch {
            stateText = "Invalid payload: \(error.localizedDescription)"
        }
    }

    func complete(deviceName: String = "FNDR iPhone", appVersion: String = "0.1.0") {
        isPairing = true
        Task {
            let state = await flow.complete(deviceName: deviceName, deviceType: .iphone, appVersion: appVersion)
            await MainActor.run {
                isPairing = false
                stateText = describe(state)
                if case .paired = state {
                    session.reloadPairingState()
                }
            }
        }
    }

    private func describe(_ state: PairingState) -> String {
        switch state {
        case .idle:
            return "Idle"
        case .ready(let payload):
            return "Ready to pair with \(payload.macName) at \(payload.host):\(payload.port)"
        case .pairing:
            return "Pairing in progress..."
        case .paired(let mac):
            return "Paired with \(mac.macName)"
        case .failed(let message):
            return "Pairing failed: \(message)"
        }
    }
}
