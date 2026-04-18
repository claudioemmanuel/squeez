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
        Some("compact") => {
            eprintln!("squeez: compact not yet implemented");
            std::process::exit(1);
        }
        Some("compress-md") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::commands::compress_md::run(&rest));
        }
        Some("setup") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::commands::setup::run_with_help(&rest));
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
        Some("mcp") => {
            // JSON-RPC 2.0 server over stdin/stdout, exposing read-only access
            // to session memory + the protocol payload. See `commands/mcp_server.rs`.
            std::process::exit(squeez::commands::mcp_server::run());
        }
        Some("calibrate") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::economy::calibrate::run(&rest));
        }
        Some("budget-params") => {
            let rest: Vec<String> = args.iter().skip(2).cloned().collect();
            std::process::exit(squeez::economy::budget::run(&rest));
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
            eprintln!("       squeez benchmark [--json] [--output <file>] [--scenario <name>]");
            eprintln!("       squeez setup [--force]");
            eprintln!("       squeez update [--check] [--insecure]");
            eprintln!("       squeez mcp                       — JSON-RPC 2.0 server over stdio");
            eprintln!("       squeez protocol                  — print the auto-teach payload");
            eprintln!("       squeez calibrate                 — auto-tune config from benchmarks");
            eprintln!("       squeez budget-params <tool>        — output JSON budget patch for tool");
            eprintln!("       squeez --version");
            std::process::exit(1);
        }
    }
}
