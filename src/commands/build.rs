use crate::commands::Handler;
use crate::config::Config;
use crate::strategies::{dedup, smart_filter, truncation};

pub struct BuildHandler;

const GRADLE_NOISE_PREFIXES: &[&str] = &["> Task :", "Executing ", "Download "];

// xcodebuild action-line prefixes that are pure progress noise. Errors surface
// on their own lines (e.g. `/path/File.swift:42:5: error: …`) and the
// terminal `** BUILD FAILED **` / `The following build commands failed:` block
// never matches these prefixes, so dropping them is safe.
const XCODEBUILD_NOISE_PREFIXES: &[&str] = &[
    "ClangStatCache ",
    "RegisterExecutionPolicyException ",
    "RegisterWithLaunchServices ",
    "WriteAuxiliaryFile ",
    "MkDir ",
    "CreateBuildDirectory ",
    "ProcessInfoPlistFile ",
    "CpResource ",
    "CopySwiftLibs ",
    "PBXCp ",
    "SwiftEmitModule ",
    "SwiftDriverJobDiscovery ",
    "SwiftCompile ",
    "SwiftMergeGeneratedHeaders ",
    "CompileC ",
    "CompileAssetCatalog ",
    "CompileStoryboard ",
    "CompileXIB ",
    "LinkStoryboards ",
    "GenerateDSYMFile ",
    "Ld ",
    "CodeSign ",
    "Touch ",
    "Validate ",
    "ComputePackagePrebuildTargetDependencyGraph",
    "SendPIFToBuildSystem",
    "CreateBuildRequest",
    "Analyze workspace",
    "Create build description",
    "Build description signature:",
    "Build description path:",
    "Command line invocation:",
    "Build settings from command line:",
    "    cd /",
    "    builtin-",
    "    /Applications/Xcode.app/",
    "    /usr/bin/",
    "    export ",
];

const XCODEBUILD_NOISE_CONTAINS: &[&str] = &[
    "note: Using new build system",
    "note: Using codesigning identity override",
    "note: Build preparation complete",
    "note: Planning",
];

fn is_xcodebuild(cmd: &str) -> bool {
    let first = cmd.split_whitespace().next().unwrap_or("");
    let base = first.rsplit('/').next().unwrap_or(first);
    base == "xcodebuild"
}

fn is_xcode_noise(l: &str) -> bool {
    XCODEBUILD_NOISE_PREFIXES.iter().any(|p| l.starts_with(p))
        || XCODEBUILD_NOISE_CONTAINS.iter().any(|p| l.contains(p))
}

impl Handler for BuildHandler {
    fn compress(&self, cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
        let lines = smart_filter::apply(lines);
        let xcode = is_xcodebuild(cmd);
        let filtered: Vec<String> = lines
            .into_iter()
            .filter(|l| !GRADLE_NOISE_PREFIXES.iter().any(|p| l.starts_with(p)))
            .filter(|l| !(xcode && is_xcode_noise(l)))
            .filter(|l| !l.trim().is_empty())
            .collect();
        let filtered = dedup::apply(filtered, config.dedup_min);
        truncation::apply(filtered, 100, truncation::Keep::Tail)
    }
}
