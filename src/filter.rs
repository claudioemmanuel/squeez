use crate::commands::Handler;
use crate::commands::{
    build::BuildHandler, cloud::CloudHandler, data_tool::DataToolHandler,
    database::DatabaseHandler, docker::DockerHandler, fs::FsHandler, generic::GenericHandler,
    git::GitHandler, network::NetworkHandler, next_build::NextBuildHandler,
    package_mgr::PackageMgrHandler, playwright::PlaywrightHandler, runtime::RuntimeHandler,
    test_runner::TestRunnerHandler, text_proc::TextProcHandler, typescript::TypescriptHandler,
    wrangler::WranglerHandler,
};
use crate::config::Config;

pub fn compress(cmd: &str, lines: Vec<String>, config: &Config) -> Vec<String> {
    // E8: a user-declared filter-DSL rule (.squeez/filters.ini or
    // ~/.claude/squeez/filters.ini) for this exact command takes priority
    // over the built-in dispatch table -- that's the whole point of
    // letting a user hand-tune the long tail `discover` surfaces.
    if let Some(def) = crate::filter_dsl::find_for_command(cmd, config.builtin_filters) {
        return crate::filter_dsl::apply(&def, lines);
    }
    let (handler, _name) = detect(cmd);
    handler.compress(cmd, lines, config)
}

/// The dispatch-table handler name for `cmd` (e.g. "git", "generic").
/// Exposed for `discover` (E8) to flag GenericHandler-routed commands as
/// candidates for a custom filter-DSL rule, without needing a downcast on
/// the trait object `detect` otherwise returns.
pub fn handler_name(cmd: &str) -> &'static str {
    detect(cmd).1
}

/// Tools whose output entirely replaces that of everything upstream of them
/// in a pipeline, so they — not the first token — decide the handler.
/// Deliberately a closed set: `cargo build | tee log` still belongs to the
/// build handler, and only a tool that *transforms* the stream qualifies.
const PIPELINE_FINAL_SAFE: &[&str] = &[
    "grep", "rg", "awk", "sed", "jq", "yq", "sort", "uniq", "head", "tail", "wc", "cut",
];

/// The segment of `cmd` that actually produced the captured output: the last
/// stage of a pipeline when that stage is in `PIPELINE_FINAL_SAFE`, otherwise
/// the whole command. `cargo build 2>&1 | grep error` emits grep matches, not
/// a cargo build log, and routing it to `package_mgr` wastes the text_proc
/// handler that fits it.
fn effective_segment(cmd: &str) -> &str {
    // `a || b` is an or-else chain, not a pipeline: which side produced the
    // output depends on an exit code we don't have here. Leave it alone.
    if cmd.contains("||") {
        return cmd;
    }
    let last = cmd.rsplit('|').next().unwrap_or(cmd).trim();
    if last.is_empty() || last == cmd.trim() {
        return cmd;
    }
    if PIPELINE_FINAL_SAFE.contains(&extract_name(last).as_str()) {
        last
    } else {
        cmd
    }
}

fn detect(cmd: &str) -> (Box<dyn Handler>, &'static str) {
    let cmd = effective_segment(cmd);
    let name = extract_name(cmd);
    match name.as_str() {
        "git" => (Box::new(GitHandler), "git"),
        "docker" | "docker-compose" | "podman" => (Box::new(DockerHandler), "docker"),
        "npm" | "pnpm" | "yarn" => (Box::new(PackageMgrHandler), "package_mgr"),
        "bun" => {
            // `bun test` / `bun run test` / `bun x vitest` behave like a test runner.
            let rest = cmd.split_whitespace().skip(1);
            if rest.clone().any(|a| a == "test")
                || rest.clone().any(|a| a == "vitest" || a == "jest" || a == "playwright")
            {
                (Box::new(TestRunnerHandler), "test_runner")
            } else {
                (Box::new(PackageMgrHandler), "package_mgr")
            }
        }
        "cargo" => {
            if cmd.split_whitespace().any(|a| a == "test") {
                (Box::new(TestRunnerHandler), "test_runner")
            } else {
                (Box::new(PackageMgrHandler), "package_mgr")
            }
        }
        "jest" | "vitest" | "pytest" | "py.test" | "nextest" => {
            (Box::new(TestRunnerHandler), "test_runner")
        }
        "go" => {
            if cmd.split_whitespace().any(|a| a == "test") {
                (Box::new(TestRunnerHandler), "test_runner")
            } else {
                (Box::new(GenericHandler), "generic")
            }
        }
        "playwright" => (Box::new(PlaywrightHandler), "playwright"),
        "tsc" | "eslint" | "biome" | "ruff" => (Box::new(TypescriptHandler), "typescript"),
        "make" | "cmake" | "gradle" | "mvn" | "xcodebuild" => (Box::new(BuildHandler), "build"),
        "next" => {
            if cmd.contains("build") || cmd.contains("dev") || cmd.contains("start") {
                (Box::new(NextBuildHandler), "next_build")
            } else if cmd.contains("lint") {
                // `next lint` wraps eslint — route to the eslint/tsc handler.
                (Box::new(TypescriptHandler), "typescript")
            } else {
                (Box::new(GenericHandler), "generic")
            }
        }
        "vite" | "turbo" => {
            if cmd.contains("build") {
                (Box::new(BuildHandler), "build")
            } else {
                (Box::new(GenericHandler), "generic")
            }
        }
        "wrangler" => (Box::new(WranglerHandler), "wrangler"),
        "kubectl" | "gh" | "aws" | "gcloud" | "az" => (Box::new(CloudHandler), "cloud"),
        "psql" | "prisma" | "mysql" | "drizzle-kit" => (Box::new(DatabaseHandler), "database"),
        "curl" | "wget" | "http" => (Box::new(NetworkHandler), "network"),
        "node" | "python" | "python3" | "ruby" => (Box::new(RuntimeHandler), "runtime"),
        "find" | "ls" | "du" | "ps" | "env" | "lsof" | "netstat"
        | "cat" | "head" | "tail" | "less" | "more" | "bat"
        | "bfs" => (Box::new(FsHandler), "fs"),
        "ugrep" => (Box::new(TextProcHandler), "text_proc"),
        "monitor" => (Box::new(GenericHandler), "generic"),
        // JSON/YAML/IaC tools
        "jq" | "yq" | "terraform" | "tofu" | "helm" | "pulumi" => {
            (Box::new(DataToolHandler), "data_tool")
        }
        // Text-processing tools: grep match output
        "grep" | "rg" | "awk" | "sed" => (Box::new(TextProcHandler), "text_proc"),
        _ => (Box::new(GenericHandler), "generic"),
    }
}

