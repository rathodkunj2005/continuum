#if canImport(AppIntents)
import AppIntents

struct AskContinuumIntent: AppIntent {
    static var title: LocalizedStringResource = "Ask Continuum"
    static var description = IntentDescription("Ask your paired Continuum Mac.")

    @Parameter(title: "Question")
    var question: String

    func perform() async throws -> some IntentResult & ProvidesDialog {
        .result(dialog: "Sent to Continuum: \(question)")
    }
}

struct RememberWithContinuumIntent: AppIntent {
    static var title: LocalizedStringResource = "Remember with Continuum"
    static var description = IntentDescription("Create a manual memory in Continuum.")

    @Parameter(title: "Note")
    var note: String

    func perform() async throws -> some IntentResult & ProvidesDialog {
        .result(dialog: "Queued memory: \(note)")
    }
}

struct PauseCaptureIntent: AppIntent {
    static var title: LocalizedStringResource = "Pause Continuum Capture"
    static var description = IntentDescription("Pause capture on your paired Continuum Mac.")

    func perform() async throws -> some IntentResult & ProvidesDialog {
        .result(dialog: "Requested capture pause on Continuum Mac.")
    }
}
#endif
