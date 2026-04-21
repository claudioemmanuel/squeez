/// Extract a string value from a flat JSON object: {"key":"value",...}
/// Tolerates whitespace between `:` and `"` (GitHub API pretty-prints).
pub fn extract_str(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\":", key);
    let mut pos = json.find(&pat)? + pat.len();
    let bytes = json.as_bytes();
    while pos < bytes.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t' || bytes[pos] == b'\n' || bytes[pos] == b'\r') {
        pos += 1;
    }
    if pos >= bytes.len() || bytes[pos] != b'"' {
        return None;
    }
    pos += 1;
    let end = json[pos..].find('"')?;
    Some(json[pos..pos + end].to_string())
}

/// Extract a u64 value from a flat JSON object: {"key":123,...}
pub fn extract_u64(json: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{}\":", key);
    let start = json.find(&pat)? + pat.len();
    let s = json[start..].trim_start();
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 {
        return None;
    }
    s[..end].parse().ok()
}

/// Extract a bool value from a flat JSON object: {"key":true,...}
pub fn extract_bool(json: &str, key: &str) -> Option<bool> {
    let pat = format!("\"{}\":", key);
    let start = json.find(&pat)? + pat.len();
    let s = json[start..].trim_start();
    if s.starts_with("true") {
        Some(true)
    } else if s.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// Split a JSON array interior into items respecting quoted strings.
fn split_json_array_items(inner: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut in_string = false;
    let mut escape_next = false;
    let mut start = 0;
    for (i, ch) in inner.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        match ch {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            ',' if !in_string => {
                items.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < inner.len() {
        items.push(&inner[start..]);
    }
    items
}

/// Extract a string array from a flat JSON object: {"key":["a","b"],...}
pub fn extract_str_array(json: &str, key: &str) -> Vec<String> {
    let pat = format!("\"{}\":[", key);
    let start = match json.find(&pat) {
        Some(i) => i + pat.len(),
        None => return Vec::new(),
    };
    let end = match json[start..].find(']') {
        Some(i) => start + i,
        None => return Vec::new(),
    };
    let arr = &json[start..end];
    if arr.trim().is_empty() {
        return Vec::new();
    }
    split_json_array_items(arr)
        .iter()
        .filter_map(|s| {
            let s = s.trim().trim_matches('"');
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
        .collect()
}

/// Escape a string for inclusion in a JSON string value (not quoted).
pub fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "")
}

/// Serialize a string slice as a JSON array of strings.
pub fn str_array(items: &[String]) -> String {
    let inner: Vec<String> = items
        .iter()
        .map(|s| format!("\"{}\"", escape_str(s)))
        .collect();
    format!("[{}]", inner.join(","))
}

/// Extract a u64 array from a flat JSON object: {"key":[1,2,3],...}
/// Non-digit values are skipped.
pub fn extract_u64_array(json: &str, key: &str) -> Vec<u64> {
    let pat = format!("\"{}\":[", key);
    let start = match json.find(&pat) {
        Some(i) => i + pat.len(),
        None => return Vec::new(),
    };
    let end = match json[start..].find(']') {
        Some(i) => start + i,
        None => return Vec::new(),
    };
    let arr = &json[start..end];
    if arr.trim().is_empty() {
        return Vec::new();
    }
    arr.split(',')
        .filter_map(|s| s.trim().parse::<u64>().ok())
        .collect()
}

/// Serialize a u64 slice as a JSON array of numbers.
pub fn u64_array(items: &[u64]) -> String {
    let inner: Vec<String> = items.iter().map(|v| v.to_string()).collect();
    format!("[{}]", inner.join(","))
}

/// Serialize a usize slice as a JSON array of numbers.
pub fn usize_array(items: &[usize]) -> String {
    let inner: Vec<String> = items.iter().map(|v| v.to_string()).collect();
    format!("[{}]", inner.join(","))
}

// ── Single-pass JSON field extractor ─────────────────────────────────────

use std::collections::HashMap;

/// Single-pass extraction of all top-level key->raw_value pairs from a flat JSON object.
/// Returns a map of key -> raw value string (not including quotes for strings,
/// not including brackets for arrays). Values are slices into the input.
/// Handles: string values, number values, array values (no nesting beyond one level).
pub fn extract_all(json: &str) -> HashMap<&str, &str> {
    let mut map = HashMap::new();
    let bytes = json.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    // Skip to opening brace
    while i < len && bytes[i] != b'{' {
        i += 1;
    }
    if i >= len {
        return map;
    }
    i += 1;

    loop {
        // Skip whitespace and commas
        while i < len
            && (bytes[i] == b' '
                || bytes[i] == b','
                || bytes[i] == b'\n'
                || bytes[i] == b'\r'
                || bytes[i] == b'\t')
        {
            i += 1;
        }
        if i >= len || bytes[i] == b'}' {
            break;
        }

        // Expect a quoted key
        if bytes[i] != b'"' {
            break;
        }
        i += 1;
        let key_start = i;
        while i < len && bytes[i] != b'"' {
            i += 1;
        }
        if i >= len {
            break;
        }
        let key = &json[key_start..i];
        i += 1; // closing quote

        // Expect colon
        while i < len && bytes[i] != b':' {
            i += 1;
        }
        if i >= len {
            break;
        }
        i += 1;

        // Skip whitespace
        while i < len && bytes[i] == b' ' {
            i += 1;
        }

        if i >= len {
            break;
        }

        // Determine value type and extract raw slice
        match bytes[i] {
            b'"' => {
                // String value: find closing unescaped quote
                i += 1;
                let val_start = i;
                while i < len {
                    if bytes[i] == b'\\' {
                        i += 2;
                        if i >= len { break; }
                        continue;
                    }
                    if bytes[i] == b'"' {
                        break;
                    }
                    i += 1;
                }
                let val_end = i.min(len);
                if i < len {
                    i += 1; // closing quote
                }
                map.insert(key, &json[val_start..val_end]);
            }
            b'[' => {
                // Array value: find matching close bracket (one level nesting)
                let val_start = i;
                i += 1;
                let mut depth = 1i32;
                while i < len && depth > 0 {
                    match bytes[i] {
                        b'[' => depth += 1,
                        b']' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        b'"' => {
                            i += 1;
                            while i < len {
                                if bytes[i] == b'\\' {
                                    i += 2;
                                    if i >= len { break; }
                                    continue;
                                }
                                if bytes[i] == b'"' {
                                    break;
                                }
                                i += 1;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                let val_end = i.min(len);
                map.insert(key, &json[val_start..val_end]);
            }
            _ => {
                // Number, bool, null: read until comma, }, or whitespace
                let val_start = i;
                while i < len
                    && bytes[i] != b','
                    && bytes[i] != b'}'
                    && bytes[i] != b' '
                    && bytes[i] != b'\n'
                {
                    i += 1;
                }
                let val_end = i;
                map.insert(key, &json[val_start..val_end]);
            }
        }
    }
    map
}

pub fn map_str(map: &HashMap<&str, &str>, key: &str) -> Option<String> {
    map.get(key).map(|v| v.to_string())
}

pub fn map_u64(map: &HashMap<&str, &str>, key: &str) -> Option<u64> {
    map.get(key)?.trim().parse().ok()
}

pub fn map_bool(map: &HashMap<&str, &str>, key: &str) -> Option<bool> {
    match map.get(key)?.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

pub fn map_str_array(map: &HashMap<&str, &str>, key: &str) -> Vec<String> {
    let raw = match map.get(key) {
        Some(r) => r,
        None => return Vec::new(),
    };
    let inner = raw.trim().trim_start_matches('[').trim_end_matches(']');
    if inner.trim().is_empty() {
        return Vec::new();
    }
    split_json_array_items(inner)
        .iter()
        .filter_map(|s| {
            let s = s.trim().trim_matches('"');
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
        .collect()
}

pub fn map_u64_array(map: &HashMap<&str, &str>, key: &str) -> Vec<u64> {
    let raw = match map.get(key) {
        Some(r) => r,
        None => return Vec::new(),
    };
    let inner = raw.trim().trim_start_matches('[').trim_end_matches(']');
    if inner.trim().is_empty() {
        return Vec::new();
    }
    inner.split(',').filter_map(|s| s.trim().parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_all_basic() {
        let json = r#"{"name":"squeez","version":42,"active":true}"#;
        let map = extract_all(json);
        assert_eq!(map_str(&map, "name"), Some("squeez".to_string()));
        assert_eq!(map_u64(&map, "version"), Some(42));
        assert_eq!(map_bool(&map, "active"), Some(true));
    }

    #[test]
    fn test_extract_all_arrays() {
        let json = r#"{"nums":[1,2,3],"strs":["a","b","c"],"empty":[]}"#;
        let map = extract_all(json);
        assert_eq!(map_u64_array(&map, "nums"), vec![1, 2, 3]);
        assert_eq!(
            map_str_array(&map, "strs"),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert!(map_u64_array(&map, "empty").is_empty());
        assert!(map_str_array(&map, "empty").is_empty());
    }

    #[test]
    fn test_extract_all_trailing_backslash_no_panic() {
        let json = r#"{"key":"value\"}"#;
        let map = extract_all(json);
        // Should not panic; partial parse is acceptable
        let _ = map_str(&map, "key");
    }

    #[test]
    fn test_extract_all_array_trailing_backslash() {
        let json = r#"{"arr":["a\\"]}"#;
        let map = extract_all(json);
        let arr = map_str_array(&map, "arr");
        assert!(!arr.is_empty());
    }

    #[test]
    fn test_split_json_array_comma_in_string() {
        let items = split_json_array_items(r#""echo \"a,b,c\"","other""#);
        assert_eq!(items.len(), 2);
        assert!(items[0].contains("a,b,c"));
    }

    #[test]
    fn test_extract_str_array_comma_in_value() {
        let json = r#"{"cmds":["echo \"a,b\"","ls"]}"#;
        let arr = extract_str_array(json, "cmds");
        assert_eq!(arr.len(), 2);
        assert!(arr[0].contains("a,b"));
        assert_eq!(arr[1], "ls");
    }

    #[test]
    fn test_extract_all_roundtrip_context() {
        // Build a context JSON with the old per-field extractors and verify
        // that extract_all produces identical results.
        let json = r#"{"session_file":"test.jsonl","call_counter":5,"tokens_bash":100,"tokens_read":200,"tokens_grep":50,"tokens_other":10,"reread_count":3,"exact_dedup_hits":1,"fuzzy_dedup_hits":2,"summarize_triggers":0,"intensity_ultra_calls":0,"agent_spawns":0,"agent_estimated_tokens":0,"seen_errors":[111,222],"seen_git_refs":["abc1234"],"call_log_n":[1,2],"call_log_cmd":["ls","git status"],"call_log_hash":[999,888],"call_log_len":[10,20],"call_log_short":["deadbeef","cafebabe"]}"#;

        let map = extract_all(json);

        // Verify against individual extractors
        assert_eq!(
            map_str(&map, "session_file").unwrap(),
            extract_str(json, "session_file").unwrap()
        );
        assert_eq!(
            map_u64(&map, "call_counter").unwrap(),
            extract_u64(json, "call_counter").unwrap()
        );
        assert_eq!(
            map_u64(&map, "tokens_bash").unwrap(),
            extract_u64(json, "tokens_bash").unwrap()
        );
        assert_eq!(
            map_u64_array(&map, "seen_errors"),
            extract_u64_array(json, "seen_errors")
        );
        assert_eq!(
            map_str_array(&map, "seen_git_refs"),
            extract_str_array(json, "seen_git_refs")
        );
        assert_eq!(
            map_u64_array(&map, "call_log_n"),
            extract_u64_array(json, "call_log_n")
        );
        assert_eq!(
            map_str_array(&map, "call_log_cmd"),
            extract_str_array(json, "call_log_cmd")
        );
    }
}
