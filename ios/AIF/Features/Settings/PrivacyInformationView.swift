import SwiftUI

struct PrivacyInformationView: View {
    var body: some View {
        List {
            Section("Stored on this iPhone") {
                Text("Animal records, estimates, measurements, and provider settings are stored in the app's local SwiftData database. Photos and installed Core ML models live in protected application storage.")
                Text("No AIF account, hosted backend, database server, or automatic sync is used by this app.")
            }
            Section("Photo estimation") {
                Text("When a compatible local model is installed, Local AI processes the image on this iPhone and makes no network request. No model is bundled, so local photo inference is unavailable on a fresh install. Cloud AI is optional; a photo is sent only after you choose cloud mode and confirm the send action.")
                Text("Before cloud upload, the image is resized and re-encoded as JPEG to remove camera location metadata. The configured provider receives the photo and the animal details you entered.")
            }
            Section("Credentials and exports") {
                Text("Provider API keys are stored in iOS Keychain. Keys are excluded from JSON exports and backup packages.")
                Text("Exports and backup files contain animal data. Share them only with people you trust.")
            }
        }
        .navigationTitle("Privacy")
        .navigationBarTitleDisplayMode(.inline)
    }
}
