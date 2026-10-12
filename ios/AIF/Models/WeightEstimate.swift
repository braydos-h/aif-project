import Foundation
import SwiftData

enum EstimateSource: String, CaseIterable, Identifiable, Codable {
    case tape, localModel, cloudModel

    var id: String { rawValue }
    var title: String {
        switch self {
        case .tape: "Tape estimate"
        case .localModel: "On-device model"
        case .cloudModel: "Cloud AI estimate"
        }
    }
}

@Model
final class WeightEstimate {
    @Attribute(.unique) var id: UUID
    var estimatedWeightKg: Double
    var lowerBoundKg: Double?
    var upperBoundKg: Double?
    var sourceRawValue: String
    var processingMode: String
    var providerName: String
    var modelName: String
    var modelVersion: String
    var method: String
    var limitations: String
    var createdAt: Date
    var photoID: UUID?
    var breedContext: String
    var sexContext: String
    var ageContextYears: Double?
    var heartGirthCm: Double?
    var bodyLengthCm: Double?
    var animal: Animal?

    init(
        id: UUID = UUID(),
        estimatedWeightKg: Double,
        lowerBoundKg: Double? = nil,
        upperBoundKg: Double? = nil,
        source: EstimateSource,
        processingMode: String,
        providerName: String,
        modelName: String,
        modelVersion: String,
        method: String,
        limitations: String,
        createdAt: Date = .now,
        photoID: UUID? = nil,
        breedContext: String = "",
        sexContext: String = "unknown",
        ageContextYears: Double? = nil,
        heartGirthCm: Double? = nil,
        bodyLengthCm: Double? = nil,
        animal: Animal? = nil
    ) {
        self.id = id
        self.estimatedWeightKg = estimatedWeightKg
        self.lowerBoundKg = lowerBoundKg
        self.upperBoundKg = upperBoundKg
        self.sourceRawValue = source.rawValue
        self.processingMode = processingMode
        self.providerName = providerName
        self.modelName = modelName
        self.modelVersion = modelVersion
        self.method = method
        self.limitations = limitations
        self.createdAt = createdAt
        self.photoID = photoID
        self.breedContext = breedContext
        self.sexContext = sexContext
        self.ageContextYears = ageContextYears
        self.heartGirthCm = heartGirthCm
        self.bodyLengthCm = bodyLengthCm
        self.animal = animal
    }

    var source: EstimateSource { EstimateSource(rawValue: sourceRawValue) ?? .tape }
}
