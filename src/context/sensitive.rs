//! Sensitive-content guard (Epic E7).
//!
//! `retrieve::store()` persists any sufficiently-compressed output verbatim
//! to `~/.claude/squeez/blobs/` for `retrieve_ttl_days` (default 7), and
//! `cache::note_errors()` persists a 128-char verbatim snippet of every new
//! error line into `context.json`. Neither path previously checked *what* it
//! was about to write — `cat .env`, a leaked private key, or a curl command
//! echoing an `Authorization` header would land on disk unexamined. This
//! module scans text for shapes that look like real credentials so callers
//! can refuse to persist it.
//!
//! Zero-dependency: hand-rolled literal/shape scanners, no regex, in the
//! style of `factsheet.rs`. Detection is deliberately conservative (biased
//! against false positives) over exhaustive — this is a first line of
//! defense against accidental verbatim persistence, not a full DLP scanner.

/// Only the first 64 KiB is scanned — secrets that matter overwhelmingly
/// appear near the top of a file (top of `.env`, top of a key file) or
/// within the truncated error/blob text this guard runs on, and this bounds
/// worst-case scan cost on large tool outputs.
const SCAN_CAP_BYTES: usize = 64 * 1024;

/// Scan `text` for credential-shaped content. Returns the matched class name
/// (stable, for logging/tests) or `None` if nothing tripped. Callers should
/// treat `Some(_)` as "do not persist this verbatim."
pub fn is_sensitive(text: &str) -> Option<&'static str> {
    let mut end = text.len().min(SCAN_CAP_BYTES);
    // Land on a char boundary so slicing never panics on multi-byte UTF-8.
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let scan = &text[..end];

    if has_private_key_block(scan) {
        return Some("private-key-block");
    }
    if has_aws_access_key(scan) {
        return Some("aws-access-key");
    }
    if has_github_token(scan) {
        return Some("github-token");
    }
    if has_slack_token(scan) {
        return Some("slack-token");
    }
    if has_api_secret_key(scan) {
        return Some("api-secret-key");
    }
    if has_bearer_token(scan) {
        return Some("bearer-token");
    }
    if has_aws_secret_line(scan) {
        return Some("aws-secret-line");
    }
    if has_dotenv_bulk_secrets(scan) {
        return Some("dotenv-bulk-secrets");
    }
    None
}

// ── Shared byte-level scan helpers ──────────────────────────────────────

fn is_alnum(c: u8) -> bool {
    c.is_ascii_alphanumeric()
}

fn is_upper_alnum(c: u8) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit()
}

/// First byte offset of `needle` in `hay` at or after `from`, or `None`.
fn find_from(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from > hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Length of the contiguous run starting at `start` whose bytes satisfy
/// `charset`.
fn run_len(b: &[u8], start: usize, charset: impl Fn(u8) -> bool) -> usize {
    let mut n = 0;
    while start + n < b.len() && charset(b[start + n]) {
        n += 1;
    }
    n
}

/// True if any occurrence of `prefix` is immediately followed by a run of
/// `charset` bytes at least `min_len` long.
fn has_token_after(s: &str, prefix: &[u8], min_len: usize, charset: impl Fn(u8) -> bool) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while let Some(pos) = find_from(b, prefix, i) {
        let start = pos + prefix.len();
        if run_len(b, start, &charset) >= min_len {
            return true;
        }
        i = pos + 1;
    }
    false
}

// ── Detection classes ────────────────────────────────────────────────────

/// PEM key block: `-----BEGIN` ... `PRIVATE KEY-----` (RSA/EC/OPENSSH/plain
/// all end their header the same way). Both markers must be present; they
/// don't need to be adjacent since real PEM headers wrap the key type
/// between them (e.g. `-----BEGIN RSA PRIVATE KEY-----`).
fn has_private_key_block(s: &str) -> bool {
    s.contains("-----BEGIN") && s.contains("PRIVATE KEY-----")
}

