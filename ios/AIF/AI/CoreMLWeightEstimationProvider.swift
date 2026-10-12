import CoreML
import CoreGraphics
import Foundation
import Vision

struct CoreMLWeightEstimationProvider: WeightEstimationProvider {
    private let modelInfoProvider: () -> InstalledModelInfo?

    init(modelInfoProvider: @escaping () -> InstalledModelInfo? = LocalModelStore.currentModelInfo) {
        self.modelInfoProvider = modelInfoProvider
    }

    func estimate(image: Data, context: AnimalContext) async throws -> WeightEstimationResult {
        guard let info = modelInfoProvider() else { throw WeightEstimationError.modelUnavailable }
        return try await Task.detached(priority: .userInitiated) {
            try Self.performInference(image: image, info: info)
        }.value
    }

    private static func performInference(image: Data, info: InstalledModelInfo) throws -> WeightEstimationResult {
        guard let model = try? MLModel(contentsOf: info.location) else {
            throw WeightEstimationError.unsupportedModel("Core ML could not load the installed file.")
        }
        let visionModel: VNCoreMLModel
        do { visionModel = try VNCoreMLModel(for: model) }
        catch { throw WeightEstimationError.unsupportedModel("the model does not accept images through Vision.") }
        let input: CGImage
        do {
            guard let decoded = try ImageProcessingService.validate(image).cgImage else { throw WeightEstimationError.invalidImage }
            input = decoded
        } catch is ImageInputError { throw WeightEstimationError.invalidImage }
        let request = VNCoreMLRequest(model: visionModel)
        request.imageCropAndScaleOption = .scaleFit
        do { try VNImageRequestHandler(cgImage: input, options: [:]).perform([request]) }
        catch { throw WeightEstimationError.unsupportedModel("image preprocessing or inference failed.") }
        let observations = request.results as? [VNCoreMLFeatureValueObservation] ?? []
        let values = Dictionary(uniqueKeysWithValues: observations.map { ($0.featureName.lowercased(), $0.featureValue) })
        guard let weight = numericOutput(values, names: ["weight_kg", "estimated_weight_kg"]),
              weight.isFinite, weight > 0 else {
            throw WeightEstimationError.unsupportedModel("it must provide a positive numeric `weight_kg` or `estimated_weight_kg` output.")
        }
        let lower = numericOutput(values, names: ["lower_bound_kg", "weight_min_kg"])
        let upper = numericOutput(values, names: ["upper_bound_kg", "weight_max_kg"])
        guard (lower == nil && upper == nil) || (lower != nil && upper != nil),
              lower.map({ $0.isFinite && $0 > 0 }) ?? true,
              upper.map({ $0.isFinite && $0 > 0 }) ?? true,
              lower == nil || upper == nil || lower! <= upper! else {
            throw WeightEstimationError.invalidPrediction
        }
        return WeightEstimationResult(
            estimatedWeightKg: weight,
            lowerBoundKg: lower,
            upperBoundKg: upper,
            source: .localModel,
            processingMode: "local",
            providerName: "Apple Core ML / Vision",
            modelName: info.name,
            modelVersion: info.version,
            method: "On-device Core ML image inference",
            limitations: "Model-generated estimate. Accuracy depends on the supplied model and photo conditions. Verify with a scale; do not dose medication from an estimate.",
            createdAt: .now
        )
    }

    private static func numericOutput(_ outputs: [String: MLFeatureValue], names: [String]) -> Double? {
        for name in names {
            guard let value = outputs[name] else { continue }
            if value.type == .double { return value.doubleValue }
            if value.type == .int64 { return Double(value.int64Value) }
            if let array = value.multiArrayValue, array.count == 1 { return array[0].doubleValue }
        }
        return nil
    }
}
