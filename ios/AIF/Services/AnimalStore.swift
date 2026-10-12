import Foundation
import SwiftData

enum AnimalStore {
    static func create(name: String, tagNumber: String = "", breed: String = "", sex: AnimalSex = .unknown,
                       birthDate: Date? = nil, approximateAgeYears: Double = 0, notes: String = "",
                       context: ModelContext) throws -> Animal {
        let animal = Animal(name: name.trimmingCharacters(in: .whitespacesAndNewlines),
                            tagNumber: tagNumber.trimmingCharacters(in: .whitespacesAndNewlines),
                            breed: breed.trimmingCharacters(in: .whitespacesAndNewlines),
                            sex: sex, birthDate: birthDate, approximateAgeYears: approximateAgeYears, notes: notes)
        context.insert(animal)
        do { try context.save() }
        catch { context.rollback(); throw error }
        return animal
    }

    static func delete(_ animal: Animal, context: ModelContext) throws {
        let photoIDs = Set(animal.estimates.compactMap(\.photoID) + [animal.profilePhotoID].compactMap { $0 })
        let photos = try context.fetch(FetchDescriptor<StoredPhoto>()).filter {
            photoIDs.contains($0.id) || $0.animalID == animal.id
        }
        photos.forEach { context.delete($0) }
        context.delete(animal)
        do { try context.save() }
        catch { context.rollback(); throw error }
        photos.forEach(PhotoStore.delete)
    }

    static func setArchived(_ animal: Animal, archived: Bool, context: ModelContext) throws {
        animal.isArchived = archived
        do { try context.save() }
        catch { context.rollback(); throw error }
    }

    static func markViewed(_ animal: Animal, at date: Date = .now, context: ModelContext) throws {
        animal.lastViewedAt = date
        do { try context.save() }
        catch { context.rollback(); throw error }
    }
}
