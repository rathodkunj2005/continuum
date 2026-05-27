import Foundation
import Combine
import FNDRKit

@MainActor
final class CaptureViewModel: ObservableObject {
    @Published var note: String = ""
    @Published var captureType: String = "note"
    @Published var project: String = ""
    @Published var topic: String = ""
    @Published private(set) var statusText: String?
    @Published private(set) var queueDepth: Int = 0
    @Published var isSaving = false

    func refreshQueueDepth(session: CompanionSession) {
        Task {
            let pending = await session.offlineQueue.pending()
            await MainActor.run {
                queueDepth = pending.count
            }
        }
    }

    func save(session: CompanionSession) {
        let trimmed = note.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        isSaving = true
        statusText = nil

        Task {
            let immediate = await session.captureNowOrQueue(
                text: trimmed,
                captureType: captureType,
                project: nilIfEmpty(project),
                topic: nilIfEmpty(topic)
            )

            let pending = await session.offlineQueue.pending()
            await MainActor.run {
                isSaving = false
                queueDepth = pending.count
                if immediate {
                    statusText = "Saved to Mac"
                } else {
                    statusText = "Mac unavailable. Queued offline (\(pending.count) pending)."
                }
                note = ""
            }
        }
    }

    func flushQueue(session: CompanionSession) {
        Task {
            if let result = await session.flushOfflineQueueIfPossible() {
                let pending = await session.offlineQueue.pending()
                await MainActor.run {
                    queueDepth = pending.count
                    statusText = "Flushed \(result.succeeded)/\(result.attempted). Remaining: \(result.remaining)."
                }
            } else {
                await MainActor.run {
                    statusText = "Could not reach paired Mac to flush queue."
                }
            }
        }
    }

    private func nilIfEmpty(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
