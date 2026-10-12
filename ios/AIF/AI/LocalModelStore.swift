import CryptoKit
import Foundation
import CoreML

struct InstalledModelInfo: Equatable, Sendable {
    let name: String
    let version: String
    let location: URL
}

enum LocalModelStoreError: LocalizedError {
    case unsupportedFile
    case incompatibleModel
    case unavailable

    var errorDescription: String? {
        switch self {
        case .unsupportedFile: "Choose a compiled Core ML model, .mlmodel, or .mlpackage."
        case .incompatibleModel: "The model must accept an image and return a numeric `weight_kg` or `estimated_weight_kg` output."
        case .unavailable: "The model could not be installed in protected app storage."
        }
    }
}

enum LocalModelStore {
    static func installedModels() -> [URL] {
        guard let folder = try? modelsDirectory(),
              let contents = try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.isDirectoryKey]) else { return [] }
        return contents.filter { $0.pathExtension == "mlmodelc" }
    }

    static func currentModelInfo() -> InstalledModelInfo? {
        guard let model = installedModels().sorted(by: { $0.lastPathComponent < $1.lastPathComponent })
                .first(where: { url in
                    guard let loaded = try? MLModel(contentsOf: url) else { return false }
                    return supportsWeightInference(loaded.modelDescription)
                }),
              let digest = modelDigest(at: model) else { return nil }
        return InstalledModelInfo(name: model.deletingPathExtension().lastPathComponent,
                                  version: "sha256-\(digest)", location: model)
    }

    @discardableResult
    static func install(from source: URL) throws -> InstalledModelInfo {
        let ext = source.pathExtension.lowercased()
        guard ["mlmodel", "mlpackage", "mlmodelc"].contains(ext) else { throw LocalModelStoreError.unsupportedFile }
        let folder = try modelsDirectory()
        let destination = folder.appendingPathComponent("\(source.deletingPathExtension().lastPathComponent)-\(UUID().uuidString).mlmodelc", isDirectory: true)
        let temporary: URL
        if ext == "mlmodelc" {
            temporary = source
        } else {
            do { temporary = try MLModel.compileModel(at: source) }
            catch { throw LocalModelStoreError.unavailable }
        }
        do {
            try FileManager.default.copyItem(at: temporary, to: destination)
            guard let model = try? MLModel(contentsOf: destination),
                  supportsWeightInference(model.modelDescription) else {
                throw LocalModelStoreError.incompatibleModel
            }
            guard let digest = modelDigest(at: destination) else { throw LocalModelStoreError.unavailable }
            for old in installedModels() where old != destination {
                try FileManager.default.removeItem(at: old)
            }
            if ext != "mlmodelc" { try? FileManager.default.removeItem(at: temporary) }
            return InstalledModelInfo(name: destination.deletingPathExtension().lastPathComponent,
                                      version: "sha256-\(digest)", location: destination)
        } catch {
            try? FileManager.default.removeItem(at: destination)
            if ext != "mlmodelc" { try? FileManager.default.removeItem(at: temporary) }
            if let error = error as? LocalModelStoreError { throw error }
            throw LocalModelStoreError.unavailable
        }
    }

    static func removeAll() throws {
        for model in installedModels() {
            try FileManager.default.removeItem(at: model)
        }
    }

    private static func supportsWeightInference(_ description: MLModelDescription) -> Bool {
        let acceptsImage = description.inputDescriptionsByName.values.contains { $0.type == .image }
        let supportedWeightNames: Set<String> = ["weight_kg", "estimated_weight_kg"]
        let returnsNumericWeight = description.outputDescriptionsByName.values.contains { feature in
            supportedWeightNames.contains(feature.name.lowercased())
                && [.double, .int64, .multiArray].contains(feature.type)
        }
        return acceptsImage && returnsNumericWeight
    }

    private static func modelsDirectory() throws -> URL {
        let base = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        let directory = base.appendingPathComponent("AIF/Models", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: directory.path)
        return directory
    }

    private static func modelDigest(at modelURL: URL) -> String? {
        guard let enumerator = FileManager.default.enumerator(at: modelURL, includingPropertiesForKeys: [.isRegularFileKey]) else { return nil }
        let files = (enumerator.allObjects as? [URL] ?? []).filter {
            (try? $0.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true
        }.sorted { $0.path < $1.path }
        guard !files.isEmpty else { return nil }
        var hasher = SHA256()
        for file in files {
            hasher.update(data: Data(file.lastPathComponent.utf8))
            guard let handle = try? FileHandle(forReadingFrom: file) else { return nil }
            defer { try? handle.close() }
            while true {
                let block: Data?
                do { block = try handle.read(upToCount: 1_048_576) }
                catch { return nil }
                guard let block, !block.isEmpty else { break }
                hasher.update(data: block)
            }
        }
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }
}
