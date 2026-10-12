import Foundation
import SwiftData

struct BackupPhoto: Codable {
    var id: UUID
    var animalID: UUID?
    var width: Int
    var height: Int
    var jpegData: Data
    var analysisJPEGData: Data? = nil
}

struct BackupAnimal: Codable {
    var id: UUID
    var name: String
    var tagNumber: String
    var breed: String
    var sex: String
    var birthDate: Date?
    var approximateAgeYears: Double
    var notes: String
    var profilePhotoID: UUID?
    var createdAt: Date
    var isArchived: Bool
}

struct BackupEstimate: Codable {
    var id: UUID
    var estimatedWeightKg: Double
    var lowerBoundKg: Double?
    var upperBoundKg: Double?
    var source: String
    var processingMode: String
    var providerName: String
    var modelName: String
    var modelVersion: String
    var method: String
    var limitations: String
    var createdAt: Date
    var photoID: UUID?
    var animalID: UUID?
    var breedContext: String
    var sexContext: String
    var ageContextYears: Double?
    var heartGirthCm: Double?
    var bodyLengthCm: Double?
}

struct BackupTapeMeasurement: Codable {
    var id: UUID
    var heartGirthCm: Double
    var bodyLengthCm: Double
    var estimateKg: Double
    var measuredAt: Date
    var relatedEstimateID: UUID?
    var animalID: UUID?
}

struct BackupScaleMeasurement: Codable {
    var id: UUID
    var weightKg: Double
    var measuredAt: Date
    var note: String
    var animalID: UUID?
}

struct AIFBackup: Codable {
    var format: String
    var version: Int
    var exportedAt: Date
    var animals: [BackupAnimal]
    var estimates: [BackupEstimate]
    var tapeMeasurements: [BackupTapeMeasurement]
    var scaleMeasurements: [BackupScaleMeasurement]
    var photos: [BackupPhoto]
}

struct BackupImportResult: Equatable {
    var animalsAdded: Int
    var estimatesAdded: Int
    var measurementsAdded: Int
    var photosAdded: Int
    var duplicatesSkipped: Int
}

enum BackupServiceError: LocalizedError {
    case fileTooLarge
    case unsupportedFormat
    case tooManyRecords
    case duplicateIdentifiers
    case invalidRecord(String)

    var errorDescription: String? {
        switch self {
        case .fileTooLarge: "The backup is larger than the 512 MB import limit."
        case .unsupportedFormat: "This is not a supported AIF backup package."
        case .tooManyRecords: "The backup contains more records than this version can import safely."
        case .duplicateIdentifiers: "The backup contains duplicate record identifiers."
        case .invalidRecord(let name): "The backup contains an invalid \(name) record."
        }
    }
}

enum BackupService {
    static let maximumBackupBytes = 512 * 1_024 * 1_024
    private static let maximumRecords = 100_000

    static func jsonExport(context: ModelContext) throws -> Data {
        try encode(makeBackup(context: context, includePhotos: false))
    }

    static func fullBackup(context: ModelContext) throws -> Data {
        try encode(makeBackup(context: context, includePhotos: true))
    }

