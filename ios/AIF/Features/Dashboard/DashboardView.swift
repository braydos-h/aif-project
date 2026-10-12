import Charts
import Network
import SwiftData
import SwiftUI

struct DashboardView: View {
    @Binding var selectedTab: Int
    @Binding var openCameraRequest: Bool
    @Query(sort: \WeightEstimate.createdAt, order: .reverse) private var estimates: [WeightEstimate]
    @Query private var animals: [Animal]
    @Query(sort: \ScaleMeasurement.measuredAt, order: .reverse) private var scaleMeasurements: [ScaleMeasurement]
    @Query private var providerConfigurations: [AIProviderConfiguration]
    @Query private var preferences: [ApplicationPreferences]
    @State private var modelInfo: InstalledModelInfo?
    @StateObject private var internetStatus = InternetStatusObserver()

    private var activeAnimals: [Animal] { animals.filter { !$0.isArchived } }
    private var weightUnit: WeightUnit { preferences.first?.weightUnit ?? .kilograms }
    private var averageWeight: Double? {
        guard !estimates.isEmpty else { return nil }
        return estimates.reduce(0) { $0 + $1.estimatedWeightKg } / Double(estimates.count)
    }
    private var cloudProviderIsReady: Bool {
        guard let config = providerConfigurations.first(where: { $0.isDefault && $0.provider.isImplemented }) else { return false }
        return (try? KeychainSecretStore().read(reference: config.keychainReference))?.isEmpty == false
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                header
                HStack(spacing: 12) {
                    metricCard(title: "Animals", value: "\(activeAnimals.count)", icon: "pawprint.fill", detail: "active records")
                    metricCard(title: "Estimates", value: "\(estimates.count)", icon: "scalemass.fill", detail: "saved locally")
                }
                AIFCard {
                    HStack(alignment: .firstTextBaseline) {
                        VStack(alignment: .leading, spacing: 5) {
                            Text("Average estimate").font(.subheadline).foregroundStyle(.secondary)
                            Text(averageWeight.map { weightUnit.formatted($0) } ?? "—")
                                .font(.system(.largeTitle, design: .rounded, weight: .bold))
                            Text("From \(estimates.count) saved estimates · verified scale weights are separate")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer(minLength: 8)
                        Image(systemName: "chart.line.uptrend.xyaxis")
                            .font(.title2).foregroundStyle(AIFTheme.green)
                    }
                }
                trendCard
                HStack(spacing: 12) {
                    Button { openCameraRequest = true; selectedTab = 1 } label: {
                        Label("New estimate", systemImage: "plus.viewfinder")
                            .frame(maxWidth: .infinity).frame(minHeight: 52)
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(AIFTheme.green)
                    Button { selectedTab = 1 } label: {
                        Label("Camera", systemImage: "camera.fill")
                            .frame(maxWidth: .infinity).frame(minHeight: 52)
                    }
                    .buttonStyle(.bordered)
                }
                aiStatusCard
                recentEstimates
                recentlyViewed
                if let latestScale = scaleMeasurements.first {
                    AIFCard {
                        Label {
                            Text("Last verified scale weight: \(weightUnit.formatted(latestScale.weightKg))")
                        } icon: { Image(systemName: "checkmark.seal.fill").foregroundStyle(AIFTheme.green) }
                        Text(latestScale.animal?.displayName ?? "No animal linked")
                            .font(.caption).foregroundStyle(.secondary).padding(.leading, 27)
                    }
                }
            }
            .padding(.horizontal, 18)
            .padding(.top, 12)
            .padding(.bottom, 28)
        }
        .background(AIFTheme.warm.opacity(0.45))
        .navigationTitle("Dashboard")
        .navigationBarTitleDisplayMode(.large)
        .task {
            modelInfo = await Task.detached(priority: .utility) { LocalModelStore.currentModelInfo() }.value
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(Date.now.formatted(date: .complete, time: .omitted))
                .font(.subheadline).foregroundStyle(.secondary)
            Text("Your herd, at a glance")
                .font(.title2.weight(.semibold))
        }
        .accessibilityElement(children: .combine)
    }

