//! `squeez uninstall` — reverse the per-host registration performed by
//! `squeez setup`. Does NOT delete data directories (sessions / memory)
//! so reinstalling is lossless.

use crate::hosts::{all_hosts, find, HostAdapter};

pub fn run(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return 0;
    }

    let mut host_filter: Option<String> = None;
    for a in args {
        if let Some(rest) = a.strip_prefix("--host=") {
            host_filter = Some(rest.to_string());
        }
    }

    let targets: Vec<Box<dyn HostAdapter>> = match &host_filter {
        Some(slug) => match find(slug) {
            Some(a) => vec![a],
            None => {
                eprintln!("squeez uninstall: unknown host '{}'", slug);
                eprintln!("available: claude-code, copilot, opencode, gemini, codex");
                return 1;
            }
        },
        None => all_hosts(),
    };

    let mut failures = 0;
    for adapter in targets {
        let name = adapter.name();
        if !adapter.is_installed() {
            println!(
                "squeez uninstall: {}  ⏭ skipped (host not detected)",
                name
            );
            continue;
        }
        match adapter.uninstall() {
            Ok(()) => println!("squeez uninstall: {}  ✓ squeez entries removed", name),
            Err(e) => {
                eprintln!("squeez uninstall: {}  ✗ {}", name, e);
                failures += 1;
            }
        }
    }

    if failures > 0 {
        return 1;
    }
    println!(
        "squeez uninstall: done. Session data and config.ini preserved under each host's squeez/ dir."
    );
    0
}

fn print_help() {
    println!("squeez uninstall — remove squeez entries from every detected host CLI");
    println!();
    println!("Usage:");
    println!("  squeez uninstall                 Uninstall from every detected host");
    println!("  squeez uninstall --host=<slug>   Uninstall from one host");
    println!();
    println!("Note: session data under each host's squeez/ dir is preserved.");
    println!("Supported hosts: claude-code, copilot, opencode, gemini, codex");
}
