import Foundation
import SwiftData

@Model
final class StoredPhoto {
    @Attribute(.unique) var id: UUID
    var relativePath: String
    var processedRelativePath: String?
    var mimeType: String
    var byteCount: Int
    var pixelWidth: Int
    var pixelHeight: Int
    var createdAt: Date
    var animalID: UUID?

    init(
        id: UUID = UUID(),
        relativePath: String,
        processedRelativePath: String? = nil,
        mimeType: String = "image/jpeg",
        byteCount: Int,
        pixelWidth: Int,
        pixelHeight: Int,
        createdAt: Date = .now,
        animalID: UUID? = nil
    ) {
        self.id = id
        self.relativePath = relativePath
        self.processedRelativePath = processedRelativePath
        self.mimeType = mimeType
        self.byteCount = byteCount
        self.pixelWidth = pixelWidth
        self.pixelHeight = pixelHeight
        self.createdAt = createdAt
        self.animalID = animalID
    }
}
