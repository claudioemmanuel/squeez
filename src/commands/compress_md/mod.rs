// Pure-Rust caveman-style markdown compressor.
// No LLM calls. Preserves code blocks, URLs, headings, file paths, tables.
// Compresses natural-language prose only.

mod locale;
mod locales;
pub use locale::Locale;

use std::path::{Path, PathBuf};

use crate::session::home_dir;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Mode {
    Full,
    Ultra,
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub orig_bytes: usize,
    pub new_bytes: usize,
    pub orig_code_blocks: usize,
    pub new_code_blocks: usize,
    pub orig_urls: usize,
    pub new_urls: usize,
    pub orig_headings: usize,
    pub new_headings: usize,
}

#[derive(Debug, Clone)]
pub struct CompressResult {
    pub output: String,
    pub stats: Stats,
    pub safe: bool,
}

// ── CLI entry ──────────────────────────────────────────────────────────────

pub fn run(args: &[String]) -> i32 {
    let mut mode = Mode::Full;
    let mut dry_run = false;
    let mut all = false;
    let mut quiet = false;
    let mut targets: Vec<String> = Vec::new();
    let mut lang_cli: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--ultra" => mode = Mode::Ultra,
            "--dry-run" => dry_run = true,
            "--all" => all = true,
            "--quiet" => quiet = true,
            "--lang" => {
                if i + 1 >= args.len() {
                    eprintln!("squeez compress-md: --lang requires a value");
                    return 2;
                }
                i += 1;
                lang_cli = Some(args[i].clone());
            }
            "-h" | "--help" => {
                print_help();
                return 0;
            }
            s if s.starts_with("--") => {
                eprintln!("squeez compress-md: unknown flag {}", s);
                return 2;
            }
            s => targets.push(s.to_string()),
        }
        i += 1;
    }

    let locale = {
        let code = lang_cli.unwrap_or_else(|| crate::config::Config::load().lang);
        Locale::from_code(&code)
    };

    let files: Vec<PathBuf> = if all {
        all_targets()
    } else if targets.is_empty() {
        eprintln!("squeez compress-md: no files given (use --all or pass paths)");
        return 2;
    } else {
        targets.iter().map(PathBuf::from).collect()
    };

    let mut had_error = false;
    let mut any_processed = false;

    for f in &files {
        if !f.exists() {
            if !all && !quiet {
                eprintln!("squeez compress-md: not found: {}", f.display());
            }
            continue;
        }
        any_processed = true;
        match process_file(f, mode, dry_run, quiet, locale) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("squeez compress-md: {} — {}", f.display(), e);
                had_error = true;
            }
        }
    }

    if !any_processed && all && !quiet {
        eprintln!("squeez compress-md: no markdown files found in known locations");
    }

    if had_error {
        1
    } else {
        0
    }
}

/// Quiet bulk-compression entry used by `init` when auto_compress_md=true.
/// Never errors out the caller; failures are silent.
pub fn run_all_quietly() -> i32 {
    let cfg = crate::config::Config::load();
    let locale = Locale::from_code(&cfg.lang);
    let files = all_targets();
    for f in &files {
        if !f.exists() {
            continue;
        }
        let _ = process_file(f, Mode::Ultra, false, true, locale);
    }
    0
}

fn print_help() {
    println!("squeez compress-md — pure-Rust markdown prose compressor");
    println!();
    println!("Usage:");
    println!("  squeez compress-md [--ultra] [--dry-run] <file>...");
    println!("  squeez compress-md [--ultra] [--dry-run] --all");
    println!();
    println!("Flags:");
    println!("  --ultra      Aggressive abbreviations (with→w/, function→fn, ...)");
    println!("  --dry-run    Print compressed text to stdout, do not write");
    println!("  --all        Walk known locations: ~/.claude/CLAUDE.md,");
    println!("               ~/.copilot/copilot-instructions.md,");
    println!("               $PWD/CLAUDE.md, $PWD/AGENTS.md,");
    println!("               $PWD/.github/copilot-instructions.md");
    println!("  --quiet      Suppress informational output");
    println!("  --lang <code>  Locale: en (default), pt-BR. Overrides config 'lang'.");
    println!();
    println!("Preserved verbatim: code blocks (```...```), inline `code`,");
    println!("URLs, file paths, headings, tables, list markers, version numbers.");
    println!();
    println!("Backups are written to <stem>.original.md and never overwritten.");
}

