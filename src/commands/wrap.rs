use crate::config::Config;
use crate::context;
use crate::filter;
use crate::{json_util, session};
use std::io::Read;
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
static CHILD_PID: AtomicI32 = AtomicI32::new(-1);

/// Returns a `Command` pre-configured to run `cmd` through the platform shell.
/// Unix/Git Bash: `sh -c <cmd>`
/// Windows native: `cmd /C <cmd>`
fn shell_command(cmd: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    }
}

/// Result of spawning and fully draining one command. `Fatal` carries the
/// process exit code `run()` should return immediately (spawn/pipe error,
/// timeout) — distinct from `Ok`'s `exit_code`, which is the CHILD's own
/// exit status and gets fed into the normal compression pipeline.
enum SpawnOutcome {
    Ok { exit_code: i32, combined: String },
    Fatal(i32),
}

/// Spawns `cmd_str` via the platform shell with `env_vars` applied
/// (`Command::env()` — never changes the command's own text/semantics),
/// drains stdout+stderr on background threads to avoid pipe-buffer
/// deadlock, and waits up to 120s. Factored out of `run()` so flag-forcing
/// (E3) can call it a second time for the one-shot un-forced fallback
/// without duplicating the spawn/drain/timeout logic.
fn spawn_and_capture(cmd_str: &str, env_vars: &[(&str, &str)]) -> SpawnOutcome {
    let mut cmd = shell_command(cmd_str);
    for (k, v) in env_vars {
        cmd.env(k, v);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("squeez: {}", e);
            return SpawnOutcome::Fatal(1);
        }
    };

    // Store PID for signal forwarding (Unix only)
    #[cfg(unix)]
    CHILD_PID.store(child.id() as i32, Ordering::SeqCst);

    // Drain stdout/stderr on background threads to prevent pipe-buffer deadlock.
    // This MUST happen before the try_wait loop — if we wait first, the child can
    // block writing to a full pipe and never exit, causing a deadlock.
    let stdout_pipe = match child.stdout.take() {
        Some(p) => p,
        None => {
            eprintln!("squeez: failed to capture stdout");
            return SpawnOutcome::Fatal(1);
        }
    };
    let stderr_pipe = match child.stderr.take() {
        Some(p) => p,
        None => {
            eprintln!("squeez: failed to capture stderr");
            return SpawnOutcome::Fatal(1);
        }
    };
    // Cap capture at 10 MB per stream to prevent OOM on runaway output.
    const MAX_CAPTURE: u64 = 10 * 1024 * 1024;
    let stdout_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        stdout_pipe.take(MAX_CAPTURE).read_to_end(&mut buf).ok();
        buf
    });
    let stderr_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        stderr_pipe.take(MAX_CAPTURE).read_to_end(&mut buf).ok();
        buf
    });

    // Poll for exit with 120s timeout
    let call_start = Instant::now();
    let timeout = Duration::from_secs(120);
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.code().unwrap_or(1),
            Ok(None) => {
                if call_start.elapsed() >= timeout {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGTERM);
                        std::thread::sleep(Duration::from_millis(200));
                    }
                    let _ = child.kill();
                    eprintln!("squeez: command timed out after 120s");
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return SpawnOutcome::Fatal(124);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("squeez: wait error: {}", e);
                return SpawnOutcome::Fatal(1);
            }
        }
    };

    // Pipes are closed (child exited), join safely
    let stdout_bytes = stdout_thread.join().unwrap_or_default();
    let stderr_bytes = stderr_thread.join().unwrap_or_default();

    // Merge stderr + stdout
    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&stderr_bytes));
    combined.push_str(&String::from_utf8_lossy(&stdout_bytes));

    SpawnOutcome::Ok { exit_code, combined }
}

