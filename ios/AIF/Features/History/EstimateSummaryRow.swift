import SwiftUI

struct EstimateSummaryRow: View {
    let estimate: WeightEstimate
    var unit: WeightUnit = .kilograms

    var body: some View {
        AIFCard {
            HStack(spacing: 12) {
                Image(systemName: estimate.source == .tape ? "ruler" : "camera.fill")
                    .foregroundStyle(AIFTheme.green).frame(width: 34, height: 34)
                    .background(AIFTheme.paleGreen, in: Circle())
                VStack(alignment: .leading, spacing: 3) {
                    Text(estimate.animal?.displayName ?? estimate.source.title).font(.headline)
                    Text("\(estimate.source.title) · \(estimate.createdAt.formatted(date: .abbreviated, time: .shortened))")
                        .font(.caption).foregroundStyle(.secondary)
                }
                Spacer()
                Text(unit.formatted(estimate.estimatedWeightKg)).font(.headline.monospacedDigit())
            }
        }
    }
}
