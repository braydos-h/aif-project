import Foundation
import SwiftData

@Model
final class TapeMeasurement {
    @Attribute(.unique) var id: UUID
    var heartGirthCm: Double
    var bodyLengthCm: Double
    var estimateKg: Double
    var measuredAt: Date
    var relatedEstimateID: UUID?
    var animal: Animal?

    init(
        id: UUID = UUID(),
        heartGirthCm: Double,
        bodyLengthCm: Double,
        estimateKg: Double,
        measuredAt: Date = .now,
        relatedEstimateID: UUID? = nil,
        animal: Animal? = nil
    ) {
        self.id = id
        self.heartGirthCm = heartGirthCm
        self.bodyLengthCm = bodyLengthCm
        self.estimateKg = estimateKg
        self.measuredAt = measuredAt
        self.relatedEstimateID = relatedEstimateID
        self.animal = animal
    }
}
