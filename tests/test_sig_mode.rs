use squeez::commands::{fs::FsHandler, Handler};
use squeez::config::Config;

// ── helpers ────────────────────────────────────────────────────────────────

fn make_rust_file(n: usize) -> Vec<String> {
    // ~10 body lines per function so signatures are sparse (≈n/10 sigs).
    let mut lines = Vec::with_capacity(n);
    lines.push("// auto-generated synthetic Rust file".to_string());
    lines.push("use std::collections::HashMap;".to_string());
    lines.push("".to_string());
    let mut i = 0usize;
    while lines.len() + 12 <= n {
        lines.push(format!("pub fn function_{}(x: u32) -> u32 {{", i));
        // 9 body lines + closing brace + blank = 11 more lines
        for j in 0..9usize {
            lines.push(format!("    let v{} = x * {};", j, i + j));
        }
        lines.push("}".to_string());
        lines.push("".to_string());
        i += 1;
    }
    // pad to exactly n lines
    while lines.len() < n {
        lines.push(format!("// padding line {}", lines.len()));
    }
    lines.truncate(n);
    lines
}

fn make_py_file(n: usize) -> Vec<String> {
    let mut lines = Vec::with_capacity(n);
    lines.push("# synthetic Python file".to_string());
    lines.push("import os".to_string());
    lines.push("".to_string());
    for i in 0..(n / 5) {
        lines.push(format!("def func_{}(arg):", i));
        lines.push(format!("    return arg + {}", i));
        lines.push("".to_string());
        lines.push(format!("class MyClass_{}:", i));
        lines.push("    pass".to_string());
    }
    while lines.len() < n {
        lines.push(format!("# pad {}", lines.len()));
    }
    lines.truncate(n);
    lines
}

fn make_txt_file(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("line {}", i)).collect()
}

// ── (a) 1000-line Rust file → sig-mode fires ──────────────────────────────

#[test]
fn rust_1000_lines_sig_mode_fires() {
    let lines = make_rust_file(1000);
    let cfg = Config::default(); // sig_mode_enabled=true, threshold=400
    let result = FsHandler.compress("cat src/foo.rs", lines, &cfg);

    // Marker present
    assert!(
        result[0].starts_with("[squeez: sig-mode"),
        "expected marker as first line, got: {:?}",
        result[0]
    );
    assert!(result[0].contains("from 1000 lines"));
    assert!(result[0].contains("src/foo.rs"));

    // Compressed well below 150 output lines
    assert!(
        result.len() < 150,
        "expected < 150 lines, got {}",
        result.len()
    );

    // Every fn signature from the synthetic file is present
    let body = result[1..].join("\n");
    assert!(body.contains("pub fn function_0("), "missing function_0");
    assert!(body.contains("pub fn function_1("), "missing function_1");
}

// ── (b) 500-line .txt file → untouched ────────────────────────────────────

#[test]
fn txt_file_not_compressed() {
    let lines = make_txt_file(500);
    let original_len = lines.len();
    let cfg = Config::default();
    let result = FsHandler.compress("cat notes.txt", lines, &cfg);

    // No marker
    assert!(
        !result[0].starts_with("[squeez: sig-mode"),
        "sig-mode should not fire on .txt"
    );
    // Line count unchanged (possibly truncated by find_max_results, but no sig-mode)
    // Key check: marker absent
    for line in &result {
        assert!(
            !line.starts_with("[squeez: sig-mode"),
            "unexpected sig-mode marker in .txt output"
        );
    }
    let _ = original_len; // used above
}

// ── (c) sig_mode_enabled=false + 1000-line .rs → untouched ───────────────

#[test]
fn sig_mode_disabled_skips_compression() {
    let lines = make_rust_file(1000);
    let mut cfg = Config::default();
    cfg.sig_mode_enabled = false;

    let result = FsHandler.compress("cat src/lib.rs", lines, &cfg);

    for line in &result {
        assert!(
            !line.starts_with("[squeez: sig-mode"),
            "sig-mode marker must not appear when disabled"
        );
    }
}

