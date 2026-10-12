import XCTest
@testable import AIF

final class TapeEstimatorTests: XCTestCase {
    func testMetricSchaefferCalculationAndNominalRange() throws {
        let result = try TapeEstimator.estimate(heartGirthCm: 180, bodyLengthCm: 150)
        XCTAssertEqual(result.weightKg, (180 * 180 * 150) / 10_838, accuracy: 0.000_001)
        XCTAssertEqual(result.lowerBoundKg, result.weightKg * 0.95, accuracy: 0.000_001)
        XCTAssertEqual(result.upperBoundKg, result.weightKg * 1.05, accuracy: 0.000_001)
    }

    func testInclusiveMeasurementBounds() throws {
        XCTAssertNoThrow(try TapeEstimator.estimate(heartGirthCm: 50, bodyLengthCm: 300))
        XCTAssertNoThrow(try TapeEstimator.estimate(heartGirthCm: 300, bodyLengthCm: 50))
        XCTAssertThrowsError(try TapeEstimator.estimate(heartGirthCm: 49.99, bodyLengthCm: 150))
        XCTAssertThrowsError(try TapeEstimator.estimate(heartGirthCm: 150, bodyLengthCm: 300.01))
    }

    func testRejectsInvalidAndNonFiniteMeasurements() {
        XCTAssertThrowsError(try TapeEstimator.estimate(heartGirthCm: .nan, bodyLengthCm: 150))
        XCTAssertThrowsError(try TapeEstimator.estimate(heartGirthCm: 0, bodyLengthCm: 150))
        XCTAssertThrowsError(try TapeEstimator.estimate(heartGirthCm: .infinity, bodyLengthCm: 150))
    }

    func testKilogramPoundConversions() {
        XCTAssertEqual(UnitConverter.pounds(fromKilograms: 1), 2.204_622_621_85, accuracy: 0.000_000_1)
        XCTAssertEqual(UnitConverter.kilograms(fromPounds: 1), 0.453_592_37, accuracy: 0.000_000_1)
        XCTAssertEqual(UnitConverter.kilograms(fromPounds: UnitConverter.pounds(fromKilograms: 612)), 612, accuracy: 0.000_001)
    }
}
