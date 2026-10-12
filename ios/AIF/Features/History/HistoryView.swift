import Charts
import SwiftData
import SwiftUI

struct HistoryView: View {
    @Environment(\.modelContext) private var modelContext
    @Query(sort: \WeightEstimate.createdAt, order: .reverse) private var estimates: [WeightEstimate]
    @Query(sort: \ScaleMeasurement.measuredAt, order: .reverse) private var scales: [ScaleMeasurement]
    @Query(sort: \Animal.name) private var animals: [Animal]
    @Query private var preferences: [ApplicationPreferences]
    @State private var searchText = ""
    @State private var method: HistoryMethodFilter = .all
    @State private var date: HistoryDateFilter = .all
    @State private var animalID: UUID?
    @State private var pendingDelete: HistoryRecord?
    @State private var shareURL: URL?
    @State private var showScaleEditor = false
    @State private var errorMessage = ""
    @State private var showErrorAlert = false

    private var rows: [HistoryRecord] {
        HistoryQueryService.rows(estimates: estimates, scales: scales, search: searchText,
                                 animalID: animalID, method: method, date: date)
    }

    var body: some View {
        List {
            Section {
                Picker("Animal", selection: $animalID) {
                    Text("All animals").tag(UUID?.none)
                    ForEach(animals.filter { !$0.isArchived }) { animal in Text(animal.displayName).tag(Optional(animal.id)) }
                }
                Picker("Method", selection: $method) { ForEach(HistoryMethodFilter.allCases) { Text($0.rawValue).tag($0) } }
                Picker("Date", selection: $date) { ForEach(HistoryDateFilter.allCases) { Text($0.rawValue).tag($0) } }
            } header: { Text("Filter history") }

            if rows.count >= 2 {
                Section {
                    Chart(Array(rows.reversed())) { row in
                        LineMark(x: .value("Date", row.date),
                                 y: .value(preferences.first?.weightUnit.abbreviation ?? "kg",
                                           (preferences.first?.weightUnit ?? .kilograms).value(fromKilograms: row.weightKg)),
                                 series: .value("Record type", row.method == "Verified scale" ? "Verified scale" : "Estimate"))
                        PointMark(x: .value("Date", row.date),
                                  y: .value(preferences.first?.weightUnit.abbreviation ?? "kg",
                                            (preferences.first?.weightUnit ?? .kilograms).value(fromKilograms: row.weightKg)))
                            .foregroundStyle(row.method == "Verified scale" ? .blue : AIFTheme.green)
                    }
                    .chartForegroundStyleScale(["Estimate": AIFTheme.green, "Verified scale": .blue])
                    .chartYAxisLabel((preferences.first?.weightUnit ?? .kilograms).abbreviation)
                    .frame(height: 190)
                    Text("Verified scale weights remain a separate series from estimates.")
                        .font(.caption).foregroundStyle(.secondary)
                } header: { Text("Weight trend") }
            }

            Section("\(rows.count) records") {
                if rows.isEmpty {
                    ContentUnavailableView("No matching history", systemImage: "clock",
                                           description: Text("Try a different filter or save an estimate."))
                } else {
                    ForEach(rows) { row in
                        NavigationLink { destination(for: row) } label: { HistoryRowView(record: row, unit: preferences.first?.weightUnit ?? .kilograms) }
                            .contextMenu {
                                Button("Delete record", systemImage: "trash", role: .destructive) { pendingDelete = row }
                            }
                    }
                }
            }
            if let shareURL {
                Section {
                    ShareLink(item: shareURL) { Label("Share \(shareURL.lastPathComponent)", systemImage: "square.and.arrow.up") }
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("History")
        .searchable(text: $searchText, prompt: "Animal, provider, model, or note")
        .toolbar {
            ToolbarItemGroup(placement: .topBarTrailing) {
                Button { showScaleEditor = true } label: { Image(systemName: "scalemass") }
                    .accessibilityLabel("Add verified scale weight")
                Menu {
                    Button("Export CSV", systemImage: "tablecells") { export(.csv) }
                    Button("Export JSON", systemImage: "curlybraces") { export(.json) }
                } label: { Image(systemName: "square.and.arrow.up") }
            }
        }
        .sheet(isPresented: $showScaleEditor) { NavigationStack { ScaleMeasurementEditor(animal: animals.first { $0.id == animalID }) } }
        .confirmationDialog("Delete this history record?", isPresented: Binding(
            get: { pendingDelete != nil }, set: { if !$0 { pendingDelete = nil } }
        ), titleVisibility: .visible) {
            Button("Delete Record", role: .destructive) { deletePending() }
        } message: { Text("A linked photo and tape measurement will also be removed when deleting an estimate.") }
        .alert("History action failed", isPresented: $showErrorAlert) {
            Button("OK", role: .cancel) { }
        } message: { Text(errorMessage) }
    }

    @ViewBuilder
    private func destination(for row: HistoryRecord) -> some View {
        switch row.kind {
        case .estimate(let estimate): EstimateDetailView(estimate: estimate)
        case .scale(let scale): ScaleDetailView(measurement: scale)
        }
    }

    private enum ExportKind: Equatable { case csv, json }
    private func export(_ kind: ExportKind) {
        do {
            let data: Data
            let name: String
            if kind == .csv {
                data = HistoryExportService.csv(estimates: estimates, scales: scales)
                name = "aif-history.csv"
            } else {
                data = try BackupService.jsonExport(context: modelContext)
                name = "aif-history.json"
            }
            let url = FileManager.default.temporaryDirectory.appendingPathComponent(name)
            try data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
            shareURL = url
        } catch {
            errorMessage = "Could not create the export: \(error.localizedDescription)"
            showErrorAlert = true
        }
    }

    private func deletePending() {
        guard let pendingDelete else { return }
        do { try HistoryQueryService.delete(pendingDelete, context: modelContext) }
        catch {
            errorMessage = "Could not delete the history record: \(error.localizedDescription)"
            showErrorAlert = true
        }
        self.pendingDelete = nil
    }
}

private struct HistoryRowView: View {
    let record: HistoryRecord
    let unit: WeightUnit
    private var verified: Bool { record.method == "Verified scale" }

    var body: some View {
        HStack(spacing: 11) {
            Image(systemName: verified ? "checkmark.seal.fill" : (record.method == "Tape estimate" ? "ruler" : "camera.fill"))
                .foregroundStyle(verified ? .blue : AIFTheme.green).frame(width: 32)
            VStack(alignment: .leading, spacing: 3) {
                Text(record.animalName).font(.headline)
                Text("\(record.method) · \(record.date.formatted(date: .abbreviated, time: .shortened))")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            Text(unit.formatted(record.weightKg)).font(.subheadline.weight(.semibold).monospacedDigit())
        }
        .padding(.vertical, 4)
        .accessibilityElement(children: .combine)
    }
}
