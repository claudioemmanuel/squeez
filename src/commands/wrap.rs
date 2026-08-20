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

/// The shell `wrap` re-executes a captured command under: program plus its
/// "run this string" flag.
///
/// Pure over `on_path` so the Windows branch stays testable from any host.
///
/// Windows used to hardcode `cmd /C`, but every agent host squeez supports
/// hands its terminal commands to git-bash there — so a command the agent
/// wrote for bash got re-executed by cmd.exe and quietly came out wrong:
/// `python -c 'print(1)'` produced empty output, `$(…)` and backticks were not
/// expanded, `$HOME` stayed literal, and `;` was passed to the first program as
/// an argument instead of separating statements (issue #208). Wrong-but-
/// plausible output is worse than a loud failure, because the agent then
/// reasons on top of it. So: prefer bash when it is there, keep `cmd` as the
/// fallback, and let `SQUEEZ_SHELL` override both.
fn shell_choice(
    shell_override: Option<String>,
    windows: bool,
    on_path: impl Fn(&str) -> bool,
) -> (String, &'static str) {
    if let Some(shell) = shell_override.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        let flag = if is_cmd_shell(&shell) { "/C" } else { "-c" };
        return (shell, flag);
    }
    if !windows {
        return ("sh".to_string(), "-c");
    }
    for candidate in ["bash", "sh"] {
        if on_path(candidate) {
            return (candidate.to_string(), "-c");
        }
    }
    ("cmd".to_string(), "/C")
}

/// `cmd.exe` is the only shell squeez drives with `/C`; everything else
/// (bash, sh, zsh, dash, busybox) takes `-c`.
fn is_cmd_shell(shell: &str) -> bool {
    let base = shell
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(shell)
        .to_ascii_lowercase();
    base == "cmd" || base == "cmd.exe"
}

/// True if `program` is an executable on PATH. Only consulted on Windows, and
/// only once per process — see [`resolved_shell`].
#[cfg(windows)]
fn program_on_path(program: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        dir.join(format!("{program}.exe")).is_file() || dir.join(program).is_file()
    })
}

#[cfg(not(windows))]
fn program_on_path(_program: &str) -> bool {
    false
}