    static func importBackup(_ data: Data, context: ModelContext) throws -> BackupImportResult {
        guard data.count <= maximumBackupBytes else { throw BackupServiceError.fileTooLarge }
        let backup: AIFBackup
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        do { backup = try decoder.decode(AIFBackup.self, from: data) }
        catch { throw BackupServiceError.unsupportedFormat }
        guard backup.format == "com.aif.cattle-backup", backup.version == 1 else {
            throw BackupServiceError.unsupportedFormat
        }
        let recordCount = backup.animals.count + backup.estimates.count + backup.tapeMeasurements.count
            + backup.scaleMeasurements.count + backup.photos.count
        guard recordCount <= maximumRecords else { throw BackupServiceError.tooManyRecords }
        try validateUniqueIDs(backup)
        try validateRecords(backup)

        let currentAnimals = try context.fetch(FetchDescriptor<Animal>())
        let currentEstimates = try context.fetch(FetchDescriptor<WeightEstimate>())
        let currentTapes = try context.fetch(FetchDescriptor<TapeMeasurement>())
        let currentScales = try context.fetch(FetchDescriptor<ScaleMeasurement>())
        let currentPhotos = try context.fetch(FetchDescriptor<StoredPhoto>())
        var animalsByID = Dictionary(uniqueKeysWithValues: currentAnimals.map { ($0.id, $0) })
        var estimateIDs = Set(currentEstimates.map(\.id))
        var tapeIDs = Set(currentTapes.map(\.id))
        var scaleIDs = Set(currentScales.map(\.id))
        var photoIDs = Set(currentPhotos.map(\.id))
        let duplicates = backup.animals.filter { animalsByID[$0.id] != nil }.count
            + backup.estimates.filter { estimateIDs.contains($0.id) }.count
            + backup.tapeMeasurements.filter { tapeIDs.contains($0.id) }.count
            + backup.scaleMeasurements.filter { scaleIDs.contains($0.id) }.count
            + backup.photos.filter { photoIDs.contains($0.id) }.count
        var addedAnimals = 0
        var addedEstimates = 0
        var addedMeasurements = 0
        var addedPhotos = 0
        var createdPhotoFiles: [StoredPhoto] = []

        do {
            for item in backup.animals where animalsByID[item.id] == nil {
                let animal = Animal(id: item.id, name: item.name, tagNumber: item.tagNumber, breed: item.breed,
                                    sex: AnimalSex(rawValue: item.sex) ?? .unknown, birthDate: item.birthDate,
                                    approximateAgeYears: item.approximateAgeYears, notes: item.notes,
                                    profilePhotoID: item.profilePhotoID, createdAt: item.createdAt,
                                    isArchived: item.isArchived)
                context.insert(animal)
                animalsByID[item.id] = animal
                addedAnimals += 1
            }
            for item in backup.photos where !photoIDs.contains(item.id) {
                let photo: StoredPhoto
                if let analysis = item.analysisJPEGData {
                    let processed = try ImageProcessingService.prepare(analysis)
                    photo = try PhotoStore.saveEstimate(originalData: item.jpegData, processed: processed,
                                                        id: item.id, animalID: item.animalID, context: context)
                } else {
                    let original = try ImageProcessingService.archive(item.jpegData)
                    photo = try PhotoStore.save(original, id: item.id, animalID: item.animalID, context: context)
                }
                createdPhotoFiles.append(photo)
                photoIDs.insert(item.id)
                addedPhotos += 1
            }
            for item in backup.estimates where !estimateIDs.contains(item.id) {
                let estimate = WeightEstimate(
                    id: item.id, estimatedWeightKg: item.estimatedWeightKg,
                    lowerBoundKg: item.lowerBoundKg, upperBoundKg: item.upperBoundKg,
                    source: EstimateSource(rawValue: item.source) ?? .tape,
                    processingMode: item.processingMode, providerName: item.providerName,
                    modelName: item.modelName, modelVersion: item.modelVersion,
                    method: item.method, limitations: item.limitations, createdAt: item.createdAt,
                    photoID: item.photoID, breedContext: item.breedContext, sexContext: item.sexContext,
                    ageContextYears: item.ageContextYears, heartGirthCm: item.heartGirthCm,
                    bodyLengthCm: item.bodyLengthCm, animal: item.animalID.flatMap { animalsByID[$0] }
                )
                context.insert(estimate)
                estimateIDs.insert(item.id)
                addedEstimates += 1
            }
            for item in backup.tapeMeasurements where !tapeIDs.contains(item.id) {
                context.insert(TapeMeasurement(id: item.id, heartGirthCm: item.heartGirthCm,
                                               bodyLengthCm: item.bodyLengthCm, estimateKg: item.estimateKg,
                                               measuredAt: item.measuredAt, relatedEstimateID: item.relatedEstimateID,
                                               animal: item.animalID.flatMap { animalsByID[$0] }))
                tapeIDs.insert(item.id)
                addedMeasurements += 1
            }
            for item in backup.scaleMeasurements where !scaleIDs.contains(item.id) {
                context.insert(ScaleMeasurement(id: item.id, weightKg: item.weightKg,
                                                measuredAt: item.measuredAt, note: item.note,
                                                animal: item.animalID.flatMap { animalsByID[$0] }))
                scaleIDs.insert(item.id)
                addedMeasurements += 1
            }
            try context.save()
        } catch {
            context.rollback()
            createdPhotoFiles.forEach(PhotoStore.delete)
            throw error
        }
        return BackupImportResult(animalsAdded: addedAnimals, estimatesAdded: addedEstimates,
                                  measurementsAdded: addedMeasurements, photosAdded: addedPhotos,
                                  duplicatesSkipped: duplicates)
    }

