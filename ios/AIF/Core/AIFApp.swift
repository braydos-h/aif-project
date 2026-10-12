import SwiftData
import SwiftUI

@main
struct AIFApp: App {
    private let modelContainer: ModelContainer

    init() {
        do { modelContainer = try ModelContainerFactory.make() }
        catch { fatalError("AIF could not open its private local database: \(error.localizedDescription)") }
    }

    var body: some Scene {
        WindowGroup { RootTabView() }
            .modelContainer(modelContainer)
    }
}