    private func metricCard(title: String, value: String, icon: String, detail: String) -> some View {
        AIFCard {
            VStack(alignment: .leading, spacing: 8) {
                Image(systemName: icon).font(.title3).foregroundStyle(AIFTheme.green)
                Text(value).font(.system(.title, design: .rounded, weight: .bold)).contentTransition(.numericText())
                Text(title).font(.headline)
                Text(detail).font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private var trendCard: some View {
        AIFCard {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("Weight trend").font(.headline)
                    Spacer()
                    Text("ESTIMATES").font(.caption2.weight(.bold)).tracking(1).foregroundStyle(.secondary)
                }
                if estimates.count >= 2 {
                    Chart(Array(estimates.reversed())) { estimate in
                        LineMark(x: .value("Date", estimate.createdAt),
                                 y: .value(weightUnit.abbreviation, weightUnit.value(fromKilograms: estimate.estimatedWeightKg)))
                            .foregroundStyle(AIFTheme.green.gradient)
                            .interpolationMethod(.catmullRom)
                        PointMark(x: .value("Date", estimate.createdAt),
                                  y: .value(weightUnit.abbreviation, weightUnit.value(fromKilograms: estimate.estimatedWeightKg)))
                            .foregroundStyle(AIFTheme.green)
                    }
                    .chartYAxisLabel(weightUnit.abbreviation)
                    .frame(height: 170)
                    Text("Estimates from different animals and methods are combined. This chart does not represent herd growth.")
                        .font(.caption2).foregroundStyle(.secondary)
                } else {
                    ContentUnavailableView("Trend appears after two estimates", systemImage: "chart.xyaxis.line",
                                           description: Text("Saved tape and photo estimates are stored on this iPhone."))
                        .frame(height: 145)
                }
            }
        }
    }

    private var aiStatusCard: some View {
        AIFCard {
            VStack(alignment: .leading, spacing: 10) {
                Label("AI availability", systemImage: "cpu.fill").font(.headline)
                HStack(alignment: .top, spacing: 10) {
                    Image(systemName: modelInfo == nil ? "exclamationmark.circle" : "checkmark.circle.fill")
                        .foregroundStyle(modelInfo == nil ? .orange : AIFTheme.green)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(modelInfo.map { "Compatible Core ML model installed: \($0.name)" } ?? "Local photo inference unavailable")
                            .font(.subheadline.weight(.medium))
                        Text(modelInfo == nil
                             ? "No compatible model is installed. Tape estimation still works offline."
                             : "Runs on this iPhone without network requests. AIF checks the model interface, not its cattle accuracy.")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
                if let cloud = providerConfigurations.first(where: { $0.isDefault && $0.provider.isImplemented }), cloudProviderIsReady {
                    Label("\(cloud.displayName) ready · internet required when you start a cloud estimate", systemImage: "network")
                        .font(.caption).foregroundStyle(.secondary)
                } else if providerConfigurations.contains(where: { $0.provider.isImplemented }) {
                    Label("Provider details saved · add an API key to use cloud estimates", systemImage: "key")
                        .font(.caption).foregroundStyle(.secondary)
                } else {
                    Label("No cloud provider configured", systemImage: "lock.shield")
                        .font(.caption).foregroundStyle(.secondary)
                }
                if let pathStatus = internetStatus.status {
                    Label(pathStatus == .satisfied ? "Network path available" : "No network path available",
                          systemImage: pathStatus == .satisfied ? "wifi" : "wifi.slash")
                        .font(.caption).foregroundStyle(.secondary)
                } else {
                    Label("Checking internet path…", systemImage: "network")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
        }
    }

    private var recentEstimates: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Recent estimates").font(.title3.weight(.semibold))
                Spacer()
                Button("See history") { selectedTab = 3 }.font(.subheadline.weight(.semibold))
            }
            if estimates.isEmpty {
                AIFCard {
                    Label("Your saved estimates will appear here", systemImage: "clock.arrow.circlepath")
                        .foregroundStyle(.secondary)
                }
            } else {
                ForEach(estimates.prefix(3)) { estimate in
                    NavigationLink {
                        EstimateDetailView(estimate: estimate)
                    } label: {
                        EstimateSummaryRow(estimate: estimate, unit: weightUnit)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private var recentlyViewed: some View {
        let recent = activeAnimals.filter { $0.lastViewedAt != nil }.sorted { ($0.lastViewedAt ?? .distantPast) > ($1.lastViewedAt ?? .distantPast) }.prefix(4)
        return Group {
            if !recent.isEmpty {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Recently viewed").font(.title3.weight(.semibold))
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 10) {
                            ForEach(recent) { animal in
                                NavigationLink { AnimalDetailView(animal: animal) } label: {
                                    VStack(alignment: .leading, spacing: 6) {
                                        Image(systemName: "pawprint.fill").foregroundStyle(AIFTheme.green)
                                        Text(animal.displayName).font(.subheadline.weight(.semibold))
                                        Text(animal.breed.isEmpty ? animal.sex.title : animal.breed)
                                            .font(.caption).foregroundStyle(.secondary)
                                    }
                                    .padding(14).frame(width: 150, alignment: .leading)
                                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 16))
                                }.buttonStyle(.plain)
                            }
                        }
                    }
                }
            }
        }
    }
}

private final class InternetStatusObserver: ObservableObject {
    @Published private(set) var status: NWPath.Status?
    private let monitor = NWPathMonitor()
    private let queue = DispatchQueue(label: "com.aif.network-status")

    init() {
        monitor.pathUpdateHandler = { [weak self] path in
            DispatchQueue.main.async { self?.status = path.status }
        }
        monitor.start(queue: queue)
    }

    deinit { monitor.cancel() }
}