/// AWS access key id: literal `AKIA` followed by exactly-or-more 16
/// uppercase-alphanumeric characters. Boundary checks (the byte before
/// `AKIA` and the byte after the run, if present, must not themselves be
/// upper-alnum) stop this from firing on a substring of some longer
/// uppercase-alnum token.
fn has_aws_access_key(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while let Some(pos) = find_from(b, b"AKIA", i) {
        let start = pos + 4;
        let pre_ok = pos == 0 || !is_upper_alnum(b[pos - 1]);
        let len = run_len(b, start, is_upper_alnum);
        let post_ok = start + len >= b.len() || !is_upper_alnum(b[start + len]);
        if pre_ok && post_ok && len >= 16 {
            return true;
        }
        i = pos + 1;
    }
    false
}

/// GitHub tokens: classic `ghp_` (personal access token) and fine-grained
/// `github_pat_`, each followed by a long run of token characters.
fn has_github_token(s: &str) -> bool {
    has_token_after(s, b"ghp_", 20, is_alnum)
        || has_token_after(s, b"github_pat_", 20, |c| is_alnum(c) || c == b'_')
}

/// Slack tokens: bot (`xoxb-`) and user (`xoxp-`) prefixes, followed by the
/// dash-separated alphanumeric segments Slack issues.
fn has_slack_token(s: &str) -> bool {
    has_token_after(s, b"xoxb-", 10, |c| is_alnum(c) || c == b'-')
        || has_token_after(s, b"xoxp-", 10, |c| is_alnum(c) || c == b'-')
}

/// OpenAI/Anthropic-style secret keys: `sk-` followed by >=20 alnum
/// characters. Ordinary prose that merely ends a word in "sk" before a
/// hyphen ("desk-top", "risk-averse", "task-1234") is excluded by requiring
/// the byte before `sk-` to not be an ASCII letter — real keys are always
/// preceded by a delimiter (quote, `=`, whitespace, start of string).
fn has_api_secret_key(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while let Some(pos) = find_from(b, b"sk-", i) {
        let pre_ok = pos == 0 || !b[pos - 1].is_ascii_alphabetic();
        let start = pos + 3;
        if pre_ok && run_len(b, start, is_alnum) >= 20 {
            return true;
        }
        i = pos + 1;
    }
    false
}

/// `Authorization: Bearer <token>` header with a long token — the shape
/// curl -v / HAR dumps / proxy logs echo back verbatim.
fn has_bearer_token(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while let Some(pos) = find_from(b, b"Authorization:", i) {
        let mut j = pos + "Authorization:".len();
        while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
            j += 1;
        }
        if b[j..].starts_with(b"Bearer") {
            let mut k = j + "Bearer".len();
            while k < b.len() && b[k] == b' ' {
                k += 1;
            }
            let charset = |c: u8| is_alnum(c) || matches!(c, b'.' | b'-' | b'_');
            if run_len(b, k, charset) >= 20 {
                return true;
            }
        }
        i = pos + 1;
    }
    false
}

/// AWS secret access key assignment line, either `~/.aws/credentials` style
/// (`aws_secret_access_key = <value>`) or env-var style
/// (`AWS_SECRET_ACCESS_KEY=<value>`), where `<value>` is a long
/// base64-ish run (AWS secret keys are 40 base64 characters).
fn has_aws_secret_line(s: &str) -> bool {
    has_assignment_after(s, "aws_secret_access_key") || has_assignment_after(s, "AWS_SECRET_ACCESS_KEY")
}

fn has_assignment_after(s: &str, key: &str) -> bool {
    let b = s.as_bytes();
    let kb = key.as_bytes();
    let mut i = 0;
    while let Some(pos) = find_from(b, kb, i) {
        let mut j = pos + kb.len();
        while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
            j += 1;
        }
        if j < b.len() && b[j] == b'=' {
            j += 1;
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            let charset = |c: u8| is_alnum(c) || matches!(c, b'/' | b'+' | b'=');
            if run_len(b, j, charset) >= 30 {
                return true;
            }
        }
        i = pos + 1;
    }
    false
}

