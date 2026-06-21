import SwiftUI

struct WatchRememberView: View {
    @State private var text = ""
    @State private var status = ""
    @State private var isSaving = false

    var body: some View {
        VStack(spacing: 8) {
            TextField("Remember", text: $text)
            Button(isSaving ? "Saving…" : "Save") {
                let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !trimmed.isEmpty else { return }
                isSaving = true
                Task {
                    let response = await WatchBridgeClient.shared.remember(trimmed)
                    await MainActor.run {
                        isSaving = false
                        if response.ok {
                            status = response.payload["result"] == "saved"
                                ? "Saved on Mac"
                                : "Queued on iPhone"
                            text = ""
                        } else {
                            status = response.message ?? "Save failed"
                        }
                    }
                }
            }
            .disabled(isSaving || text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            Text(status)
                .font(.footnote)
        }
        .padding()
    }
}
