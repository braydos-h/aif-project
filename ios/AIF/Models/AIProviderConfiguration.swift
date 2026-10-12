import Foundation
import SwiftData

enum AIProviderKind: String, CaseIterable, Identifiable, Codable {
    case openAICompatible
    case gemini
    case ollama

    var id: String { rawValue }
    var title: String {
        switch self {
        case .openAICompatible: "OpenAI-compatible vision API"
        case .gemini: "Gemini (not implemented)"
        case .ollama: "Ollama (not implemented)"
        }
    }
    var isImplemented: Bool { self == .openAICompatible }
}

@Model
final class AIProviderConfiguration {
    @Attribute(.unique) var id: UUID
    var providerRawValue: String
    var displayName: String
    var modelName: String
    var endpoint: String
    var isDefault: Bool
    /// An opaque Keychain account name. This field is not a credential.
    var keychainReference: String
    var createdAt: Date
    var updatedAt: Date

    init(
        id: UUID = UUID(),
        provider: AIProviderKind = .openAICompatible,
        displayName: String = "OpenAI-compatible provider",
        modelName: String = "gpt-4o-mini",
        endpoint: String = "https://api.openai.com/v1/chat/completions",
        isDefault: Bool = true,
        keychainReference: String = UUID().uuidString,
        createdAt: Date = .now,
        updatedAt: Date = .now
    ) {
        self.id = id
        self.providerRawValue = provider.rawValue
        self.displayName = displayName
        self.modelName = modelName
        self.endpoint = endpoint
        self.isDefault = isDefault
        self.keychainReference = keychainReference
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }

    var provider: AIProviderKind { AIProviderKind(rawValue: providerRawValue) ?? .openAICompatible }
}
