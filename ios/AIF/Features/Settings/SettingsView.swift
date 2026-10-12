import SwiftData
import SwiftUI
import UniformTypeIdentifiers

struct SettingsView: View {
    @Environment(\.modelContext) private var modelContext
    @Query private var preferences: [ApplicationPreferences]
    @Query private var providers: [AIProviderConfiguration]
    @Query private var photos: [StoredPhoto]
    @State private var showProviderSettings = false
    @State private var showFileImporter = false
    @State private var importKind: ImportKind = .backup
    @State private var showDeleteConfirmation = false
    @State private var showImportSummary = false
    @State private var importSummary = ""
    @State private var notice: String?
    @State private var shareURL: URL?
    @State private var isWorking = false
    @State private var modelInfo: InstalledModelInfo?
    @State private var storageBytes: Int64 = 0

    private var settings: ApplicationPreferences { preferences.first ?? ApplicationPreferences() }
    private var provider: AIProviderConfiguration? { providers.first(where: { $0.provider.isImplemented }) }

    var body: some View {
        Form {
            Section("General") {
                Picker("Default weight unit", selection: Binding(
                    get: { settings.weightUnit }, set: { value in updatePreferences { $0.weightUnit = value } }
                )) {
                    ForEach(WeightUnit.allCases) { Text($0.title).tag($0) }
                }
                Toggle("Show pounds alongside kilograms", isOn: Binding(
                    get: { settings.includePounds }, set: { value in updatePreferences { $0.includePounds = value } }
                ))
                Picker("Appearance", selection: Binding(
                    get: { settings.appearance }, set: { value in updatePreferences { $0.appearance = value } }
                )) {
                    ForEach(AppAppearance.allCases) { Text($0.title).tag($0) }
                }
                Picker("Photo inference mode", selection: Binding(
                    get: { settings.inferenceMode }, set: { value in updatePreferences { $0.inferenceMode = value } }
                )) {
                    ForEach(InferenceMode.allCases) { Text($0.title).tag($0) }
                }
                Text("Tape estimates and animal records work without an AI provider or internet connection.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section("Artificial Intelligence") {
                LabeledContent("Local AI", value: modelInfo.map { "Compatible model · \($0.name)" } ?? "Unavailable")
                if let modelInfo {
                    LabeledContent("Model version", value: String(modelInfo.version.prefix(23)))
                    Text("AIF checks the image input and numeric weight output, not the model's cattle training, accuracy, or licence.")
                        .font(.caption).foregroundStyle(.secondary)
                    Button("Remove installed model", role: .destructive) {
                        do {
                            try LocalModelStore.removeAll()
                            self.modelInfo = nil
                            notice = "Local model removed. Offline photo inference is unavailable."
                        } catch { notice = "Could not remove the local model: \(error.localizedDescription)" }
                    }
                }
                Button {
                    importKind = .model
                    showFileImporter = true
                } label: {
                    Label(isWorking ? "Installing model…" : "Install Core ML model", systemImage: "square.and.arrow.down")
                }.disabled(isWorking)
                Text("Choose a cattle-weight model that you have independently validated and are licensed to use. AIF checks the image input and weight output format. It never downloads model weights.")
                    .font(.caption).foregroundStyle(.secondary)
                Button { showProviderSettings = true } label: {
                    HStack {
                        Label("Bring Your Own Key", systemImage: "key.fill")
                        Spacer()
                        Text(provider?.displayName ?? "Not configured").font(.caption).foregroundStyle(.secondary)
                    }
                }
                Text("Implemented provider: OpenAI-compatible chat-completions vision APIs. Gemini and Ollama support are not implemented.")
                    .font(.caption).foregroundStyle(.secondary)
            }

            Section("Privacy and Storage") {
                LabeledContent("App storage", value: ByteCountFormatter.string(fromByteCount: storageBytes, countStyle: .file))
                LabeledContent("Stored photos", value: "\(photos.count)")
                NavigationLink { StoredPhotosView() } label: { Label("Manage stored photos", systemImage: "photo.on.rectangle") }
                Button { createExport(.json) } label: { Label("Export JSON data", systemImage: "curlybraces") }
                Button { createExport(.csv) } label: { Label("Export CSV history", systemImage: "tablecells") }
                Button { createExport(.backup) } label: { Label("Create full backup with photos", systemImage: "archivebox") }
                Button {
                    importKind = .backup
                    showFileImporter = true
                } label: { Label("Import backup", systemImage: "square.and.arrow.down") }
                if let shareURL {
                    ShareLink(item: shareURL) { Label("Share \(shareURL.lastPathComponent)", systemImage: "square.and.arrow.up") }
                }
                if let notice { Text(notice).font(.caption).foregroundStyle(.secondary) }
                Button("Delete all local data", role: .destructive) { showDeleteConfirmation = true }
                    .accessibilityIdentifier("delete-all-data")
            }

            Section("About") {
                LabeledContent("Version", value: appVersion)
                NavigationLink { PrivacyInformationView() } label: { Label("Privacy information", systemImage: "hand.raised") }
                NavigationLink {
                    OpenSourceLicensesView()
                } label: { Label("Open-source licences", systemImage: "text.book.closed") }
                Text("Weight estimates are not verified measurements. Always confirm weight with a scale.")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .navigationTitle("Settings")
        .task {
            modelInfo = await Task.detached(priority: .utility) { LocalModelStore.currentModelInfo() }.value
            storageBytes = StorageUsageService.applicationSupportBytes()
        }
        .sheet(isPresented: $showProviderSettings) { NavigationStack { ProviderSettingsView(configuration: provider) } }
        .fileImporter(isPresented: $showFileImporter,
                      allowedContentTypes: importKind == .model ? [.item] : [.aifBackup, .json],
                      allowsMultipleSelection: false) { result in
            switch importKind {
            case .model: installModel(result)
            case .backup: importBackup(result)
            }
        }
        .confirmationDialog("Delete all local AIF data?", isPresented: $showDeleteConfirmation, titleVisibility: .visible) {
            Button("Delete All Local Data", role: .destructive) {
                do {
                    try DataManagementService.deleteAllLocalData(context: modelContext)
                    shareURL = nil
                    modelInfo = nil
                    storageBytes = StorageUsageService.applicationSupportBytes()
                    notice = "Local animal data, estimates, measurements, photos, provider credentials, and installed model files were deleted."
                } catch { notice = "Could not delete all data: \(error.localizedDescription)" }
            }
        } message: {
            Text("This permanently removes animal records, estimates, measurement history, photos, saved provider keys, and installed local models from this iPhone.")
        }
        .alert("Backup import", isPresented: $showImportSummary) {
            Button("Done", role: .cancel) { }
        } message: { Text(importSummary) }
    }

    private var appVersion: String {
        let version = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "1.0"
        let build = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "1"
        return "\(version) (\(build))"
    }

    private func updatePreferences(_ update: (ApplicationPreferences) -> Void) {
        let record = preferences.first ?? ApplicationPreferences()
        update(record)
        if preferences.isEmpty { modelContext.insert(record) }
        try? modelContext.save()
    }

    private enum ExportKind { case json, csv, backup }
    private enum ImportKind: Equatable { case model, backup }

    private func createExport(_ kind: ExportKind) {
        do {
            let data: Data
            let fileName: String
            switch kind {
            case .json:
                data = try BackupService.jsonExport(context: modelContext)
                fileName = "aif-data-\(timestamp()).json"
            case .csv:
                data = try HistoryExportService.csv(estimates: modelContext.fetch(FetchDescriptor<WeightEstimate>()),
                                                    scales: modelContext.fetch(FetchDescriptor<ScaleMeasurement>()))
                fileName = "aif-history-\(timestamp()).csv"
            case .backup:
                data = try BackupService.fullBackup(context: modelContext)
                fileName = "aif-backup-\(timestamp()).aifbackup"
            }
            let url = FileManager.default.temporaryDirectory.appendingPathComponent(fileName)
            try data.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
            shareURL = url
            notice = "Export ready. Use Share to choose where to save or send it. API keys are excluded."
        } catch { notice = "Could not create export: \(error.localizedDescription)" }
    }

    private func installModel(_ result: Result<[URL], Error>) {
        switch result {
        case .failure(let error): notice = error.localizedDescription
        case .success(let urls):
            guard let source = urls.first else { return }
            let access = source.startAccessingSecurityScopedResource()
            isWorking = true
            Task {
                do {
                    let info = try await Task.detached(priority: .userInitiated) { try LocalModelStore.install(from: source) }.value
                    modelInfo = info
                    notice = "Installed local model \(info.name). Check its model card before using it for cattle weight estimates."
                } catch { notice = error.localizedDescription }
                if access { source.stopAccessingSecurityScopedResource() }
                isWorking = false
                storageBytes = StorageUsageService.applicationSupportBytes()
            }
        }
    }

    private func importBackup(_ result: Result<[URL], Error>) {
        switch result {
        case .failure(let error): notice = error.localizedDescription
        case .success(let urls):
            guard let url = urls.first else { return }
            let access = url.startAccessingSecurityScopedResource()
            do {
                let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
                let size = (attributes[.size] as? NSNumber)?.intValue ?? 0
                guard size <= BackupService.maximumBackupBytes else { throw BackupServiceError.fileTooLarge }
                let data = try Data(contentsOf: url, options: .mappedIfSafe)
                let imported = try BackupService.importBackup(data, context: modelContext)
                importSummary = "Added \(imported.animalsAdded) animals, \(imported.estimatesAdded) estimates, \(imported.measurementsAdded) measurements, and \(imported.photosAdded) photos. Skipped \(imported.duplicatesSkipped) duplicate records."
                showImportSummary = true
                storageBytes = StorageUsageService.applicationSupportBytes()
            } catch { notice = "Import failed safely: \(error.localizedDescription)" }
            if access { url.stopAccessingSecurityScopedResource() }
        }
    }

    private func timestamp() -> String {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyyMMdd-HHmmss"
        return formatter.string(from: .now)
    }
}

private struct OpenSourceLicensesView: View {
    var body: some View {
        List {
            Section("Apple frameworks") {
                Text("AIF uses Apple SwiftUI, SwiftData, Swift Charts, Vision, Core ML, PhotosUI, AVFoundation, Security, and CryptoKit. These are provided by the iOS SDK.")
            }
            Section("External packages") {
                Text("No third-party package dependencies are currently included.")
            }
            Section("Project licence") {
                Text("See the repository LICENSE file for AIF project terms.")
            }
        }.navigationTitle("Open-source licences").navigationBarTitleDisplayMode(.inline)
    }
}
