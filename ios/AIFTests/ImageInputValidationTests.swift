import Foundation
import XCTest
@testable import AIF

final class ImageInputValidationTests: XCTestCase {
    private let onePixelPNG = Data(base64Encoded: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/JXcAAAAASUVORK5CYII=")!

    func testRejectsEmptyAndOversizedPhotoInput() {
        XCTAssertThrowsError(try ImageProcessingService.validate(Data()))
        XCTAssertThrowsError(try ImageProcessingService.validate(Data(repeating: 0, count: ImageProcessingService.maximumInputBytes + 1)))
    }

    func testRejectsNonImageData() {
        XCTAssertThrowsError(try ImageProcessingService.validate(Data("not an image".utf8)))
    }

    func testPreprocessingReencodesImageAsMetadataFreeJPEG() throws {
        let image = try ImageProcessingService.prepare(onePixelPNG)
        XCTAssertGreaterThan(image.data.count, 0)
        XCTAssertEqual(Array(image.data.prefix(3)), [0xFF, 0xD8, 0xFF])
        XCTAssertEqual(image.width, 1)
        XCTAssertEqual(image.height, 1)
    }
}
