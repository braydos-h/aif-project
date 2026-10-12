import Foundation
import SwiftData

enum EstimateStore {
    static func updateTape(_ measurement: TapeMeasurement, heartGirthCm: Double, bodyLengthCm: Double,
                           context: ModelContext) throws {
        let result = try TapeEstimator.estimate(heartGirthCm: heartGirthCm, bodyLengthCm: bodyLengthCm)
        measurement.heartGirthCm = heartGirthCm
        measurement.bodyLengthCm = bodyLengthCm
        measurement.estimateKg = result.weightKg
        if let id = measurement.relatedEstimateID,
           let estimate = try context.fetch(FetchDescriptor<WeightEstimate>()).first(where: { $0.id == id }) {
            estimate.estimatedWeightKg = result.weightKg
            estimate.lowerBoundKg = result.lowerBoundKg
            estimate.upperBoundKg = result.upperBoundKg
            estimate.heartGirthCm = heartGirthCm
            estimate.bodyLengthCm = bodyLengthCm
        }
        do { try context.save() }
        catch { context.rollback(); throw error }
    }

    static func saveTape(_ tape: TapeEstimate, heartGirthCm: Double, bodyLengthCm: Double,
                         animal: Animal?, context: ModelContext, at date: Date = .now) throws -> WeightEstimate {
        let estimate = WeightEstimate(
            estimatedWeightKg: tape.weightKg,
            lowerBoundKg: tape.lowerBoundKg,
            upperBoundKg: tape.upperBoundKg,
            source: .tape,
            processingMode: "offline",
            providerName: "Schaeffer tape formula",
            modelName: "Metric Schaeffer",
            modelVersion: "1",
            method: "(heart girth cm² × body length cm) / 10838",
            limitations: "Tape estimates vary with measurement technique, posture, breed, and body condition. Use a scale for verified weight. Do not dose medication from this estimate.",
            createdAt: date,
            heartGirthCm: heartGirthCm,
            bodyLengthCm: bodyLengthCm,
            animal: animal
        )
        let measurement = TapeMeasurement(heartGirthCm: heartGirthCm, bodyLengthCm: bodyLengthCm,
                                          estimateKg: tape.weightKg, measuredAt: date,
                                          relatedEstimateID: estimate.id, animal: animal)
        context.insert(estimate)
        context.insert(measurement)
        do { try context.save() }
        catch { context.rollback(); throw error }
        return estimate
    }

    static func saveAI(_ result: WeightEstimationResult, originalPhotoData: Data, photo: ProcessedImage, animal: Animal?,
                       context: ModelContext, contextInfo: AnimalContext) throws -> WeightEstimate {
        let photoRecord = try PhotoStore.saveEstimate(originalData: originalPhotoData, processed: photo,
                                                      animalID: animal?.id, context: context)
        let estimate = WeightEstimate(
            estimatedWeightKg: result.estimatedWeightKg,
            lowerBoundKg: result.lowerBoundKg,
            upperBoundKg: result.upperBoundKg,
            source: result.source,
            processingMode: result.processingMode,
            providerName: result.providerName,
            modelName: result.modelName,
            modelVersion: result.modelVersion,
            method: result.method,
            limitations: result.limitations,
            createdAt: result.createdAt,
            photoID: photoRecord.id,
            breedContext: contextInfo.breed,
            sexContext: contextInfo.sex,
            ageContextYears: contextInfo.approximateAgeYears,
            animal: animal
        )
        context.insert(estimate)
        do { try context.save() }
        catch { PhotoStore.delete(photoRecord); context.delete(photoRecord); context.rollback(); throw error }
        return estimate
    }

    static func saveScale(weightKg: Double, measuredAt: Date = .now, note: String = "", animal: Animal?,
                          context: ModelContext) throws -> ScaleMeasurement {
        guard weightKg.isFinite, weightKg > 0 else { throw ScaleMeasurementError.invalidWeight }
        let record = ScaleMeasurement(weightKg: weightKg, measuredAt: measuredAt, note: note, animal: animal)
        context.insert(record)
        do { try context.save() }
        catch { context.rollback(); throw error }
        return record
    }
}

enum ScaleMeasurementError: LocalizedError {
    case invalidWeight
    var errorDescription: String? { "Enter a valid scale weight greater than zero." }
}
