import XCTest
@testable import AIF

final class ProviderConfigurationTests: XCTestCase {
    func testAcceptsHostedHTTPSAndLoopbackHTTP() throws {
        XCTAssertEqual(try ProviderConfigurationValidator.validate(endpoint: "https://api.example.com/v1/chat/completions", modelName: "vision" ).scheme, "https")
        XCTAssertEqual(try ProviderConfigurationValidator.validate(endpoint: "http://localhost:11434/v1/chat/completions", modelName: "vision").scheme, "http")
        XCTAssertEqual(try ProviderConfigurationValidator.validate(endpoint: "http://127.0.0.1:1234/v1", modelName: "vision").host, "127.0.0.1")
    }

    func testRejectsInsecureRemoteAndMalformedEndpoints() {
        XCTAssertThrowsError(try ProviderConfigurationValidator.validate(endpoint: "http://api.example.com/v1", modelName: "vision"))
        XCTAssertThrowsError(try ProviderConfigurationValidator.validate(endpoint: "file:///tmp/key", modelName: "vision"))
        XCTAssertThrowsError(try ProviderConfigurationValidator.validate(endpoint: "https://user:pass@example.com/v1", modelName: "vision"))
        XCTAssertThrowsError(try ProviderConfigurationValidator.validate(endpoint: "https://example.com/v1?api_key=secret", modelName: "vision"))
        XCTAssertThrowsError(try ProviderConfigurationValidator.validate(endpoint: "https://example.com/v1#fragment", modelName: "vision"))
        XCTAssertThrowsError(try ProviderConfigurationValidator.validate(endpoint: "https://example.com/v1", modelName: "  "))
    }

    func testLocalAIUnavailableIsAnExplicitOfflineError() async {
        let provider = CoreMLWeightEstimationProvider(modelInfoProvider: { nil })
        do {
            _ = try await provider.estimate(image: Data(), context: AnimalContext())
            XCTFail("No installed model must never create an estimate")
        } catch let error as WeightEstimationError {
            XCTAssertEqual(error, .modelUnavailable)
        } catch {
            XCTFail("Expected modelUnavailable, received \(error)")
        }
    }
}
