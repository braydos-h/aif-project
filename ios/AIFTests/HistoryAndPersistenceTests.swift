import Foundation
import XCTest
import SwiftData
@testable import AIF

@MainActor
final class HistoryAndPersistenceTests: XCTestCase {
    func testAnimalCRUDAndEstimateHistoryAreLocal() throws {
        let container = try ModelContainerFactory.make(inMemory: true)
        let context = container.mainContext
        let animal = try AnimalStore.create(name: "Daisy", tagNumber: "A-17", breed: "Angus", sex: .cow,
                                            approximateAgeYears: 4, context: context)
        let tape = try TapeEstimator.estimate(heartGirthCm: 180, bodyLengthCm: 150)
        _ = try EstimateStore.saveTape(tape, heartGirthCm: 180, bodyLengthCm: 150,
                                       animal: animal, context: context)
        _ = try EstimateStore.saveScale(weightKg: 455, animal: animal, context: context)

        let savedAnimals = try context.fetch(FetchDescriptor<Animal>())
        let estimates = try context.fetch(FetchDescriptor<WeightEstimate>())
        let scales = try context.fetch(FetchDescriptor<ScaleMeasurement>())
        XCTAssertEqual(savedAnimals.count, 1)
        XCTAssertEqual(savedAnimals.first?.displayName, "Daisy")
        XCTAssertEqual(estimates.count, 1)
        XCTAssertEqual(estimates.first?.animal?.id, animal.id)
        XCTAssertEqual(scales.count, 1)
        XCTAssertEqual(scales.first?.animal?.id, animal.id)
        XCTAssertNotEqual(estimates.first?.sourceRawValue, "verifiedScale")

        animal.name = "Daisy May"
        try context.save()
        XCTAssertEqual(try context.fetch(FetchDescriptor<Animal>()).first?.displayName, "Daisy May")
        try AnimalStore.setArchived(animal, archived: true, context: context)
        XCTAssertTrue(try context.fetch(FetchDescriptor<Animal>()).first?.isArchived == true)
        try AnimalStore.delete(animal, context: context)
        XCTAssertTrue(try context.fetch(FetchDescriptor<Animal>()).isEmpty)
        XCTAssertTrue(try context.fetch(FetchDescriptor<WeightEstimate>()).isEmpty)
        XCTAssertTrue(try context.fetch(FetchDescriptor<TapeMeasurement>()).isEmpty)
        XCTAssertTrue(try context.fetch(FetchDescriptor<ScaleMeasurement>()).isEmpty)
    }

    func testPersistentContainerReopensLocalRecords() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let storeURL = directory.appendingPathComponent("AIF.sqlite")

        let animalID = try createAnimal(storeURL: storeURL)
        let reopened = try ModelContainerFactory.make(storeURL: storeURL)
        let animals = try reopened.mainContext.fetch(FetchDescriptor<Animal>())
        XCTAssertEqual(animals.map(\.id), [animalID])
        XCTAssertEqual(animals.first?.displayName, "Persists offline")
    }

    private func createAnimal(storeURL: URL) throws -> UUID {
        let container = try ModelContainerFactory.make(storeURL: storeURL)
        let animal = try AnimalStore.create(name: "Persists offline", context: container.mainContext)
        return animal.id
    }

    func testHistoryQueriesKeepAnimalAndVerifiedWeightsSeparate() throws {
        let container = try ModelContainerFactory.make(inMemory: true)
        let context = container.mainContext
        let daisy = try AnimalStore.create(name: "Daisy", context: context)
        let rose = try AnimalStore.create(name: "Rose", context: context)
        let tape = try TapeEstimator.estimate(heartGirthCm: 180, bodyLengthCm: 150)
        _ = try EstimateStore.saveTape(tape, heartGirthCm: 180, bodyLengthCm: 150, animal: daisy, context: context)
        _ = try EstimateStore.saveScale(weightKg: 460, animal: rose, context: context)

        let rows = HistoryQueryService.rows(estimates: try context.fetch(FetchDescriptor<WeightEstimate>()),
                                            scales: try context.fetch(FetchDescriptor<ScaleMeasurement>()),
                                            animalID: daisy.id)
        XCTAssertEqual(rows.count, 1)
        XCTAssertEqual(rows.first?.animalName, "Daisy")
        XCTAssertEqual(rows.first?.method, EstimateSource.tape.title)
        XCTAssertFalse(rows.contains { $0.method == "Verified scale" })
    }

    func testBackupRoundTripAndDuplicateDetection() throws {
        let source = try ModelContainerFactory.make(inMemory: true)
        let sourceContext = source.mainContext
        let animal = try AnimalStore.create(name: "Daisy", breed: "Hereford", context: sourceContext)
        let tape = try TapeEstimator.estimate(heartGirthCm: 175, bodyLengthCm: 145)
        _ = try EstimateStore.saveTape(tape, heartGirthCm: 175, bodyLengthCm: 145,
                                       animal: animal, context: sourceContext)
        _ = try EstimateStore.saveScale(weightKg: 432, animal: animal, context: sourceContext)
        let backup = try BackupService.fullBackup(context: sourceContext)

        let destination = try ModelContainerFactory.make(inMemory: true)
        let imported = try BackupService.importBackup(backup, context: destination.mainContext)
        XCTAssertEqual(imported.animalsAdded, 1)
        XCTAssertEqual(imported.estimatesAdded, 1)
        XCTAssertEqual(imported.measurementsAdded, 2)
        XCTAssertEqual(try destination.mainContext.fetch(FetchDescriptor<Animal>()).first?.displayName, "Daisy")
        let repeated = try BackupService.importBackup(backup, context: destination.mainContext)
        XCTAssertEqual(repeated.duplicatesSkipped, 4)
        XCTAssertEqual(try destination.mainContext.fetch(FetchDescriptor<WeightEstimate>()).count, 1)
    }

    func testCSVProtectsSpreadsheetFormulaCells() {
        let text = String(decoding: HistoryExportService.csv(estimates: [], scales: []), as: UTF8.self)
        XCTAssertTrue(text.hasPrefix("date,animal,source"))
        let safe = HistoryExportService.csvCell("=HYPERLINK(\"x\")")
        XCTAssertTrue(safe.hasPrefix("'="))
    }
}
