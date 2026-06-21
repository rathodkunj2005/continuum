import SwiftUI

struct WatchAskView: View {
    @State private var query = ""
    @State private var answer = ""
    @State private var isLoading = false

    var body: some View {
        VStack(spacing: 8) {
            TextField("Ask", text: $query)
            Button(isLoading ? "Sending…" : "Send") {
                let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !trimmed.isEmpty else { return }
                isLoading = true
                Task {
                    let response = await WatchBridgeClient.shared.ask(trimmed)
                    await MainActor.run {
                        isLoading = false
                        answer = response.message ?? response.payload["answer"] ?? "No answer"
                    }
                }
            }
            .disabled(isLoading || query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            Text(answer)
                .font(.footnote)
        }
        .padding()
    }
}
