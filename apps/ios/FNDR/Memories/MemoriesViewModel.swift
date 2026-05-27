import Foundation
import Combine
import FNDRKit

@MainActor
final class MemoriesViewModel: ObservableObject {
    @Published var query: String = ""
    @Published var timeFilter: String = ""
    @Published var appFilter: String = ""
    @Published var projectFilter: String = ""
    @Published private(set) var cards: [CompanionMemoryCard] = []
    @Published private(set) var selectedCard: CompanionMemoryCard?
    @Published var isLoading = false
    @Published var errorText: String?

    func search(session: CompanionSession) {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            cards = []
            return
        }

        isLoading = true
        errorText = nil

        Task {
            do {
                let client = try session.makeClient()
                let response = try await client.searchMemories(
                    request: MemorySearchRequest(
                        query: trimmed,
                        limit: 20,
                        timeFilter: nilIfEmpty(timeFilter),
                        appFilter: nilIfEmpty(appFilter),
                        projectFilter: nilIfEmpty(projectFilter)
                    )
                )
                await MainActor.run {
                    cards = response.cards
                    isLoading = false
                }
            } catch {
                await MainActor.run {
                    errorText = error.localizedDescription
                    isLoading = false
                }
            }
        }
    }

    func loadDetail(session: CompanionSession, memoryId: String) {
        Task {
            do {
                let client = try session.makeClient()
                let response = try await client.memoryDetail(memoryId: memoryId)
                await MainActor.run {
                    selectedCard = response.card
                }
            } catch {
                await MainActor.run {
                    errorText = error.localizedDescription
                }
            }
        }
    }

    func clearDetail() {
        selectedCard = nil
    }

    private func nilIfEmpty(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