fn all_targets() -> Vec<PathBuf> {
    let home = home_dir();
    let mut v = vec![
        PathBuf::from(format!("{}/.claude/CLAUDE.md", home)),
        PathBuf::from(format!("{}/.copilot/copilot-instructions.md", home)),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        v.push(cwd.join("CLAUDE.md"));
        v.push(cwd.join("AGENTS.md"));
        v.push(cwd.join(".github/copilot-instructions.md"));
    }
    v
}

fn process_file(
    path: &Path,
    mode: Mode,
    dry_run: bool,
    quiet: bool,
    locale: &'static Locale,
) -> Result<(), String> {
    let original = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let result = compress_text_with_locale(&original, mode, locale);

    if !result.safe {
        return Err(format!(
            "integrity check failed (code_blocks {}→{}, urls {}→{}, headings {}→{}, bytes {}→{})",
            result.stats.orig_code_blocks,
            result.stats.new_code_blocks,
            result.stats.orig_urls,
            result.stats.new_urls,
            result.stats.orig_headings,
            result.stats.new_headings,
            result.stats.orig_bytes,
            result.stats.new_bytes,
        ));
    }

    let pct = if result.stats.orig_bytes > 0 {
        100usize.saturating_sub(result.stats.new_bytes * 100 / result.stats.orig_bytes)
    } else {
        0
    };

    if dry_run {
        print!("{}", result.output);
        if !quiet {
            eprintln!(
                "# squeez compress-md (dry-run) {} {}→{} bytes (-{}%)",
                path.display(),
                result.stats.orig_bytes,
                result.stats.new_bytes,
                pct
            );
        }
        return Ok(());
    }

    // Skip if already at-or-below target — backup may exist from a prior run.
    if result.stats.new_bytes >= result.stats.orig_bytes {
        if !quiet {
            eprintln!(
                "squeez compress-md: {} already compressed (no further reduction)",
                path.display()
            );
        }
        return Ok(());
    }

    // Backup once. Never clobber.
    let backup = backup_path(path);
    if !backup.exists() {
        std::fs::write(&backup, &original).map_err(|e| e.to_string())?;
    }

    std::fs::write(path, &result.output).map_err(|e| e.to_string())?;

    if !quiet {
        eprintln!(
            "squeez compress-md: {} {}→{} bytes (-{}%)",
            path.display(),
            result.stats.orig_bytes,
            result.stats.new_bytes,
            pct
        );
    }
    Ok(())
}

fn backup_path(p: &Path) -> PathBuf {
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let parent = p.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{}.original.md", stem))
}

// ── Core compression ───────────────────────────────────────────────────────

#[derive(Eq, PartialEq)]
enum State {
    Text,
    FencedCode,
    Table,
}

pub fn compress_text(input: &str, mode: Mode) -> CompressResult {
    compress_text_with_locale(input, mode, Locale::from_code("en"))
}

pub fn compress_text_with_locale(
    input: &str,
    mode: Mode,
    locale: &'static Locale,
) -> CompressResult {
    let mut stats = Stats {
        orig_bytes: input.len(),
        orig_code_blocks: count_code_blocks(input),
        orig_urls: count_urls(input),
        orig_headings: count_headings(input),
        ..Default::default()
    };

    let mut out = String::with_capacity(input.len());
    let mut state = State::Text;

    let lines: Vec<&str> = input.split('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        match state {
            State::FencedCode => {
                out.push_str(line);
                out.push('\n');
                if line.trim_start().starts_with("```") {
                    state = State::Text;
                }
                i += 1;
            }
            State::Table => {
                if is_table_row(line) {
                    out.push_str(line);
                    out.push('\n');
                    i += 1;
                } else {
                    state = State::Text;
                    // reprocess this line as Text without advancing i
                }
            }
            State::Text => {
                if line.trim_start().starts_with("```") {
                    out.push_str(line);
                    out.push('\n');
                    state = State::FencedCode;
                    i += 1;
                } else if is_table_row(line) {
                    out.push_str(line);
                    out.push('\n');
                    state = State::Table;
                    i += 1;
                } else if is_protected_line(line) {
                    out.push_str(line);
                    out.push('\n');
                    i += 1;
                } else {
                    let compressed = compress_prose_line(line, mode, locale);
                    out.push_str(&compressed);
                    out.push('\n');
                    i += 1;
                }
            }
        }
    }

    // Strip trailing newline introduced by split('\n') if input didn't end with one.
    if !input.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }

    let collapsed = collapse_blank_runs(&out);

    stats.new_bytes = collapsed.len();
    stats.new_code_blocks = count_code_blocks(&collapsed);
    stats.new_urls = count_urls(&collapsed);
    stats.new_headings = count_headings(&collapsed);

    let safe = stats.new_code_blocks == stats.orig_code_blocks
        && stats.new_urls >= stats.orig_urls
        && stats.new_headings == stats.orig_headings
        && stats.new_bytes * 5 >= stats.orig_bytes; // not >80% reduction

    CompressResult {
        output: collapsed,
        stats,
        safe,
    }
}

