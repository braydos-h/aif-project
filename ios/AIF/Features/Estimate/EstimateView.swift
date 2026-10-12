import PhotosUI
import SwiftData
import SwiftUI
import UIKit

private struct SelectedPhoto: Identifiable, Sendable {
    var id = UUID()
    var name: String
    var data: Data
    var rotationDegrees = 0
    var centerSquareCrop = false
}

private struct EstimateDraft: Identifiable {
    var id: UUID
    var prepared: ProcessedImage
    var result: WeightEstimationResult?
    var errorMessage: String?
    var contextInfo: AnimalContext
    var animalID: UUID?
}

private enum EstimateSection: String, CaseIterable, Identifiable {
    case photo = "Photo AI"
    case tape = "Tape estimate"
    var id: String { rawValue }
}

struct EstimateView: View {
    @Binding var cameraRequested: Bool
    @Environment(\.modelContext) private var modelContext
    @Query(sort: \Animal.name) private var animals: [Animal]
    @Query private var preferences: [ApplicationPreferences]
    @Query private var configurations: [AIProviderConfiguration]
    @State private var section: EstimateSection = .photo
    @State private var photoItems: [PhotosPickerItem] = []
    @State private var photos: [SelectedPhoto] = []
    @State private var drafts: [EstimateDraft] = []
    @State private var selectedAnimalID: UUID?
    @State private var breed = ""
    @State private var sex: AnimalSex = .unknown
    @State private var ageYears = ""
    @State private var showingCamera = false
    @State private var confirmCloud = false
    @State private var isEstimating = false
    @State private var estimationTask: Task<Void, Never>?
    @State private var modelInfo: InstalledModelInfo?
    @State private var retryingPhotoID: UUID?
    @State private var progressText = ""
    @State private var message: String?
    @State private var tapeGirth = ""
    @State private var tapeLength = ""
    @State private var tapeResult: TapeEstimate?
    @State private var tapeError: String?