pub fn run(cmd_str: &str) -> i32 {
    #[cfg(unix)]
    setup_signals();
    let config = Config::load();

    if !config.enabled || config.is_bypassed(cmd_str) || is_streaming(cmd_str) {
        return passthrough(cmd_str);
    }

    if config.plan_mode_passthrough
        && std::env::var("SQUEEZ_PLAN_MODE").as_deref() == Ok("1")
    {
        return passthrough(cmd_str);
    }

    // ── Context engine pre-pass ────────────────────────────────────────
    let sessions_dir_pp = session::sessions_dir();
    let used_tokens = session::CurrentSession::load(&sessions_dir_pp)
        .map(|c| c.total_tokens)
        .unwrap_or(0);
    let (mut ctx, intensity, eff_cfg) =
        context::pre_pass(&config, &sessions_dir_pp, used_tokens);

    // Optional cross-call hint for raw cat/head/tail of seen files
    if let Some(hint) = context::cache::raw_read_hint(&ctx, cmd_str) {
        println!("{}", hint);
    }

    let start = Instant::now();

    // ── Flag forcing (E3) ────────────────────────────────────────────────
    // Tier "env" (default): safe formatting env vars only, applied via
    // Command::env() below — never changes what the command does. Tier
    // "full" additionally appends a machine-readable-output flag for a
    // known reporter-backed runner, gated on no shell metacharacters, no
    // deny-list match, and no prior failure this session.
    let flag_force_tier = config.flag_force.as_str();
    let env_vars: &[(&str, &str)] = if flag_force_tier == "off" {
        &[]
    } else {
        crate::commands::flag_force::ENV_VARS
    };
    let mut spawn_cmd = cmd_str.to_string();
    let mut forced_label: Option<String> = None;
    if flag_force_tier == "full"
        && !crate::commands::flag_force::has_shell_metachars(cmd_str)
        && !config
            .flag_force_deny
            .iter()
            .any(|d| !d.is_empty() && cmd_str.starts_with(d.as_str()))
        && !ctx.flag_force_failed.iter().any(|f| f == cmd_str)
    {
        if let Some(inj) = crate::commands::flag_force::arg_injection(cmd_str) {
            spawn_cmd = inj.cmd;
            forced_label = Some(inj.label);
        }
    }

    let (exit_code, combined) = match spawn_and_capture(&spawn_cmd, env_vars) {
        SpawnOutcome::Fatal(code) => return code,
        SpawnOutcome::Ok { exit_code, combined } => {
            if forced_label.is_some()
                && exit_code != 0
                && crate::commands::flag_force::looks_like_unrecognized_option(&combined)
            {
                // Escape: the injected flag was rejected by the tool. Re-run
                // the ORIGINAL command once and remember not to retry this
                // exact command again this session (bounded FIFO memo).
                forced_label = None;
                ctx.flag_force_failed.push(cmd_str.to_string());
                const MAX_FLAG_FORCE_FAILED: usize = 32;
                if ctx.flag_force_failed.len() > MAX_FLAG_FORCE_FAILED {
                    ctx.flag_force_failed.remove(0);
                }
                match spawn_and_capture(cmd_str, env_vars) {
                    SpawnOutcome::Fatal(code) => return code,
                    SpawnOutcome::Ok { exit_code, combined } => (exit_code, combined),
                }
            } else {
                (exit_code, combined)
            }
        }
    };

    let elapsed_ms = start.elapsed().as_millis();

    // Content-class calibrated estimate (R2); flat path via class_density=false.
    let input_tokens = if config.class_density {
        crate::tokens::estimate_classed(&combined, config.tokenizer_scale)
    } else {
        crate::tokens::estimate_scaled(&combined, config.tokenizer_scale)
    };
    let lines: Vec<String> = combined.lines().map(String::from).collect();
    let orig_line_count = lines.len();
    let benign = context::summarize::is_benign(&lines);

    // ── Structured test reporters take precedence over summarize ───────
    // A large test run (many suites, high context → Ultra) would otherwise trip
    // the summarize line threshold and get a lossy generic summary, discarding
    // the reporter's exact "N passed (M suites)" / failures-only condensation —
    // which is both more accurate AND smaller. Try the reporters on the raw
    // lines first; only fall through to summarize/filter when none recognizes
    // the shape.
    let mut compressed = if let Some(condensed) =
        crate::commands::reporters::detect_and_condense(cmd_str, &lines)
    {
        condensed
    } else if context::summarize::should_apply(&lines, &eff_cfg) {
        // ── Summarize fallback for huge outputs (pre-handler) ──────────────
        // Decision based on raw line count so handlers can't hide huge inputs.
        let fmt = {
            use context::summarize::SummaryFormat;
            use context::intensity::Intensity;
            match config.summary_format.as_str() {
                "prose"      => SummaryFormat::Prose,
                "structured" => SummaryFormat::Structured,
                _            => if intensity == Intensity::Ultra {
                    SummaryFormat::Structured
                } else {
                    SummaryFormat::Prose
                },
            }
        };
        context::summarize::apply_with_format(lines, cmd_str, fmt)
    } else {
        filter::compress(cmd_str, lines, &eff_cfg)
    };

    // ── Redundancy short-circuit ───────────────────────────────────────
    let mut redundancy_hit = false;
    if eff_cfg.redundancy_cache_enabled {
        if let Some(hit) = context::redundancy::check(&ctx, &compressed) {
            compressed = vec![match hit.similarity {
                None => format!(
                    "[squeez: identical to {} at bash#{} — output omitted]",
                    hit.short_hash, hit.call_n
                ),
                Some(j) => format!(
                    "[squeez: ~{}% similar to {} at bash#{} — output omitted]",
                    (j * 100.0).round() as u32,
                    hit.short_hash,
                    hit.call_n
                ),
            }];
            redundancy_hit = true;
        }
    }

    // ── Success collapse (E5) ───────────────────────────────────────────
    // A zero-signal successful run of a low-signal command (git push/pull/
    // fetch/add/commit, package installs, docker pull/build, wrangler
    // deploy) collapses to a single `ok <cmd> (...)` line. The original is
    // always still stashed below (retrieve_marker bypasses its usual
    // line-count/size gates for this case) so nothing is unrecoverable.
    let success_collapsed = !redundancy_hit
        && config.success_collapse
        && exit_code == 0
        && benign
        && crate::strategies::success_collapse::is_eligible(cmd_str, &config.success_collapse_deny);
    if success_collapsed {
        compressed = vec![crate::strategies::success_collapse::collapse(cmd_str, &combined)];
    }

    let output_str = compressed.join("\n");
    let output_tokens = if config.class_density {
        crate::tokens::estimate_classed(&output_str, config.tokenizer_scale)
    } else {
        crate::tokens::estimate_scaled(&output_str, config.tokenizer_scale)
    };

    // ── Net-win gate (R4) ──────────────────────────────────────────────
    // The `# squeez` header itself costs ~15-25 tokens. When the call saved
    // less than `net_win_min_tokens` — whether compression ran and barely
    // helped, or didn't apply at all (a -0% no-op is a save of zero by
    // definition) — the call is a net loss: emit the original output and
    // suppress the header line instead. Follow-up warnings (compact, burst,
    // cache-idle, cache-ratio) still print below: they're decision-relevant
    // regardless of this call. Redundancy hits are exempt: the marker is a
    // cross-call pointer, not a marginal compression win, and must keep its
    // header context.
    // Degenerate compression: a handler/filter stripped the output to nothing
    // (e.g. output that was entirely progress/spinner/timestamp lines) while the
    // command DID produce content. Emitting the empty result would drop that
    // content outright and the header would claim a false -100%. Treat it as a
    // non-win passthrough so the verbatim original is emitted and the misleading
    // header is suppressed. Redundancy/success-collapse always leave a non-empty
    // marker, so they never land here.
    let degenerate_empty = output_str.trim().is_empty() && !combined.trim().is_empty();
    let net_win_gate = degenerate_empty
        || (config.net_win_min_tokens > 0
            && !redundancy_hit
            && input_tokens.saturating_sub(output_tokens) < config.net_win_min_tokens);
    // Session accounting records what is actually emitted — zero savings on
    // a gated passthrough.
    let emitted_tokens = if net_win_gate { input_tokens } else { output_tokens };

    // ── Reversible compression: stash the original so the model can recover it ──
    // When a large output was meaningfully compressed, save the verbatim
    // original to a content-addressed blob and surface a retrieve marker. This
    // is the safety net that lets compression be aggressive: the dropped lines
    // are one `squeez_retrieve` away instead of gone. Skipped on a redundancy
    // hit (the prior call already holds the content). Success collapse always
    // stashes regardless of line count -- a collapsed `ok git push (...)` on
    // a 6-line output still needs a recovery path, the usual size gates exist
    // to bound the (very different) compression-worth-it decision.
    let retrieve_marker = if config.retrieve_enabled
        && !redundancy_hit
        && !net_win_gate
        && !combined.trim().is_empty()
        && (success_collapsed
            || (orig_line_count >= config.retrieve_min_lines && output_str.len() + 256 < combined.len()))
    {
        context::retrieve::prune(config.retrieve_ttl_days.saturating_mul(86_400));
        context::retrieve::store(&combined).map(|id| {
            format!(
                "[squeez: full {}-line output stored — call squeez_retrieve with key=\"{}\" to expand, or squeez_stash_search to find it later]",
                orig_line_count, id
            )
        })
    } else {
        None
    };

    // ── Sensitive path warning (E7 tier 2) ──────────────────────────────
    // The content sniffer above only catches text SHAPED like a credential.
    // A command that references a conventionally-sensitive path (.env,
    // ~/.ssh/id_rsa, .netrc, ...) is worth a heads-up even when its output
    // doesn't match a content pattern. Warn-only, independent of whether
    // anything got stashed -- never blocks (see path_denylist_match's doc
    // for why .env.example legitimately still matches).
    let sensitive_path_warning = context::sensitive::path_denylist_match(cmd_str).map(|pat| {
        format!(
            "[squeez: cmd references a sensitive-looking path (\"{}\") — verify no secrets before sharing this output]",
            pat
        )
    });

    // ── Artifact capture + session tracking ───────────────────────────────
    let files      = extract_file_paths(&combined);
    let errors     = extract_errors(&combined);
    let git_events = extract_git_events(cmd_str, &combined);
    let test_sum   = extract_test_summary(&combined);

    let compact_warning = record_bash_event(
        cmd_str, input_tokens, emitted_tokens, &files, &errors, &git_events, &test_sum, &config,
        ctx.real_ctx_tokens, forced_label.as_deref().unwrap_or(""),
    );

    // Report the tokens actually emitted, not the discarded compressed count.
    // On a gated/degenerate passthrough the verbatim original is what the model
    // receives (emitted_tokens == input_tokens), so a "→0 (-100%)" header would
    // be a lie; emitted_tokens keeps the header honest (→input, -0%).
    let reduction = if input_tokens > 0 {
        100usize.saturating_sub(emitted_tokens * 100 / input_tokens)
    } else {
        0
    };

    let cmd_name = cmd_str.split_whitespace().next().unwrap_or("cmd");

    // ── Overhead accounting (E1) ────────────────────────────────────────────
    // Every squeez-authored line the model has to read (header, warnings,
    // nudges, retrieve marker) costs tokens independent of any compression
    // win. Collected here so `squeez_session_efficiency` can report the
    // honest net_saved = tokens_saved - overhead_tokens.
    let mut overhead_lines: Vec<String> = Vec::new();

    // `show_header` separately controls whether the header LINE prints —
    // "always" ignores the net-win gate, "off" never prints it, "net"
    // (default, and any unrecognized value) follows the gate as before.
    let header_shown = match config.show_header.as_str() {
        "off" => false,
        "always" => true,
        _ => !net_win_gate,
    };
    if header_shown {
        let intensity_tag = if config.adaptive_intensity {
            format!(" [adaptive: {}]", intensity.as_str())
        } else {
            String::new()
        };
        let this_call_n = ctx.call_counter.saturating_add(1);
        // Token economy: burn rate prediction — deduped against the last
        // emitted value so an unchanged tag doesn't repeat every call.
        let budget_tag_raw = crate::economy::burn_rate::pressure_warning(&ctx, &config)
            .or_else(|| {
                crate::economy::burn_rate::calls_remaining(&ctx, &config)
                    .map(|r| crate::economy::burn_rate::format_pressure_header(r))
            })
            .unwrap_or_default();
        let budget_tag = if context::cache::dedup_header_tag(
            &mut ctx.last_budget_tag,
            &mut ctx.last_budget_tag_call_n,
            &budget_tag_raw,
            this_call_n,
        ) {
            format!(" {}", budget_tag_raw)
        } else {
            String::new()
        };
        // Token economy: agent cost warning — same dedup treatment.
        let agent_tag_raw = crate::economy::agent_tracker::agent_cost_warning(&ctx, &config)
            .unwrap_or_default();
        let agent_tag = if context::cache::dedup_header_tag(
            &mut ctx.last_agent_tag,
            &mut ctx.last_agent_tag_call_n,
            &agent_tag_raw,
            this_call_n,
        ) {
            format!(" {}", agent_tag_raw)
        } else {
            String::new()
        };
        // Token economy: enterprise transport indicator
        let enterprise_mode = crate::economy::enterprise::detect();
        let enterprise_tag = if enterprise_mode.is_enterprise() {
            format!(" {}", crate::economy::enterprise::header_tag(enterprise_mode))
        } else {
            String::new()
        };
        // Flag-forcing provenance (E3): only shown when the forced variant
        // actually ran (the escape fallback clears forced_label on rejection).
        let forced_tag = match &forced_label {
            Some(label) => format!(" [forced: {}]", label),
            None => String::new(),
        };
        let header = format!(
            "# squeez [{}] {}→{} tokens (-{}%) {}ms{}{}{}{}{}",
            cmd_name, input_tokens, emitted_tokens, reduction, elapsed_ms,
            intensity_tag, budget_tag, agent_tag, enterprise_tag, forced_tag
        );
        println!("{}", header);
        overhead_lines.push(header);
    }
    if let Some(ref warning) = compact_warning {
        println!("{}", warning);
        overhead_lines.push(warning.clone());
    }
    // Workflow burst warning: N agents within burst_window_secs.
    if let Some(w) = crate::economy::agent_tracker::burst_warning(&ctx, &config) {
        println!("{}", w);
        overhead_lines.push(w);
    }
    // Cache idle expiry warning: 5-min ephemeral cache may have expired
    // if the agent was stalled waiting for sub-subagents or user input.
    if config.cache_idle_warn_secs > 0 && ctx.last_activity_ts > 0 {
        let idle = crate::session::unix_now().saturating_sub(ctx.last_activity_ts);
        if idle >= config.cache_idle_warn_secs {
            let w = format!(
                "[squeez: cache idle {}s — 5-min ephemeral cache may have expired; \
                 next turn will re-create context at full write cost]",
                idle
            );
            println!("{}", w);
            overhead_lines.push(w);
        }
    }
    // Cache-read:I/O ratio warning (G1): when cache_read >> actual I/O
    // tokens the context has grown very large (long CLAUDE.md, many files
    // loaded). Threshold 50× — the openwatch Wave 3 session hit 49×.
    // Fire once per session via nudged_keys; skip when data is absent.
    if ctx.real_cache_read_tokens > 10_000
        && !ctx.nudged_keys.iter().any(|k| k == "cache_ratio_warn")
    {
        let io = ctx.real_ctx_tokens.saturating_sub(ctx.real_cache_read_tokens).max(1);
        if ctx.real_cache_read_tokens / io >= 50 {
            ctx.nudged_keys.push("cache_ratio_warn".to_string());
            let w = format!(
                "[squeez: HIGH CACHE RATIO — cache_read {}K vs I/O {}K (~{}×) — \
                 context is very large; consider /compact or trimming CLAUDE.md]",
                ctx.real_cache_read_tokens / 1000,
                io / 1000,
                ctx.real_cache_read_tokens / io,
            );
            println!("{}", w);
            overhead_lines.push(w);
        }
    }
    // Warnings queued by observer paths with no stdout channel of their own
    // (track-result quota escalation, SubagentStop size guard) drain here —
    // the next bash output is the first surface the model actually reads.
    for w in ctx.drain_warnings() {
        println!("{}", w);
        overhead_lines.push(w);
    }
    if let Some(ref marker) = retrieve_marker {
        println!("{}", marker);
        overhead_lines.push(marker.clone());
    }
    if let Some(ref w) = sensitive_path_warning {
        println!("{}", w);
        overhead_lines.push(w.clone());
    }
    if net_win_gate {
        // Gated passthrough: the header would cost more than compression
        // saved, so the model gets the verbatim original.
        if !combined.is_empty() {
            print!("{}", combined);
            if !combined.ends_with('\n') {
                println!();
            }
        }
    } else if !output_str.is_empty() {
        println!("{}", output_str);
    }

    // ── Context engine post-pass ───────────────────────────────────────
    if config.context_cache_enabled && !redundancy_hit {
        context::redundancy::record(&mut ctx, cmd_str, &compressed);
    } else if config.context_cache_enabled {
        // still bump the call counter so future calls reference the right index
        ctx.next_call_n();
    }
    if config.context_cache_enabled {
        let access = detect_file_access(cmd_str);
        for f in &files {
            ctx.note_file(f, access.clone());
        }
        ctx.note_errors(&errors);
        ctx.note_git(&git_events);
        ctx.note_tool_tokens("Bash", input_tokens as u64);
        // Token economy: record burn rate (what was actually emitted)
        ctx.note_burn(emitted_tokens as u64);

        // ── Auto-curation nudges (item 1) ──────────────────────────────
        let nudges = crate::economy::nudge::evaluate(
            &mut ctx, cmd_str, &files, access, &errors, &config,
        );
        for n in &nudges {
            println!("{}", n);
            overhead_lines.push(n.clone());
        }
        // Quota/plan-limit escalation (CF-3) for bash output (curl 429s etc.).
        // Queued warnings print on the next wrap call.
        crate::economy::nudge::note_quota_errors(&mut ctx, "Bash", &combined, &config);

        ctx.save(&sessions_dir_pp);
    }

    // ── Continuous handler calibration (item 2) ────────────────────────
    if config.handler_stats_enabled {
        let mut stats = crate::economy::handler_stats::HandlerStats::load(&sessions_dir_pp);
        stats.record(cmd_name, input_tokens as u64, emitted_tokens as u64);
        stats.save(&sessions_dir_pp);
    }

    // ── Overhead accounting (E1) ─────────────────────────────────────────
    // Tally what this call actually cost the model to read in squeez-authored
    // lines, independent of the compression accounting above.
    if !overhead_lines.is_empty() {
        let overhead_est = crate::tokens::estimate(&overhead_lines.join("\n")) as u64;
        if overhead_est > 0 {
            if let Some(mut current) = session::CurrentSession::load(&sessions_dir_pp) {
                current.overhead_tokens = current.overhead_tokens.saturating_add(overhead_est);
                current.save(&sessions_dir_pp);
            }
        }
    }

    exit_code
}

