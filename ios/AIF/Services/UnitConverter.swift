import Foundation

enum UnitConverter {
    static let kilogramsPerPound = 0.453_592_37

    static func pounds(fromKilograms kilograms: Double) -> Double {
        kilograms / kilogramsPerPound
    }

    static func kilograms(fromPounds pounds: Double) -> Double {
        pounds * kilogramsPerPound
    }
}
