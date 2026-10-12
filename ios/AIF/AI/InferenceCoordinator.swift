import Foundation

enum InferenceCoordinator {
    static func estimate(
        image: Data,
        context: AnimalContext,
        mode: InferenceMode,
        cloudSettings: CloudProviderSettings? = nil,
        apiKey: String? = nil
    ) async throws -> WeightEstimationResult {
        switch mode {
        case .local:
            return try await CoreMLWeightEstimationProvider().estimate(image: image, context: context)
        case .cloud:
            guard let cloudSettings, let apiKey, !apiKey.isEmpty else {
                throw WeightEstimationError.providerNotConfigured
            }
            return try await OpenAICompatibleVisionProvider(settings: cloudSettings, apiKey: apiKey)
                .estimate(image: image, context: context)
        }
    }
}
