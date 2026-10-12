import SwiftUI

struct CameraSheet: View {
    @Environment(\.dismiss) private var dismiss
    @StateObject private var camera = CameraCaptureService()
    let onCapture: (Data) -> Void

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            CameraPreview(session: camera.session).ignoresSafeArea()
            VStack {
                HStack {
                    Button("Close") { camera.stop(); dismiss() }
                        .padding(12).background(.ultraThinMaterial, in: Capsule())
                    Spacer()
                }.padding()
                Spacer()
                if let error = camera.errorMessage {
                    Text(error).font(.subheadline).multilineTextAlignment(.center)
                        .padding().background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 14))
                        .padding(.horizontal)
                }
                Button {
                    camera.capture()
                } label: {
                    Circle().fill(.white).frame(width: 72, height: 72)
                        .overlay(Circle().stroke(.black.opacity(0.2), lineWidth: 2))
                        .padding(6).overlay(Circle().stroke(.white, lineWidth: 3))
                }
                .disabled(!camera.isReady)
                .accessibilityLabel("Take photo")
                .padding(.bottom, 26)
            }
        }
        .task { await camera.start() }
        .onChange(of: camera.capturedData) { _, data in
            guard let data else { return }
            camera.stop()
            onCapture(data)
            dismiss()
        }
        .onDisappear { camera.stop() }
    }
}