    private var inferenceMode: InferenceMode { preferences.first?.inferenceMode ?? .local }
    private var activeAnimals: [Animal] { animals.filter { !$0.isArchived } }
    private var cloudConfiguration: AIProviderConfiguration? {
        configurations.first(where: { $0.isDefault && $0.provider.isImplemented })
            ?? configurations.first(where: { $0.provider.isImplemented })
    }
    private var selectedAnimal: Animal? { activeAnimals.first { $0.id == selectedAnimalID } }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                Picker("Estimate method", selection: $section) {
                    ForEach(EstimateSection.allCases) { Text($0.rawValue).tag($0) }
                }
                .pickerStyle(.segmented)
                if section == .photo { photoEstimator } else { tapeEstimator }
            }
            .padding(18)
        }
        .background(AIFTheme.warm.opacity(0.45))
        .navigationTitle("Estimate weight")
        .navigationBarTitleDisplayMode(.large)
        .sheet(isPresented: $showingCamera) {
            CameraSheet { data in appendPhoto(data, name: "Camera photo") }
                .ignoresSafeArea()
        }
        .confirmationDialog(
            "Send selected photos to \(cloudConfiguration?.displayName ?? "your configured provider")?",
            isPresented: $confirmCloud,
            titleVisibility: .visible
        ) {
            Button(retryingPhotoID == nil
                   ? "Send \(photos.count) photo\(photos.count == 1 ? "" : "s") and estimate"
                   : "Retry this photo", role: .destructive) {
                if let retryingPhotoID {
                    startRetry(retryingPhotoID)
                    self.retryingPhotoID = nil
                } else {
                    startEstimation()
                }
            }
            Button("Cancel", role: .cancel) { retryingPhotoID = nil }
        } message: {
            Text(retryingPhotoID == nil
                 ? "This requires internet. The selected \(photos.count == 1 ? "photo" : "photos") and the breed, sex, and age you entered will be sent to the configured provider. Nothing is sent until you confirm."
                 : "This requires internet. The selected photo and animal details will be sent to the configured provider again. Nothing is sent until you confirm.")
        }
        .onChange(of: photoItems) { _, items in loadPhotos(items) }
        .onChange(of: selectedAnimalID) { _, id in applyAnimalContext(id) }
        .onChange(of: cameraRequested) { _, requested in
            guard requested else { return }
            showingCamera = true
            cameraRequested = false
        }
        .task {
            modelInfo = await Task.detached(priority: .utility) { LocalModelStore.currentModelInfo() }.value
        }
    }

    private var photoEstimator: some View {
        VStack(alignment: .leading, spacing: 16) {
            AIFCard {
                VStack(alignment: .leading, spacing: 9) {
                    Label("Choose photo inference", systemImage: inferenceMode == .local ? "iphone" : "network")
                        .font(.headline)
                    Picker("AI mode", selection: Binding(
                        get: { inferenceMode },
                        set: { newValue in
                            let settings = preferences.first ?? ApplicationPreferences()
                            settings.inferenceMode = newValue
                            if preferences.isEmpty { modelContext.insert(settings) }
                            try? modelContext.save()
                        }
                    )) {
                        ForEach(InferenceMode.allCases) { Text($0.title).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .disabled(isEstimating)
                    Text(inferenceMode == .local
                         ? (modelInfo.map { "Compatible Core ML model installed: \($0.name). AIF checks its input/output interface, not its cattle accuracy." }
                            ?? "No cattle-weight model is installed. Estimation will show unavailable until a compatible Core ML model is added in Settings.")
                         : (cloudConfiguration.map { "\($0.displayName) · \($0.modelName). Internet is required, and photos are sent only after your confirmation." }
                            ?? "Set up an OpenAI-compatible vision provider and API key in Settings before estimating."))
                        .font(.caption).foregroundStyle(.secondary)
                }
            }

            AIFCard {
                VStack(alignment: .leading, spacing: 12) {
                    Label("Photos", systemImage: "photo.stack").font(.headline)
                    HStack(spacing: 10) {
                        PhotosPicker(selection: $photoItems, maxSelectionCount: 6, matching: .images) {
                            Label("Choose photos", systemImage: "photo.on.rectangle.angled")
                                .frame(maxWidth: .infinity, minHeight: 46)
                        }
                        .buttonStyle(.bordered)
                        .disabled(isEstimating)
                        Button { showingCamera = true } label: {
                            Label("Camera", systemImage: "camera.fill").frame(minWidth: 96, minHeight: 46)
                        }
                        .buttonStyle(.borderedProminent).tint(AIFTheme.green)
                        .disabled(isEstimating)
                    }
                    Text("Up to six photos. Images are resized and re-encoded as JPEG before processing, removing camera location metadata.")
                        .font(.caption).foregroundStyle(.secondary)
                    if !photos.isEmpty {
                        ForEach($photos) { $photo in photoRow(photo: $photo) }
                            .disabled(isEstimating)
                    } else {
                        ContentUnavailableView("No photos selected", systemImage: "camera.metering.center.weighted",
                                               description: Text("Take a photo or choose one from your library."))
                            .frame(maxWidth: .infinity).frame(height: 150)
                    }
                }
            }

            animalContextCard.disabled(isEstimating)

            if !photos.isEmpty {
                Button {
                    savePhotosLocally()
                } label: {
                    Label("Save selected photos to this iPhone", systemImage: "square.and.arrow.down")
                        .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.bordered)
                .disabled(isEstimating)
                .accessibilityIdentifier("save-photos-locally")

                if isEstimating {
                    Button("Cancel estimation") {
                        message = "Cancelling estimation…"
                        estimationTask?.cancel()
                    }
                    .buttonStyle(.bordered)
                    .accessibilityIdentifier("cancel-photo-estimate")
                    Text(progressText).font(.caption).foregroundStyle(.secondary)
                } else {
                    Button {
                        if inferenceMode == .cloud { confirmCloud = true }
                        else { startEstimation() }
                    } label: {
                        Text(inferenceMode == .local ? "Estimate on this iPhone" : "Send photos to provider")
                            .fontWeight(.semibold)
                            .frame(maxWidth: .infinity, minHeight: 54)
                    }
                    .buttonStyle(.borderedProminent).tint(AIFTheme.green)
                    .accessibilityIdentifier("start-photo-estimate")
                }
            }
            if let message { Text(message).font(.subheadline).foregroundStyle(message.hasPrefix("Saved") ? AIFTheme.green : .red).accessibilityIdentifier("estimate-message") }
            if !drafts.isEmpty { draftResults }
        }
    }

    private var animalContextCard: some View {
        AIFCard {
            VStack(alignment: .leading, spacing: 12) {
                Label("Animal details", systemImage: "pawprint").font(.headline)
                Picker("Associate with", selection: $selectedAnimalID) {
                    Text("No animal").tag(UUID?.none)
                    ForEach(activeAnimals) { animal in Text(animal.displayName).tag(Optional(animal.id)) }
                }
                .disabled(isEstimating)
                TextField("Breed (optional)", text: $breed).textInputAutocapitalization(.words)
                    .disabled(isEstimating)
                Picker("Sex", selection: $sex) { ForEach(AnimalSex.allCases) { Text($0.title).tag($0) } }
                    .pickerStyle(.menu)
                    .disabled(isEstimating)
                TextField("Approximate age in years (optional)", text: $ageYears).keyboardType(.decimalPad)
                    .disabled(isEstimating)
                if activeAnimals.isEmpty {
                    Text("Create an animal record in the Animals tab to link this estimate.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }

    private func photoRow(photo: Binding<SelectedPhoto>) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Group {
                if let image = try? ImageProcessingService.validate(photo.wrappedValue.data) {
                    Image(uiImage: image).resizable().scaledToFill()
                } else {
                    Image(systemName: "photo").resizable().scaledToFit().foregroundStyle(.secondary).padding(20)
                }
            }
            .frame(width: 78, height: 78).clipShape(RoundedRectangle(cornerRadius: 12))
            VStack(alignment: .leading, spacing: 6) {
                Text(photo.wrappedValue.name).font(.subheadline.weight(.medium)).lineLimit(1)
                HStack(spacing: 8) {
                    Button { photo.wrappedValue.rotationDegrees = (photo.wrappedValue.rotationDegrees + 90) % 360 } label: {
                        Label("Rotate", systemImage: "rotate.right").labelStyle(.iconOnly)
                    }.accessibilityLabel("Rotate \(photo.wrappedValue.name)")
                    Toggle("Square crop", isOn: photo.centerSquareCrop).font(.caption)
                    Spacer()
                    Button(role: .destructive) { removePhoto(photo.wrappedValue.id) } label: {
                        Image(systemName: "trash")
                    }.accessibilityLabel("Remove \(photo.wrappedValue.name)")
                }
                .disabled(isEstimating)
                if photo.wrappedValue.rotationDegrees != 0 || photo.wrappedValue.centerSquareCrop {
                    Text("Crop and rotation are applied before estimate and save.").font(.caption2).foregroundStyle(.secondary)
                }
            }
        }
        .padding(10)
        .background(AIFTheme.paleGreen.opacity(0.6), in: RoundedRectangle(cornerRadius: 16))
    }

    private var draftResults: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Review results").font(.title3.weight(.semibold))
            ForEach(drafts) { draft in
                AIFCard {
                    HStack(alignment: .top, spacing: 12) {
                        if let image = UIImage(data: draft.prepared.data) {
                            Image(uiImage: image).resizable().scaledToFill()
                                .frame(width: 76, height: 76).clipShape(RoundedRectangle(cornerRadius: 12))
                        }
                        VStack(alignment: .leading, spacing: 6) {
                            if let result = draft.result {
                                Text("\(result.estimatedWeightKg.weightText) kg").font(.title2.weight(.bold).monospacedDigit())
                                if let lower = result.lowerBoundKg, let upper = result.upperBoundKg {
                                    Text("Range: \(lower.weightText)–\(upper.weightText) kg (\(UnitConverter.pounds(fromKilograms: lower).weightText)–\(UnitConverter.pounds(fromKilograms: upper).weightText) lb)")
                                        .font(.caption).foregroundStyle(.secondary)
                                }
                                Text("\(UnitConverter.pounds(fromKilograms: result.estimatedWeightKg).weightText) lb")
                                    .font(.subheadline).foregroundStyle(.secondary)
                                Text("\(result.providerName) · \(result.modelName) · \(result.processingMode.capitalized)")
                                    .font(.caption).foregroundStyle(.secondary)
                                Text("\(result.method). Not a verified measurement.")
                                    .font(.caption2).foregroundStyle(.secondary)
                                Button("Save to local history") { save(draft) }
                                    .buttonStyle(.borderedProminent).tint(AIFTheme.green).padding(.top, 3)
                            } else {
                                Text("Estimate unavailable").font(.headline)
                                Text(draft.errorMessage ?? "The provider did not return a valid estimate.")
                                    .font(.caption).foregroundStyle(.red)
                                Button("Retry this photo") { retry(draft.id) }.buttonStyle(.bordered)
                            }
                        }
                        Spacer(minLength: 0)
                    }
                }
            }
        }
    }

    private var tapeEstimator: some View {
        VStack(alignment: .leading, spacing: 16) {
            AIFCard {
                VStack(alignment: .leading, spacing: 12) {
                    Label("Schaeffer tape method", systemImage: "ruler").font(.headline)
                    Text("Measure heart girth just behind the front legs, then body length from chest to tail head while the animal stands square.")
                        .font(.subheadline).foregroundStyle(.secondary)
                    Text("Weight kg = (heart girth cm² × body length cm) / 10,838")
                        .font(.system(.subheadline, design: .monospaced, weight: .medium))
                    Text("Both measurements must be between 50 and 300 cm.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            AIFCard {
                VStack(spacing: 14) {
                    measurementField("Heart girth", value: $tapeGirth, identifier: "tape-girth")
                    measurementField("Body length", value: $tapeLength, identifier: "tape-length")
                    Picker("Associate with", selection: $selectedAnimalID) {
                        Text("No animal").tag(UUID?.none)
                        ForEach(activeAnimals) { animal in Text(animal.displayName).tag(Optional(animal.id)) }
                    }
                }
            }
            Button("Calculate estimate") { calculateTape() }
                .buttonStyle(.borderedProminent).tint(AIFTheme.green).frame(maxWidth: .infinity, minHeight: 50)
                .accessibilityIdentifier("calculate-tape")
            if let tapeError { Text(tapeError).foregroundStyle(.red).font(.subheadline) }
            if let tapeResult {
                AIFCard {
                    VStack(alignment: .leading, spacing: 10) {
                        Text("Tape estimate").font(.headline)
                        Text("\(tapeResult.weightKg.weightText) kg")
                            .font(.system(.largeTitle, design: .rounded, weight: .bold)).accessibilityIdentifier("tape-result")
                        Text("\(tapeResult.weightLb.weightText) lb")
                        Text("Nominal range ±5%: \(tapeResult.lowerBoundKg.weightText)–\(tapeResult.upperBoundKg.weightText) kg")
                            .font(.subheadline)
                        Text("Tape-based estimate only. Results vary with technique, posture, breed, and body condition. Verify with a scale; do not use for medication dosing.")
                            .font(.caption).foregroundStyle(.secondary)
                        Button("Save to local history") { saveTape() }
                            .buttonStyle(.borderedProminent).tint(AIFTheme.green)
                            .accessibilityIdentifier("save-tape")
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
    }

    private func measurementField(_ title: String, value: Binding<String>, identifier: String) -> some View {
        HStack {
            Text(title).font(.subheadline.weight(.medium))
            Spacer()
            TextField("cm", text: value).keyboardType(.decimalPad).multilineTextAlignment(.trailing)
                .frame(width: 100).accessibilityIdentifier(identifier)
            Text("cm").foregroundStyle(.secondary)
        }
    }

    private func calculateTape() {
        guard let girth = parseMeasurement(tapeGirth), let length = parseMeasurement(tapeLength) else {
            tapeResult = nil
            tapeError = TapeEstimatorError.invalidMeasurement.localizedDescription
            return
        }
        do { tapeResult = try TapeEstimator.estimate(heartGirthCm: girth, bodyLengthCm: length); tapeError = nil }
        catch { tapeResult = nil; tapeError = error.localizedDescription }
    }

    private func saveTape() {
        guard let tapeResult, let girth = parseMeasurement(tapeGirth), let length = parseMeasurement(tapeLength) else { return }
        do {
            _ = try EstimateStore.saveTape(tapeResult, heartGirthCm: girth, bodyLengthCm: length,
                                           animal: selectedAnimal, context: modelContext)
            message = "Saved tape estimate locally."
            self.tapeResult = nil
            tapeGirth = ""
            tapeLength = ""
        } catch { tapeError = error.localizedDescription }
    }

    private func loadPhotos(_ items: [PhotosPickerItem]) {
        Task {
            var loaded: [SelectedPhoto] = []
            for (index, item) in items.enumerated() {
                do {
                    guard let data = try await item.loadTransferable(type: Data.self) else { continue }
                    _ = try ImageProcessingService.validate(data)
                    loaded.append(SelectedPhoto(name: "Photo \(index + 1)", data: data))
                } catch {
                    message = error.localizedDescription
                }
            }
            photos = loaded
            drafts = []
        }
    }

    private func appendPhoto(_ data: Data, name: String) {
        do {
            _ = try ImageProcessingService.validate(data)
            if photos.count >= 6 { message = "Choose up to six photos at a time."; return }
            photos.append(SelectedPhoto(name: name, data: data))
            message = nil
        } catch { message = error.localizedDescription }
    }

    private func removePhoto(_ id: UUID) {
        photos.removeAll { $0.id == id }
        drafts.removeAll { $0.id == id }
    }

    private func savePhotosLocally() {
        var savedPhotos: [StoredPhoto] = []
        do {
            for photo in photos {
                let processed = try ImageProcessingService.prepare(photo.data,
                                                                   rotationDegrees: photo.rotationDegrees,
                                                                   centerSquareCrop: photo.centerSquareCrop)
                savedPhotos.append(try PhotoStore.save(processed, animalID: selectedAnimalID, context: modelContext))
            }
            try modelContext.save()
            photos.removeAll()
            drafts.removeAll()
            message = "Saved \(savedPhotos.count) photo\(savedPhotos.count == 1 ? "" : "s") on this iPhone."
        } catch {
            modelContext.rollback()
            savedPhotos.forEach(PhotoStore.delete)
            message = "Could not save photos: \(error.localizedDescription)"
        }
    }

    private func applyAnimalContext(_ id: UUID?) {
        guard let animal = activeAnimals.first(where: { $0.id == id }) else { return }
        breed = animal.breed
        sex = animal.sex
        ageYears = animal.approximateAgeAtRecordYears.map { String($0) } ?? ""
    }

    private func estimatePhotos() async {
        if !ageYears.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            guard let age = parseMeasurement(ageYears), age.isFinite, (0...100).contains(age) else {
                message = "Enter an approximate age from 0 to 100 years."
                isEstimating = false
                return
            }
        }
        message = nil
        drafts = []
        let selectedPhotos = photos
        let mode = inferenceMode
        let contextInfo = selectedAnimalContext()
        let animalID = selectedAnimalID
        do {
            let cloud = try providerCredentialsIfNeeded(mode: mode)
            for (index, photo) in selectedPhotos.enumerated() {
                try Task.checkCancellation()
                progressText = "Estimating \(index + 1) of \(selectedPhotos.count)…"
                await estimateOne(photo, cloud: cloud, mode: mode, contextInfo: contextInfo, animalID: animalID)
                try Task.checkCancellation()
            }
        } catch is CancellationError {
            message = "Estimation cancelled. No estimate was saved."
        } catch {
            message = error.localizedDescription
        }
        isEstimating = false
        progressText = ""
    }

    private func startEstimation() {
        guard !isEstimating else { return }
        isEstimating = true
        estimationTask = Task {
            await estimatePhotos()
            estimationTask = nil
        }
    }

    private func retry(_ id: UUID) {
        guard let photo = photos.first(where: { $0.id == id }) else { return }
        if inferenceMode == .cloud {
            retryingPhotoID = photo.id
            confirmCloud = true
        } else {
            startRetry(photo.id)
        }
    }

    private func startRetry(_ id: UUID) {
        guard !isEstimating else { return }
        isEstimating = true
        message = nil
        estimationTask = Task {
            await retryPhoto(id)
            isEstimating = false
            progressText = ""
            estimationTask = nil
        }
    }

    private func retryPhoto(_ id: UUID) async {
        guard let photo = photos.first(where: { $0.id == id }) else { return }
        let mode = inferenceMode
        do {
            let cloud = try providerCredentialsIfNeeded(mode: mode)
            await estimateOne(photo, cloud: cloud, mode: mode,
                              contextInfo: selectedAnimalContext(), animalID: selectedAnimalID)
        } catch {
            if !Task.isCancelled { message = error.localizedDescription }
        }
    }

    private func selectedAnimalContext() -> AnimalContext {
        AnimalContext(breed: breed.trimmingCharacters(in: .whitespacesAndNewlines),
                      sex: sex.rawValue, approximateAgeYears: parseMeasurement(ageYears))
    }

    private func providerCredentialsIfNeeded(mode: InferenceMode) throws -> (CloudProviderSettings, String)? {
        guard mode == .cloud else { return nil }
        guard let configuration = cloudConfiguration else { throw WeightEstimationError.providerNotConfigured }
        let url = try ProviderConfigurationValidator.validate(endpoint: configuration.endpoint, modelName: configuration.modelName)
        guard url.host != nil, let key = try KeychainSecretStore().read(reference: configuration.keychainReference), !key.isEmpty else {
            throw WeightEstimationError.providerNotConfigured
        }
        return (CloudProviderSettings(name: configuration.displayName, endpoint: configuration.endpoint,
                                      modelName: configuration.modelName), key)
    }

    private func estimateOne(_ photo: SelectedPhoto, cloud: (CloudProviderSettings, String)?,
                             mode: InferenceMode, contextInfo: AnimalContext, animalID: UUID?) async {
        do {
            let processed = try await Task.detached(priority: .userInitiated) {
                try ImageProcessingService.prepare(photo.data, rotationDegrees: photo.rotationDegrees,
                                                   centerSquareCrop: photo.centerSquareCrop)
            }.value
            try Task.checkCancellation()
            let result = try await InferenceCoordinator.estimate(image: processed.data, context: contextInfo,
                                                                 mode: mode, cloudSettings: cloud?.0,
                                                                 apiKey: cloud?.1)
            try Task.checkCancellation()
            drafts.removeAll { $0.id == photo.id }
            drafts.append(EstimateDraft(id: photo.id, prepared: processed, result: result, errorMessage: nil,
                                        contextInfo: contextInfo, animalID: animalID))
        } catch {
            guard !Task.isCancelled else { return }
            let fallback = (try? ImageProcessingService.prepare(photo.data,
                                                                rotationDegrees: photo.rotationDegrees,
                                                                centerSquareCrop: photo.centerSquareCrop))
                ?? ProcessedImage(data: Data(), width: 0, height: 0)
            drafts.removeAll { $0.id == photo.id }
            drafts.append(EstimateDraft(id: photo.id, prepared: fallback, result: nil,
                                        errorMessage: error.localizedDescription, contextInfo: contextInfo,
                                        animalID: animalID))
        }
    }

    private func save(_ draft: EstimateDraft) {
        guard let result = draft.result else { return }
        do {
            let linkedAnimal = activeAnimals.first { $0.id == draft.animalID }
            guard let selected = photos.first(where: { $0.id == draft.id }) else { return }
            _ = try EstimateStore.saveAI(result, originalPhotoData: selected.data, photo: draft.prepared, animal: linkedAnimal,
                                         context: modelContext, contextInfo: draft.contextInfo)
            drafts.removeAll { $0.id == draft.id }
            photos.removeAll { $0.id == draft.id }
            message = "Saved estimate and photo on this iPhone."
        } catch { message = "Could not save estimate: \(error.localizedDescription)" }
    }

    private func parseMeasurement(_ value: String) -> Double? {
        Double(value.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: ",", with: "."))
    }
}
