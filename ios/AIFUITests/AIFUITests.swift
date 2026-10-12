import Foundation
import XCTest

final class AIFUITests: XCTestCase {
    func testCreateAnimalSaveTapeEstimateAndReopenHistory() throws {
        let app = XCUIApplication()
        app.launchArguments += ["--uitesting"]
        app.launch()

        app.tabBars.buttons["Animals"].tap()
        app.buttons["add-animal"].tap()
        let name = app.textFields["animal-name"]
        XCTAssertTrue(name.waitForExistence(timeout: 5))
        name.tap()
        name.typeText("Daisy")
        app.buttons["save-animal"].tap()
        let daisy = app.descendants(matching: .any).matching(NSPredicate(format: "label CONTAINS %@", "Daisy")).firstMatch
        XCTAssertTrue(daisy.waitForExistence(timeout: 5))

        app.tabBars.buttons["Estimate"].tap()
        app.buttons["Tape estimate"].tap()
        let girth = app.textFields["tape-girth"]
        let length = app.textFields["tape-length"]
        XCTAssertTrue(girth.waitForExistence(timeout: 5))
        girth.tap()
        girth.typeText("180")
        length.tap()
        length.typeText("150")
        app.buttons["calculate-tape"].tap()
        XCTAssertTrue(app.staticTexts["tape-result"].waitForExistence(timeout: 5))
        app.buttons["save-tape"].tap()
        XCTAssertTrue(app.staticTexts["Saved tape estimate locally."].waitForExistence(timeout: 5))

        app.tabBars.buttons["History"].tap()
        XCTAssertTrue(app.staticTexts["\(Int((180 * 180 * 150) / 10_838)) kg"].waitForExistence(timeout: 5))
        app.terminate()
        app.launch()
        app.tabBars.buttons["History"].tap()
        XCTAssertTrue(app.staticTexts["Daisy"].waitForExistence(timeout: 5))
    }
}
