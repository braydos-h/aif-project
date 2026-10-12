import Charts
import SwiftData
import SwiftUI
import UIKit

private struct AnimalWeightPoint: Identifiable {
    let id: UUID
    let date: Date
    let kilograms: Double
    let kind: String
}

struct AnimalDetailView: View {
    @Environment(\.modelContext) private var modelContext
    @Environment(\.dismiss) private var dismiss
    let animal: Animal
    @State private var showingEdit = false
    @State private var showingScale = false
    @State private var showingDeleteConfirmation = false
    @State private var operationError = ""
    @State private var showingOperationError = false
    @Query private var storedPhotos: [StoredPhoto]
    @Query private var preferences: [ApplicationPreferences]

    private var weightUnit: WeightUnit { preferences.first?.weightUnit ?? .kilograms }

    private var points: [AnimalWeightPoint] {
        let estimatePoints = animal.estimates.map {
            AnimalWeightPoint(id: $0.id, date: $0.createdAt, kilograms: $0.estimatedWeightKg, kind: "Estimate")
        }
        let scalePoints = animal.scaleMeasurements.map {
            AnimalWeightPoint(id: $0.id, date: $0.measuredAt, kilograms: $0.weightKg, kind: "Verified scale")
        }
        return (estimatePoints + scalePoints).sorted { $0.date < $1.date }
    }
    private var profileImage: UIImage? {
        guard let id = animal.profilePhotoID,
              let photo = storedPhotos.first(where: { $0.id == id }),
              let data = PhotoStore.read(photo) else { return nil }
        return UIImage(data: data)
    }

    var body: some View {
        List {
            Section {
                HStack(spacing: 14) {
                    if let profileImage {
                        Image(uiImage: profileImage).resizable().scaledToFill()
                            .frame(width: 68, height: 68).clipShape(RoundedRectangle(cornerRadius: 18))
                    } else {
                        Image(systemName: "pawprint.fill").font(.largeTitle).foregroundStyle(AIFTheme.green)
                            .frame(width: 68, height: 68).background(AIFTheme.paleGreen, in: RoundedRectangle(cornerRadius: 18))
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        Text(animal.displayName).font(.title2.weight(.bold))
                        Text([animal.tagNumber, animal.breed, animal.sex.title].filter { !$0.isEmpty }.joined(separator: " · "))
                            .font(.subheadline).foregroundStyle(.secondary)
                        if animal.isArchived { Label("Archived", systemImage: "archivebox").font(.caption).foregroundStyle(.secondary) }
                    }
                }.padding(.vertical, 5)
            }
            Section("Record") {
                LabeledContent("Created", value: animal.createdAt.formatted(date: .abbreviated, time: .omitted))
                if let birthDate = animal.birthDate {
                    LabeledContent("Born", value: birthDate.formatted(date: .abbreviated, time: .omitted))
                }
                if animal.approximateAgeYears > 0 { LabeledContent("Approx. age", value: "\(animal.approximateAgeYears.weightText) years") }
                if !animal.notes.isEmpty { Text(animal.notes) }
            }
            Section("Weight history") {
                if points.count >= 2 {
                    Chart(points) { point in
                        LineMark(x: .value("Date", point.date),
                                 y: .value(weightUnit.abbreviation, weightUnit.value(fromKilograms: point.kilograms)))
                            .foregroundStyle(by: .value("Record", point.kind))
                        PointMark(x: .value("Date", point.date),
                                  y: .value(weightUnit.abbreviation, weightUnit.value(fromKilograms: point.kilograms)))
                            .foregroundStyle(by: .value("Record", point.kind))
                    }
                    .chartForegroundStyleScale(["Estimate": AIFTheme.leaf, "Verified scale": Color.blue])
                    .chartYAxisLabel(weightUnit.abbreviation)
                    .frame(height: 200)
                    Text("Scale readings and estimates use separate chart series.").font(.caption).foregroundStyle(.secondary)
                } else if let point = points.first {
                    Text("Latest: \(weightUnit.formatted(point.kilograms)) · \(point.kind)").font(.headline)
                } else {
                    Text("No weights recorded yet.").foregroundStyle(.secondary)
                }
                ForEach(animal.estimates.sorted { $0.createdAt > $1.createdAt }) { estimate in
                    NavigationLink { EstimateDetailView(estimate: estimate) } label: {
                        HStack {
                            Label(estimate.source.title, systemImage: estimate.source == .tape ? "ruler" : "camera")
                            Spacer()
                            Text(weightUnit.formatted(estimate.estimatedWeightKg)).font(.subheadline.monospacedDigit())
                        }
                    }
                }
                ForEach(animal.scaleMeasurements.sorted { $0.measuredAt > $1.measuredAt }) { scale in
                    HStack {
                        Label("Verified scale", systemImage: "checkmark.seal.fill").foregroundStyle(.blue)
                        Spacer()
                        Text(weightUnit.formatted(scale.weightKg)).font(.subheadline.monospacedDigit())
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle(animal.displayName)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("Edit animal", systemImage: "pencil") { showingEdit = true }
                    Button("Add scale weight", systemImage: "scalemass") { showingScale = true }
                    Button(animal.isArchived ? "Restore animal" : "Archive animal", systemImage: "archivebox") {
                        do { try AnimalStore.setArchived(animal, archived: !animal.isArchived, context: modelContext) }
                        catch { operationError = error.localizedDescription; showingOperationError = true }
                    }
                    Button("Delete animal", systemImage: "trash", role: .destructive) { showingDeleteConfirmation = true }
                } label: { Image(systemName: "ellipsis.circle") }
            }
        }
        .sheet(isPresented: $showingEdit) { NavigationStack { AnimalEditorView(animal: animal) } }
        .sheet(isPresented: $showingScale) { NavigationStack { ScaleMeasurementEditor(animal: animal) } }
        .confirmationDialog("Delete \(animal.displayName) and its linked history?", isPresented: $showingDeleteConfirmation,
                            titleVisibility: .visible) {
            Button("Delete Animal and History", role: .destructive) {
                do {
                    try AnimalStore.delete(animal, context: modelContext)
                    dismiss()
                } catch { operationError = error.localizedDescription; showingOperationError = true }
            }
        } message: {
            Text("Linked estimates, measurements, and animal photos will be removed from this iPhone.")
        }
        .alert("Animal action failed", isPresented: $showingOperationError) {
            Button("OK", role: .cancel) { }
        } message: { Text(operationError) }
        .task { try? AnimalStore.markViewed(animal, context: modelContext) }
    }
}
