import Foundation

@MainActor
final class ChatViewModel: ObservableObject {
    @Published var items: [ChatItem] = []
    @Published var inputText: String = ""
    @Published var isSending: Bool = false
    @Published var errorText: String?

    @Published var baseURL: String = "https://llore.taild1138a.ts.net"
    @Published var model: String = "gemini-2.5-pro"
    @Published var sessionId: String = "ios-session-1"
    @Published var userId: String = "llorens_iphone"
    @Published var projectId: String = "llorens_unified"

    private let client = QuironAPIClient(allowedHosts: ["llore.taild1138a.ts.net"])

    func saveToken(_ token: String) {
        do {
            try KeychainStore.saveToken(token)
            errorText = nil
        } catch {
            errorText = error.localizedDescription
        }
    }

    func send() async {
        let text = inputText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }

        isSending = true
        errorText = nil
        items.append(ChatItem(role: "user", text: text, meta: nil))
        inputText = ""

        do {
            let token = try KeychainStore.loadToken()
            let req = MobileMessageRequest(
                sessionId: sessionId,
                userId: userId,
                text: text,
                model: model,
                source: "iphone",
                projectId: projectId.isEmpty ? nil : projectId,
                maxTokens: 900
            )

            let res = try await client.sendMobileMessage(
                baseURLString: baseURL,
                token: token,
                payload: req
            )

            let meta = "\(res.modelUsed) | \(res.latencyMs)ms | in:\(res.inputTokens) out:\(res.outputTokens)"
            items.append(ChatItem(role: "quiron", text: res.reply, meta: meta))
        } catch {
            let err = error.localizedDescription
            errorText = err
            items.append(ChatItem(role: "system", text: "Error: \(err)", meta: nil))
        }

        isSending = false
    }
}
