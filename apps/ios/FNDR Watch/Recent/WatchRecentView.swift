import SwiftUI

struct WatchRecentView: View {
    @State private var items: [String] = []
    @State private var status = "Loading…"

    var body: some View {
        List {
            if items.isEmpty {
                Text(status)
            } else {
                ForEach(items, id: \.self) { item in
                    Text(item)
                }
            }
        }
        .onAppear {
            Task {
                let response = await WatchBridgeClient.shared.recent(limit: 5)
                await MainActor.run {
                    if response.ok {
                        let raw = response.payload["items"] ?? ""
                        let parsed = raw
                            .split(separator: "\n")
                            .map { String($0).trimmingCharacters(in: .whitespacesAndNewlines) }
                            .filter { !$0.isEmpty }
                        items = parsed
                        status = parsed.isEmpty ? "No recent cards" : ""
                    } else {
                        status = response.message ?? "Unable to load recent cards"
                    }
                }
            }
        }
    }
}