/// `wrap` runs on every Bash tool call, so the PATH scan is done once.
fn resolved_shell() -> &'static (String, &'static str) {
    static SHELL: std::sync::OnceLock<(String, &'static str)> = std::sync::OnceLock::new();
    SHELL.get_or_init(|| {
        shell_choice(
            std::env::var("SQUEEZ_SHELL").ok(),
            cfg!(windows),
            program_on_path,
        )
    })
}

/// Returns a `Command` pre-configured to run `cmd` through the shell.
/// Unix: `sh -c <cmd>`. Windows: `bash -c <cmd>` when bash is on PATH (the
/// de-facto agent shell there), otherwise `cmd /C <cmd>`. `SQUEEZ_SHELL`
/// overrides the choice on every platform.
fn shell_command(cmd: &str) -> Command {
    let (program, flag) = resolved_shell();
    let mut c = Command::new(program);
    c.args([flag, cmd]);
    c
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
/// deadlock, and waits up to `timeout_secs`. Factored out of `run()` so
/// flag-forcing (E3) can call it a second time for the one-shot un-forced
/// fallback without duplicating the spawn/drain/timeout logic.
fn spawn_and_capture(cmd_str: &str, env_vars: &[(&str, &str)], timeout_secs: u64) -> SpawnOutcome {
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

    // Poll for exit with the configured timeout
    let call_start = Instant::now();
    let timeout = Duration::from_secs(timeout_secs);
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
                    eprintln!(
                        "squeez: command timed out after {}s — raise it with \
                         `wrap_timeout_secs` in ~/.claude/squeez/config.ini \
                         or SQUEEZ_WRAP_TIMEOUT_SECS",
                        timeout_secs
                    );
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
    // `git status --porcelain=v1 -b` is the one injection allowed at the
    // default `env` tier: `git status` is read-only and the flag changes
    // formatting only, so it carries none of the "this changes what the
    // command does" risk that keeps the rest of the arg tier behind `full`.
    // It is also the highest-frequency command in an agent session, which is
    // where the win is.
    let arg_tier_ok = flag_force_tier == "full"
        || (flag_force_tier == "env"
            && crate::commands::flag_force::is_env_safe_injection(cmd_str));
    if arg_tier_ok
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

    let timeout_secs = crate::config::resolve_wrap_timeout_secs(
        config.wrap_timeout_secs,
        std::env::var("SQUEEZ_WRAP_TIMEOUT_SECS").ok().as_deref(),
    );

    let (exit_code, combined) = match spawn_and_capture(&spawn_cmd, env_vars, timeout_secs) {
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
                match spawn_and_capture(cmd_str, env_vars, timeout_secs) {
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

    // ── Preservation guard (runtime) ────────────────────────────────────
    // `economy::preservation` scores how many navigation anchors (file
    // paths, `file:line` refs, error markers, test verdicts) survived
    // compression. It used to run only in `benchmark`, which meant the
    // guard we describe publicly did not exist in production. A
    // high-reduction call that dropped most anchors is the regime where the
    // model re-investigates — so instead of weakening the compression, we
    // guarantee the original stays one `squeez_retrieve` away and say so in
    // the header. Gated on reduction >= 90% to keep the anchor scan off the
    // common path.
    let anchors_pct = if config.preservation_guard && !redundancy_hit && input_tokens > 0 {
        let reduction_now =
            100.0 - (output_tokens as f64 * 100.0 / input_tokens as f64);
        if reduction_now >= crate::economy::preservation::RISK_REDUCTION_THRESHOLD {
            Some(crate::economy::preservation::info_preservation(&combined, &output_str))
        } else {
            None
        }
    } else {
        None
    };
    let anchors_low = anchors_pct.is_some_and(|s| s < config.preservation_floor as f64);

    // Retrieve-marker eligibility, computed BEFORE the net-win gate.
    // The marker costs ~40 tokens and is emitted only because compression
    // ran, so it is compression-attributable overhead and belongs inside
    // the win/loss decision — unlike the session advisories further down,
    // which print regardless of this call and are exempt by design. It used
    // to be appended after the gate, so a call that saved 25 tokens could
    // emit a 40-token marker and still print a header claiming a win.
    // Eligibility is computed without the `net_win_gate` term to break the
    // circularity (the gate needs the cost, the cost needs the gate); the
    // marker is only actually stored and emitted below when the gate stays
    // open, so folding the cost in can only push toward gating.
    let stash_eligible = config.retrieve_enabled
        && !redundancy_hit
        && !combined.trim().is_empty()
        && (success_collapsed
            || anchors_low
            || (orig_line_count >= config.retrieve_min_lines
                && output_str.len() + 256 < combined.len()));
    let marker_cost = if stash_eligible {
        // Cost the real marker shape with a placeholder key, so the estimate
        // cannot drift from the text actually emitted.
        crate::tokens::estimate(&retrieve_marker_text(orig_line_count, "0000000000000000"))
    } else {
        0
    };

    // A forced call is exempt, and not as a favour: when an arg injection
    // ran, `combined` is the INJECTED command's output, not what the user's
    // command would have printed. Falling back to it is not a passthrough —
    // it hands the model a machine format nobody asked for (raw `git status`
    // porcelain codes, `-json` NDJSON). The reporter's rendering is the only
    // faithful form we have, so it ships regardless of the marginal count.
    // `input_tokens` is measured on that injected output too, which is why
    // the injection's real win is invisible to this arithmetic.
    let net_win_gate = degenerate_empty
        || (!redundancy_hit
            && forced_label.is_none()
            && is_net_loss(
                input_tokens,
                output_tokens,
                marker_cost,
                config.net_win_min_tokens,
            ));

    // ── Reversible compression: stash the original so the model can recover it ──
    // When a large output was meaningfully compressed, save the verbatim
    // original to a content-addressed blob and surface a retrieve marker. This
    // is the safety net that lets compression be aggressive: the dropped lines
    // are one `squeez_retrieve` away instead of gone. Skipped on a redundancy
    // hit (the prior call already holds the content). Success collapse always
    // stashes regardless of line count -- a collapsed `ok git push (...)` on
    // a 6-line output still needs a recovery path, the usual size gates exist
    // to bound the (very different) compression-worth-it decision.
    let retrieve_marker = if stash_eligible && !net_win_gate {
        context::retrieve::prune(config.retrieve_ttl_days.saturating_mul(86_400));
        context::retrieve::store(&combined)
            .map(|id| retrieve_marker_text(orig_line_count, &id))
    } else {
        None
    };

    // Session accounting records what is actually emitted — zero savings on
    // a gated passthrough, and the marker counts against the win when it
    // ships.
    let emitted_tokens = if net_win_gate {
        input_tokens
    } else if retrieve_marker.is_some() {
        output_tokens.saturating_add(marker_cost)
    } else {
        output_tokens
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
        // Preservation guard: only surfaced when the score fell below the
        // floor, so a healthy call pays nothing for it.
        let anchors_tag = match anchors_pct {
            Some(score) if anchors_low => {
                format!(" [anchors: {}%]", (score * 100.0).round() as u32)
            }
            _ => String::new(),
        };
        // Under a forced injection there is no honest percentage to print:
        // `input_tokens` was measured on the injected command's output, not
        // on what the user's command would have produced, so both a win and
        // a loss against it are meaningless. Report the emitted size and the
        // provenance tag, and claim nothing about reduction.
        let size_part = if forced_label.is_some() {
            format!("{} tokens (forced baseline)", emitted_tokens)
        } else {
            format!("{}→{} tokens (-{}%)", input_tokens, emitted_tokens, reduction)
        };
        let header = format!(
            "# squeez [{}] {} {}ms{}{}{}{}{}{}",
            cmd_name, size_part, elapsed_ms,
            intensity_tag, budget_tag, agent_tag, enterprise_tag, forced_tag, anchors_tag
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
        // `nudged_keys` alone loses this race — see session::claim_nudge.
        if ctx.real_cache_read_tokens / io >= 50
            && session::claim_nudge(&sessions_dir_pp, "cache_ratio_warn")
        {
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
    // focus=adhd caps the burst at 5 lines (rule 9).
    let drained = crate::commands::focus::cap_advisories(ctx.drain_warnings(), config.focus);
    for w in drained {
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

/// Did this call save less than it cost?
///
/// `marker_cost` is the retrieve marker's size when the call is eligible to
/// ship one, and 0 otherwise. Counting it here is the point: the marker only
/// exists because compression ran, so a call whose saving is smaller than
/// its own marker is a net loss no matter what the reduction percentage
/// says. `min_tokens == 0` disables the gate entirely.
///
/// The case this really decides is `success_collapse`, which stashes
/// regardless of size: a 6-line `git commit` collapsing to one line saves
/// far less than the ~40-token marker announcing the stash.
pub(crate) fn is_net_loss(
    input_tokens: usize,
    output_tokens: usize,
    marker_cost: usize,
    min_tokens: usize,
) -> bool {
    min_tokens > 0
        && input_tokens
            .saturating_sub(output_tokens)
            .saturating_sub(marker_cost)
            < min_tokens
}

/// The retrieve marker's exact text. Shared by the emitted marker and by the
/// pre-gate cost estimate so the two can never drift apart.
fn retrieve_marker_text(orig_line_count: usize, id: &str) -> String {
    format!(
        "[squeez: full {}-line output stored — call squeez_retrieve with key=\"{}\" to expand, or squeez_stash_search to find it later]",
        orig_line_count, id
    )
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
            if crate::context::intensity::window_is_assumed(config, ctx.real_ctx_window) {
                // The host never proved this session's real window, so `pct` may
                // be off by 5x (200K assumed vs. an actual 1M model — issue
                // #199). Telling the user to /clear on that unproven number risks
                // discarding a session with 800K of real headroom left, so this
                // stays informational instead of an imperative "critical" alarm.
                Some(format!(
                    "ℹ️  squeez: {}% of an assumed 200K window ({}K tokens) — the \
                     host doesn't report the real context window, so this may be a \
                     false alarm on a larger-context model.{}",
                    pct.min(100),
                    effective_used / 1000,
                    crate::context::intensity::ASSUMED_WINDOW_NOTE,
                ))
            } else {
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
            }
        } else {
            None
        }
    } else {
        None
    };

    current.save(&dir);
    warning
}

#[cfg(test)]
mod tests {
    use super::{is_cmd_shell, is_net_loss, retrieve_marker_text, shell_choice};

    /// Nothing is on PATH — the bare-Windows case.
    fn nothing(_: &str) -> bool {
        false
    }
    /// git-bash is installed, which is the normal state on any machine running
    /// one of these agent CLIs on Windows.
    fn has_bash(p: &str) -> bool {
        p == "bash"
    }

    #[test]
    fn unix_always_uses_sh() {
        assert_eq!(shell_choice(None, false, has_bash), ("sh".to_string(), "-c"));
        assert_eq!(shell_choice(None, false, nothing), ("sh".to_string(), "-c"));
    }

    /// #208: cmd.exe re-executed commands the agent had written for bash, so
    /// quotes, `$(…)`, backticks, `$HOME` and `;` all came out wrong — silently.
    #[test]
    fn windows_prefers_bash_when_it_is_on_path() {
        assert_eq!(shell_choice(None, true, has_bash), ("bash".to_string(), "-c"));
    }

    #[test]
    fn windows_falls_back_to_cmd_without_a_posix_shell() {
        assert_eq!(shell_choice(None, true, nothing), ("cmd".to_string(), "/C"));
    }

    #[test]
    fn windows_accepts_sh_when_bash_is_absent() {
        assert_eq!(
            shell_choice(None, true, |p| p == "sh"),
            ("sh".to_string(), "-c")
        );
    }

    #[test]
    fn squeez_shell_overrides_every_platform() {
        assert_eq!(
            shell_choice(Some("zsh".into()), false, nothing),
            ("zsh".to_string(), "-c")
        );
        assert_eq!(
            shell_choice(Some("C:/Program Files/Git/bin/bash.exe".into()), true, nothing),
            ("C:/Program Files/Git/bin/bash.exe".to_string(), "-c")
        );
    }

    /// An override back to cmd must come with cmd's flag, not `-c`.
    #[test]
    fn squeez_shell_pointing_at_cmd_gets_the_cmd_flag() {
        assert_eq!(
            shell_choice(Some("cmd".into()), true, has_bash),
            ("cmd".to_string(), "/C")
        );
        assert_eq!(
            shell_choice(Some(r"C:\Windows\System32\cmd.exe".into()), true, has_bash).1,
            "/C"
        );
    }

    /// An unset variable reaches us as `Some("")` in some shells; treat blank
    /// as "not set" rather than trying to exec an empty program name.
    #[test]
    fn a_blank_override_is_ignored() {
        assert_eq!(shell_choice(Some("   ".into()), true, has_bash), ("bash".to_string(), "-c"));
        assert_eq!(shell_choice(Some("".into()), false, nothing), ("sh".to_string(), "-c"));
    }

    #[test]
    fn only_cmd_is_a_cmd_shell() {
        assert!(is_cmd_shell("cmd"));
        assert!(is_cmd_shell("CMD.EXE"));
        assert!(is_cmd_shell(r"C:\Windows\System32\cmd.exe"));
        assert!(!is_cmd_shell("bash"));
        assert!(!is_cmd_shell("/bin/sh"));
        assert!(!is_cmd_shell("C:/Program Files/Git/bin/bash.exe"));
    }


    /// What the marker actually costs, measured the same way the gate does.
    fn marker_cost() -> usize {
        crate::tokens::estimate(&retrieve_marker_text(60, "0000000000000000"))
    }

    #[test]
    fn the_marker_is_expensive_enough_to_matter() {
        // If this ever drops near zero the gate below stops meaning anything.
        assert!(marker_cost() >= 20, "marker cost collapsed to {}", marker_cost());
    }

    #[test]
    fn a_saving_smaller_than_the_marker_is_a_net_loss() {
        let cost = marker_cost();
        // Saves 30 tokens, then spends `cost` announcing it.
        let (input, output) = (100, 70);
        assert!(
            is_net_loss(input, output, cost, 24),
            "30 saved minus a {cost}-token marker must not count as a win"
        );
        // The identical saving with no marker to ship is a genuine win.
        assert!(!is_net_loss(input, output, 0, 24));
    }

    #[test]
    fn a_saving_that_clears_both_marker_and_threshold_passes() {
        assert!(!is_net_loss(1000, 100, marker_cost(), 24));
    }

    #[test]
    fn zero_threshold_disables_the_gate() {
        assert!(!is_net_loss(100, 99, marker_cost(), 0));
    }

    #[test]
    fn saturating_arithmetic_survives_an_inflating_call() {
        // Compression that made the output BIGGER must gate, not underflow.
        assert!(is_net_loss(10, 500, 0, 24));
    }
}
