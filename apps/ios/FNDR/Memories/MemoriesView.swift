import SwiftUI

struct MemoriesView: View {
    @EnvironmentObject private var session: CompanionSession
    @StateObject private var model = MemoriesViewModel()

    var body: some View {
        NavigationStack {
            VStack(spacing: 10) {
                TextField("Search memories", text: $model.query)
                    .textFieldStyle(.roundedBorder)

                HStack {
                    TextField("Time (today, last_7d)", text: $model.timeFilter)
                        .textFieldStyle(.roundedBorder)
                    TextField("App", text: $model.appFilter)
                        .textFieldStyle(.roundedBorder)
                    TextField("Project", text: $model.projectFilter)
                        .textFieldStyle(.roundedBorder)
                }

                Button(model.isLoading ? "Searching…" : "Search") {
                    model.search(session: session)
                }
                .disabled(model.query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.isLoading)

                if let errorText = model.errorText {
                    Text(errorText)
                        .font(.footnote)
                        .foregroundStyle(.red)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }

                List(model.cards, id: \.memoryId) { card in
                    Button {
                        model.loadDetail(session: session, memoryId: card.memoryId)
                    } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(card.title).font(.subheadline.weight(.semibold))
                            Text(card.displaySummary.isEmpty ? card.summary : card.displaySummary)
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                            Text("\(card.appName) · \(Date(timeIntervalSince1970: TimeInterval(card.timestamp) / 1000), style: .relative)")
                                .font(.caption2)
                                .foregroundStyle(.tertiary)
                        }
                    }
                }
                .listStyle(.plain)
            }
            .padding()
            .navigationTitle("Memories")
            .sheet(item: Binding(
                get: { model.selectedCard.map(CardSheetItem.init) },
                set: { _ in model.clearDetail() }
            )) { item in
                VStack(alignment: .leading, spacing: 12) {
                    Text(item.card.title).font(.headline)
                    Text(item.card.summary)
                    if !item.card.internalContext.isEmpty {
                        Text(item.card.internalContext)
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                }
                .padding()
            }
        }
    }
}

private struct CardSheetItem: Identifiable {
    let card: CompanionMemoryCard
    var id: String { card.memoryId }
}
