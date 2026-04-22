use squeez::commands::{build::BuildHandler, Handler};
use squeez::config::Config;

#[test]
fn gradle_drops_task_progress_keeps_errors() {
    let lines = vec![
        "> Task :compileJava".to_string(),
        "> Task :processResources".to_string(),
        "error: cannot find symbol".to_string(),
        "  symbol: class Foo".to_string(),
        "BUILD FAILED in 3s".to_string(),
    ];
    let result = BuildHandler.compress("gradle build", lines, &Config::default());
    assert!(!result.iter().any(|l| l.starts_with("> Task")));
    assert!(result.iter().any(|l| l.contains("BUILD FAILED")));
    assert!(result.iter().any(|l| l.contains("error")));
}

#[test]
fn xcodebuild_drops_progress_noise_keeps_succeeded() {
    let lines = vec![
        "Command line invocation:".to_string(),
        "    /Applications/Xcode.app/Contents/Developer/usr/bin/xcodebuild build -scheme MyApp".to_string(),
        "note: Using new build system".to_string(),
        "note: Planning".to_string(),
        "Analyze workspace".to_string(),
        "Create build description".to_string(),
        "ClangStatCache /path/to/cache.sdkstatcache".to_string(),
        "WriteAuxiliaryFile /path/to/file.json (in target 'MyApp' from project 'MyApp')".to_string(),
        "SwiftEmitModule normal arm64 Emitting module for MyApp (in target 'MyApp' from project 'MyApp')".to_string(),
        "    cd /Users/dev/projects/MyApp".to_string(),
        "    builtin-swiftTaskExecution -- /Applications/Xcode.app/foo".to_string(),
        "SwiftCompile normal arm64 Compiling 'ContentView.swift' /path/ContentView.swift (in target 'MyApp' from project 'MyApp')".to_string(),
        "Ld /path/to/MyApp.app/MyApp normal (in target 'MyApp' from project 'MyApp')".to_string(),
        "CodeSign /path/to/MyApp.app (in target 'MyApp' from project 'MyApp')".to_string(),
        "** BUILD SUCCEEDED **".to_string(),
    ];
    let result = BuildHandler.compress("xcodebuild build", lines, &Config::default());
    assert!(!result.iter().any(|l| l.starts_with("ClangStatCache")), "ClangStatCache survived");
    assert!(!result.iter().any(|l| l.starts_with("SwiftEmitModule")), "SwiftEmitModule survived");
    assert!(!result.iter().any(|l| l.starts_with("SwiftCompile ")), "SwiftCompile survived");
    assert!(!result.iter().any(|l| l.starts_with("Ld ")), "Ld survived");
    assert!(!result.iter().any(|l| l.starts_with("CodeSign ")), "CodeSign survived");
    assert!(!result.iter().any(|l| l.starts_with("    cd ")), "cd continuation survived");
    assert!(!result.iter().any(|l| l.starts_with("    builtin-")), "builtin- continuation survived");
    assert!(!result.iter().any(|l| l.contains("note: Using new build system")), "new build system note survived");
    assert!(result.iter().any(|l| l.contains("** BUILD SUCCEEDED **")), "terminal marker lost");
}

#[test]
fn xcodebuild_preserves_swift_errors_and_failed_marker() {
    let lines = vec![
        "SwiftDriverJobDiscovery normal arm64 Compiling ContentView.swift (in target 'MyApp' from project 'MyApp')".to_string(),
        "SwiftCompile normal arm64 Compiling 'ContentView.swift' /path/ContentView.swift (in target 'MyApp' from project 'MyApp')".to_string(),
        "/Users/dev/projects/MyApp/MyApp/ContentView.swift:42:5: error: cannot find 'foo' in scope".to_string(),
        "    foo()".to_string(),
        "    ^~~".to_string(),
        "** BUILD FAILED **".to_string(),
        "".to_string(),
        "The following build commands failed:".to_string(),
        "        SwiftCompile normal arm64 Compiling 'ContentView.swift' /path/ContentView.swift (in target 'MyApp' from project 'MyApp')".to_string(),
        "(1 failure)".to_string(),
    ];
    let result = BuildHandler.compress("xcodebuild build", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("error: cannot find 'foo'")), "error line lost");
    assert!(result.iter().any(|l| l.contains("** BUILD FAILED **")), "BUILD FAILED lost");
    assert!(result.iter().any(|l| l.contains("The following build commands failed:")), "failure summary lost");
}

#[test]
fn xcodebuild_noise_filter_does_not_touch_gradle() {
    // Gradle output can contain an "Ld" file path by coincidence; ensure the
    // xcode-specific filter only fires for xcodebuild commands.
    let lines = vec![
        "> Task :compileJava".to_string(),
        "Ld.so.conf parsed".to_string(),
        "BUILD SUCCESSFUL in 3s".to_string(),
    ];
    let result = BuildHandler.compress("gradle build", lines, &Config::default());
    assert!(result.iter().any(|l| l.contains("Ld.so.conf parsed")), "gradle Ld-like line incorrectly dropped");
}
