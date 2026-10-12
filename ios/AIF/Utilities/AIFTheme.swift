import SwiftUI
import UIKit

enum AIFTheme {
    static let green = Color(uiColor: UIColor { traits in
        traits.userInterfaceStyle == .dark
            ? UIColor(red: 0.48, green: 0.78, blue: 0.55, alpha: 1)
            : UIColor(red: 0.10, green: 0.39, blue: 0.27, alpha: 1)
    })
    static let leaf = Color(uiColor: UIColor { traits in
        traits.userInterfaceStyle == .dark
            ? UIColor(red: 0.43, green: 0.75, blue: 0.52, alpha: 1)
            : UIColor(red: 0.20, green: 0.57, blue: 0.39, alpha: 1)
    })
    static let paleGreen = Color(uiColor: UIColor { traits in
        traits.userInterfaceStyle == .dark
            ? UIColor(red: 0.13, green: 0.23, blue: 0.17, alpha: 1)
            : UIColor(red: 0.89, green: 0.95, blue: 0.90, alpha: 1)
    })
    static let warm = Color(uiColor: .systemGroupedBackground)
    static let muted = Color.secondary
}

struct AIFCard<Content: View>: View {
    let content: Content

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    var body: some View {
        content
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).strokeBorder(.primary.opacity(0.06)))
    }
}

extension Double {
    var weightText: String { formatted(.number.precision(.fractionLength(0))) }
}