/// Dotenv-style bulk secrets: three or more lines matching `KEY=value` where
/// `KEY` is an uppercase/digit/underscore identifier containing SECRET,
/// TOKEN, PASSWORD, APIKEY, or API_KEY. A single such line is common enough
/// in ordinary output (a log line mentioning `API_TOKEN=set`) to not be
/// alarming on its own; three or more is the shape of an actual `.env` dump.
fn has_dotenv_bulk_secrets(s: &str) -> bool {
    const MARKERS: [&str; 5] = ["SECRET", "TOKEN", "PASSWORD", "APIKEY", "API_KEY"];
    let mut count = 0;
    for line in s.lines() {
        let line = line.trim();
        let Some(eq) = line.find('=') else { continue };
        let key = &line[..eq];
        let val = line[eq + 1..].trim();
        if key.is_empty() || val.is_empty() {
            continue;
        }
        if !key.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_') {
            continue;
        }
        if MARKERS.iter().any(|m| key.contains(m)) {
            count += 1;
            if count >= 3 {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_private_key_block() {
        let s = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow...\n-----END RSA PRIVATE KEY-----";
        assert_eq!(is_sensitive(s), Some("private-key-block"));
    }

    #[test]
    fn detects_aws_access_key() {
        assert_eq!(is_sensitive("key=AKIAIOSFODNN7EXAMPLE"), Some("aws-access-key"));
    }

    #[test]
    fn ignores_akia_prefix_too_short() {
        assert_eq!(is_sensitive("AKIASHORT"), None);
    }

    #[test]
    fn detects_github_pat_classic() {
        assert_eq!(
            is_sensitive("token: ghp_abcdefghijklmnopqrstuvwxyzABCDEF"),
            Some("github-token")
        );
    }

    #[test]
    fn detects_github_pat_fine_grained() {
        assert_eq!(
            is_sensitive("github_pat_11ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"),
            Some("github-token")
        );
    }

    #[test]
    fn detects_slack_bot_token() {
        // Deliberately not shaped like Slack's real numeric-group token
        // format (xoxb-<10-13 digits>-<10-13 digits>-<24 alnum>) -- this is
        // a fixture for our own loose prefix check, not a real credential,
        // but GitHub's push-protection secret scanner still flags anything
        // that looks close enough to the real shape.
        assert_eq!(
            is_sensitive("SLACK_TOKEN=xoxb-fixturenotarealtoken-testdataonly-nnnn"),
            Some("slack-token")
        );
    }

    #[test]
    fn detects_openai_style_key() {
        assert_eq!(
            is_sensitive("OPENAI_API_KEY=sk-abcdefghijklmnopqrstuvwxyz123456"),
            Some("api-secret-key")
        );
    }

    #[test]
    fn ignores_prose_ending_in_sk_dash() {
        assert_eq!(is_sensitive("this is a desk-based risk-averse task-oriented plan"), None);
    }

    #[test]
    fn detects_bearer_token() {
        assert_eq!(
            is_sensitive("Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload"),
            Some("bearer-token")
        );
    }

    #[test]
    fn detects_aws_secret_line() {
        assert_eq!(
            is_sensitive("aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"),
            Some("aws-secret-line")
        );
    }

    #[test]
    fn detects_dotenv_bulk_secrets() {
        let s = "DB_PASSWORD=hunter2\nSTRIPE_SECRET=sk_live_xxx\nAPI_TOKEN=abc123\nDEBUG=true";
        assert_eq!(is_sensitive(s), Some("dotenv-bulk-secrets"));
    }

    #[test]
    fn ignores_single_dotenv_secret_line() {
        // Only one SECRET/TOKEN/PASSWORD/APIKEY line — below the bulk threshold,
        // and none of the other shapes match either.
        assert_eq!(is_sensitive("DB_PASSWORD=hunter2\nDEBUG=true\nPORT=8080"), None);
    }

    #[test]
    fn ignores_clean_env_dump() {
        let s = "PATH=/usr/bin:/bin\nHOME=/Users/dev\nSHELL=/bin/zsh\nLANG=C.UTF-8\nTERM=xterm-256color";
        assert_eq!(is_sensitive(s), None);
    }

    #[test]
    fn ignores_ordinary_prose() {
        let s = "This function returns a Result and logs an error if the token is invalid.";
        assert_eq!(is_sensitive(s), None);
    }

    #[test]
    fn scan_cap_ignores_secrets_past_64kb() {
        let padding = "x".repeat(SCAN_CAP_BYTES + 10);
        let s = format!("{padding}\nAKIAABCDEFGHIJ1234K");
        assert_eq!(is_sensitive(&s), None, "secret past the scan cap must not be found");
    }
}
