import Foundation
import ImageIO
import UIKit

enum ImageInputError: LocalizedError, Equatable {
    case empty
    case tooLarge
    case unsupported
    case cannotDecode

    var errorDescription: String? {
        switch self {
        case .empty: "Choose a photo first."
        case .tooLarge: "The selected photo is larger than 25 MB."
        case .unsupported: "Choose a JPEG, PNG, HEIC, or other supported Photos image."
        case .cannotDecode: "The selected photo could not be read. Choose another image."
        }
    }
}

struct ProcessedImage: Sendable {
    let data: Data
    let width: Int
    let height: Int
}

enum ImageProcessingService {
    static let maximumInputBytes = 25 * 1_024 * 1_024
    static let maximumDimension: CGFloat = 1_600

    static func validate(_ data: Data) throws -> UIImage {
        guard !data.isEmpty else { throw ImageInputError.empty }
        guard data.count <= maximumInputBytes else { throw ImageInputError.tooLarge }
        guard let source = CGImageSourceCreateWithData(data as CFData, nil),
              CGImageSourceGetCount(source) > 0 else {
            throw ImageInputError.unsupported
        }
        guard let image = UIImage(data: data), image.size.width.isFinite, image.size.height.isFinite,
              image.size.width > 0, image.size.height > 0 else {
            throw ImageInputError.cannotDecode
        }
        return image
    }

    /// Re-encodes pixels as JPEG, which removes location and camera metadata.
    static func prepare(_ data: Data, rotationDegrees: Int = 0, centerSquareCrop: Bool = false) throws -> ProcessedImage {
        var image = try validate(data)
        image = normalized(image)
        if centerSquareCrop, let cgImage = image.cgImage {
            let side = min(cgImage.width, cgImage.height)
            let rect = CGRect(x: CGFloat((cgImage.width - side) / 2),
                              y: CGFloat((cgImage.height - side) / 2),
                              width: CGFloat(side), height: CGFloat(side))
            if let cropped = cgImage.cropping(to: rect) { image = UIImage(cgImage: cropped, scale: 1, orientation: .up) }
        }
        if rotationDegrees % 360 != 0 { image = rotated(image, degrees: rotationDegrees) }
        let largest = max(image.size.width, image.size.height)
        if largest > maximumDimension {
            let scale = maximumDimension / largest
            let newSize = CGSize(width: image.size.width * scale, height: image.size.height * scale)
            let source = image
            image = renderer(size: newSize).image { _ in source.draw(in: CGRect(origin: .zero, size: newSize)) }
        }
        guard let jpeg = image.jpegData(compressionQuality: 0.82), let cgImage = image.cgImage else {
            throw ImageInputError.cannotDecode
        }
        return ProcessedImage(data: jpeg, width: cgImage.width, height: cgImage.height)
    }

    /// Saves a full-size, orientation-corrected copy without EXIF or location metadata.
    static func archive(_ data: Data) throws -> ProcessedImage {
        let image = normalized(try validate(data))
        guard let jpeg = image.jpegData(compressionQuality: 0.92), let cgImage = image.cgImage else {
            throw ImageInputError.cannotDecode
        }
        return ProcessedImage(data: jpeg, width: cgImage.width, height: cgImage.height)
    }

    private static func normalized(_ image: UIImage) -> UIImage {
        renderer(size: image.size).image { _ in image.draw(in: CGRect(origin: .zero, size: image.size)) }
    }

    private static func rotated(_ image: UIImage, degrees: Int) -> UIImage {
        let radians = CGFloat(degrees) * .pi / 180
        let bounds = CGRect(origin: .zero, size: image.size).applying(CGAffineTransform(rotationAngle: radians))
        let outputSize = CGSize(width: abs(bounds.width), height: abs(bounds.height))
        return renderer(size: outputSize).image { context in
            let cg = context.cgContext
            cg.translateBy(x: outputSize.width / 2, y: outputSize.height / 2)
            cg.rotate(by: radians)
            image.draw(in: CGRect(x: -image.size.width / 2, y: -image.size.height / 2, width: image.size.width, height: image.size.height))
        }
    }

    private static func renderer(size: CGSize) -> UIGraphicsImageRenderer {
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        format.opaque = false
        return UIGraphicsImageRenderer(size: size, format: format)
    }
}