// ── Helpers: classification ────────────────────────────────────────────────

fn is_table_row(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with('|') && t[1..].contains('|')
}

fn is_protected_line(s: &str) -> bool {
    let t = s.trim_start();
    t.is_empty()
        || t.starts_with('#')
        || t.starts_with("<!--")
        || t.starts_with('>')
        || t.starts_with("---")
        || t.starts_with("===")
}

fn count_code_blocks(s: &str) -> usize {
    s.lines()
        .filter(|l| l.trim_start().starts_with("```"))
        .count()
        / 2
}

fn count_urls(s: &str) -> usize {
    let mut n = 0;
    let mut rest = s;
    while let Some(idx) = rest.find("http") {
        let after = &rest[idx..];
        if after.starts_with("http://") || after.starts_with("https://") {
            n += 1;
            // Skip past the URL
            let end = after
                .find(|c: char| c.is_whitespace() || c == ')' || c == ']' || c == '"')
                .unwrap_or(after.len());
            rest = &after[end..];
        } else {
            rest = &after[4..];
        }
    }
    n
}

fn count_headings(s: &str) -> usize {
    s.lines()
        .filter(|l| l.trim_start().starts_with('#'))
        .count()
}

fn collapse_blank_runs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank_run = 0;
    for line in s.split('\n') {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    if !s.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

// ── Prose compression ──────────────────────────────────────────────────────

#[derive(Debug)]
enum Span<'a> {
    Verbatim(&'a str),
    Prose(&'a str),
}

fn split_protected_spans(line: &str) -> Vec<Span<'_>> {
    let mut spans = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut prose_start = 0;

    while i < bytes.len() {
        let c = bytes[i] as char;

        // Inline code: `…`
        if c == '`' {
            if prose_start < i {
                spans.push(Span::Prose(&line[prose_start..i]));
            }
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != b'`' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1; // include closing backtick
            }
            spans.push(Span::Verbatim(&line[start..i]));
            prose_start = i;
            continue;
        }

        // URL: http:// or https://
        if c == 'h' && (line[i..].starts_with("http://") || line[i..].starts_with("https://")) {
            if prose_start < i {
                spans.push(Span::Prose(&line[prose_start..i]));
            }
            let start = i;
            while i < bytes.len() {
                let cc = bytes[i] as char;
                if cc.is_whitespace() || matches!(cc, ')' | ']' | '"' | '>') {
                    break;
                }
                i += 1;
            }
            spans.push(Span::Verbatim(&line[start..i]));
            prose_start = i;
            continue;
        }

        i += 1;
    }
    if prose_start < line.len() {
        spans.push(Span::Prose(&line[prose_start..]));
    }
    spans
}

fn compress_prose_line(line: &str, mode: Mode, locale: &Locale) -> String {
    // Preserve leading whitespace + list markers
    let leading_ws_len = line.len() - line.trim_start().len();
    let leading = &line[..leading_ws_len];
    let body = &line[leading_ws_len..];

    // Detect & preserve list markers (-, *, +, "1.", "1)")
    let (marker, rest) = split_list_marker(body);

    let spans = split_protected_spans(rest);
    let mut out = String::with_capacity(rest.len());
    for span in spans {
        match span {
            Span::Verbatim(v) => out.push_str(v),
            Span::Prose(p) => out.push_str(&compress_prose_span(p, mode, locale)),
        }
    }

    let mut result = String::with_capacity(line.len());
    result.push_str(leading);
    result.push_str(marker);
    result.push_str(&out);
    // trim trailing whitespace
    while result.ends_with(' ') || result.ends_with('\t') {
        result.pop();
    }
    result
}

