import SwiftData
import SwiftUI

struct AnimalsView: View {
    @Environment(\.modelContext) private var modelContext
    @Query(sort: \Animal.name) private var animals: [Animal]
    @State private var searchText = ""
    @State private var showArchived = false
    @State private var showingEditor = false
    @State private var operationError = ""
    @State private var showingOperationError = false

    private var visibleAnimals: [Animal] {
        animals.filter { animal in
            animal.isArchived == showArchived
                && (searchText.isEmpty
                    || animal.displayName.localizedCaseInsensitiveContains(searchText)
                    || animal.tagNumber.localizedCaseInsensitiveContains(searchText)
                    || animal.breed.localizedCaseInsensitiveContains(searchText))
        }
    }

    var body: some View {
        List {
            Section {
                Picker("Animals", selection: $showArchived) {
                    Text("Active").tag(false)
                    Text("Archived").tag(true)
                }
                .pickerStyle(.segmented)
                .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
            }
            Section {
                if visibleAnimals.isEmpty {
                    ContentUnavailableView(
                        searchText.isEmpty ? (showArchived ? "No archived animals" : "No animals yet") : "No matching animals",
                        systemImage: "pawprint",
                        description: Text(searchText.isEmpty ? "Add a cattle record to keep estimates and measurements together." : "Try another name, tag, or breed.")
                    )
                    .listRowSeparator(.hidden)
                } else {
                    ForEach(visibleAnimals) { animal in
                        NavigationLink { AnimalDetailView(animal: animal) } label: {
                            AnimalRow(animal: animal)
                        }
                        .contextMenu {
                            Button(animal.isArchived ? "Restore animal" : "Archive animal",
                                   systemImage: animal.isArchived ? "tray.and.arrow.up" : "archivebox") {
                                do { try AnimalStore.setArchived(animal, archived: !animal.isArchived, context: modelContext) }
                                catch { operationError = error.localizedDescription; showingOperationError = true }
                            }
                            Button("Edit animal", systemImage: "pencil") { selectedForEdit = animal }
                            Button("Delete animal", systemImage: "trash", role: .destructive) { pendingDelete = animal }
                        }
                    }
                }
            } header: {
                Text("\(visibleAnimals.count) \(visibleAnimals.count == 1 ? "animal" : "animals")")
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Animals")
        .searchable(text: $searchText, prompt: "Name, tag, or breed")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button { showingEditor = true } label: { Label("Add an animal", systemImage: "plus") }
                    .accessibilityIdentifier("add-animal")
            }
        }
        .sheet(isPresented: $showingEditor) { NavigationStack { AnimalEditorView(animal: nil) } }
        .sheet(item: $selectedForEdit) { animal in NavigationStack { AnimalEditorView(animal: animal) } }
        .confirmationDialog("Delete this animal and its linked history?", isPresented: deleteDialogPresented,
                            titleVisibility: .visible) {
            Button("Delete Animal and History", role: .destructive) {
                guard let pendingDelete else { return }
                do { try AnimalStore.delete(pendingDelete, context: modelContext) }
                catch { operationError = error.localizedDescription; showingOperationError = true }
                self.pendingDelete = nil
            }
        } message: {
            Text("Linked estimates, measurements, and animal photos will be removed from this iPhone.")
        }
        .alert("Animal action failed", isPresented: $showingOperationError) {
            Button("OK", role: .cancel) { }
        } message: { Text(operationError) }
    }

    @State private var selectedForEdit: Animal?
    @State private var pendingDelete: Animal?
    private var deleteDialogPresented: Binding<Bool> {
        Binding(get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } })
    }
}

private struct AnimalRow: View {
    let animal: Animal

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: "pawprint.fill")
                .font(.title3).foregroundStyle(AIFTheme.green)
                .frame(width: 44, height: 44).background(AIFTheme.paleGreen, in: RoundedRectangle(cornerRadius: 14))
            VStack(alignment: .leading, spacing: 3) {
                Text(animal.displayName).font(.headline)
                Text([animal.breed, animal.sex.title, animal.tagNumber].filter { !$0.isEmpty }.joined(separator: " · "))
                    .font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            if animal.isArchived { Text("Archived").font(.caption2).foregroundStyle(.secondary) }
        }
        .padding(.vertical, 5)
        .accessibilityElement(children: .combine)
    }
}
