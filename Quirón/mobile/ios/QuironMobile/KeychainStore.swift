import Foundation
import Security

enum KeychainStore {
    private static let service = "com.quiron.mobile"

    static func saveToken(_ token: String, account: String = "mobile_token") throws {
        let data = Data(token.utf8)

        let query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account
        ]
        SecItemDelete(query as CFDictionary)

        let attrs: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
            kSecValueData: data,
            kSecAttrAccessible: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        ]

        let status = SecItemAdd(attrs as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw NSError(domain: "KeychainStore", code: Int(status), userInfo: [NSLocalizedDescriptionKey: "No se pudo guardar token en Keychain"])
        }
    }

    static func loadToken(account: String = "mobile_token") throws -> String? {
        let query: [CFString: Any] = [
            kSecClass: kSecClassGenericPassword,
            kSecAttrService: service,
            kSecAttrAccount: account,
            kSecReturnData: true,
            kSecMatchLimit: kSecMatchLimitOne
        ]

        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)

        if status == errSecItemNotFound {
            return nil
        }
        guard status == errSecSuccess else {
            throw NSError(domain: "KeychainStore", code: Int(status), userInfo: [NSLocalizedDescriptionKey: "No se pudo leer token de Keychain"])
        }

        guard let data = item as? Data,
              let token = String(data: data, encoding: .utf8) else {
            return nil
        }
        return token
    }
}
