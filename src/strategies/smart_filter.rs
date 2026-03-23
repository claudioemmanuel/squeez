const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub fn apply(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(strip_ansi)
        .map(|s| s.replace('\r', ""))
        .filter(|l| !l.trim().is_empty())
        .filter(|l| !is_spinner(l))
        .filter(|l| !is_progress_bar(l))
        .filter(|l| !is_git_hint(l))
        .filter(|l| !is_npm_noise(l))
        .filter(|l| !is_node_modules_frame(l))
        .map(strip_log_timestamp)
        .collect()
}

fn strip_ansi(s: String) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            while let Some(&nc) = chars.peek() {
                chars.next();
                if nc.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn is_spinner(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with(|c| SPINNER_CHARS.contains(&c))
}

fn is_progress_bar(s: &str) -> bool {
    let t = s.trim();
    (t.contains('█') || t.contains('░'))
        || (t.contains('[') && t.contains(']') && t.contains('=') && t.contains('>'))
        || (t.ends_with('%') && t.chars().any(|c| c.is_ascii_digit()))
}

fn is_git_hint(s: &str) -> bool {
    s.trim_start().starts_with("hint:")
}

fn is_npm_noise(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with("npm warn deprecated")
        || t.starts_with("WARN deprecated")
        || t.starts_with("npm notice")
        || t.starts_with("npm warn EBADENGINE")
}

fn is_node_modules_frame(s: &str) -> bool {
    s.contains("node_modules/") && (s.trim_start().starts_with("at ") || s.contains("(/."))
}

fn strip_log_timestamp(s: String) -> String {
    if s.starts_with('[') {
        if let Some(end) = s.find("] ") {
            let prefix = &s[1..end];
            if prefix.contains('-') && (prefix.contains('T') || prefix.contains(':')) {
                return s[end + 2..].to_string();
            }
        }
    }
    s
}