// ── (d) 100-line .rs below threshold → untouched ─────────────────────────

#[test]
fn short_rs_below_threshold_untouched() {
    let lines = make_rust_file(100);
    let cfg = Config::default(); // threshold=400

    let result = FsHandler.compress("cat src/small.rs", lines, &cfg);

    for line in &result {
        assert!(
            !line.starts_with("[squeez: sig-mode"),
            "sig-mode must not fire below threshold"
        );
    }
}

// ── (e) .py file with 500 lines → sig-mode fires, class/def preserved ────

#[test]
fn python_500_lines_sig_mode_fires() {
    let lines = make_py_file(500);
    let cfg = Config::default();

    let result = FsHandler.compress("cat app/views.py", lines, &cfg);

    assert!(
        result[0].starts_with("[squeez: sig-mode"),
        "expected sig-mode marker for .py file, got: {:?}",
        result[0]
    );
    assert!(result[0].contains("from 500 lines"));

    let body = result[1..].join("\n");
    assert!(body.contains("def func_0("), "missing func_0");
    assert!(body.contains("class MyClass_0:"), "missing MyClass_0");
}

// ── config.ini parsing for new fields ─────────────────────────────────────

#[test]
fn config_sig_mode_defaults() {
    let cfg = Config::default();
    assert!(cfg.sig_mode_enabled);
    assert_eq!(cfg.sig_mode_threshold_lines, 400);
}

#[test]
fn config_sig_mode_parsed_from_ini() {
    let ini = "sig_mode_enabled=false\nsig_mode_threshold_lines=200\n";
    let cfg = Config::from_str(ini);
    assert!(!cfg.sig_mode_enabled);
    assert_eq!(cfg.sig_mode_threshold_lines, 200);
}

// ── head / tail variants also route through sig-mode ─────────────────────

#[test]
fn head_command_triggers_sig_mode() {
    let lines = make_rust_file(1000);
    let cfg = Config::default();
    let result = FsHandler.compress("head -n 1000 src/main.rs", lines, &cfg);
    assert!(
        result[0].starts_with("[squeez: sig-mode"),
        "head command should trigger sig-mode"
    );
}

#[test]
fn tail_command_triggers_sig_mode() {
    let lines = make_rust_file(1000);
    let cfg = Config::default();
    let result = FsHandler.compress("tail -n 1000 src/lib.rs", lines, &cfg);
    assert!(
        result[0].starts_with("[squeez: sig-mode"),
        "tail command should trigger sig-mode"
    );
}

// ── non-code extension with bat → untouched ──────────────────────────────

#[test]
fn bat_on_json_file_no_sig_mode() {
    let lines: Vec<String> = (0..600).map(|i| format!("{{\"key\": {}}}", i)).collect();
    let cfg = Config::default();
    let result = FsHandler.compress("bat config.json", lines, &cfg);
    for line in &result {
        assert!(
            !line.starts_with("[squeez: sig-mode"),
            "sig-mode must not fire on .json"
        );
    }
}

// ── P4 (issue #130): single-slot attribute preservation above signatures ──

/// Build a Rust file with `#[…]` attribute lines immediately above each
/// top-level signature.
fn make_rust_with_attrs(n: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::with_capacity(n);
    lines.push("//! Module-level docs.".to_string());
    lines.push("use std::collections::HashMap;".to_string());
    lines.push("".to_string());
    let mut i = 0usize;
    while lines.len() + 12 <= n {
        lines.push(format!("#[inline]"));
        lines.push(format!("pub fn helper_{}(x: u32) -> u32 {{", i));
        for j in 0..9 {
            lines.push(format!("    let v{} = x * {};", j, i + j));
        }
        lines.push("}".to_string());
        lines.push("".to_string());
        i += 1;
    }
    while lines.len() < n {
        lines.push(format!("// pad {}", lines.len()));
    }
    lines.truncate(n);
    lines
}