    private static func makeBackup(context: ModelContext, includePhotos: Bool) throws -> AIFBackup {
        let animals = try context.fetch(FetchDescriptor<Animal>())
        let estimates = try context.fetch(FetchDescriptor<WeightEstimate>())
        let tapes = try context.fetch(FetchDescriptor<TapeMeasurement>())
        let scales = try context.fetch(FetchDescriptor<ScaleMeasurement>())
        let storedPhotos = try context.fetch(FetchDescriptor<StoredPhoto>())
        let storedPhotoIDs = Set(storedPhotos.map(\.id))
        let photoRecords: [BackupPhoto]
        if includePhotos {
            let photoBytes = storedPhotos.reduce(Int64(0)) { $0 + Int64($1.byteCount) }
            guard photoBytes <= Int64(maximumBackupBytes * 3 / 4) else { throw BackupServiceError.fileTooLarge }
            var records: [BackupPhoto] = []
            for item in storedPhotos {
                guard let bytes = PhotoStore.read(item) else { throw BackupServiceError.invalidRecord("photo asset") }
                records.append(BackupPhoto(id: item.id, animalID: item.animalID, width: item.pixelWidth,
                                           height: item.pixelHeight, jpegData: bytes,
                                           analysisJPEGData: PhotoStore.readProcessed(item)))
            }
            photoRecords = records
        } else {
            photoRecords = []
        }
        return AIFBackup(
            format: "com.aif.cattle-backup", version: 1, exportedAt: .now,
            animals: animals.map {
                BackupAnimal(id: $0.id, name: $0.name, tagNumber: $0.tagNumber, breed: $0.breed,
                             sex: $0.sexRawValue, birthDate: $0.birthDate, approximateAgeYears: $0.approximateAgeYears,
                             notes: $0.notes,
                             profilePhotoID: $0.profilePhotoID.flatMap { id in
                                 includePhotos && storedPhotoIDs.contains(id) ? id : nil
                             },
                             createdAt: $0.createdAt,
                             isArchived: $0.isArchived)
            },
            estimates: estimates.map {
                BackupEstimate(id: $0.id, estimatedWeightKg: $0.estimatedWeightKg, lowerBoundKg: $0.lowerBoundKg,
                               upperBoundKg: $0.upperBoundKg, source: $0.sourceRawValue, processingMode: $0.processingMode,
                               providerName: $0.providerName, modelName: $0.modelName, modelVersion: $0.modelVersion,
                               method: $0.method, limitations: $0.limitations, createdAt: $0.createdAt,
                               photoID: $0.photoID.flatMap { id in
                                   includePhotos && storedPhotoIDs.contains(id) ? id : nil
                               },
                               animalID: $0.animal?.id, breedContext: $0.breedContext,
                               sexContext: $0.sexContext, ageContextYears: $0.ageContextYears,
                               heartGirthCm: $0.heartGirthCm, bodyLengthCm: $0.bodyLengthCm)
            },
            tapeMeasurements: tapes.map {
                BackupTapeMeasurement(id: $0.id, heartGirthCm: $0.heartGirthCm, bodyLengthCm: $0.bodyLengthCm,
                                      estimateKg: $0.estimateKg, measuredAt: $0.measuredAt,
                                      relatedEstimateID: $0.relatedEstimateID, animalID: $0.animal?.id)
            },
            scaleMeasurements: scales.map {
                BackupScaleMeasurement(id: $0.id, weightKg: $0.weightKg, measuredAt: $0.measuredAt,
                                       note: $0.note, animalID: $0.animal?.id)
            },
            photos: photoRecords
        )
    }

