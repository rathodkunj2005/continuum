import SwiftUI

struct PairingView: View {
    @StateObject private var viewModel: PairingViewModel
    @State private var isScanning = false

    init(session: CompanionSession) {
        _viewModel = StateObject(wrappedValue: PairingViewModel(session: session))
    }

    var body: some View {
        Form {
            Section("Pairing payload") {
                Button("Scan QR") {
                    isScanning = true
                }

                TextEditor(text: $viewModel.qrPayloadJSON)
                    .frame(minHeight: 140)
                    .font(.system(.footnote, design: .monospaced))

                Button("Validate payload") {
                    viewModel.acceptPayload()
                }

                Button(viewModel.isPairing ? "Pairing..." : "Complete pairing") {
                    viewModel.complete()
                }
                .disabled(viewModel.isPairing)
            }

            Section("State") {
                Text(viewModel.stateText)
                    .font(.footnote)
            }
        }
        .navigationTitle("Pair FNDR")
        .sheet(isPresented: $isScanning) {
            QRScannerView { payload in
                viewModel.accept(rawPayload: payload)
                isScanning = false
            }
            .ignoresSafeArea()
        }
    }
}
