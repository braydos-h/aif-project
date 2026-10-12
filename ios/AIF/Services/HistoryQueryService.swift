import Foundation

enum HistoryMethodFilter: String, CaseIterable, Identifiable, Equatable {
    case all = "All methods"
    case tape = "Tape"
    case local = "Local AI"
    case cloud = "Cloud AI"
    case scale = "Verified scale"
    var id: String { rawValue }
}

enum HistoryDateFilter: String, CaseIterable, Identifiable {
    case all = "Any date"
    case week = "7 days"
    case month = "30 days"
    case year = "12 months"
    var id: String { rawValue }

    func startDate(now: Date = .now, calendar: Calendar = .current) -> Date? {
        switch self {
        case .all: nil
        case .week: calendar.date(byAdding: .day, value: -7, to: now)
        case .month: calendar.date(byAdding: .day, value: -30, to: now)
        case .year: calendar.date(byAdding: .month, value: -12, to: now)
        }
    }
}

struct HistoryRecord: Identifiable {
    enum Kind { case estimate(WeightEstimate), scale(ScaleMeasurement) }
    let id: UUID
    let date: Date
    let weightKg: Double
    let animalName: String
    let method: String
    let kind: Kind
}

enum HistoryQueryService {
    static func rows(estimates: [WeightEstimate], scales: [ScaleMeasurement],
                     search: String = "", animalID: UUID? = nil,
                     method: HistoryMethodFilter = .all, date: HistoryDateFilter = .all,
                     now: Date = .now, calendar: Calendar = .current) -> [HistoryRecord] {
        let query = search.trimmingCharacters(in: .whitespacesAndNewlines)
        let startDate = date.startDate(now: now, calendar: calendar)
        let estimateRows: [HistoryRecord] = estimates.compactMap { item in
            let matchesMethod: Bool = switch method {
            case .all: true
            case .tape: item.source == .tape
            case .local: item.source == .localModel
            case .cloud: item.source == .cloudModel
            case .scale: false
            }
            guard matchesMethod, animalID == nil || item.animal?.id == animalID,
                  startDate == nil || item.createdAt >= startDate!,
                  query.isEmpty || item.animal?.displayName.localizedCaseInsensitiveContains(query) == true
                    || item.providerName.localizedCaseInsensitiveContains(query)
                    || item.modelName.localizedCaseInsensitiveContains(query) else { return nil }
            return HistoryRecord(id: item.id, date: item.createdAt, weightKg: item.estimatedWeightKg,
                                 animalName: item.animal?.displayName ?? "No linked animal",
                                 method: item.source.title, kind: .estimate(item))
        }
        let scaleRows: [HistoryRecord] = scales.compactMap { item in
            guard method == .all || method == .scale,
                  animalID == nil || item.animal?.id == animalID,
                  startDate == nil || item.measuredAt >= startDate!,
                  query.isEmpty || item.animal?.displayName.localizedCaseInsensitiveContains(query) == true
                    || item.note.localizedCaseInsensitiveContains(query) else { return nil }
            return HistoryRecord(id: item.id, date: item.measuredAt, weightKg: item.weightKg,
                                 animalName: item.animal?.displayName ?? "No linked animal",
                                 method: "Verified scale", kind: .scale(item))
        }
        return (estimateRows + scaleRows).sorted { $0.date > $1.date }
    }

    static func delete(_ record: HistoryRecord, context: ModelContext) throws {
        var photoToDelete: StoredPhoto?
        switch record.kind {
        case .estimate(let estimate):
            if let photoID = estimate.photoID,
               let photo = try context.fetch(FetchDescriptor<StoredPhoto>()).first(where: { $0.id == photoID }) {
                context.delete(photo)
                photoToDelete = photo
            }
            let tapes = try context.fetch(FetchDescriptor<TapeMeasurement>()).filter { $0.relatedEstimateID == estimate.id }
            for tape in tapes { context.delete(tape) }
            context.delete(estimate)
        case .scale(let scale): context.delete(scale)
        }
        do { try context.save() }
        catch { context.rollback(); throw error }
        if let photoToDelete { PhotoStore.delete(photoToDelete) }
    }
}
