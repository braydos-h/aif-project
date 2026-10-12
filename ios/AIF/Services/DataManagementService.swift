import Foundation
import SwiftData

enum DataManagementService {
    static func deleteAllLocalData(context: ModelContext) throws {
        let photos = try context.fetch(FetchDescriptor<StoredPhoto>())
        let configurations = try context.fetch(FetchDescriptor<AIProviderConfiguration>())
        try context.delete(model: WeightEstimate.self)
        try context.delete(model: TapeMeasurement.self)
        try context.delete(model: ScaleMeasurement.self)
        try context.delete(model: StoredPhoto.self)
        try context.delete(model: Animal.self)
        try context.delete(model: AIProviderConfiguration.self)
        try context.delete(model: ApplicationPreferences.self)
        do { try context.save() }
        catch { context.rollback(); throw error }
        photos.forEach(PhotoStore.delete)
        let keychain = KeychainSecretStore()
        configurations.forEach { keychain.delete(reference: $0.keychainReference) }
        try PhotoStore.deleteAllFiles()
        try LocalModelStore.removeAll()
        context.insert(ApplicationPreferences())
        do { try context.save() }
        catch { context.rollback(); throw error }
    }
}

enum StorageUsageService {
    static func applicationSupportBytes() -> Int64 {
        guard let root = try? FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                                       appropriateFor: nil, create: false),
              let enumerator = FileManager.default.enumerator(at: root, includingPropertiesForKeys: [.fileSizeKey]) else { return 0 }
        return enumerator.compactMap { $0 as? URL }.reduce(Int64(0)) { total, url in
            total + Int64((try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0)
        }
    }
}
