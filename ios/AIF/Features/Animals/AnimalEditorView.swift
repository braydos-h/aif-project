import PhotosUI
import SwiftData
import SwiftUI

struct AnimalEditorView: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.modelContext) private var modelContext
    let animal: Animal?

    @State private var name: String
    @State private var tagNumber: String
    @State private var breed: String
    @State private var sex: AnimalSex
    @State private var tracksBirthDate: Bool
    @State private var birthDate: Date
    @State private var age: String
    @State private var notes: String
    @State private var photoItem: PhotosPickerItem?
    @State private var photoData: Data?
    @State private var removePhoto = false
    @State private var showingCamera = false
    @State private var errorMessage: String?

    init(animal: Animal?) {
        self.animal = animal
        _name = State(initialValue: animal?.name ?? "")
        _tagNumber = State(initialValue: animal?.tagNumber ?? "")
        _breed = State(initialValue: animal?.breed ?? "")
        _sex = State(initialValue: animal?.sex ?? .unknown)
        _tracksBirthDate = State(initialValue: animal?.birthDate != nil)
        _birthDate = State(initialValue: animal?.birthDate ?? .now)
        _age = State(initialValue: animal.map { $0.approximateAgeYears == 0 ? "" : String($0.approximateAgeYears) } ?? "")
        _notes = State(initialValue: animal?.notes ?? "")
    }

    var body: some View {
        Form {
            Section("Identification") {
                TextField("Name", text: $name).textInputAutocapitalization(.words)
                    .accessibilityIdentifier("animal-name")
                TextField("Tag number", text: $tagNumber).textInputAutocapitalization(.characters)
                TextField("Breed", text: $breed).textInputAutocapitalization(.words)
                Picker("Sex", selection: $sex) { ForEach(AnimalSex.allCases) { Text($0.title).tag($0) } }
            }
            Section("Age") {
                Toggle("Use birth date", isOn: $tracksBirthDate)
                if tracksBirthDate {
                    DatePicker("Birth date", selection: $birthDate, in: ...Date.now, displayedComponents: .date)
                } else {
                    TextField("Approximate age in years", text: $age)
                        .keyboardType(.decimalPad)
                        .accessibilityHint("Enter an age from zero to one hundred years")
                }
            }
            Section("Profile photo") {
                PhotosPicker(selection: $photoItem, matching: .images) {
                    Label(photoData == nil ? "Choose profile photo" : "Replace profile photo", systemImage: "photo")
                }
                Button { showingCamera = true } label: { Label("Take profile photo", systemImage: "camera") }
                if photoData != nil { Label("A new photo is ready to save", systemImage: "checkmark.circle").font(.caption).foregroundStyle(.secondary) }
                if animal?.profilePhotoID != nil {
                    Button("Remove profile photo", role: .destructive) { removePhoto = true; photoData = nil }
                }
            }
            Section("Notes") { TextField("Optional notes", text: $notes, axis: .vertical).lineLimit(3...8) }
            if let errorMessage { Section { Text(errorMessage).foregroundStyle(.red) } }
        }
        .navigationTitle(animal == nil ? "New animal" : "Edit animal")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button("Save") { save() }.fontWeight(.semibold).accessibilityIdentifier("save-animal")
            }
        }
        .onChange(of: photoItem) { _, item in
            guard let item else { return }
            Task {
                do { photoData = try await item.loadTransferable(type: Data.self); removePhoto = false }
                catch { errorMessage = "The selected photo could not be loaded." }
            }
        }
        .sheet(isPresented: $showingCamera) {
            CameraSheet { data in photoData = data; removePhoto = false }
                .ignoresSafeArea()
        }
    }

    private func save() {
        let cleanedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let cleanedTag = tagNumber.trimmingCharacters(in: .whitespacesAndNewlines)
        let parsedAge = age.replacingOccurrences(of: ",", with: ".").isEmpty ? 0 : Double(age.replacingOccurrences(of: ",", with: "."))
        guard !cleanedName.isEmpty || !cleanedTag.isEmpty else { errorMessage = "Enter a name or tag for this animal."; return }
        guard let parsedAge, parsedAge.isFinite, (0...100).contains(parsedAge) else {
            errorMessage = "Enter an approximate age from 0 to 100 years."; return
        }
        var createdPhoto: StoredPhoto?
        var oldPhotoToDelete: StoredPhoto?
        do {
            let oldPhotoID = animal?.profilePhotoID
            var newPhotoID: UUID? = removePhoto ? nil : oldPhotoID
            if let photoData {
                let image = try ImageProcessingService.prepare(photoData)
                let photo = try PhotoStore.save(image, animalID: animal?.id, context: modelContext)
                createdPhoto = photo
                newPhotoID = photo.id
            }
            let record: Animal
            if let animal {
                record = animal
                record.name = cleanedName
                record.tagNumber = cleanedTag
                record.breed = breed.trimmingCharacters(in: .whitespacesAndNewlines)
                record.sex = sex
                record.birthDate = tracksBirthDate ? birthDate : nil
                record.approximateAgeYears = parsedAge
                record.notes = notes
                record.profilePhotoID = newPhotoID
            } else {
                record = Animal(name: cleanedName,
                                tagNumber: cleanedTag,
                                breed: breed.trimmingCharacters(in: .whitespacesAndNewlines),
                                sex: sex, birthDate: tracksBirthDate ? birthDate : nil,
                                approximateAgeYears: parsedAge, notes: notes, profilePhotoID: newPhotoID)
                modelContext.insert(record)
            }
            if removePhoto || newPhotoID != oldPhotoID, let oldPhotoID {
                oldPhotoToDelete = try modelContext.fetch(FetchDescriptor<StoredPhoto>()).first(where: { $0.id == oldPhotoID })
                if let oldPhotoToDelete { modelContext.delete(oldPhotoToDelete) }
            }
            try modelContext.save()
            if let oldPhotoToDelete { PhotoStore.delete(oldPhotoToDelete) }
            dismiss()
        } catch {
            modelContext.rollback()
            if let createdPhoto { PhotoStore.delete(createdPhoto) }
            errorMessage = error.localizedDescription
        }
    }
}
