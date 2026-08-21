fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--version") | Some("-V") => {
            println!("squeez {}", env!("CARGO_PKG_VERSION"));
        }
        Some("wrap") => {
            let cmd = args[2..].join(" ");
            if cmd.is_empty() {
                eprintln!("squeez wrap: no command given");
                std::process::exit(1);
            }
            let exit_code = squeez::commands::wrap::run(&cmd);
            std::process::exit(exit_code);
        }
        Some("filter") => {
            let hint = args.get(2).map(String::as_str).unwrap_or("generic");
            let exit_code = squeez::commands::filter_stdin::run(hint);
            std::process::exit(exit_code);
        }
        Some("track") => {
            let tool = args.get(2).map(String::as_str).unwrap_or("unknown");
            let bytes = args.get(3).map(String::as_str).unwrap_or("0");
            let exit_code = squeez::commands::track::run(tool, bytes);
            std::process::exit(exit_code);
        }
        Some("track-spawn") => {
            let tool = args.get(2).map(String::as_str).unwrap_or("unknown");
            let exit_code = squeez::commands::track::run_spawn(tool);
            std::process::exit(exit_code);
        }
        Some("init") => {
            let flag = args.get(2).map(String::as_str);
            let exit_code = match flag {
                Some("--copilot") => squeez::commands::init::run_copilot(),
                Some(s) if s.starts_with("--host=") => {
                    squeez::commands::init::run_for_host(&s["--host=".len()..])
                }
                _ => squeez::commands::init::run(),
            };
            std::process::exit(exit_code);
        }
        Some("compact-summary") => {
            // PostCompact hook: emit dense session state as additionalContext
            // so it survives /compact. See commands/compact.rs.
            std::process::exit(squeez::commands::compact::run());
        }
        Some("should-wrap") => {
            // PreToolUse hook gate (#150): exit 0 → safe to rewrite to
            // `squeez wrap '…'`; exit 1 → leave the command alone so the host's
            // native permission flow evaluates the original (risky/bypassed/off).
            let cmd = args[2..].join(" ");
            let ok = squeez::config::Config::load().should_wrap_bash(&cmd);
            std::process::exit(if ok { 0 } else { 1 });
        }
        Some("compress-md") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::commands::compress_md::run(&rest));
        }
        Some("compress-prompt") => {
            std::process::exit(squeez::commands::compress_prompt::run());
        }
        Some("config") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::commands::config_cmd::run(&rest));
        }
        Some("setup") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::commands::setup::run_with_help(&rest));
        }
        Some("uninstall") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::commands::uninstall::run(&rest));
        }
        Some("update") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::commands::update::run(&rest));
        }
        Some("benchmark") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::commands::benchmark::run(&rest));
        }
        Some("track-result") => {
            let tool = args.get(2).map(String::as_str).unwrap_or("unknown");
            std::process::exit(squeez::commands::track_result::run(tool));
        }
        Some("compress-output") => {
            let tool = args.get(2).map(String::as_str).unwrap_or("unknown");
            std::process::exit(squeez::commands::compress_output::run(tool));
        }
        Some("mcp") => {
            // JSON-RPC 2.0 server over stdin/stdout, exposing read-only access
            // to session memory + the protocol payload. See `commands/mcp_server.rs`.
            std::process::exit(squeez::commands::mcp_server::run());
        }
        Some("doctor") => {
            std::process::exit(squeez::commands::doctor::run());
        }
        Some("prune") => {
            // On-demand cleanup for the blob store + session memory (#201) —
            // both already prune opportunistically elsewhere; this just gives
            // the `run squeez prune` hints in mcp_server.rs somewhere to land.
            std::process::exit(squeez::commands::prune::run());
        }
        Some("calibrate") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::economy::calibrate::run(&rest));
        }
        Some("budget-params") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::economy::budget::run(&rest));
        }
        Some("discover") => {
            std::process::exit(squeez::commands::discover::run());
        }
        Some("filter-test") => {
            std::process::exit(squeez::filter_dsl::run_cli());
        }
        Some("protocol") => {
            // Print the auto-teach payload (markers + protocol) to stdout.
            // Same content the MCP `squeez_protocol` tool returns.
            print!("{}", squeez::commands::protocol::full_payload());
            std::process::exit(0);
        }
        _ => {
            eprintln!("Usage: squeez wrap <command>");
            eprintln!("       squeez filter <hint>");
            eprintln!("       squeez init [--copilot]");
            eprintln!("       squeez track <tool> <bytes>");
            eprintln!("       squeez track-result <tool> (reads stdin)");
            eprintln!("       squeez compress-md [--ultra] [--dry-run] [--all] <file>...");
            eprintln!("       squeez benchmark [--json] [--showcase] [--output <file>] [--scenario <name>]");
            eprintln!(
                "       squeez config <get|set|list|reset|path> ... — inspect/change settings"
            );
            eprintln!("       squeez setup [--host=<slug>]");
            eprintln!("       squeez uninstall [--host=<slug>]");
            eprintln!("       squeez update [--check] [--insecure]");
            eprintln!("       squeez mcp                       — JSON-RPC 2.0 server over stdio");
            eprintln!("       squeez protocol                  — print the auto-teach payload");
            eprintln!("       squeez discover                  — rank commands worth a custom filter-DSL rule");
            eprintln!("       squeez filter-test                — run inline tests from .squeez/filters.ini");
            eprintln!("       squeez compact-summary           — PostCompact hook: re-inject session state");
            eprintln!("       squeez calibrate                 — auto-tune config from benchmarks");
            eprintln!("       squeez doctor                    — self-health check (hooks, config, tracking)");
            eprintln!("       squeez prune                     — clear expired stashed blobs + old session summaries");
            eprintln!(
                "       squeez budget-params <tool>        — output JSON budget patch for tool"
            );
            eprintln!("       squeez --version");
            std::process::exit(1);
        }
    }
}
