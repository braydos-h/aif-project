import SwiftData
import SwiftUI

struct ProviderSettingsView: View {
    @Environment(\.dismiss) private var dismiss
    @Environment(\.modelContext) private var modelContext
    let configuration: AIProviderConfiguration?

    @State private var displayName: String
    @State private var modelName: String
    @State private var endpoint: String
    @State private var apiKey = ""
    @State private var statusMessage: String?
    @State private var isWorking = false

    init(configuration: AIProviderConfiguration?) {
        self.configuration = configuration
        _displayName = State(initialValue: configuration?.displayName ?? "OpenAI-compatible provider")
        _modelName = State(initialValue: configuration?.modelName ?? "gpt-4o-mini")
        _endpoint = State(initialValue: configuration?.endpoint ?? "https://api.openai.com/v1/chat/completions")
    }

    var body: some View {
        Form {
            Section("Implemented provider adapter") {
                Label("OpenAI-compatible vision API", systemImage: "checkmark.seal.fill")
                    .foregroundStyle(AIFTheme.green)
                Text("This adapter sends one selected photo in a user-initiated HTTPS request. Compatible endpoints must accept chat completions with image_url content.")
                    .font(.caption).foregroundStyle(.secondary)
                Text("Gemini and Ollama adapters are not implemented in this version.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            Section("Connection") {
                TextField("Provider name", text: $displayName).textInputAutocapitalization(.words)
                TextField("Model name", text: $modelName).textInputAutocapitalization(.never).autocorrectionDisabled()
                TextField("Chat completions endpoint", text: $endpoint)
                    .keyboardType(.URL).textInputAutocapitalization(.never).autocorrectionDisabled()
                SecureField("API key", text: $apiKey).textInputAutocapitalization(.never).autocorrectionDisabled()
                    .accessibilityIdentifier("provider-api-key")
                if configurationHasKey {
                    Label("An API key is stored in this iPhone's Keychain.", systemImage: "lock.fill")
                        .font(.caption).foregroundStyle(.secondary)
                    Button("Delete saved API key", role: .destructive) { deleteKey() }
                } else {
                    Text("API keys are saved only in Keychain. They are never added to the local records or backup.")
                        .font(.caption).foregroundStyle(.secondary)
                }
            }
            Section("Connection test") {
                Button {
                    Task { await testConnection() }
                } label: {
                    HStack {
                        if isWorking { ProgressView().padding(.trailing, 4) }
                        Text("Send text-only connection test")
                    }
                }
                .disabled(isWorking)
                Text("This sends a short text prompt to the configured endpoint. It does not send a photo.")
                    .font(.caption).foregroundStyle(.secondary)
            }
            if let statusMessage {
                Section { Text(statusMessage).font(.subheadline).foregroundStyle(statusMessage.hasPrefix("Connected") ? AIFTheme.green : .red) }
            }
        }
        .navigationTitle("Bring Your Own Key")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) { Button("Save") { save() }.disabled(isWorking) }
        }
    }

    private var configurationHasKey: Bool {
        guard let configuration else { return false }
        return (try? KeychainSecretStore().read(reference: configuration.keychainReference))?.isEmpty == false
    }

    private var settings: CloudProviderSettings {
        CloudProviderSettings(name: displayName.trimmingCharacters(in: .whitespacesAndNewlines),
                              endpoint: endpoint.trimmingCharacters(in: .whitespacesAndNewlines),
                              modelName: modelName.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    private func save() {
        var keychainReferenceToRemove: String?
        do {
            _ = try ProviderConfigurationValidator.validate(endpoint: settings.endpoint, modelName: settings.modelName)
            let cleanName = settings.name.isEmpty ? "OpenAI-compatible provider" : settings.name
            let record = configuration ?? AIProviderConfiguration(displayName: cleanName, modelName: settings.modelName,
                                                                    endpoint: settings.endpoint, keychainReference: UUID().uuidString)
            record.displayName = cleanName
            record.modelName = settings.modelName
            record.endpoint = settings.endpoint
            record.isDefault = true
            record.updatedAt = .now
            if let configuration {
                let others = try modelContext.fetch(FetchDescriptor<AIProviderConfiguration>()).filter { $0.id != configuration.id }
                others.forEach { $0.isDefault = false }
            }
            if !apiKey.isEmpty {
                try KeychainSecretStore().save(apiKey, reference: record.keychainReference)
                if configuration == nil { keychainReferenceToRemove = record.keychainReference }
            }
            if configuration == nil { modelContext.insert(record) }
            try modelContext.save()
            apiKey = ""
            dismiss()
        } catch {
            modelContext.rollback()
            if let keychainReferenceToRemove { KeychainSecretStore().delete(reference: keychainReferenceToRemove) }
            statusMessage = error.localizedDescription
        }
    }

    private func deleteKey() {
        guard let configuration else { return }
        KeychainSecretStore().delete(reference: configuration.keychainReference)
        statusMessage = "Saved API key deleted from Keychain."
    }

    private func testConnection() async {
        isWorking = true
        statusMessage = nil
        do {
            _ = try ProviderConfigurationValidator.validate(endpoint: settings.endpoint, modelName: settings.modelName)
            let key: String
            if !apiKey.isEmpty { key = apiKey }
            else if let configuration,
                    let saved = try KeychainSecretStore().read(reference: configuration.keychainReference) { key = saved }
            else { throw WeightEstimationError.providerNotConfigured }
            try await OpenAICompatibleVisionProvider.testConnection(settings: settings, apiKey: key)
            statusMessage = "Connected. The provider accepted the text-only request."
        } catch { statusMessage = error.localizedDescription }
        isWorking = false
    }
}
