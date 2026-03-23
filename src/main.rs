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
            let exit_code = squeez::commands::init::run();
            std::process::exit(exit_code);
        }
        Some("compact") => {
            eprintln!("squeez: compact not yet implemented");
            std::process::exit(1);
        }
        _ => {
            eprintln!("Usage: squeez wrap <command>\n       squeez filter <hint>\n       squeez --version");
            std::process::exit(1);
        }
    }
}
