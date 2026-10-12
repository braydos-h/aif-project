import Foundation

enum ProviderConfigurationError: LocalizedError, Equatable {
    case invalidURL
    case insecureRemoteEndpoint
    case invalidModel

    var errorDescription: String? {
        switch self {
        case .invalidURL: "Enter an absolute provider endpoint URL without credentials, query parameters, or a fragment."
        case .insecureRemoteEndpoint: "Hosted AI endpoints must use HTTPS. HTTP is allowed only for localhost testing."
        case .invalidModel: "Enter a model name."
        }
    }
}

enum ProviderConfigurationValidator {
    static func validate(endpoint: String, modelName: String) throws -> URL {
        guard !modelName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            throw ProviderConfigurationError.invalidModel
        }
        guard let components = URLComponents(string: endpoint),
              let scheme = components.scheme?.lowercased(),
              let host = components.host?.lowercased(),
              components.user == nil, components.password == nil,
              components.query == nil, components.fragment == nil,
              let url = components.url else {
            throw ProviderConfigurationError.invalidURL
        }
        let isLocalHost = ["localhost", "127.0.0.1", "::1", "[::1]"].contains(host)
        guard scheme == "https" || (scheme == "http" && isLocalHost) else {
            throw ProviderConfigurationError.insecureRemoteEndpoint
        }
        return url
    }
}
