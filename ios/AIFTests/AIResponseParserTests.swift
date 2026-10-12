import XCTest
@testable import AIF

final class AIResponseParserTests: XCTestCase {
    func testParsesWeightAndOptionalModelRange() throws {
        let result = try AIResponseParser.parse("```json\n{\"weight_kg\": 612.5, \"lower_bound_kg\": 570, \"upper_bound_kg\": 650}\n```")
        XCTAssertEqual(result.weightKg, 612.5)
        XCTAssertEqual(result.lowerBoundKg, 570)
        XCTAssertEqual(result.upperBoundKg, 650)
    }

    func testRangeIsOptionalAndNeverSynthesized() throws {
        let result = try AIResponseParser.parse("{\"estimated_weight_kg\": 400}")
        XCTAssertEqual(result.weightKg, 400)
        XCTAssertNil(result.lowerBoundKg)
        XCTAssertNil(result.upperBoundKg)
    }

    func testRejectsMissingInvalidAndMalformedOutput() {
        XCTAssertThrowsError(try AIResponseParser.parse("{\"confidence\":0.99}"))
        XCTAssertThrowsError(try AIResponseParser.parse("{\"weight_kg\":-1}"))
        XCTAssertThrowsError(try AIResponseParser.parse("{\"weight_kg\":\"612\"}"))
        XCTAssertThrowsError(try AIResponseParser.parse("{\"weight_kg\":612,\"lower_bound_kg\":620,\"upper_bound_kg\":610}"))
        XCTAssertThrowsError(try AIResponseParser.parse("no JSON here"))
    }
}