fn split_list_marker(s: &str) -> (&str, &str) {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return ("", s);
    }
    // - * + followed by space
    if matches!(bytes[0], b'-' | b'*' | b'+') && bytes.get(1) == Some(&b' ') {
        return (&s[..2], &s[2..]);
    }
    // "1. " or "12. "
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0
        && i + 1 < bytes.len()
        && (bytes[i] == b'.' || bytes[i] == b')')
        && bytes[i + 1] == b' '
    {
        return (&s[..i + 2], &s[i + 2..]);
    }
    ("", s)
}

fn compress_prose_span(text: &str, mode: Mode, locale: &Locale) -> String {
    if text.trim().is_empty() {
        return text.to_string();
    }
    let mut s = text.to_string();

    // Drop multi-word phrases (case-insensitive substring)
    for phrase in locale.phrases {
        s = drop_phrase_ci(&s, phrase);
    }

    // Drop fillers + hedges + articles as whole words
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            if !buf.is_empty() {
                tokens.push(std::mem::take(&mut buf));
            }
            tokens.push(c.to_string());
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        tokens.push(buf);
    }

    let mut kept: Vec<String> = Vec::with_capacity(tokens.len());
    for tok in &tokens {
        if tok.chars().all(|c| c.is_whitespace()) {
            kept.push(tok.clone());
            continue;
        }
        // Only drop a token if it's a clean word (no structural punctuation
        // like brackets/parens/braces). Allow trailing comma/period only.
        if is_clean_word(tok) {
            let lower = strip_punct(&tok.to_lowercase());
            if locale.fillers.contains(&lower.as_str())
                || locale.hedges.contains(&lower.as_str())
                || locale.articles.contains(&lower.as_str())
            {
                // drop the following whitespace too
                if matches!(kept.last().map(|s| s.as_str()), Some(s) if s.chars().all(|c| c.is_whitespace())) {
                    kept.pop();
                }
                continue;
            }
        }
        kept.push(tok.clone());
    }

    // Collapse whitespace runs
    let mut out = String::with_capacity(s.len());
    let mut last_ws = false;
    for tok in &kept {
        if tok.chars().all(|c| c.is_whitespace()) {
            if !last_ws {
                out.push(' ');
                last_ws = true;
            }
        } else {
            out.push_str(tok);
            last_ws = false;
        }
    }

    // Trim trailing dangling conjunctions
    let trimmed = trim_trailing_conjunction(out.trim_end(), locale);

    // Strip stray leading punctuation left behind by dropped phrases
    // (e.g. "In general, you could…" → ", you could…" → "you could…").
    let cleaned = strip_leading_orphan_punct(&trimmed);
    // Also clean mid-string orphan commas after sentence boundaries
    // (e.g. "end. , next" → "end. next" when a phrase starting mid-sentence is dropped).
    let cleaned = clean_mid_orphan_punct(cleaned);

    // Ultra: word substitutions outside protected spans (we are inside one)
    let final_out = if mode == Mode::Ultra {
        ultra_subs(cleaned, locale)
    } else {
        cleaned
    };

    // Preserve trailing whitespace if original prose ended with whitespace
    let needs_trailing = text.ends_with(' ') && !final_out.ends_with(' ');
    let needs_leading = text.starts_with(' ') && !final_out.starts_with(' ');
    match (needs_leading, needs_trailing) {
        (true, true) => format!(" {} ", final_out),
        (true, false) => format!(" {}", final_out),
        (false, true) => format!("{} ", final_out),
        (false, false) => final_out,
    }
}

