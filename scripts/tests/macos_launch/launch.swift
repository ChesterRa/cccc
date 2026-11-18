import AppKit
import Darwin

let arguments = Array(CommandLine.arguments.dropFirst())
guard let bundle = arguments.first else { exit(2) }
let configuration = NSWorkspace.OpenConfiguration()
configuration.activates = false
configuration.createsNewApplicationInstance = true
configuration.allowsRunningApplicationSubstitution = false
configuration.promptsUserIfNeeded = false
configuration.addsToRecentItems = false
configuration.arguments = Array(arguments.dropFirst())

NSWorkspace.shared.openApplication(at: URL(fileURLWithPath: bundle),
                                   configuration: configuration) { app, error in
    var result: [String: Any] = [:]
    if let app = app { result["pid"] = Int(app.processIdentifier) }
    if let error = error as NSError? {
        result["error_domain"] = error.domain
        result["error_code"] = error.code
    }
    guard let data = try? JSONSerialization.data(withJSONObject: result) else { exit(2) }
    FileHandle.standardOutput.write(data)
    exit(error == nil && app != nil ? 0 : 1)
}
_ = Timer.scheduledTimer(withTimeInterval: 35, repeats: false) { _ in exit(2) }
RunLoop.main.run()
exit(2)