fn passthrough(cmd: &str) -> i32 {
    let status = shell_command(cmd)
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

/// Infer the file access type from the shell command name.
/// Defaults to `Read` when ambiguous (most bash-extracted file paths are reads).
fn detect_file_access(cmd: &str) -> crate::context::cache::FileAccess {
    use crate::context::cache::FileAccess;
    let first = cmd.split_whitespace().next().unwrap_or("");
    let name = first.rsplit('/').next().unwrap_or(first);
    match name {
        "rm" | "unlink" | "rmdir" => FileAccess::Deleted,
        "tee" => FileAccess::Write,
        "cat" | "head" | "tail" | "less" | "more" | "bat" => FileAccess::Read,
        _ => {
            // Redirection operators in the full command → write.
            if cmd.contains(" > ") || cmd.contains(" >> ") {
                FileAccess::Write
            } else {
                FileAccess::Read
            }
        }
    }
}

#[cfg(unix)]
fn setup_signals() {
    unsafe {
        libc::signal(libc::SIGTERM, forward_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, forward_signal as *const () as libc::sighandler_t);
    }
}

#[cfg(unix)]
extern "C" fn forward_signal(sig: libc::c_int) {
    let pid = CHILD_PID.load(Ordering::SeqCst);
    if pid > 0 {
        unsafe {
            libc::kill(-pid, sig);
        }
    }
}

// ── Artifact extraction ────────────────────────────────────────────────────

const MAX_FILE_PATHS: usize = 100;

pub fn extract_file_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for word in text.split_whitespace() {
        if out.len() >= MAX_FILE_PATHS {
            break;
        }
        let w = word.trim_matches(|c| c == ',' || c == ':' || c == '(' || c == ')' || c == '\'' || c == '"');
        if looks_like_path(w) && seen.insert(w.to_string()) {
            out.push(w.to_string());
        }
    }
    out
}

