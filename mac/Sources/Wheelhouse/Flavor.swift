import Foundation

/// Which Wheelhouse this is: the daily one, or Wheelhouse Dev beside it
/// (`mac/build.sh --dev`, `just dev-install`). Read from Info.plist, so a bare
/// binary (`swift run`, `--snapshot`) is the daily flavor, as before.
enum Flavor {
    private static let info = Bundle.main.infoDictionary ?? [:]

    /// Wheelhouse Dev: the app of a second hub, marked DEV.
    static let isDev = (info["WHFlavor"] as? String) == "dev"

    /// The app's name, as its windows say it.
    static let name = isDev ? "Wheelhouse Dev" : "Wheelhouse"

    /// Its links: `wheelhouse://`, or Dev's `wheelhouse-dev://`, so a link
    /// opens the app it came from.
    static let scheme = (info["WHURLScheme"] as? String) ?? "wheelhouse"

    /// Where its hub answers unless Settings › Hub says otherwise.
    static let defaultHub = (info["WHDefaultHub"] as? String) ?? "http://127.0.0.1:7500"

    /// Its `wh`: Dev's runs its own hub's (`wh-dev`), never the daily one.
    static let command = (info["WHCommand"] as? String).map { NSString(string: $0).expandingTildeInPath }

    /// The quick panel's chord: ⌥⌘K, or ⇧⌥⌘K for Dev.
    static let chord = isDev ? "⇧⌥⌘K" : "⌥⌘K"

    /// How to install it, for when its `wh` can't be found.
    static let setup = isDev ? "just dev-install" : "just setup"
}
