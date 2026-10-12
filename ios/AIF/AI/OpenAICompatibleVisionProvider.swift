import Foundation

struct CloudProviderSettings: Sendable {
    var name: String
    var endpoint: String
    var modelName: String
}

struct OpenAICompatibleVisionProvider: WeightEstimationProvider {
    let settings: CloudProviderSettings
    let apiKey: String
    var session: URLSession = ProviderURLSessions.noRedirect

    func estimate(image: Data, context: AnimalContext) async throws -> WeightEstimationResult {
        let url = try ProviderConfigurationValidator.validate(endpoint: settings.endpoint, modelName: settings.modelName)
        guard !apiKey.isEmpty else { throw WeightEstimationError.providerNotConfigured }
        let cleanImage: Data
        do { cleanImage = try ImageProcessingService.prepare(image).data }
        catch { throw WeightEstimationError.invalidImage }
        let base64 = cleanImage.base64EncodedString()
        let contextLine = [
            context.breed.isEmpty ? nil : "Breed: \(context.breed).",
            context.sex == "unknown" ? nil : "Sex: \(context.sex).",
            context.approximateAgeYears.map { "Approximate age: \($0) years." }
        ].compactMap { $0 }.joined(separator: " ")
        let prompt = "Estimate the cattle animal's body weight from this photo. Return only JSON with weight_kg as a positive number. If your method supports a weight interval, also include lower_bound_kg and upper_bound_kg. Do not return confidence scores. This is an estimate, not a verified scale measurement. \(contextLine)"
        let body: [String: Any] = [
            "model": settings.modelName,
            "temperature": 0,
            "messages": [[
                "role": "user",
                "content": [
                    ["type": "text", "text": prompt],
                    ["type": "image_url", "image_url": ["url": "data:image/jpeg;base64,\(base64)"]]
                ]
            ]]
        ]
        var request = URLRequest(url: url, timeoutInterval: 45)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        do { request.httpBody = try JSONSerialization.data(withJSONObject: body) }
        catch { throw WeightEstimationError.invalidPrediction }
        let responseData: Data
        let response: URLResponse
        do { (responseData, response) = try await session.data(for: request) }
        catch is CancellationError { throw WeightEstimationError.cancelled }
        catch let error as URLError where error.code == .cancelled { throw WeightEstimationError.cancelled }
        catch { throw WeightEstimationError.networkUnavailable }
        guard responseData.count <= 1_048_576 else { throw WeightEstimationError.invalidPrediction }
        guard let http = response as? HTTPURLResponse else { throw WeightEstimationError.networkUnavailable }
        guard (200..<300).contains(http.statusCode) else { throw WeightEstimationError.requestFailed(statusCode: http.statusCode) }
        let text = try Self.messageText(from: responseData)
        let parsed = try AIResponseParser.parse(text)
        return WeightEstimationResult(
            estimatedWeightKg: parsed.weightKg,
            lowerBoundKg: parsed.lowerBoundKg,
            upperBoundKg: parsed.upperBoundKg,
            source: .cloudModel,
            processingMode: "cloud",
            providerName: settings.name,
            modelName: settings.modelName,
            modelVersion: "provider-managed",
            method: "User-initiated OpenAI-compatible vision API request",
            limitations: "Cloud model estimate. The selected photo and animal context were sent to your configured provider over the internet. Verify with a scale; do not dose medication from an estimate.",
            createdAt: .now
        )
    }

    static func testConnection(settings: CloudProviderSettings, apiKey: String) async throws {
        let url = try ProviderConfigurationValidator.validate(endpoint: settings.endpoint, modelName: settings.modelName)
        guard !apiKey.isEmpty else { throw WeightEstimationError.providerNotConfigured }
        let body: [String: Any] = [
            "model": settings.modelName,
            "messages": [["role": "user", "content": "Reply with the word connected." ]],
            "max_tokens": 16
        ]
        var request = URLRequest(url: url, timeoutInterval: 20)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        let (_, response): (Data, URLResponse)
        do { (_, response) = try await ProviderURLSessions.noRedirect.data(for: request) }
        catch is CancellationError { throw WeightEstimationError.cancelled }
        catch let error as URLError where error.code == .cancelled { throw WeightEstimationError.cancelled }
        catch { throw WeightEstimationError.networkUnavailable }
        guard let http = response as? HTTPURLResponse else { throw WeightEstimationError.networkUnavailable }
        guard (200..<300).contains(http.statusCode) else { throw WeightEstimationError.requestFailed(statusCode: http.statusCode) }
    }

    private static func messageText(from data: Data) throws -> String {
        guard let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let choices = root["choices"] as? [[String: Any]],
              let message = choices.first?["message"] as? [String: Any],
              let content = message["content"] as? String else {
            throw WeightEstimationError.invalidPrediction
        }
        return content
    }
}

private final class NoRedirectDelegate: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask,
                    willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest,
                    completionHandler: @escaping (URLRequest?) -> Void) {
        completionHandler(nil)
    }
}

private enum ProviderURLSessions {
    static let noRedirect: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.urlCache = nil
        configuration.httpCookieStorage = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        return URLSession(configuration: configuration, delegate: NoRedirectDelegate(), delegateQueue: nil)
    }()
}
