import Foundation
import SwiftData

enum PhotoStoreError: LocalizedError {
    case invalidPath
    case unavailable

    var errorDescription: String? {
        switch self {
        case .invalidPath: "The photo file reference is invalid."
        case .unavailable: "The photo file could not be saved or opened."
        }
    }
}

enum PhotoStore {
    static func save(_ image: ProcessedImage, id: UUID = UUID(), animalID: UUID? = nil, context: ModelContext) throws -> StoredPhoto {
        try save(original: image, processed: nil, id: id, animalID: animalID, context: context)
    }

    static func saveEstimate(originalData: Data, processed: ProcessedImage, id: UUID = UUID(),
                             animalID: UUID? = nil, context: ModelContext) throws -> StoredPhoto {
        let original = try ImageProcessingService.archive(originalData)
        return try save(original: original, processed: processed, id: id, animalID: animalID, context: context)
    }

    private static func save(original: ProcessedImage, processed: ProcessedImage?, id: UUID,
                             animalID: UUID?, context: ModelContext) throws -> StoredPhoto {
        let directory = try photosDirectory()
        let originalName = "\(id.uuidString)-original.jpg"
        let processedName = processed.map { _ in "\(id.uuidString)-analysis.jpg" }
        let originalURL = directory.appendingPathComponent(originalName, isDirectory: false)
        let processedURL = processedName.map { directory.appendingPathComponent($0, isDirectory: false) }
        do {
            try original.data.write(to: originalURL, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
            if let processed, let processedURL {
                try processed.data.write(to: processedURL, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
            }
        } catch {
            try? FileManager.default.removeItem(at: originalURL)
            if let processedURL { try? FileManager.default.removeItem(at: processedURL) }
            throw PhotoStoreError.unavailable
        }
        let photo = StoredPhoto(
            id: id,
            relativePath: originalName,
            processedRelativePath: processedName,
            byteCount: original.data.count + (processed?.data.count ?? 0),
            pixelWidth: original.width,
            pixelHeight: original.height,
            animalID: animalID
        )
        context.insert(photo)
        return photo
    }

    static func read(_ photo: StoredPhoto) -> Data? {
        readFile(photo.relativePath)
    }

    static func readProcessed(_ photo: StoredPhoto) -> Data? {
        guard let path = photo.processedRelativePath else { return nil }
        return readFile(path)
    }

    private static func readFile(_ path: String) -> Data? {
        guard !path.contains("/"), !path.contains("..") else { return nil }
        guard let root = try? photosDirectory() else { return nil }
        return try? Data(contentsOf: root.appendingPathComponent(path, isDirectory: false))
    }

    static func delete(_ photo: StoredPhoto) {
        guard !photo.relativePath.contains("/"), !photo.relativePath.contains(".."),
              let root = try? photosDirectory() else { return }
        try? FileManager.default.removeItem(at: root.appendingPathComponent(photo.relativePath, isDirectory: false))
        if let path = photo.processedRelativePath, !path.contains("/"), !path.contains("..") {
            try? FileManager.default.removeItem(at: root.appendingPathComponent(path, isDirectory: false))
        }
    }

    static func deleteAllFiles() throws {
        let root = try photosDirectory()
        guard FileManager.default.fileExists(atPath: root.path) else { return }
        try FileManager.default.removeItem(at: root)
    }

    static func totalBytes(context: ModelContext) -> Int64 {
        let photos = (try? context.fetch(FetchDescriptor<StoredPhoto>())) ?? []
        return photos.reduce(Int64(0)) { $0 + Int64($1.byteCount) }
    }

    private static func photosDirectory() throws -> URL {
        let base = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let directory = base.appendingPathComponent("AIF/Photos", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: directory.path)
        return directory
    }
}
