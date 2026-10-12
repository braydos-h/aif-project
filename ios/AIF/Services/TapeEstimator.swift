import Foundation

struct TapeEstimate: Equatable, Sendable {
    let weightKg: Double
    let lowerBoundKg: Double
    let upperBoundKg: Double

    var weightLb: Double { UnitConverter.pounds(fromKilograms: weightKg) }
    var lowerBoundLb: Double { UnitConverter.pounds(fromKilograms: lowerBoundKg) }
    var upperBoundLb: Double { UnitConverter.pounds(fromKilograms: upperBoundKg) }
}

enum TapeEstimatorError: LocalizedError, Equatable {
    case invalidMeasurement
    case outOfBounds
    case resultOutOfBounds

    var errorDescription: String? {
        switch self {
        case .invalidMeasurement: "Enter valid heart-girth and body-length measurements."
        case .outOfBounds: "Both measurements must be between 50 and 300 cm."
        case .resultOutOfBounds: "The measurement result could not be calculated. Check both values."
        }
    }
}

enum TapeEstimator {
    static func estimate(heartGirthCm: Double, bodyLengthCm: Double) throws -> TapeEstimate {
        guard heartGirthCm.isFinite, bodyLengthCm.isFinite,
              heartGirthCm > 0, bodyLengthCm > 0 else {
            throw TapeEstimatorError.invalidMeasurement
        }
        guard (50...300).contains(heartGirthCm), (50...300).contains(bodyLengthCm) else {
            throw TapeEstimatorError.outOfBounds
        }
        let kilograms = (heartGirthCm * heartGirthCm * bodyLengthCm) / 10_838
        guard kilograms.isFinite, kilograms > 0 else { throw TapeEstimatorError.resultOutOfBounds }
        return TapeEstimate(weightKg: kilograms, lowerBoundKg: kilograms * 0.95, upperBoundKg: kilograms * 1.05)
    }
}
