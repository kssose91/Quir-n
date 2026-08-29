import Foundation

struct MobileMessageRequest: Encodable {
    let sessionId: String
    let userId: String
    let text: String
    let model: String
    let source: String
    let projectId: String?
    let maxTokens: Int?

    enum CodingKeys: String, CodingKey {
        case sessionId = "session_id"
        case userId = "user_id"
        case text
        case model
        case source
        case projectId = "project_id"
        case maxTokens = "max_tokens"
    }
}

struct MobileMessageResponse: Decodable {
    let reply: String
    let eventId: String
    let modelUsed: String
    let latencyMs: Int
    let inputTokens: Int
    let outputTokens: Int
    let memoriesInjected: Int
    let sessionId: String
    let userId: String
    let source: String

    enum CodingKeys: String, CodingKey {
        case reply
        case eventId = "event_id"
        case modelUsed = "model_used"
        case latencyMs = "latency_ms"
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
        case memoriesInjected = "memories_injected"
        case sessionId = "session_id"
        case userId = "user_id"
        case source
    }
}

struct ChatItem: Identifiable {
    let id = UUID()
    let role: String
    let text: String
    let meta: String?
}
