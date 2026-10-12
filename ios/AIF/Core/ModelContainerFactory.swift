import Foundation
import SwiftData

enum AIFSchemaV1: VersionedSchema {
    static var versionIdentifier = Schema.Version(1, 0, 0)
    static var models: [any PersistentModel.Type] {
        [Animal.self, WeightEstimate.self, TapeMeasurement.self, ScaleMeasurement.self,
         StoredPhoto.self, AIProviderConfiguration.self, ApplicationPreferences.self]
    }
}

enum AIFMigrationPlan: SchemaMigrationPlan {
    static var schemas: [any VersionedSchema.Type] { [AIFSchemaV1.self] }
    static var stages: [MigrationStage] { [] }
}

enum ModelContainerFactory {
    static func make(inMemory: Bool = false, storeURL: URL? = nil) throws -> ModelContainer {
        let schema = Schema(versionedSchema: AIFSchemaV1.self)
        let configuration: ModelConfiguration
        if let storeURL {
            configuration = ModelConfiguration("AIF", schema: schema, url: storeURL, cloudKitDatabase: .none)
        } else {
            configuration = ModelConfiguration("AIF", schema: schema, isStoredInMemoryOnly: inMemory,
                                               cloudKitDatabase: .none)
        }
        return try ModelContainer(for: schema, migrationPlan: AIFMigrationPlan.self, configurations: [configuration])
    }
}