fn looks_like_path(s: &str) -> bool {
    s.contains('/')
        && !s.starts_with("http")
        && !s.starts_with("//")
        && s.len() > 4
        && s.len() < 160
        && s.chars().all(|c| c.is_alphanumeric() || "/_.-:".contains(c))
        && s.contains('.')
}

pub fn extract_errors(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("error:") || t.starts_with("Error:")
                || t.starts_with("error[") || t.starts_with("FAILED")
                || t.starts_with("fatal:") || t.starts_with("panic:")
        })
        .take(3)
        .map(|l| l.trim().chars().take(120).collect())
        .collect()
}

pub fn extract_test_summary(text: &str) -> String {
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with("test result:") { return l.chars().take(80).collect(); }
        if l.contains(" passed") && l.contains(" failed") { return l.chars().take(80).collect(); }
        if l.starts_with("PASSED") || l.starts_with("FAILED") { return l.chars().take(80).collect(); }
    }
    String::new()
}

/// Public wrapper for tests (private logic is `extract_git_events`).
pub fn extract_git_events_pub(cmd: &str, text: &str) -> Vec<String> {
    extract_git_events(cmd, text)
}

fn extract_git_events(cmd: &str, text: &str) -> Vec<String> {
    let name = cmd.split_whitespace().next().unwrap_or("");
    let is_git = name == "git" || name.ends_with("/git");
    if !is_git { return Vec::new(); }
    text.lines()
        .filter(|l| {
            let t = l.trim();
            t.chars().take(7).count() == 7 && t.chars().take(7).all(|c| c.is_ascii_hexdigit())
        })
        .take(5)
        .map(|l| l.trim().chars().take(100).collect())
        .collect()
}

