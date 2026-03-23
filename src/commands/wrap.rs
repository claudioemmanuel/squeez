use std::process::{Command, Stdio};
use std::os::unix::process::CommandExt;
use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicI32, Ordering};
use std::io::Read;
use std::thread;
use crate::config::Config;
use crate::filter;

static CHILD_PID: AtomicI32 = AtomicI32::new(-1);

pub fn run(cmd_str: &str) -> i32 {
    setup_signals();
    let config = Config::load();

    if !config.enabled || config.is_bypassed(cmd_str) || is_streaming(cmd_str) {
        return passthrough(cmd_str);
    }

    let start = Instant::now();

    // Spawn via sh -c to handle pipes, &&, redirections, builtins
    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(cmd_str)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => { eprintln!("squeez: {}", e); return 1; }
    };

    // Store PID for signal forwarding
    CHILD_PID.store(child.id() as i32, Ordering::SeqCst);

    // Drain stdout/stderr on background threads to prevent pipe-buffer deadlock.
    // This MUST happen before the try_wait loop — if we wait first, the child can
    // block writing to a full pipe and never exit, causing a deadlock.
    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr piped");
    let stdout_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        stdout_pipe.read_to_end(&mut buf).ok();
        buf
    });
    let stderr_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        stderr_pipe.read_to_end(&mut buf).ok();
        buf
    });

    // Poll for exit with 120s timeout
    let timeout = Duration::from_secs(120);
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.code().unwrap_or(1),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    unsafe { libc::kill(-(child.id() as i32), libc::SIGTERM); }
                    std::thread::sleep(Duration::from_millis(200));
                    let _ = child.kill();
                    eprintln!("squeez: command timed out after 120s");
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return 124;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => { eprintln!("squeez: wait error: {}", e); return 1; }
        }
    };

    // Pipes are closed (child exited), join safely
    let stdout_bytes = stdout_thread.join().unwrap_or_default();
    let stderr_bytes = stderr_thread.join().unwrap_or_default();

    let elapsed_ms = start.elapsed().as_millis();

    // Merge stderr + stdout
    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&stderr_bytes));
    combined.push_str(&String::from_utf8_lossy(&stdout_bytes));

    let input_tokens = combined.len() / 4;
    let lines: Vec<String> = combined.lines().map(String::from).collect();

    let compressed = filter::compress(cmd_str, lines, &config);
    let output_str = compressed.join("\n");
    let output_tokens = output_str.len() / 4;

    let reduction = if input_tokens > 0 {
        100usize.saturating_sub(output_tokens * 100 / input_tokens)
    } else {
        0
    };

    let cmd_name = cmd_str.split_whitespace().next().unwrap_or("cmd");

    if config.show_header {
        println!(
            "# squeez [{}] {}→{} tokens (-{}%) {}ms",
            cmd_name, input_tokens, output_tokens, reduction, elapsed_ms
        );
    }
    if !output_str.is_empty() {
        println!("{}", output_str);
    }

    exit_code
}

fn passthrough(cmd: &str) -> i32 {
    let status = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("squeez: {}", e);
            std::process::exit(1);
        });
    status.code().unwrap_or(1)
}

fn is_streaming(cmd: &str) -> bool {
    let name = cmd.split_whitespace().next().unwrap_or("");
    let follow_cmds = ["tail", "docker", "kubectl"];
    follow_cmds.iter().any(|c| name.contains(c))
        && cmd.split_whitespace().any(|a| a == "-f" || a == "--follow")
}

fn setup_signals() {
    unsafe {
        libc::signal(libc::SIGTERM, forward_signal as libc::sighandler_t);
        libc::signal(libc::SIGINT, forward_signal as libc::sighandler_t);
    }
}

extern "C" fn forward_signal(sig: libc::c_int) {
    let pid = CHILD_PID.load(Ordering::SeqCst);
    if pid > 0 {
        unsafe { libc::kill(-pid, sig); }
    }
}
