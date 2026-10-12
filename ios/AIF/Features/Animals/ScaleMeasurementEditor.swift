import SwiftData
import SwiftUI

struct ScaleMeasurementEditor: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.modelContext) private var modelContext
    let animal: Animal?
    @State private var weight = ""
    @State private var date = Date.now
    @State private var note = ""
    @State private var errorMessage: String?

    var body: some View {
        Form {
            Section("Verified measurement") {
                TextField("Weight in kilograms", text: $weight).keyboardType(.decimalPad)
                    .accessibilityIdentifier("scale-weight")
                DatePicker("Measured", selection: $date, displayedComponents: [.date, .hourAndMinute])
                TextField("Note", text: $note, axis: .vertical).lineLimit(2...5)
            }
            Text("Scale readings are kept separate from AI and tape estimates.").font(.caption).foregroundStyle(.secondary)
            if let errorMessage { Text(errorMessage).foregroundStyle(.red) }
        }
        .navigationTitle("Add scale weight")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) { Button("Save") { save() }.accessibilityIdentifier("save-scale") }
        }
    }

    private func save() {
        guard let value = Double(weight.replacingOccurrences(of: ",", with: ".")), value.isFinite, value > 0 else {
            errorMessage = "Enter a scale weight greater than zero kilograms."; return
        }
        do {
            _ = try EstimateStore.saveScale(weightKg: value, measuredAt: date, note: note, animal: animal, context: modelContext)
            dismiss()
        } catch { errorMessage = error.localizedDescription }
    }
}