fn record_bash_event(
    cmd: &str,
    in_tk: usize,
    out_tk: usize,
    files: &[String],
    errors: &[String],
    git: &[String],
    test_summary: &str,
    config: &Config,
    real_ctx_tokens: u64,
    forced: &str,
) -> Option<String> {
    let dir = session::sessions_dir();
    let mut current = session::CurrentSession::load(&dir)?;

    current.total_tokens += out_tk as u64;
    if in_tk > out_tk {
        current.tokens_saved += (in_tk - out_tk) as u64;
    }

    let event = format!(
        "{{\"type\":\"bash\",\"cmd\":\"{}\",\"in_tk\":{},\"out_tk\":{},\
\"files\":{},\"errors\":{},\"git\":{},\"test_summary\":\"{}\",\"forced\":\"{}\",\"ts\":{}}}",
        json_util::escape_str(cmd),
        in_tk, out_tk,
        json_util::str_array(files),
        json_util::str_array(errors),
        json_util::str_array(git),
        json_util::escape_str(test_summary),
        json_util::escape_str(forced),
        session::unix_now(),
    );
    session::append_event(&dir, &current.session_file, &event);

    // Real measured context (CF-1) is authoritative over squeez's own byte
    // counters: total_tokens is a cumulative monotonic sum of all tool I/O, not
    // context occupancy, so on a long session it balloons past the true context
    // and would falsely trip the critical warning. Use the measured value when
    // present. Budget keys off the real window detected from the transcript
    // model id, honoring `context_window_tokens` config first (CF-2).
    let ctx = crate::context::cache::SessionContext::load(&dir);
    let effective_used = if real_ctx_tokens > 0 {
        real_ctx_tokens
    } else {
        current.total_tokens
    };
    let budget = crate::context::intensity::budget_for(config, ctx.real_ctx_window);
    let pct = effective_used * 100 / budget.max(1);

    let compact_trigger = if config.context_window_tokens > 0 {
        // Window-relative trigger: warn at the same fraction of the window
        // that compact_threshold represents of the legacy budget (4/5).
        budget.saturating_mul(4) / 5
    } else {
        config.compact_threshold_tokens
    };
    let warning = if !current.compact_warned && effective_used >= compact_trigger {
        current.compact_warned = true;
        // Per-tool breakdown from the already-loaded context.
        Some(format!(
            "⚠️  squeez: session ~{}K tokens ({}% of budget). Run /compact to free context.\n    Token breakdown: Bash {}K | Read {}K | Grep {}K | Other {}K",
            effective_used / 1000,
            pct,
            ctx.tokens_bash / 1000,
            ctx.tokens_read / 1000,
            ctx.tokens_grep / 1000,
            ctx.tokens_other / 1000,
        ))
    } else if !current.state_warned {
        // Tier-2: State-First Pattern suggestion at critical pressure.
        let critical = if pct >= 90 {
            true
        } else {
            crate::economy::burn_rate::calls_remaining(&ctx, config)
                .map(|r| r <= config.state_warn_calls)
                .unwrap_or(false)
        };
        if critical {
            current.state_warned = true;
            Some(format!(
                "🚨 squeez: context critical ({}%) — save state before clearing:\n\
                 \n\
                 Write `.claude/session_state.md` with:\n\
                 ## Current Objective\n\
                 <what you're solving now>\n\
                 ## Files Read\n\
                 <paths + what was learned>\n\
                 ## Decisions Taken\n\
                 <why approach X not Y>\n\
                 ## Next Steps\n\
                 <immediate plan>\n\
                 \n\
                 Then run `/clear` to reset context (or `/compact [describe focus area]` for a focused summary).",
                pct.min(100),
            ))
        } else {
            None
        }
    } else {
        None
    };

    current.save(&dir);
    warning
}
