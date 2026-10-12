# AIF for iPhone

AIF is a native SwiftUI cattle-record and weight-estimation app for iOS 17 and
later. It is a separate product from the Rust web app and does not call or
require the AIF server.

## Requirements

- macOS with Xcode 15 or later and the iOS 17 SDK.
- An iOS 17 simulator or physical iPhone.
- No third-party Swift packages, server, account, subscription, or production
  secret is required to build or launch the app.

## Open, build, and run

Open `AIF.xcodeproj` in Xcode, select the `AIF` scheme, choose an iOS 17 or
later iPhone simulator, and press Run. The shared scheme includes unit and UI
tests. From the repository root, run them with:

```sh
xcodebuild -project ios/AIF.xcodeproj -scheme AIF \
  -destination 'platform=iOS Simulator,name=iPhone 16' \
  CODE_SIGNING_ALLOWED=NO test
```

For a physical iPhone, connect and trust the device, select it as the run
destination, then choose a development team in **Signing & Capabilities**.
The project uses the placeholder bundle identifier
`com.aif.cattleweightestimator`; change it if it conflicts with an identifier
in your Apple developer account. Camera access requires a physical device.

## What works offline

These features use SwiftData and protected files on the iPhone and work without
internet, AIF accounts, or a server:

- Create, edit, archive, search, and delete cattle records, including breed,
  sex, age, notes, and a profile photo.
- Take or choose photos for an animal, or save selected estimate photos
  directly to local storage. Photos are re-encoded to remove camera and
  location metadata.
- Calculate metric Schaeffer tape estimates from 50–300 cm heart-girth and
  body-length measurements, including the nominal ±5% range; edit the
  measurements and keep estimates in local history.
- Record verified scale weights separately from estimates.
- Browse animal histories, filter and search history, view trend charts,
  remove records and associated files, and export JSON, CSV, or a full backup.
- Import a compatible AIF backup and manage local photos and stored records.

Photo capture, image selection, editing, and local storage do not require an
AI provider. **There is no trained cattle-weight model in this repository and
no local model is bundled with the app. Offline photo weight inference is
therefore unavailable in a fresh installation.** The Core ML/Vision adapter
accepts an imported image model only when its input/output schema is
compatible. AIF cannot verify the model's training data, cattle specificity,
accuracy, or redistribution rights. Only install a cattle-weight model that
you have independently validated and are licensed to use. AIF never downloads
model weights.

## Bring Your Own Key

Settings currently implements one cloud adapter: OpenAI-compatible
chat-completions vision APIs. Gemini and Ollama are listed as future adapter
directions and are **not supported** by this build. Configure the provider
name, model, endpoint, and API key under **Settings → Artificial Intelligence**.
Hosted endpoints must use HTTPS; HTTP is accepted only for loopback testing.
The connection test sends a short text-only request and does not send a photo.

Choosing **Bring Your Own Key** for photo inference requires internet. A photo
is sent only after the user presses the cloud estimate action and confirms the
disclosure. The selected, metadata-stripped image and entered animal context
go to the configured endpoint. API keys are stored in Keychain, are not stored
in SwiftData, and are omitted from every export. Cloud results are labeled as
cloud estimates and retain provider/model provenance.

## Local model installation

In **Settings → Artificial Intelligence**, use **Install Core ML model** to
choose a compiled `.mlmodelc`, `.mlmodel`, or `.mlpackage` file from Files. The
model must accept a Vision image input and return a numeric `weight_kg` or
`estimated_weight_kg` output. Optional bounds must be returned as numeric
`lower_bound_kg`/`upper_bound_kg` (or the documented min/max aliases). AIF
checks this interface and rejects malformed predictions; it cannot validate a
model's scientific quality. Model files stay in protected local app storage,
and each saved estimate records the model name and a content digest version.

## Local storage, privacy, and backups

Animal records and estimate metadata are stored in a versioned SwiftData
container. Photos and imported model packages are stored as files in protected
Application Support storage; large image bytes are not put in SwiftData.
Photos, records, and provider configuration are never uploaded automatically.
The only photo upload is the explicit cloud-estimation action described above.

Use **Export JSON data** for a portable record export without photo bytes, **Export
CSV history** for spreadsheet use, or **Create full backup with photos** to
make an `.aifbackup` file. Backups include animal and estimate records plus
their photos, but never API keys. Use **Import backup** on the new device.
Imports validate identifiers, relationships, measurements, images, and file
size before saving; existing record IDs are skipped as duplicates. AIF does
not use CloudKit or an AIF server. iOS device-backup behavior is controlled by
the device owner and Apple system settings.

## Current limitations

- No cattle-weight Core ML model or cattle reference dataset is included, so
  local photo inference is unavailable until an independently validated
  compatible model is supplied.
- The OpenAI-compatible vision adapter is implemented, but requires the user's
  own provider account, API key, internet access, and a vision-capable model.
  Provider response compatibility varies; failed or malformed responses do
  not create estimates.
- Gemini and Ollama adapters are not implemented.
- The app has not been compiled or run in this Linux development environment:
  Xcode, the iOS SDK, and `xcodebuild` are unavailable here. Run the shared
  Xcode scheme on macOS before distributing a build.
- SwiftData migration version 1 is in place. Future schema changes need a new
  versioned schema and explicit migration stage.