#[test]
fn rust_attribute_above_signature_is_kept() {
    let lines = make_rust_with_attrs(500);
    let cfg = Config::default();
    let result = FsHandler.compress("cat src/lib.rs", lines, &cfg);
    let body = result.join("\n");
    // Attribute line immediately above each kept signature must survive.
    let kept_attrs = result
        .iter()
        .filter(|l| l.starts_with("#[inline]"))
        .count();
    assert!(
        kept_attrs >= 5,
        "expected ≥5 attribute lines preserved, got {}\n{}",
        kept_attrs,
        body,
    );
}

#[test]
fn doc_comment_lines_are_not_promoted_above_signatures() {
    // Single-slot context buffer holds only attributes/decorators.
    // Pure `///` doc-comment lines are not promoted on dense-signature files,
    // because the cumulative bloat would push sig-mode past the pipeline's
    // truncation budget (see comment on `compress_signatures`).
    let mut lines: Vec<String> = Vec::new();
    lines.push("//! mod docs".to_string());
    lines.push("use std::io;".to_string());
    lines.push("".to_string());
    for i in 0..80 {
        lines.push(format!("/// Doc for fn {}.", i));
        lines.push(format!("pub fn f_{}() {{", i));
        for j in 0..5 {
            lines.push(format!("    let _ = {} + {};", j, i));
        }
        lines.push("}".to_string());
    }
    let cfg = Config::default();
    let result = FsHandler.compress("cat src/many.rs", lines, &cfg);
    let kept_docs = result.iter().filter(|l| l.starts_with("/// Doc for fn")).count();
    let kept_sigs = result.iter().filter(|l| l.starts_with("pub fn f_")).count();
    assert!(kept_sigs >= 50, "signatures must still come through: got {}", kept_sigs);
    assert!(
        kept_docs <= 3,
        "doc-comment lines must not be promoted in bulk (got {}); only anchor lines may carry them",
        kept_docs,
    );
}

#[test]
fn python_decorator_above_signature_is_kept() {
    let mut lines: Vec<String> = Vec::new();
    lines.push("\"\"\"mod\"\"\"".to_string());
    lines.push("import asyncio".to_string());
    lines.push("".to_string());
    for i in 0..40 {
        lines.push("@staticmethod".to_string());
        lines.push(format!("def func_{}():", i));
        for j in 0..6 {
            lines.push(format!("    x{} = {}", j, i));
        }
    }
    while lines.len() < 500 {
        lines.push(format!("# pad {}", lines.len()));
    }
    let cfg = Config::default();
    let result = FsHandler.compress("cat service.py", lines, &cfg);
    let kept = result.iter().filter(|l| l.starts_with("@staticmethod")).count();
    assert!(
        kept >= 5,
        "decorator lines must survive above kept signatures, got {}\n{}",
        kept,
        result.join("\n"),
    );
}

// ── #139: sig_mode_include_indented — capture methods inside impl/class ───

/// Rust impl block: methods are indented and currently invisible to sig-mode.
fn make_rust_impl_heavy(n: usize) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push("//! Library with impl-heavy structure.".to_string());
    lines.push("use std::fmt;".to_string());
    lines.push("".to_string());
    let mut i = 0usize;
    while lines.len() + 25 <= n {
        lines.push(format!("pub struct Widget{} {{", i));
        lines.push(format!("    pub id: u64,"));
        lines.push("}".to_string());
        lines.push("".to_string());
        lines.push(format!("impl Widget{} {{", i));
        for j in 0..4usize {
            lines.push(format!("    pub fn method_{}_{}(&self) -> u64 {{", i, j));
            for k in 0..3usize {
                lines.push(format!("        let x{} = self.id + {};", k, k));
            }
            lines.push("    }".to_string());
        }
        lines.push("}".to_string());
        lines.push("".to_string());
        i += 1;
    }
    while lines.len() < n {
        lines.push(format!("// padding {}", lines.len()));
    }
    lines.truncate(n);
    lines
}

