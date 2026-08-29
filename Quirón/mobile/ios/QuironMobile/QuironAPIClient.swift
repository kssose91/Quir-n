import Foundation

final class QuironAPIClient: NSObject {
    enum ClientError: LocalizedError {
        case invalidBaseURL
        case insecureURL
        case invalidHost
        case missingToken
        case serverError(status: Int, message: String)
        case decodeFailed

        var errorDescription: String? {
            switch self {
            case .invalidBaseURL: return "Base URL invalida"
            case .insecureURL: return "Solo HTTPS esta permitido"
            case .invalidHost: return "Host no permitido"
            case .missingToken: return "Falta token de autenticacion"
            case .serverError(let status, let message): return "HTTP \(status): \(message)"
            case .decodeFailed: return "No se pudo decodificar la respuesta"
            }
        }
    }

    private let allowedHosts: Set<String>

    init(allowedHosts: Set<String>) {
        self.allowedHosts = allowedHosts
        super.init()
    }

    func sendMobileMessage(
        baseURLString: String,
        token: String?,
        payload: MobileMessageRequest
    ) async throws -> MobileMessageResponse {
        guard let token, !token.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw ClientError.missingToken
        }

        guard let baseURL = URL(string: baseURLString), let scheme = baseURL.scheme else {
            throw ClientError.invalidBaseURL
        }
        guard scheme.lowercased() == "https" else {
            throw ClientError.insecureURL
        }
        guard let host = baseURL.host, isHostAllowed(host) else {
            throw ClientError.invalidHost
        }

        let endpoint = baseURL.appendingPathComponent("mobile/message")
        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.timeoutInterval = 25
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.httpBody = try JSONEncoder().encode(payload)

        let config = URLSessionConfiguration.ephemeral
        config.waitsForConnectivity = true
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        config.timeoutIntervalForRequest = 25
        config.timeoutIntervalForResource = 35

        let session = URLSession(configuration: config)
        let (data, response) = try await session.data(for: request)

        guard let http = response as? HTTPURLResponse else {
            throw ClientError.serverError(status: -1, message: "Respuesta no HTTP")
        }

        guard (200..<300).contains(http.statusCode) else {
            let msg = String(data: data, encoding: .utf8) ?? "error"
            throw ClientError.serverError(status: http.statusCode, message: msg)
        }

        guard let decoded = try? JSONDecoder().decode(MobileMessageResponse.self, from: data) else {
            throw ClientError.decodeFailed
        }

        return decoded
    }

    private func isHostAllowed(_ host: String) -> Bool {
        if allowedHosts.contains(host) {
            return true
        }
        // Permitir dominios privados Tailscale.
        return host.hasSuffix(".ts.net")
    }
}