/// The dispatch name of `cmd`: the first real program token, with runner
/// wrappers, `VAR=value` prefixes and any leading path stripped.
///
/// Wrappers are peeled in a loop rather than one pass, because they stack in
/// practice (`sudo npx vitest`, `time pnpm exec tsc`). A wrapper carrying its
/// own flags (`sudo -u alice cmd`) is left alone deliberately: guessing which
/// flags take a value is how you end up dispatching on `alice`.
pub(crate) fn extract_name(cmd: &str) -> String {
    const WRAPPERS: &[&str] =
        &["npx ", "bunx ", "pnpm exec ", "yarn exec ", "sudo ", "time ", "command ", "nice ", "env "];
    let mut s = cmd.trim();
    loop {
        let before = s;
        // `VAR=value cmd …` — strip every leading assignment.
        while let Some(part) = s.split_whitespace().next() {
            if part.contains('=') && !part.starts_with('-') {
                s = s[part.len()..].trim_start();
            } else {
                break;
            }
        }
        for w in WRAPPERS {
            if let Some(rest) = s.strip_prefix(*w) {
                s = rest.trim_start();
                break;
            }
        }
        if s == before {
            break;
        }
    }
    let first = s.split_whitespace().next().unwrap_or("");
    first.rsplit('/').next().unwrap_or(first).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{effective_segment, extract_name, handler_name};

    #[test]
    fn wrappers_peel_until_stable() {
        assert_eq!(extract_name("sudo npx vitest run"), "vitest");
        assert_eq!(extract_name("time pnpm exec tsc --noEmit"), "tsc");
        assert_eq!(extract_name("env NODE_ENV=test jest"), "jest");
        assert_eq!(extract_name("TF_LOG=debug terraform plan"), "terraform");
        assert_eq!(extract_name("/usr/local/bin/cargo test"), "cargo");
    }

    #[test]
    fn wrapper_with_own_flags_is_left_alone() {
        // Documented limitation: we do not guess which wrapper flags take a
        // value, so this dispatches on the flag rather than mis-guessing.
        assert_eq!(extract_name("sudo -u alice vitest"), "-u");
    }

    #[test]
    fn env_prefixed_terraform_reaches_the_terraform_branch() {
        // Regression: data_tool re-derived the name itself and missed these.
        assert_eq!(handler_name("TF_LOG=debug terraform plan"), "data_tool");
        assert_eq!(extract_name("npx terraform plan"), "terraform");
    }

    #[test]
    fn pipeline_dispatches_on_a_safe_final_stage() {
        assert_eq!(handler_name("cargo build 2>&1 | grep error"), "text_proc");
        assert_eq!(effective_segment("cargo build | grep error").trim(), "grep error");
    }

    #[test]
    fn pipeline_keeps_the_producer_when_the_tail_is_not_safe() {
        // `tee` does not transform the stream — the build handler still fits.
        assert_eq!(handler_name("cargo build | tee build.log"), "package_mgr");
        // An or-else chain is not a pipeline: which side ran is unknown here.
        let or_else = "cargo build || grep error build.log";
        assert_eq!(effective_segment(or_else), or_else);
    }
}
