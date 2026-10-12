import Foundation
import SwiftData

@Model
final class ScaleMeasurement {
    @Attribute(.unique) var id: UUID
    var weightKg: Double
    var measuredAt: Date
    var note: String
    var animal: Animal?

    init(id: UUID = UUID(), weightKg: Double, measuredAt: Date = .now, note: String = "", animal: Animal? = nil) {
        self.id = id
        self.weightKg = weightKg
        self.measuredAt = measuredAt
        self.note = note
        self.animal = animal
    }
}