/// TypeScript class: methods are indented.
fn make_ts_class_heavy(n: usize) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push("// TypeScript class fixture".to_string());
    lines.push("import { Injectable } from '@angular/core';".to_string());
    lines.push("".to_string());
    let mut i = 0usize;
    while lines.len() + 20 <= n {
        lines.push(format!("export class Service{} {{", i));
        for j in 0..3usize {
            lines.push(format!("  public method_{}_{}(x: number): string {{", i, j));
            for k in 0..4usize {
                lines.push(format!("    const v{} = x + {};", k, k));
            }
            lines.push("  }".to_string());
        }
        lines.push("}".to_string());
        lines.push("".to_string());
        i += 1;
    }
    while lines.len() < n {
        lines.push(format!("// pad {}", lines.len()));
    }
    lines.truncate(n);
    lines
}

#[test]
fn indented_sig_mode_off_by_default_no_impl_methods() {
    let lines = make_rust_impl_heavy(500);
    let cfg = Config::default(); // sig_mode_include_indented defaults to false
    let result = FsHandler.compress("cat src/lib.rs", lines, &cfg);
    // Sig-mode fires (file >400 lines) but indented methods are NOT captured.
    assert!(
        result[0].starts_with("[squeez: sig-mode"),
        "sig-mode must fire: {:?}",
        result[0]
    );
    let body = result[1..].join("\n");
    // Top-level impl declarations ARE captured (they're at column 0).
    // Indented method sigs (pub fn method_0_0) are NOT in default mode.
    assert!(
        !body.contains("pub fn method_0_0"),
        "indented methods must NOT appear without flag: {body}",
    );
}

#[test]
fn rust_impl_methods_captured_when_include_indented_enabled() {
    let lines = make_rust_impl_heavy(500);
    let mut cfg = Config::default();
    cfg.sig_mode_include_indented = true;
    let result = FsHandler.compress("cat src/lib.rs", lines, &cfg);
    assert!(
        result[0].starts_with("[squeez: sig-mode"),
        "sig-mode must fire: {:?}",
        result[0]
    );
    let body = result[1..].join("\n");
    assert!(
        body.contains("pub fn method_0_0"),
        "indented Rust methods must be captured: {body}",
    );
    assert!(
        body.contains("pub fn method_0_1"),
        "multiple indented methods must be captured: {body}",
    );
}

#[test]
fn ts_class_methods_captured_when_include_indented_enabled() {
    let lines = make_ts_class_heavy(500);
    let mut cfg = Config::default();
    cfg.sig_mode_include_indented = true;
    let result = FsHandler.compress("cat src/service.ts", lines, &cfg);
    assert!(
        result[0].starts_with("[squeez: sig-mode"),
        "sig-mode must fire: {:?}",
        result[0]
    );
    let body = result[1..].join("\n");
    assert!(
        body.contains("public method_0_0"),
        "indented TS class methods must be captured: {body}",
    );
}

#[test]
fn indented_sig_cap_limits_indented_count() {
    // The cap on indented sigs is (find_max_results - 6) to guard against runaway
    // growth on dense impl-heavy files. Top-level sigs remain uncapped.
    let lines = make_rust_impl_heavy(500);
    let mut cfg = Config::default();
    cfg.sig_mode_include_indented = true;
    let result = FsHandler.compress("cat src/lib.rs", lines, &cfg);
    assert!(
        result[0].starts_with("[squeez: sig-mode"),
        "sig-mode must fire: {:?}",
        result[0]
    );
    // Count captured indented method lines (they contain "pub fn method_").
    let indented_captured = result.iter().filter(|l| l.contains("pub fn method_")).count();
    let cap = cfg.find_max_results.saturating_sub(6);
    assert!(
        indented_captured <= cap,
        "indented sigs captured ({indented_captured}) must not exceed cap ({cap})",
    );
}
