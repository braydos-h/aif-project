import Foundation
import SwiftData

enum AnimalSex: String, CaseIterable, Identifiable, Codable {
    case cow, bull, steer, heifer, calf, unknown

    var id: String { rawValue }
    var title: String { rawValue.capitalized }
}

@Model
final class Animal {
    @Attribute(.unique) var id: UUID
    var name: String
    var tagNumber: String
    var breed: String
    var sexRawValue: String
    var birthDate: Date?
    var approximateAgeYears: Double
    var notes: String
    var profilePhotoID: UUID?
    var createdAt: Date
    var lastViewedAt: Date?
    var isArchived: Bool

    @Relationship(deleteRule: .cascade, inverse: \WeightEstimate.animal)
    var estimates: [WeightEstimate] = []
    @Relationship(deleteRule: .cascade, inverse: \TapeMeasurement.animal)
    var tapeMeasurements: [TapeMeasurement] = []
    @Relationship(deleteRule: .cascade, inverse: \ScaleMeasurement.animal)
    var scaleMeasurements: [ScaleMeasurement] = []

    init(
        id: UUID = UUID(),
        name: String = "",
        tagNumber: String = "",
        breed: String = "",
        sex: AnimalSex = .unknown,
        birthDate: Date? = nil,
        approximateAgeYears: Double = 0,
        notes: String = "",
        profilePhotoID: UUID? = nil,
        createdAt: Date = .now,
        lastViewedAt: Date? = nil,
        isArchived: Bool = false
    ) {
        self.id = id
        self.name = name
        self.tagNumber = tagNumber
        self.breed = breed
        self.sexRawValue = sex.rawValue
        self.birthDate = birthDate
        self.approximateAgeYears = approximateAgeYears
        self.notes = notes
        self.profilePhotoID = profilePhotoID
        self.createdAt = createdAt
        self.lastViewedAt = lastViewedAt
        self.isArchived = isArchived
    }

    var sex: AnimalSex {
        get { AnimalSex(rawValue: sexRawValue) ?? .unknown }
        set { sexRawValue = newValue.rawValue }
    }

    var displayName: String {
        let label = name.trimmingCharacters(in: .whitespacesAndNewlines)
        if !label.isEmpty { return label }
        let tag = tagNumber.trimmingCharacters(in: .whitespacesAndNewlines)
        return tag.isEmpty ? "Unnamed animal" : tag
    }

    var approximateAgeAtRecordYears: Double? {
        guard let birthDate else { return approximateAgeYears > 0 ? approximateAgeYears : nil }
        let days = Calendar.current.dateComponents([.day], from: birthDate, to: .now).day ?? 0
        return max(Double(days) / 365.2425, 0)
    }
}
