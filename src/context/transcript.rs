// Real-context tracking (transcript audit CF-1).
//
// Squeez's own token accounting only sees the bytes it processes through
// hooks — in MCP/image-heavy sessions that can be <1% of the real context
// (audit of task-33834: squeez saw ~5.7K of a ~250K-token context). The
// Claude Code hook payload carries `transcript_path`, the session's JSONL
// file, whose assistant records embed `message.usage`. The last assistant
// record's `input_tokens + cache_read_input_tokens + cache_creation_input_tokens`
// IS the effective context size of the most recent API turn — measured, not
// estimated.
//
// We read only the tail of the file (transcripts grow to tens of MB; the
// last turn is always within the final few hundred KB) and scan lines in
// reverse for the newest assistant record carrying a usage block. No JSON
// tree is built — field extraction is a linear scan, keeping the
// zero-dependency constraint.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// How much of the transcript tail to read. One assistant record is at most
/// a few hundred KB even with embedded tool results; 512 KB of tail always
/// contains the latest turn's usage.
const TAIL_BYTES: u64 = 512 * 1024;

/// How much of the transcript head to read when hunting for the session's
/// model-identity record. Claude Code writes that record among the first few
/// entries, so 256 KB covers it even with a large system prompt attached.
const HEAD_BYTES: u64 = 256 * 1024;

/// Effective context tokens of the most recent API turn recorded in the
/// transcript at `path`: input + cache_read + cache_creation of the last
/// assistant record with a usage block. Returns `None` when the file is
/// missing, unreadable, or contains no parsable usage.
pub fn last_context_tokens(path: &Path) -> Option<u64> {
    tail_read(path).and_then(|t| last_context_tokens_in(&t))
}

/// Returns `(cache_read, io_tokens)` for the most recent non-sidechain
/// assistant record in the transcript, where `io_tokens` = input + output.
/// Used to compute the cache-read:I/O ratio for context-leak detection.
pub fn last_context_cache_ratio(path: &Path) -> Option<(u64, u64)> {
    tail_read(path).and_then(|t| last_cache_ratio_in(&t))
}

