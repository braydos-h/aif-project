import Foundation
import XCTest
@testable import AIF

final class OpenAIProviderTests: XCTestCase {
    func testImplementedVisionProviderSendsOneInitiatedPhotoAndParsesTypedResult() async throws {
        let image = Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/JXcAAAAASUVORK5CYII=")!
        MockURLProtocol.requestHandler = { request in
            XCTAssertEqual(request.url?.host, "provider.test")
            XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "Bearer test-secret-never-log")
            let body = try XCTUnwrap(request.httpBody)
            let object = try XCTUnwrap(JSONSerialization.jsonObject(with: body) as? [String: Any])
            let messages = try XCTUnwrap(object["messages"] as? [[String: Any]])
            let content = try XCTUnwrap(messages.first?["content"] as? [[String: Any]])
            XCTAssertTrue(content.contains { ($0["type"] as? String) == "image_url" })
            let response = Data("{\"choices\":[{\"message\":{\"content\":\"{\\\"weight_kg\\\":520,\\\"lower_bound_kg\\\":480,\\\"upper_bound_kg\\\":560}\"}}]}".utf8)
            return (200, response)
        }
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [MockURLProtocol.self]
        let provider = OpenAICompatibleVisionProvider(
            settings: CloudProviderSettings(name: "Test provider", endpoint: "https://provider.test/v1/chat/completions", modelName: "vision-test"),
            apiKey: "test-secret-never-log",
            session: URLSession(configuration: configuration)
        )
        let result = try await provider.estimate(image: image, context: AnimalContext(breed: "Angus", sex: "cow", approximateAgeYears: 4))
        XCTAssertEqual(result.estimatedWeightKg, 520)
        XCTAssertEqual(result.lowerBoundKg, 480)
        XCTAssertEqual(result.upperBoundKg, 560)
        XCTAssertEqual(result.source, .cloudModel)
        XCTAssertEqual(result.processingMode, "cloud")
        XCTAssertTrue(result.limitations.contains("sent to your configured provider"))
        MockURLProtocol.requestHandler = nil
    }
}

private final class MockURLProtocol: URLProtocol {
    static var requestHandler: ((URLRequest) throws -> (Int, Data))?

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func startLoading() {
        do {
            guard let requestHandler = Self.requestHandler, let url = request.url else { throw URLError(.badServerResponse) }
            let (status, data) = try requestHandler(request)
            let response = try XCTUnwrap(HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: nil))
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: data)
            client?.urlProtocolDidFinishLoading(self)
        } catch { client?.urlProtocol(self, didFailWithError: error) }
    }
    override func stopLoading() { }
}
