import SwiftUI

struct AskView: View {
    @EnvironmentObject private var session: CompanionSession
    @StateObject private var model = AskViewModel()

    var body: some View {
        NavigationStack {
            VStack(spacing: 12) {
                HStack {
                    TextField("Ask your memories…", text: $model.query)
                        .textFieldStyle(.roundedBorder)

                    Button(model.isLoading ? "…" : "Ask") {
                        model.runAsk(session: session)
                    }
                    .disabled(model.isLoading || model.query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }

                Picker("Style", selection: $model.answerStyle) {
                    Text("Short").tag("short")
                    Text("Detailed").tag("detailed")
                    Text("Context Pack").tag("context_pack")
                }
                .pickerStyle(.segmented)

                if let errorText = model.errorText {
                    Text(errorText)
                        .font(.footnote)
                        .foregroundStyle(.red)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }

                if !model.answer.isEmpty {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 12) {
                            Text(model.answer)
                                .frame(maxWidth: .infinity, alignment: .leading)

                            if !model.cards.isEmpty {
                                Text("Sources")
                                    .font(.headline)
                                ForEach(model.cards, id: \.memoryId) { card in
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text(card.title).font(.subheadline.weight(.semibold))
                                        Text(card.displaySummary.isEmpty ? card.summary : card.displaySummary)
                                            .font(.footnote)
                                            .foregroundStyle(.secondary)
                                    }
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(10)
                                    .background(.thinMaterial, in: RoundedRectangle(cornerRadius: 10))
                                }
                            }
                        }
                    }
                }

                if !model.history.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Recent queries")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack {
                                ForEach(model.history, id: \.self) { value in
                                    Button(value) {
                                        model.useHistory(value)
                                    }
                                    .buttonStyle(.bordered)
                                    .controlSize(.small)
                                }
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                Spacer()
            }
            .padding()
            .navigationTitle("Ask")
        }
    }
}
