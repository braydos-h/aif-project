import AVFoundation
import Combine
import Foundation

final class CameraCaptureService: NSObject, ObservableObject, AVCapturePhotoCaptureDelegate {
    let session = AVCaptureSession()
    @Published private(set) var errorMessage: String?
    @Published private(set) var isReady = false
    @Published private(set) var capturedData: Data?

    private let sessionQueue = DispatchQueue(label: "com.aif.camera.session")
    private let output = AVCapturePhotoOutput()

    func start() async {
        let granted: Bool
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized: granted = true
        case .notDetermined: granted = await AVCaptureDevice.requestAccess(for: .video)
        default: granted = false
        }
        guard granted else {
            DispatchQueue.main.async { self.errorMessage = "Camera access is turned off. Allow camera access in iPhone Settings or choose from Photos." }
            return
        }
        sessionQueue.async {
            do {
                self.session.beginConfiguration()
                self.session.sessionPreset = .photo
                guard let device = AVCaptureDevice.default(.builtInWideAngleCamera, for: .video, position: .back),
                      let input = try? AVCaptureDeviceInput(device: device),
                      self.session.canAddInput(input), self.session.canAddOutput(self.output) else {
                    throw CameraCaptureError.unavailable
                }
                self.session.addInput(input)
                self.session.addOutput(self.output)
                self.session.commitConfiguration()
                self.session.startRunning()
                DispatchQueue.main.async { self.isReady = true }
            } catch {
                self.session.commitConfiguration()
                DispatchQueue.main.async { self.errorMessage = CameraCaptureError.unavailable.localizedDescription }
            }
        }
    }

    func capture() {
        guard isReady else { return }
        output.capturePhoto(with: AVCapturePhotoSettings(), delegate: self)
    }

    func stop() {
        sessionQueue.async {
            if self.session.isRunning { self.session.stopRunning() }
        }
    }

    func photoOutput(_ output: AVCapturePhotoOutput, didFinishProcessingPhoto photo: AVCapturePhoto, error: Error?) {
        guard error == nil, let data = photo.fileDataRepresentation() else {
            DispatchQueue.main.async { self.errorMessage = "The camera could not capture that photo. Try again." }
            return
        }
        DispatchQueue.main.async { self.capturedData = data }
    }
}

private enum CameraCaptureError: LocalizedError {
    case unavailable
    var errorDescription: String? { "The iPhone camera is not available." }
}
