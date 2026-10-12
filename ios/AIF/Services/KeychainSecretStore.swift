import Foundation
import Security

enum KeychainSecretError: LocalizedError {
    case saveFailed
    case readFailed

    var errorDescription: String? {
        switch self {
        case .saveFailed: "The API key could not be saved to Keychain."
        case .readFailed: "The saved API key could not be read from Keychain."
        }
    }
}

struct KeychainSecretStore {
    private let service = "com.aif.CattleWeightEstimator.provider-key"

    func save(_ secret: String, reference: String) throws {
        let data = Data(secret.utf8)
        let base: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: reference
        ]
        let update = [kSecValueData as String: data,
                      kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly]
        let status = SecItemUpdate(base as CFDictionary, update as CFDictionary)
        if status == errSecItemNotFound {
            var item = base
            item.merge(update) { _, new in new }
            guard SecItemAdd(item as CFDictionary, nil) == errSecSuccess else {
                throw KeychainSecretError.saveFailed
            }
        } else if status != errSecSuccess {
            throw KeychainSecretError.saveFailed
        }
    }

    func read(reference: String) throws -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: reference,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data,
              let value = String(data: data, encoding: .utf8) else {
            throw KeychainSecretError.readFailed
        }
        return value
    }

    func delete(reference: String) {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: reference
        ]
        SecItemDelete(query as CFDictionary)
    }
}