fn tail_read(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn head_read(path: &Path) -> Option<String> {
    let f = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.take(HEAD_BYTES).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Scan transcript text (newest record last) for the most recent assistant
/// record with a usage block and return its effective context size.
/// Split out from the file wrapper for testability.
pub fn last_context_tokens_in(text: &str) -> Option<u64> {
    for line in text.lines().rev() {
        if !line.contains("\"type\":\"assistant\"") || !line.contains("\"usage\"") {
            continue;
        }
        // Sidechain (subagent) records have their own context — skip them.
        if line.contains("\"isSidechain\":true") {
            continue;
        }
        let input = extract_u64(line, "input_tokens");
        let cache_read = extract_u64(line, "cache_read_input_tokens");
        let cache_creation = extract_u64(line, "cache_creation_input_tokens");
        // `input_tokens` alone can legitimately be 0 mid-turn, but a record
        // where every component is absent has no usable usage block.
        if input.is_none() && cache_read.is_none() && cache_creation.is_none() {
            continue;
        }
        return Some(
            input.unwrap_or(0) + cache_read.unwrap_or(0) + cache_creation.unwrap_or(0),
        );
    }
    None
}

/// Scan transcript text for the most recent non-sidechain assistant record
/// Everything a finished sub-agent actually cost, summed across every request
/// in its own transcript.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AgentUsage {
    pub requests: u64,
    pub input: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub output: u64,
}

impl AgentUsage {
    /// Every billable token the sub-agent consumed.
    ///
    /// Summed, not last-turn: a sub-agent's cost is the whole conversation it
    /// held, and that is dominated by `cache_read` — it re-sends its entire
    /// growing context on every one of its turns. Measured on two research
    /// agents: 3.78M and 5.09M of cache_read against 40K and 44K of output.
    /// Any accounting that skips cache_read understates a sub-agent by ~40x.
    pub fn total(&self) -> u64 {
        self.input
            .saturating_add(self.cache_creation)
            .saturating_add(self.cache_read)
            .saturating_add(self.output)
    }
}

/// Sum usage across every assistant record in a sub-agent transcript.
///
/// This is the measurement that replaces guessing. `agent_spawn_cost` was a
/// flat compiled-in constant, and even scaling it by observed context is still
/// an estimate — a hook cannot know how many turns an agent will take. Once the
/// agent has STOPPED, it no longer has to: the turns already happened and are
/// on disk. Returns `None` when the file is missing or carries no usage.
pub fn measure_agent_usage(path: &Path) -> Option<AgentUsage> {
    let text = std::fs::read_to_string(path).ok()?;
    measure_agent_usage_in(&text)
}

pub fn measure_agent_usage_in(text: &str) -> Option<AgentUsage> {
    let mut u = AgentUsage::default();
    for line in text.lines() {
        if !line.contains("\"usage\"") {
            continue;
        }
        let input = extract_u64(line, "input_tokens").unwrap_or(0);
        let cc = extract_u64(line, "cache_creation_input_tokens").unwrap_or(0);
        let cr = extract_u64(line, "cache_read_input_tokens").unwrap_or(0);
        let out = extract_u64(line, "output_tokens").unwrap_or(0);
        if input == 0 && cc == 0 && cr == 0 && out == 0 {
            continue;
        }
        u.requests = u.requests.saturating_add(1);
        u.input = u.input.saturating_add(input);
        u.cache_creation = u.cache_creation.saturating_add(cc);
        u.cache_read = u.cache_read.saturating_add(cr);
        u.output = u.output.saturating_add(out);
    }
    if u.requests == 0 {
        None
    } else {
        Some(u)
    }
}

/// and return `(cache_read_input_tokens, input_tokens + output_tokens)`.
/// Returns `None` if no parsable record is found.
pub fn last_cache_ratio_in(text: &str) -> Option<(u64, u64)> {
    for line in text.lines().rev() {
        if !line.contains("\"type\":\"assistant\"") || !line.contains("\"usage\"") {
            continue;
        }
        if line.contains("\"isSidechain\":true") {
            continue;
        }
        let input = extract_u64(line, "input_tokens");
        let cache_read = extract_u64(line, "cache_read_input_tokens");
        let output = extract_u64(line, "output_tokens");
        if input.is_none() && cache_read.is_none() && output.is_none() {
            continue;
        }
        let cr = cache_read.unwrap_or(0);
        let io = input.unwrap_or(0) + output.unwrap_or(0);
        return Some((cr, io));
    }
    None
}

/// Context window (in tokens) implied by a model id string. Claude models
/// ship a 200K window by default; the `[1m]` / `-1m` long-context variants
/// expose 1M. squeez keys budget/pressure math to this so it never warns
/// against the wrong window (e.g. flagging 17%-of-1M as "critical" because it
/// assumed 200K). Unknown ids fall back to the conservative 200K standard.
///
/// `model` may be an id (`claude-sonnet-5[1m]`), a marketing name
/// (`Sonnet 5 (1M context)`), or the two joined — the marketing name is the
/// only channel that carries the tier on some hosts (#219), and it spells the
/// marker as `(1M context)` rather than a `[1m]` suffix.
pub fn window_for_model(model: &str) -> u64 {
    let m = model.to_ascii_lowercase();
    if m.contains("[1m]") || m.contains("-1m") || m.contains(" 1m") || m.contains("(1m") {
        1_000_000
    } else {
        200_000
    }
}

/// Window implied by the session's model-identity record, when the transcript
/// carries one.
///
/// Claude Code writes an `"type":"attachment"` record holding
/// `identity.modelId` + `identity.marketingName`. Unlike the per-message
/// `message.model` field — which records the bare `claude-sonnet-5` even on a
/// 1M session (#199, #219) — that record keeps the `[1m]` suffix and the
/// `(1M context)` marketing name, so it proves the tier below 200K of observed
/// context, where the two tiers are otherwise indistinguishable.
///
/// Scanned in reverse: a mid-session model switch writes a fresh record, and
/// the newest one is the session's current model.
pub fn window_from_identity_in(text: &str) -> Option<u64> {
    for line in text.lines().rev() {
        // Anchor on the attachment record itself. A tool result that merely
        // quotes transcript JSON (a `grep` over `~/.claude/projects`, say) is
        // a `"type":"user"` line and must not be read as this session's model.
        if !line.contains("\"type\":\"attachment\"") || !line.contains("\"identity\":{") {
            continue;
        }
        if line.contains("\"isSidechain\":true") {
            continue;
        }
        let id = extract_str(line, "modelId");
        let name = extract_str(line, "marketingName");
        if id.is_none() && name.is_none() {
            continue;
        }
        let joined = format!("{} {}", id.unwrap_or_default(), name.unwrap_or_default());
        return Some(window_for_model(&joined));
    }
    None
}

/// The standard Claude context window. The only larger tier is 1M, so any
/// observed context above this implies the host is on the 1M window.
pub const STANDARD_WINDOW: u64 = 200_000;

/// Infer the real context window from two signals: the model id's baseline
/// window and the largest context actually observed. The Claude transcript
/// records the base model id (`claude-opus-4-8`) WITHOUT the `[1m]` suffix even
/// on 1M sessions, so the model id alone can't prove 1M — but a model that has
/// already accepted >200K of context cannot be on the 200K tier, so the
/// observed ceiling promotes it to 1M. Below 200K the two tiers are
/// indistinguishable from the transcript; set `context_window_tokens` to pin it.
pub fn infer_window(model_window: u64, observed_ctx: u64) -> u64 {
    let base = if model_window > 0 {
        model_window
    } else {
        STANDARD_WINDOW
    };
    if observed_ctx > STANDARD_WINDOW {
        base.max(1_000_000)
    } else {
        base
    }
}

/// Detect the host's context window from the most recent non-sidechain
/// assistant record's `model` field. Returns `None` when no model id is found.
pub fn detect_window_in(text: &str) -> Option<u64> {
    for line in text.lines().rev() {
        if !line.contains("\"type\":\"assistant\"") {
            continue;
        }
        if line.contains("\"isSidechain\":true") {
            continue;
        }
        if let Some(model) = extract_str(line, "model") {
            return Some(window_for_model(&model));
        }
    }
    None
}

/// Detect the host's context window from the transcript at `path`.
///
/// The model-identity record is authoritative and is checked first: in the
/// tail (a mid-session model switch appends a new one), then — only for a
/// transcript longer than the tail window — in the head, where the record the
/// session opened with lives. Falls back to the per-message `model` field,
/// which cannot distinguish the tiers on its own (#219).
pub fn detect_window(path: &Path) -> Option<u64> {
    let tail = tail_read(path)?;
    if let Some(w) = window_from_identity_in(&tail) {
        return Some(w);
    }
    let longer_than_tail = std::fs::metadata(path)
        .map(|m| m.len() > TAIL_BYTES)
        .unwrap_or(false);
    if longer_than_tail {
        if let Some(w) = head_read(path).as_deref().and_then(window_from_identity_in) {
            return Some(w);
        }
    }
    detect_window_in(&tail)
}

/// Extract the first `"key":"<value>"` string occurrence from `line`.
fn extract_str(line: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\":\"", key);
    let idx = line.find(&pat)?;
    let after = &line[idx + pat.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

/// Extract the first `"key":<number>` occurrence from `line`.
/// `"input_tokens"` must not match `"cache_read_input_tokens"`, so the
/// match is anchored on the full quoted key.
fn extract_u64(line: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{}\":", key);
    let idx = line.find(&pat)?;
    let after = line[idx + pat.len()..].trim_start();
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TURN: &str = r#"{"type":"assistant","message":{"id":"msg_1","usage":{"input_tokens":120,"cache_read_input_tokens":180000,"cache_creation_input_tokens":4500,"output_tokens":900}}}"#;

    #[test]
    fn extracts_last_assistant_usage() {
        let text = format!(
            "{}\n{}\n{}",
            r#"{"type":"user","message":{"content":"hi"}}"#,
            r#"{"type":"assistant","message":{"id":"msg_0","usage":{"input_tokens":10,"cache_read_input_tokens":100,"cache_creation_input_tokens":5}}}"#,
            TURN,
        );
        // Last assistant turn wins: 120 + 180000 + 4500
        assert_eq!(last_context_tokens_in(&text), Some(184_620));
    }

    #[test]
    fn skips_sidechain_records() {
        let side = r#"{"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":1,"cache_read_input_tokens":999999,"cache_creation_input_tokens":0}}}"#;
        let text = format!("{}\n{}", TURN, side);
        assert_eq!(last_context_tokens_in(&text), Some(184_620));
    }

    #[test]
    fn input_tokens_does_not_match_cache_keys() {
        // Only cache_read present — input must read as absent, not as the
        // cache_read value via substring match.
        let line = r#"{"type":"assistant","message":{"usage":{"cache_read_input_tokens":5000}}}"#;
        assert_eq!(last_context_tokens_in(line), Some(5000));
    }

    #[test]
    fn no_assistant_usage_returns_none() {
        assert_eq!(last_context_tokens_in(""), None);
        assert_eq!(
            last_context_tokens_in(r#"{"type":"user","message":{"content":"x"}}"#),
            None
        );
        // assistant without usage block
        assert_eq!(
            last_context_tokens_in(r#"{"type":"assistant","message":{"content":[]}}"#),
            None
        );
    }

    #[test]
    fn missing_file_returns_none() {
        assert_eq!(
            last_context_tokens(Path::new("/nonexistent/squeez/transcript.jsonl")),
            None
        );
    }

    #[test]
    fn infer_window_promotes_on_observed_ceiling() {
        // Base model id maps to 200K, but 223K observed can only be the 1M tier.
        assert_eq!(infer_window(200_000, 223_328), 1_000_000);
        // Below the standard window: stays at the model-id baseline.
        assert_eq!(infer_window(200_000, 150_000), 200_000);
        // Explicit 1M model id stays 1M regardless of observed.
        assert_eq!(infer_window(1_000_000, 10_000), 1_000_000);
        // Unknown model id (0) defaults to standard, still promotes on ceiling.
        assert_eq!(infer_window(0, 50_000), 200_000);
        assert_eq!(infer_window(0, 250_000), 1_000_000);
    }

    #[test]
    fn window_for_model_detects_1m_and_default() {
        assert_eq!(window_for_model("claude-opus-4-8[1m]"), 1_000_000);
        assert_eq!(window_for_model("claude-sonnet-5"), 200_000);
        assert_eq!(window_for_model("claude-opus-4-8"), 200_000);
    }

    #[test]
    fn window_for_model_reads_marketing_name_marker() {
        // #219: the marketing name is the only 1M signal on some hosts.
        assert_eq!(window_for_model("Sonnet 5 (1M context)"), 1_000_000);
        assert_eq!(window_for_model("Opus 5 (1M context)"), 1_000_000);
        assert_eq!(window_for_model("claude-sonnet-5 Sonnet 5"), 200_000);
    }

    const IDENTITY_1M: &str = r#"{"isSidechain":false,"attachment":{"type":"model","identity":{"modelId":"claude-sonnet-5[1m]","marketingName":"Sonnet 5 (1M context)","knowledgeCutoff":"May 2026"}},"type":"attachment"}"#;
    const IDENTITY_STD: &str = r#"{"isSidechain":false,"attachment":{"type":"model","identity":{"modelId":"claude-sonnet-5","marketingName":"Sonnet 5","knowledgeCutoff":"May 2026"}},"type":"attachment"}"#;

    #[test]
    fn identity_record_proves_1m_where_message_model_cannot() {
        // The per-message model field of the same session says plain
        // `claude-sonnet-5` — 200K — while the identity record says 1M.
        let msg = r#"{"type":"assistant","message":{"model":"claude-sonnet-5","usage":{"input_tokens":1}}}"#;
        assert_eq!(detect_window_in(msg), Some(200_000));
        assert_eq!(
            window_from_identity_in(&format!("{}\n{}", IDENTITY_1M, msg)),
            Some(1_000_000)
        );
        assert_eq!(window_from_identity_in(IDENTITY_STD), Some(200_000));
    }

    #[test]
    fn identity_scan_takes_the_newest_record() {
        // A mid-session switch off the 1M tier must win over the opening record.
        let text = format!("{}\n{}", IDENTITY_1M, IDENTITY_STD);
        assert_eq!(window_from_identity_in(&text), Some(200_000));
    }

    #[test]
    fn identity_scan_ignores_quoted_transcript_json() {
        // A tool result that greps other transcripts embeds the same keys in a
        // `"type":"user"` record; it is not this session's model.
        let quoted = r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"\"identity\":{\"modelId\":\"claude-opus-5[1m]\""}]}}"#;
        assert_eq!(window_from_identity_in(quoted), None);
        assert_eq!(window_from_identity_in(""), None);
    }

    #[test]
    fn detect_window_reads_model_from_last_assistant() {
        let line = r#"{"type":"assistant","message":{"model":"claude-opus-4-8[1m]","usage":{"input_tokens":1}}}"#;
        assert_eq!(detect_window_in(line), Some(1_000_000));
        let std = r#"{"type":"assistant","message":{"model":"claude-sonnet-5","usage":{"input_tokens":1}}}"#;
        assert_eq!(detect_window_in(std), Some(200_000));
    }

    #[test]
    fn detect_window_skips_sidechain_and_handles_missing() {
        let side = r#"{"type":"assistant","isSidechain":true,"message":{"model":"x[1m]"}}"#;
        let main = r#"{"type":"assistant","message":{"model":"claude-sonnet-5"}}"#;
        assert_eq!(detect_window_in(&format!("{}\n{}", side, main)), Some(200_000));
        assert_eq!(detect_window_in(""), None);
        assert_eq!(detect_window_in(r#"{"type":"user"}"#), None);
    }

    #[test]
    fn cache_ratio_extracts_cache_read_and_io() {
        // TURN: input=120, cache_read=180000, output=900 → (180000, 1020)
        let (cr, io) = last_cache_ratio_in(TURN).expect("should parse");
        assert_eq!(cr, 180_000);
        assert_eq!(io, 1_020); // 120 input + 900 output
    }

    #[test]
    fn cache_ratio_skips_sidechain() {
        let side = r#"{"type":"assistant","isSidechain":true,"message":{"usage":{"input_tokens":1,"cache_read_input_tokens":999999,"output_tokens":1}}}"#;
        let text = format!("{}\n{}", TURN, side);
        let (cr, _) = last_cache_ratio_in(&text).expect("should parse non-sidechain");
        assert_eq!(cr, 180_000);
    }

    #[test]
    fn cache_ratio_returns_none_when_no_data() {
        assert_eq!(last_cache_ratio_in(""), None);
    }

    #[test]
    fn reads_tail_of_real_file() {
        let dir = std::env::temp_dir().join(format!(
            "squeez_transcript_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.jsonl");
        std::fs::write(&path, format!("{}\n{}\n", r#"{"type":"user"}"#, TURN)).unwrap();
        assert_eq!(last_context_tokens(&path), Some(184_620));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
