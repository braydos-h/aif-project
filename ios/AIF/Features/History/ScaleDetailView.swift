import SwiftUI

struct ScaleDetailView: View {
    let measurement: ScaleMeasurement

    var body: some View {
        List {
            Section {
                Label("Verified scale measurement", systemImage: "checkmark.seal.fill").foregroundStyle(.blue)
                Text("\(measurement.weightKg.weightText) kg").font(.largeTitle.weight(.bold))
                Text("\(UnitConverter.pounds(fromKilograms: measurement.weightKg).weightText) lb").font(.title3).foregroundStyle(.secondary)
            }
            Section("Record") {
                LabeledContent("Date", value: measurement.measuredAt.formatted(date: .long, time: .shortened))
                LabeledContent("Animal", value: measurement.animal?.displayName ?? "Not linked")
                if !measurement.note.isEmpty { Text(measurement.note) }
            }
        }
        .navigationTitle("Scale weight")
        .navigationBarTitleDisplayMode(.inline)
    }
}
