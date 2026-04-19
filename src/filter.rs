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
    let handler: Box<dyn Handler> = detect(cmd);
    handler.compress(cmd, lines, config)
}

fn detect(cmd: &str) -> Box<dyn Handler> {
    let name = extract_name(cmd);
    match name.as_str() {
        "git" => Box::new(GitHandler),
        "docker" | "docker-compose" | "podman" => Box::new(DockerHandler),
        "npm" | "pnpm" | "yarn" => Box::new(PackageMgrHandler),
        "bun" => {
            // `bun test` / `bun run test` / `bun x vitest` behave like a test runner.
            let rest = cmd.split_whitespace().skip(1);
            if rest.clone().any(|a| a == "test")
                || rest.clone().any(|a| a == "vitest" || a == "jest" || a == "playwright")
            {
                Box::new(TestRunnerHandler)
            } else {
                Box::new(PackageMgrHandler)
            }
        }
        "cargo" => {
            if cmd.split_whitespace().any(|a| a == "test") {
                Box::new(TestRunnerHandler)
            } else {
                Box::new(PackageMgrHandler)
            }
        }
        "jest" | "vitest" | "pytest" | "py.test" | "nextest" => Box::new(TestRunnerHandler),
        "playwright" => Box::new(PlaywrightHandler),
        "tsc" | "eslint" | "biome" => Box::new(TypescriptHandler),
        "make" | "cmake" | "gradle" | "mvn" | "xcodebuild" => Box::new(BuildHandler),
        "next" => {
            if cmd.contains("build") || cmd.contains("dev") || cmd.contains("start") {
                Box::new(NextBuildHandler)
            } else {
                Box::new(GenericHandler)
            }
        }
        "vite" | "turbo" => {
            if cmd.contains("build") {
                Box::new(BuildHandler)
            } else {
                Box::new(GenericHandler)
            }
        }
        "wrangler" => Box::new(WranglerHandler),
        "kubectl" | "gh" | "aws" | "gcloud" | "az" => Box::new(CloudHandler),
        "psql" | "prisma" | "mysql" | "drizzle-kit" => Box::new(DatabaseHandler),
        "curl" | "wget" | "http" => Box::new(NetworkHandler),
        "node" | "python" | "python3" | "ruby" => Box::new(RuntimeHandler),
        "find" | "ls" | "du" | "ps" | "env" | "lsof" | "netstat"
        | "cat" | "head" | "tail" | "less" | "more" | "bat" => Box::new(FsHandler),
        // JSON/YAML/IaC tools
        "jq" | "yq" | "terraform" | "tofu" | "helm" | "pulumi" => Box::new(DataToolHandler),
        // Text-processing tools: grep match output
        "grep" | "rg" | "awk" | "sed" => Box::new(TextProcHandler),
        _ => Box::new(GenericHandler),
    }
}

fn extract_name(cmd: &str) -> String {
    let wrappers = ["npx ", "bunx ", "pnpm exec ", "yarn exec "];
    let mut s = cmd.trim();
    for part in s.split_whitespace() {
        if part.contains('=') {
            s = s[part.len()..].trim_start();
        } else {
            break;
        }
    }
    for w in &wrappers {
        if s.starts_with(w) {
            s = &s[w.len()..];
        }
    }
    let first = s.split_whitespace().next().unwrap_or("");
    first.rsplit('/').next().unwrap_or(first).to_lowercase()
}
