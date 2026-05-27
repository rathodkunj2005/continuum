import SwiftUI

struct WatchStatusView: View {
    @State private var status = "Unknown"
    @State private var isLoading = false

    var body: some View {
        VStack(spacing: 8) {
            Text("Capture")
                .font(.headline)
            Text(status)
            Button(isLoading ? "Working…" : "Pause") {
                isLoading = true
                Task {
                    let response = await WatchBridgeClient.shared.pauseCapture()
                    let updatedStatus = if response.ok {
                        response.payload["capture_status"] ?? "paused"
                    } else {
                        response.message ?? "Pause failed"
                    }
                    await MainActor.run {
                        status = updatedStatus
                        isLoading = false
                    }
                }
            }
            .disabled(isLoading)

            Button("Refresh") {
                isLoading = true
                Task {
                    let response = await WatchBridgeClient.shared.status()
                    let updatedStatus = if response.ok {
                        response.payload["capture_status"] ?? "unknown"
                    } else {
                        response.message ?? "Status unavailable"
                    }
                    await MainActor.run {
                        status = updatedStatus
                        isLoading = false
                    }
                }
            }
            .disabled(isLoading)
        }
        .padding()
        .onAppear {
            Task {
                let response = await WatchBridgeClient.shared.status()
                await MainActor.run {
                    status = response.payload["capture_status"] ?? response.message ?? "unknown"
                }
            }
        }
    }
}
