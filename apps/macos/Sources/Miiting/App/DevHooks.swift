import Foundation

/// Environment switches for development. All are inert unless set.
///
/// - `MIITING_DATA_DIR`: use another data folder (e.g. one filled by `miiting-cli seed-demo`).
/// - `MIITING_SKIP_ONBOARDING=1`: don't show the welcome sheet.
/// - `MIITING_SELECT=<meeting id>`: select a meeting on launch.
enum DevHooks {
    private static var env: [String: String] { ProcessInfo.processInfo.environment }

    static var dataDirOverride: String? { env["MIITING_DATA_DIR"] }
    static var skipOnboarding: Bool { env["MIITING_SKIP_ONBOARDING"] == "1" }
    static var selectMeeting: String? { env["MIITING_SELECT"] }
}
