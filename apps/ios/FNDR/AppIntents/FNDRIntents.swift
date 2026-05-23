#if canImport(AppIntents)
import AppIntents

struct AskFNDRIntent: AppIntent {
    static var title: LocalizedStringResource = "Ask FNDR"
    static var description = IntentDescription("Ask your paired FNDR Mac.")

    @Parameter(title: "Question")
    var question: String

    func perform() async throws -> some IntentResult & ProvidesDialog {
        .result(dialog: "Sent to FNDR: \(question)")
    }
}

struct RememberWithFNDRIntent: AppIntent {
    static var title: LocalizedStringResource = "Remember with FNDR"
    static var description = IntentDescription("Create a manual memory in FNDR.")

    @Parameter(title: "Note")
    var note: String

    func perform() async throws -> some IntentResult & ProvidesDialog {
        .result(dialog: "Queued memory: \(note)")
    }
}

struct PauseCaptureIntent: AppIntent {
    static var title: LocalizedStringResource = "Pause FNDR Capture"
    static var description = IntentDescription("Pause capture on your paired FNDR Mac.")

    func perform() async throws -> some IntentResult & ProvidesDialog {
        .result(dialog: "Requested capture pause on FNDR Mac.")
    }
}
#endif
