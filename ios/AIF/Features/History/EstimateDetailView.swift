import SwiftData
import SwiftUI
import UIKit

struct EstimateDetailView: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.modelContext) private var modelContext
    let estimate: WeightEstimate
    @Query private var photos: [StoredPhoto]
    @Query private var tapeMeasurements: [TapeMeasurement]
    @Query private var preferences: [ApplicationPreferences]
    @State private var showingTapeEditor = false
    @State private var showingDeleteConfirmation = false
    @State private var showingDeleteAlert = false

    private var photoImage: UIImage? {
        guard let id = estimate.photoID, let photo = photos.first(where: { $0.id == id }),
              let data = PhotoStore.read(photo) else { return nil }
        return UIImage(data: data)
    }
    private var analysisImage: UIImage? {
        guard let id = estimate.photoID, let photo = photos.first(where: { $0.id == id }),
              let data = PhotoStore.readProcessed(photo) else { return nil }
        return UIImage(data: data)
    }
    private var tapeMeasurement: TapeMeasurement? {
        tapeMeasurements.first { $0.relatedEstimateID == estimate.id }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                AIFCard {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(estimate.source.title.uppercased()).font(.caption.weight(.bold)).tracking(1).foregroundStyle(AIFTheme.green)
                        Text("\(estimate.estimatedWeightKg.weightText) kg")
                            .font(.system(.largeTitle, design: .rounded, weight: .bold)).accessibilityIdentifier("estimate-weight")
                        if preferences.first?.includePounds ?? true {
                            Text("\(UnitConverter.pounds(fromKilograms: estimate.estimatedWeightKg).weightText) lb")
                                .font(.title3).foregroundStyle(.secondary)
                        }
                        if let low = estimate.lowerBoundKg, let high = estimate.upperBoundKg {
                            Text(preferences.first?.includePounds ?? true
                                 ? "Estimated range: \(low.weightText)–\(high.weightText) kg (\(UnitConverter.pounds(fromKilograms: low).weightText)–\(UnitConverter.pounds(fromKilograms: high).weightText) lb)"
                                 : "Estimated range: \(low.weightText)–\(high.weightText) kg")
                                .font(.subheadline)
                        } else {
                            Text("No range was returned by the model.").font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
                if let photoImage {
                    AIFCard {
                        VStack(alignment: .leading, spacing: 10) {
                            Text("Original photo").font(.headline)
                            Image(uiImage: photoImage).resizable().scaledToFit()
                                .clipShape(RoundedRectangle(cornerRadius: 16))
                                .accessibilityLabel("Saved estimate photo")
                        }
                    }
                }
                if let analysisImage {
                    AIFCard {
                        VStack(alignment: .leading, spacing: 10) {
                            Text("Photo used for estimation").font(.headline)
                            Image(uiImage: analysisImage).resizable().scaledToFit()
                                .clipShape(RoundedRectangle(cornerRadius: 16))
                        }
                    }
                }
                AIFCard {
                    VStack(alignment: .leading, spacing: 12) {
                        Text("Estimate details").font(.headline)
                        detail("Date", estimate.createdAt.formatted(date: .long, time: .shortened))
                        detail("Processing", estimate.processingMode.capitalized)
                        detail("Provider", estimate.providerName)
                        detail("Model", estimate.modelName)
                        detail("Model version", estimate.modelVersion)
                        detail("Method", estimate.method)
                        if !estimate.breedContext.isEmpty { detail("Breed", estimate.breedContext) }
                        detail("Sex", AnimalSex(rawValue: estimate.sexContext)?.title ?? estimate.sexContext.capitalized)
                        if let age = estimate.ageContextYears { detail("Approximate age", "\(age.weightText) years") }
                        if let animal = estimate.animal {
                            NavigationLink { AnimalDetailView(animal: animal) } label: {
                                Label("Animal: \(animal.displayName)", systemImage: "pawprint")
                            }
                        } else { detail("Animal", "Not linked") }
                    }
                }
                if let girth = estimate.heartGirthCm, let length = estimate.bodyLengthCm {
                    AIFCard {
                        VStack(alignment: .leading, spacing: 10) {
                            Text("Tape readings").font(.headline)
                            detail("Heart girth", "\(girth.weightText) cm")
                            detail("Body length", "\(length.weightText) cm")
                            if let tapeMeasurement {
                                Button("Edit tape measurements") { showingTapeEditor = true }
                                    .buttonStyle(.bordered)
                            }
                        }
                    }
                }
                AIFCard {
                    VStack(alignment: .leading, spacing: 8) {
                        Label("Limitations", systemImage: "info.circle").font(.headline)
                        Text(estimate.limitations).font(.subheadline).foregroundStyle(.secondary)
                    }
                }
            }
            .padding(18)
        }
        .background(AIFTheme.warm.opacity(0.4))
        .navigationTitle("Estimate details")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("Delete estimate", systemImage: "trash", role: .destructive) {
                        showingDeleteConfirmation = true
                    }
                } label: { Image(systemName: "ellipsis.circle") }
            }
        }
        .sheet(isPresented: $showingTapeEditor) {
            if let tapeMeasurement { NavigationStack { TapeMeasurementEditor(measurement: tapeMeasurement) } }
        }
        .confirmationDialog("Delete this estimate?", isPresented: $showingDeleteConfirmation, titleVisibility: .visible) {
            Button("Delete Estimate", role: .destructive) {
                let row = HistoryRecord(id: estimate.id, date: estimate.createdAt, weightKg: estimate.estimatedWeightKg,
                                        animalName: estimate.animal?.displayName ?? "", method: estimate.source.title,
                                        kind: .estimate(estimate))
                do {
                    try HistoryQueryService.delete(row, context: modelContext)
                    dismiss()
                } catch { showingDeleteAlert = true }
            }
        } message: { Text("The estimate and its associated saved photo and tape record will be deleted.") }
        .alert("Could not delete estimate", isPresented: $showingDeleteAlert) {
            Button("OK", role: .cancel) { }
        }
    }

    private func detail(_ title: String, _ value: String) -> some View {
        HStack(alignment: .top) {
            Text(title).foregroundStyle(.secondary)
            Spacer(minLength: 10)
            Text(value).multilineTextAlignment(.trailing)
        }.font(.subheadline)
    }
}
