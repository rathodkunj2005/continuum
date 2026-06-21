import Foundation
import Combine
import ContinuumKit

@MainActor
final class AskViewModel: ObservableObject {
    @Published var query: String = ""
    @Published var answer: String = ""
    @Published var cards: [CompanionMemoryCard] = []
    @Published var answerStyle: String = "short"
    @Published var isLoading = false
    @Published var errorText: String?
    @Published private(set) var history: [String] = []

    private let historyKey = "continuum.ask.history"

    init() {
        history = UserDefaults.standard.stringArray(forKey: historyKey) ?? []
    }

    func runAsk(session: CompanionSession) {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        isLoading = true
        errorText = nil

        Task {
            do {
                let client = try session.makeClient()
                let response = try await client.ask(
                    request: AskRequest(
                        query: trimmed,
                        limit: 8,
                        answerStyle: answerStyle
                    )
                )
                await MainActor.run {
                    answer = response.answer
                    cards = response.sourceCards
                    isLoading = false
                    appendHistory(trimmed)
                }
            } catch {
                await MainActor.run {
                    isLoading = false
                    errorText = error.localizedDescription
                }
            }
        }
    }

    func useHistory(_ value: String) {
        query = value
    }

    private func appendHistory(_ value: String) {
        var next = history.filter { $0 != value }
        next.insert(value, at: 0)
        if next.count > 20 {
            next = Array(next.prefix(20))
        }
        history = next
        UserDefaults.standard.set(next, forKey: historyKey)
    }
}
