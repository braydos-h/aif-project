import SwiftData
import SwiftUI
import UIKit

struct StoredPhotosView: View {
    @Environment(\.modelContext) private var modelContext
    @Query(sort: \StoredPhoto.createdAt, order: .reverse) private var photos: [StoredPhoto]
    @Query private var animals: [Animal]
    @State private var pendingDelete: StoredPhoto?
    @State private var errorMessage = ""
    @State private var showErrorAlert = false

    var body: some View {
        List {
            if photos.isEmpty {
                ContentUnavailableView("No saved photos", systemImage: "photo", description: Text("Photos are stored on this iPhone with their estimate or animal record."))
            }
            ForEach(photos) { photo in
                HStack(spacing: 12) {
                    if let data = PhotoStore.read(photo), let image = UIImage(data: data) {
                        Image(uiImage: image).resizable().scaledToFill()
                            .frame(width: 64, height: 64).clipShape(RoundedRectangle(cornerRadius: 12))
                    }
                    VStack(alignment: .leading, spacing: 4) {
                        Text(photo.animalID.flatMap { id in animals.first { $0.id == id }?.displayName } ?? "Photo")
                            .font(.headline)
                        Text("\(ByteCountFormatter.string(fromByteCount: Int64(photo.byteCount), countStyle: .file)) · \(photo.createdAt.formatted(date: .abbreviated, time: .shortened))")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button(role: .destructive) { pendingDelete = photo } label: { Image(systemName: "trash") }
                        .accessibilityLabel("Delete saved photo")
                }
            }
        }
        .navigationTitle("Stored photos")
        .confirmationDialog("Delete this stored photo?", isPresented: Binding(
            get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } }
        ), titleVisibility: .visible) {
            Button("Delete Photo", role: .destructive) {
                guard let pendingDelete else { return }
                modelContext.delete(pendingDelete)
                do {
                    try modelContext.save()
                    PhotoStore.delete(pendingDelete)
                } catch {
                    modelContext.rollback()
                    errorMessage = "Could not delete the photo: \(error.localizedDescription)"
                    showErrorAlert = true
                }
                self.pendingDelete = nil
            }
        } message: { Text("An estimate may remain in history without its photo.") }
        .alert("Photo action failed", isPresented: $showErrorAlert) {
            Button("OK", role: .cancel) { }
        } message: { Text(errorMessage) }
    }
}
