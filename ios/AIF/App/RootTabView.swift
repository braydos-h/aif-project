import SwiftData
import SwiftUI

struct RootTabView: View {
    @Environment(\.modelContext) private var modelContext
    @Query private var preferences: [ApplicationPreferences]
    @State private var selection = 0
    @State private var openCameraRequest = false

    var body: some View {
        TabView(selection: $selection) {
            NavigationStack { DashboardView(selectedTab: $selection, openCameraRequest: $openCameraRequest) }
                .tabItem { Label("Dashboard", systemImage: "square.grid.2x2.fill") }
                .tag(0)
            NavigationStack { EstimateView(cameraRequested: $openCameraRequest) }
                .tabItem { Label("Estimate", systemImage: "camera.viewfinder") }
                .tag(1)
            NavigationStack { AnimalsView() }
                .tabItem { Label("Animals", systemImage: "pawprint.fill") }
                .tag(2)
            NavigationStack { HistoryView() }
                .tabItem { Label("History", systemImage: "chart.xyaxis.line") }
                .tag(3)
            NavigationStack { SettingsView() }
                .tabItem { Label("Settings", systemImage: "slider.horizontal.3") }
                .tag(4)
        }
        .tint(AIFTheme.green)
        .preferredColorScheme(colorScheme)
        .task {
            if preferences.isEmpty {
                modelContext.insert(ApplicationPreferences())
                try? modelContext.save()
            }
        }
    }

    private var colorScheme: ColorScheme? {
        switch preferences.first?.appearance ?? .system {
        case .system: nil
        case .light: .light
        case .dark: .dark
        }
    }
}
