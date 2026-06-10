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

/// Effective context tokens of the most recent API turn recorded in the
/// transcript at `path`: input + cache_read + cache_creation of the last
/// assistant record with a usage block. Returns `None` when the file is
/// missing, unreadable, or contains no parsable usage.
pub fn last_context_tokens(path: &Path) -> Option<u64> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.read_to_end(&mut buf).ok()?;
    let tail = String::from_utf8_lossy(&buf);
    last_context_tokens_in(&tail)
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
