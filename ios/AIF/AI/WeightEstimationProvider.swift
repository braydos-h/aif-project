import Foundation

struct AnimalContext: Sendable {
    var breed: String
    var sex: String
    var approximateAgeYears: Double?

    init(breed: String = "", sex: String = "unknown", approximateAgeYears: Double? = nil) {
        self.breed = breed
        self.sex = sex
        self.approximateAgeYears = approximateAgeYears
    }
}

struct WeightEstimationResult: Sendable {
    let estimatedWeightKg: Double
    let lowerBoundKg: Double?
    let upperBoundKg: Double?
    let source: EstimateSource
    let processingMode: String
    let providerName: String
    let modelName: String
    let modelVersion: String
    let method: String
    let limitations: String
    let createdAt: Date
}

enum WeightEstimationError: LocalizedError, Equatable {
    case modelUnavailable
    case unsupportedModel(String)
    case invalidImage
    case invalidPrediction
    case providerNotConfigured
    case invalidEndpoint
    case networkUnavailable
    case requestFailed(statusCode: Int)
    case cancelled

    var errorDescription: String? {
        switch self {
        case .modelUnavailable:
            "On-device cattle-weight estimation is unavailable. Install an independently validated, compatible cattle-weight Core ML model in Settings."
        case .unsupportedModel(let reason): "The installed model cannot estimate cattle weight: \(reason)"
        case .invalidImage: "The selected image could not be decoded or exceeds the supported size."
        case .invalidPrediction: "The selected model did not return a valid weight estimate."
        case .providerNotConfigured: "Add a supported vision provider and API key in Settings."
        case .invalidEndpoint: "The provider endpoint is invalid. Hosted endpoints must use HTTPS."
        case .networkUnavailable: "The cloud provider could not be reached. Check the internet connection and try again."
        case .requestFailed(let status): "The provider returned HTTP \(status). Check the endpoint, model, and API key."
        case .cancelled: "The estimate was cancelled."
        }
    }
}

protocol WeightEstimationProvider {
    func estimate(image: Data, context: AnimalContext) async throws -> WeightEstimationResult
}
