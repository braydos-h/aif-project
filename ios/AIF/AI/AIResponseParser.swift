import Foundation

struct ParsedWeightPrediction: Equatable {
    let weightKg: Double
    let lowerBoundKg: Double?
    let upperBoundKg: Double?
}

enum AIResponseParserError: LocalizedError, Equatable {
    case missingWeight
    case invalidWeight
    case invalidRange

    var errorDescription: String? {
        switch self {
        case .missingWeight: "The provider response did not include a weight_kg value."
        case .invalidWeight: "The provider returned an invalid weight value."
        case .invalidRange: "The provider returned an invalid weight range."
        }
    }
}

enum AIResponseParser {
    private struct Payload: Decodable {
        var weightKg: Double?
        var estimatedWeightKg: Double?
        var lowerBoundKg: Double?
        var upperBoundKg: Double?

        enum CodingKeys: String, CodingKey {
            case weightKg = "weight_kg"
            case estimatedWeightKg = "estimated_weight_kg"
            case lowerBoundKg = "lower_bound_kg"
            case upperBoundKg = "upper_bound_kg"
        }
    }

    static func parse(_ text: String) throws -> ParsedWeightPrediction {
        let json = try extractJSONObject(from: text)
        let payload: Payload
        do { payload = try JSONDecoder().decode(Payload.self, from: Data(json.utf8)) }
        catch { throw AIResponseParserError.missingWeight }
        guard let value = payload.weightKg ?? payload.estimatedWeightKg else {
            throw AIResponseParserError.missingWeight
        }
        guard value.isFinite, value > 0 else { throw AIResponseParserError.invalidWeight }

        let low = payload.lowerBoundKg
        let high = payload.upperBoundKg
        guard low == nil && high == nil || (low != nil && high != nil) else {
            throw AIResponseParserError.invalidRange
        }
        if let low, let high {
            guard low.isFinite, high.isFinite, low > 0, high >= low else {
                throw AIResponseParserError.invalidRange
            }
            return ParsedWeightPrediction(weightKg: value, lowerBoundKg: low, upperBoundKg: high)
        }
        return ParsedWeightPrediction(weightKg: value, lowerBoundKg: nil, upperBoundKg: nil)
    }

    private static func extractJSONObject(from text: String) throws -> String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.first == "{", trimmed.last == "}" { return trimmed }
        guard let start = trimmed.firstIndex(of: "{") else { throw AIResponseParserError.missingWeight }
        var depth = 0
        var inString = false
        var escaped = false
        var index = start
        while index < trimmed.endIndex {
            let character = trimmed[index]
            if inString {
                if escaped { escaped = false }
                else if character == "\\" { escaped = true }
                else if character == "\"" { inString = false }
                trimmed.formIndex(after: &index)
                continue
            }
            if character == "\"" { inString = true }
            else if character == "{" { depth += 1 }
            else if character == "}" {
                depth -= 1
                if depth == 0 { return String(trimmed[start...index]) }
            }
            trimmed.formIndex(after: &index)
        }
        throw AIResponseParserError.missingWeight
    }
}
