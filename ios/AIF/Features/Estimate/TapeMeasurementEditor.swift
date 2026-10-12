import SwiftData
import SwiftUI

struct TapeMeasurementEditor: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.modelContext) private var modelContext
    let measurement: TapeMeasurement
    @State private var girth: String
    @State private var length: String
    @State private var errorMessage: String?

    init(measurement: TapeMeasurement) {
        self.measurement = measurement
        _girth = State(initialValue: String(measurement.heartGirthCm))
        _length = State(initialValue: String(measurement.bodyLengthCm))
    }

    var body: some View {
        Form {
            Section("Tape measurements") {
                TextField("Heart girth (cm)", text: $girth).keyboardType(.decimalPad)
                TextField("Body length (cm)", text: $length).keyboardType(.decimalPad)
            }
            Text("Valid range for each measurement: 50–300 cm. The associated estimate will be recalculated using the metric Schaeffer formula.")
                .font(.caption).foregroundStyle(.secondary)
            if let errorMessage { Text(errorMessage).foregroundStyle(.red) }
        }
        .navigationTitle("Edit tape measurement")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button("Save") { save() }.accessibilityIdentifier("save-tape-edit")
            }
        }
    }

    private func save() {
        guard let girthValue = parse(girth), let lengthValue = parse(length) else {
            errorMessage = TapeEstimatorError.invalidMeasurement.localizedDescription
            return
        }
        do {
            try EstimateStore.updateTape(measurement, heartGirthCm: girthValue,
                                         bodyLengthCm: lengthValue, context: modelContext)
            dismiss()
        } catch { errorMessage = error.localizedDescription }
    }

    private func parse(_ value: String) -> Double? {
        Double(value.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: ",", with: "."))
    }
}
