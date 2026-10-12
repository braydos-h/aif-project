import Foundation
import SwiftData

enum WeightUnit: String, CaseIterable, Identifiable, Codable {
    case kilograms, pounds
    var id: String { rawValue }
    var title: String { self == .kilograms ? "Kilograms (kg)" : "Pounds (lb)" }
    var abbreviation: String { self == .kilograms ? "kg" : "lb" }

    func value(fromKilograms kilograms: Double) -> Double {
        self == .kilograms ? kilograms : UnitConverter.pounds(fromKilograms: kilograms)
    }

    func formatted(_ kilograms: Double) -> String {
        "\(value(fromKilograms: kilograms).weightText) \(abbreviation)"
    }
}

enum AppAppearance: String, CaseIterable, Identifiable, Codable {
    case system, light, dark
    var id: String { rawValue }
    var title: String { rawValue.capitalized }
}

enum InferenceMode: String, CaseIterable, Identifiable, Codable {
    case local, cloud
    var id: String { rawValue }
    var title: String { self == .local ? "Local AI" : "Bring Your Own Key" }
}

@Model
final class ApplicationPreferences {
    @Attribute(.unique) var id: String
    var weightUnitRawValue: String
    var appearanceRawValue: String
    var inferenceModeRawValue: String
    var includePounds: Bool

    init(
        id: String = "primary",
        weightUnit: WeightUnit = .kilograms,
        appearance: AppAppearance = .system,
        inferenceMode: InferenceMode = .local,
        includePounds: Bool = true
    ) {
        self.id = id
        self.weightUnitRawValue = weightUnit.rawValue
        self.appearanceRawValue = appearance.rawValue
        self.inferenceModeRawValue = inferenceMode.rawValue
        self.includePounds = includePounds
    }

    var weightUnit: WeightUnit {
        get { WeightUnit(rawValue: weightUnitRawValue) ?? .kilograms }
        set { weightUnitRawValue = newValue.rawValue }
    }
    var appearance: AppAppearance {
        get { AppAppearance(rawValue: appearanceRawValue) ?? .system }
        set { appearanceRawValue = newValue.rawValue }
    }
    var inferenceMode: InferenceMode {
        get { InferenceMode(rawValue: inferenceModeRawValue) ?? .local }
        set { inferenceModeRawValue = newValue.rawValue }
    }
}