    private static func encode(_ backup: AIFBackup) throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        let data = try encoder.encode(backup)
        guard data.count <= maximumBackupBytes else { throw BackupServiceError.fileTooLarge }
        return data
    }

    private static func validateUniqueIDs(_ backup: AIFBackup) throws {
        func unique<T>(_ items: [T], id: (T) -> UUID) -> Bool {
            Set(items.map(id)).count == items.count
        }
        guard unique(backup.animals, id: \.id), unique(backup.estimates, id: \.id),
              unique(backup.tapeMeasurements, id: \.id), unique(backup.scaleMeasurements, id: \.id),
              unique(backup.photos, id: \.id) else { throw BackupServiceError.duplicateIdentifiers }
    }

    private static func validateRecords(_ backup: AIFBackup) throws {
        let animalIDs = Set(backup.animals.map(\.id))
        let estimateIDs = Set(backup.estimates.map(\.id))
        let photoIDs = Set(backup.photos.map(\.id))
        for item in backup.animals where !item.approximateAgeYears.isFinite || item.approximateAgeYears < 0 || item.approximateAgeYears > 100 {
            throw BackupServiceError.invalidRecord("animal")
        }
        for item in backup.animals where item.profilePhotoID != nil && !photoIDs.contains(item.profilePhotoID!) {
            throw BackupServiceError.invalidRecord("animal photo relationship")
        }
        for item in backup.estimates {
            guard EstimateSource(rawValue: item.source) != nil,
                  item.estimatedWeightKg.isFinite, item.estimatedWeightKg > 0,
                  item.animalID == nil || animalIDs.contains(item.animalID!),
                  item.photoID == nil || photoIDs.contains(item.photoID!) else {
                throw BackupServiceError.invalidRecord("weight estimate")
            }
            let lower = item.lowerBoundKg
            let upper = item.upperBoundKg
            guard lower == nil && upper == nil || (lower != nil && upper != nil),
                  lower.map({ $0.isFinite && $0 > 0 }) ?? true,
                  upper.map({ $0.isFinite && $0 > 0 }) ?? true,
                  lower == nil || upper == nil || lower! <= upper! else {
                throw BackupServiceError.invalidRecord("weight range")
            }
        }
        for item in backup.tapeMeasurements {
            do {
                let result = try TapeEstimator.estimate(heartGirthCm: item.heartGirthCm, bodyLengthCm: item.bodyLengthCm)
                guard item.estimateKg.isFinite, abs(result.weightKg - item.estimateKg) < 0.01 else {
                    throw BackupServiceError.invalidRecord("tape estimate")
                }
            }
            catch { throw BackupServiceError.invalidRecord("tape measurement") }
            guard item.animalID == nil || animalIDs.contains(item.animalID!),
                  item.relatedEstimateID == nil || estimateIDs.contains(item.relatedEstimateID!) else {
                throw BackupServiceError.invalidRecord("tape relationship")
            }
        }
        for item in backup.scaleMeasurements {
            guard item.weightKg.isFinite, item.weightKg > 0,
                  item.animalID == nil || animalIDs.contains(item.animalID!) else {
                throw BackupServiceError.invalidRecord("scale measurement")
            }
        }
        for item in backup.photos {
            guard item.animalID == nil || animalIDs.contains(item.animalID!) else {
                throw BackupServiceError.invalidRecord("photo relationship")
            }
            do { _ = try ImageProcessingService.prepare(item.jpegData) }
            catch { throw BackupServiceError.invalidRecord("photo") }
            if let analysis = item.analysisJPEGData {
                do { _ = try ImageProcessingService.prepare(analysis) }
                catch { throw BackupServiceError.invalidRecord("processed photo") }
            }
        }
    }
}

enum HistoryExportService {
    static func csv(estimates: [WeightEstimate], scales: [ScaleMeasurement]) -> Data {
        let estimateRows = estimates.map { item in
            (item.createdAt,
             [date(item.createdAt), csvCell(item.animal?.displayName ?? ""), csvCell(item.source.title),
             csvCell(item.method), number(item.estimatedWeightKg), optionalNumber(item.lowerBoundKg),
             optionalNumber(item.upperBoundKg), csvCell(item.providerName), csvCell(item.modelName),
             csvCell(item.processingMode)])
        }
        let scaleRows = scales.map { item in
            (item.measuredAt,
             [date(item.measuredAt), csvCell(item.animal?.displayName ?? ""), "Verified scale",
              csvCell(item.note.isEmpty ? "Scale measurement" : item.note), number(item.weightKg), "", "", "", "", "offline"])
        }
        let rows = (estimateRows + scaleRows).sorted { $0.0 > $1.0 }.map { $0.1.joined(separator: ",") }
        let output = ["date,animal,source,method,weight_kg,lower_kg,upper_kg,provider,model,processing_mode"] + rows
        return Data(output.joined(separator: "\r\n").utf8)
    }

    private static func date(_ value: Date) -> String { ISO8601DateFormatter().string(from: value) }
    private static func number(_ value: Double) -> String {
        String(format: "%.2f", locale: Locale(identifier: "en_US_POSIX"), value)
    }
    private static func optionalNumber(_ value: Double?) -> String { value.map(number) ?? "" }
    static func csvCell(_ value: String) -> String {
        var safe = value
        if let first = safe.first, "=+-@\t\r".contains(first) { safe = "'" + safe }
        return "\"\(safe.replacingOccurrences(of: "\"", with: "\"\""))\""
    }
}