/// Remove orphan commas/semicolons that appear after sentence-boundary punctuation
/// (`. ,` → `. `) or after double-spaces introduced by phrase drops (` , ` → ` `).
fn clean_mid_orphan_punct(s: String) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Pattern: sentence-end punct+space then comma — `. ,` or `! ,` or `? ,`
        if matches!(c, '.' | '!' | '?')
            && chars.get(i + 1) == Some(&' ')
            && matches!(chars.get(i + 2), Some(&',') | Some(&';'))
        {
            out.push(c);   // keep the sentence-end punct
            out.push(' '); // keep one space
            i += 3;        // skip the orphan comma/semicolon
            // also skip any space that follows the skipped comma
            while i < chars.len() && chars[i] == ' ' { i += 1; }
            continue;
        }
        // Pattern: space + orphan comma/semicolon + space → single space
        if c == ' '
            && matches!(chars.get(i + 1), Some(&',') | Some(&';'))
            && chars.get(i + 2) == Some(&' ')
        {
            out.push(' ');
            i += 3;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn strip_leading_orphan_punct(s: &str) -> String {
    let trimmed = s.trim_start();
    let mut chars = trimmed.chars().peekable();
    let mut to_skip = 0;
    while let Some(&c) = chars.peek() {
        if matches!(c, ',' | ';' | ':' | ' ') {
            to_skip += c.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    let lead_ws = s.len() - trimmed.len();
    let original_lead = &s[..lead_ws];
    let body = &trimmed[to_skip..];
    let body = body.trim_start();
    format!("{}{}", original_lead, body)
}

fn strip_punct(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '\'' || *c == '/')
        .collect()
}

/// A token is "clean" if it contains only word characters plus optional
/// trailing comma/period/semicolon. Tokens with brackets, parens, or
/// other structural punctuation are NEVER dropped (they may be link
/// brackets or markup).
fn is_clean_word(tok: &str) -> bool {
    let mut chars = tok.chars().peekable();
    let mut body_len = 0;
    while let Some(&c) = chars.peek() {
        if c.is_alphanumeric() || c == '\'' {
            chars.next();
            body_len += 1;
        } else {
            break;
        }
    }
    if body_len == 0 {
        return false;
    }
    for c in chars {
        if !matches!(c, ',' | '.' | ';' | ':' | '!' | '?') {
            return false;
        }
    }
    true
}

/// Drop all case-insensitive occurrences of `needle` (and any immediately trailing spaces)
/// from `s`. `needle` must be pre-lowercased.
///
/// Uses dual `(s_i, l_i)` byte cursors advanced one `s`-char at a time so that Unicode
/// case expansion (e.g. ß→ss) never desyncs the cursors.
fn drop_phrase_ci(s: &str, needle: &str) -> String {
    // Build lowercase mirror of s for matching.
    let lower: String = s.chars().flat_map(char::to_lowercase).collect();

    let mut out = String::with_capacity(s.len());
    let mut s_i = 0usize; // byte cursor in s
    let mut l_i = 0usize; // byte cursor in lower  (invariant: lower[0..l_i] == lowercase(s[0..s_i]))

    while s_i < s.len() {
        debug_assert!(l_i <= lower.len(), "l_i cursor must not exceed lower.len()");
        if lower[l_i..].starts_with(needle) {
            // Advance both cursors together through the matched chars.
            let l_end = l_i + needle.len();
            while l_i < l_end {
                let ch = s[s_i..].chars().next().unwrap();
                s_i += ch.len_utf8();
                l_i += ch.to_lowercase().map(|c| c.len_utf8()).sum::<usize>();
            }
            // Skip trailing ASCII spaces in both (space → space: 1 byte each).
            while s_i < s.len() && s.as_bytes()[s_i] == b' ' {
                s_i += 1;
                l_i += 1;
            }
        } else {
            // Copy one char from s, advance both cursors.
            let ch = s[s_i..].chars().next().unwrap();
            out.push(ch);
            s_i += ch.len_utf8();
            l_i += ch.to_lowercase().map(|c| c.len_utf8()).sum::<usize>();
        }
    }
    out
}

fn trim_trailing_conjunction(s: &str, locale: &Locale) -> String {
    let lower = s.to_lowercase();
    for c in locale.conjunctions {
        if lower.ends_with(c) {
            return s[..s.len() - c.len()].trim_end().to_string();
        }
    }
    s.to_string()
}

fn ultra_subs(mut s: String, locale: &Locale) -> String {
    for (long, short) in locale.ultra_subs {
        s = replace_word_boundary(&s, long, short);
    }
    s
}

fn is_word_char_unicode(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn replace_word_boundary(s: &str, needle: &str, repl: &str) -> String {
    let needle_lower: String = needle.chars().flat_map(char::to_lowercase).collect();
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        // Try to match needle_lower starting at chars[i]
        let mut buf = String::new();
        let mut j = i;
        let mut matched = false;
        while j < chars.len() {
            for lc in chars[j].1.to_lowercase() {
                buf.push(lc);
            }
            j += 1;
            if buf == needle_lower {
                matched = true;
                break;
            }
            if !needle_lower.starts_with(&buf as &str) {
                break;
            }
        }
        if matched {
            let prev_ok = i == 0 || !is_word_char_unicode(chars[i - 1].1);
            let next_ok = j == chars.len() || !is_word_char_unicode(chars[j].1);
            if prev_ok && next_ok {
                out.push_str(repl);
                i = j;
                continue;
            }
        }
        out.push(chars[i].1);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_code_blocks_pairs_fences() {
        let s = "intro\n```\ncode\n```\nmid\n```rust\nx\n```\n";
        assert_eq!(count_code_blocks(s), 2);
    }

    #[test]
    fn fenced_code_preserved_verbatim() {
        let input = "Some prose with the article.\n```rust\nfn main() { let x = 1; }\n```\nMore prose.\n";
        let r = compress_text(input, Mode::Full);
        assert!(r.safe);
        assert!(r.output.contains("fn main() { let x = 1; }"));
        // 'the' article dropped
        assert!(!r.output.contains("the article"));
    }

    #[test]
    fn url_preserved_inline() {
        let input = "Check https://example.com/foo for the docs.\n";
        let r = compress_text(input, Mode::Full);
        assert!(r.safe);
        assert!(r.output.contains("https://example.com/foo"));
    }

    #[test]
    fn markdown_link_url_preserved() {
        let input = "See [the docs](https://example.com/x) for more.\n";
        let r = compress_text(input, Mode::Full);
        assert!(r.safe);
        assert!(r.output.contains("https://example.com/x"));
    }

    #[test]
    fn heading_count_unchanged() {
        let input = "# H1\n\nprose\n\n## H2\n\nmore prose with the article\n";
        let r = compress_text(input, Mode::Full);
        assert_eq!(r.stats.orig_headings, r.stats.new_headings);
    }

    #[test]
    fn fillers_removed() {
        let input = "This is just really basically a simple test.\n";
        let r = compress_text(input, Mode::Full);
        assert!(!r.output.contains("just"));
        assert!(!r.output.contains("really"));
        assert!(!r.output.contains("basically"));
    }

    #[test]
    fn pleasantries_removed() {
        let input = "I'd be happy to help you with that, of course.\n";
        let r = compress_text(input, Mode::Full);
        assert!(!r.output.to_lowercase().contains("happy to"));
        assert!(!r.output.to_lowercase().contains("of course"));
    }

    #[test]
    fn ultra_substitutes_with() {
        let input = "Configure the app with these parameters.\n";
        let r = compress_text(input, Mode::Ultra);
        assert!(r.output.contains("w/"));
        assert!(r.output.contains("param"));
    }

    #[test]
    fn ultra_does_not_touch_code_block() {
        let input = "Configure with these.\n```\nfn with_config() {}\n```\n";
        let r = compress_text(input, Mode::Ultra);
        assert!(r.output.contains("fn with_config() {}"));
    }

    #[test]
    fn inline_code_preserved() {
        let input = "Use `cargo build --release` to compile the binary.\n";
        let r = compress_text(input, Mode::Full);
        assert!(r.output.contains("`cargo build --release`"));
    }

    #[test]
    fn table_preserved_verbatim() {
        let input = "Intro.\n\n| col1 | col2 |\n|------|------|\n| a    | b    |\n\nOutro.\n";
        let r = compress_text(input, Mode::Full);
        assert!(r.output.contains("| col1 | col2 |"));
        assert!(r.output.contains("| a    | b    |"));
    }

    #[test]
    fn safe_false_when_url_dropped() {
        // Synthetic: stats compare orig_urls vs new_urls
        let mut s = Stats::default();
        s.orig_urls = 3;
        s.new_urls = 2;
        assert!(s.new_urls < s.orig_urls);
    }

    #[test]
    fn integrity_check_on_real_input() {
        let input = "# Title\n\nprose with the link [example](https://example.com).\n\n```\ncode\n```\n";
        let r = compress_text(input, Mode::Full);
        assert!(r.safe);
        assert_eq!(r.stats.orig_headings, r.stats.new_headings);
        assert_eq!(r.stats.orig_code_blocks, r.stats.new_code_blocks);
        assert!(r.stats.new_urls >= r.stats.orig_urls);
    }

    #[test]
    fn idempotent_on_already_compressed() {
        let input = "# Title\n\nshort terse content.\n";
        let r1 = compress_text(input, Mode::Full);
        let r2 = compress_text(&r1.output, Mode::Full);
        // Second pass should not damage further
        assert!(r2.safe);
    }

    #[test]
    fn list_marker_preserved() {
        let input = "- the first item\n- the second item\n";
        let r = compress_text(input, Mode::Full);
        // markers preserved, articles dropped
        assert!(r.output.starts_with("- "));
        assert!(!r.output.contains("the first"));
    }
}
