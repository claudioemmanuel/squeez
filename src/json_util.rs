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

// ── Recursive JSON value parser ──────────────────────────────────────────
//
// The extractors above only handle flat objects (one level of array
// nesting). Reporters for nested test-runner JSON (jest/vitest --json,
// eslint --format json) need arbitrary object/array nesting — this is a
// small recursive-descent parser over `Vec<char>` (not bytes, so multi-byte
// UTF-8 in test names/messages is handled correctly without slicing mid-codepoint).

#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JsonValue>),
    Obj(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            JsonValue::Num(n) if *n >= 0.0 => Some(*n as u64),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> &[JsonValue] {
        match self {
            JsonValue::Arr(items) => items,
            _ => &[],
        }
    }

    pub fn is_obj(&self) -> bool {
        matches!(self, JsonValue::Obj(_))
    }

    /// String value of `key`, or "" when absent or not a string. Mirrors the
    /// `str(x.get(key, ""))` idiom the settings patchers rely on.
    pub fn get_str(&self, key: &str) -> &str {
        self.get(key).and_then(|v| v.as_str()).unwrap_or("")
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut JsonValue> {
        match self {
            JsonValue::Obj(fields) => {
                fields.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
            }
            _ => None,
        }
    }

    pub fn as_arr_mut(&mut self) -> Option<&mut Vec<JsonValue>> {
        match self {
            JsonValue::Arr(items) => Some(items),
            _ => None,
        }
    }

    /// Insert or replace `key`. An existing key keeps its position, matching
    /// Python dict-assignment semantics — settings files stay diff-stable.
    pub fn set(&mut self, key: &str, val: JsonValue) {
        if let JsonValue::Obj(fields) = self {
            match fields.iter_mut().find(|(k, _)| k == key) {
                Some((_, slot)) => *slot = val,
                None => fields.push((key.to_string(), val)),
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<JsonValue> {
        match self {
            JsonValue::Obj(fields) => fields
                .iter()
                .position(|(k, _)| k == key)
                .map(|i| fields.remove(i).1),
            _ => None,
        }
    }

    /// Borrow `key` as an array, replacing any non-array value with `[]` first.
    /// The `ensure_list` primitive of the settings patchers.
    pub fn ensure_arr(&mut self, key: &str) -> &mut Vec<JsonValue> {
        if !matches!(self.get(key), Some(JsonValue::Arr(_))) {
            self.set(key, JsonValue::Arr(Vec::new()));
        }
        self.get_mut(key)
            .and_then(|v| v.as_arr_mut())
            .expect("just ensured an array")
    }

    /// Borrow `key` as an object, replacing any non-object value with `{}` first.
    pub fn ensure_obj(&mut self, key: &str) -> &mut JsonValue {
        if !matches!(self.get(key), Some(JsonValue::Obj(_))) {
            self.set(key, JsonValue::Obj(Vec::new()));
        }
        self.get_mut(key).expect("just ensured an object")
    }

    /// Key/value pairs of an object, in insertion order. Empty for any other
    /// kind — lets a caller walk a settings map without knowing its keys.
    pub fn obj_entries(&self) -> &[(String, JsonValue)] {
        match self {
            JsonValue::Obj(fields) => fields,
            _ => &[],
        }
    }

    pub fn obj_is_empty(&self) -> bool {
        matches!(self, JsonValue::Obj(f) if f.is_empty())
    }

    /// Convenience constructor for the `{"type":"command","command":…}` shape
    /// used by every host's hook and statusLine entries.
    pub fn command_entry(command: &str) -> JsonValue {
        JsonValue::Obj(vec![
            ("type".to_string(), JsonValue::Str("command".to_string())),
            ("command".to_string(), JsonValue::Str(command.to_string())),
        ])
    }
}

/// Strict JSON string escaping — unlike [`escape_str`], which is lossy (it
/// drops `\r`) and tuned for one-line log records, this preserves every input
/// byte so user settings files round-trip unchanged.
fn escape_strict(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
}

/// Numbers are parsed into `f64`, which loses the int/float distinction. Emit
/// integral values without a decimal point so `"maxLines": 120` survives a
/// round-trip as `120` rather than `120.0`.
fn write_num(n: f64, out: &mut String) {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
        out.push_str(&format!("{}", n as i64));
    } else if n.is_finite() {
        out.push_str(&format!("{}", n));
    } else {
        out.push_str("null");
    }
}

fn write_pretty(v: &JsonValue, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    let inner_pad = "  ".repeat(indent + 1);
    match v {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsonValue::Num(n) => write_num(*n, out),
        JsonValue::Str(s) => {
            out.push('"');
            escape_strict(s, out);
            out.push('"');
        }
        JsonValue::Arr(items) if items.is_empty() => out.push_str("[]"),
        JsonValue::Arr(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&inner_pad);
                write_pretty(item, indent + 1, out);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push(']');
        }
        JsonValue::Obj(fields) if fields.is_empty() => out.push_str("{}"),
        JsonValue::Obj(fields) => {
            out.push_str("{\n");
            for (i, (k, val)) in fields.iter().enumerate() {
                out.push_str(&inner_pad);
                out.push('"');
                escape_strict(k, out);
                out.push_str("\": ");
                write_pretty(val, indent + 1, out);
                if i + 1 < fields.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push('}');
        }
    }
}

/// Serialize with 2-space indentation, byte-compatible with the
/// `json.dump(obj, f, indent=2)` this replaced.
pub fn to_pretty(v: &JsonValue) -> String {
    let mut out = String::new();
    write_pretty(v, 0, &mut out);
    out
}

/// Parses a single JSON value from `input`. Returns `None` on malformed input
/// rather than panicking — callers should treat that as "not this format" and
/// fall back to a simpler condenser.
pub fn parse_value(input: &str) -> Option<JsonValue> {
    let chars: Vec<char> = input.chars().collect();
    let mut pos = 0usize;
    skip_ws(&chars, &mut pos);
    let v = parse_val(&chars, &mut pos)?;
    Some(v)
}

fn skip_ws(c: &[char], pos: &mut usize) {
    while *pos < c.len() && c[*pos].is_whitespace() {
        *pos += 1;
    }
}

fn matches_lit(c: &[char], pos: usize, lit: &str) -> bool {
    let lit: Vec<char> = lit.chars().collect();
    pos + lit.len() <= c.len() && c[pos..pos + lit.len()] == lit[..]
}

fn parse_val(c: &[char], pos: &mut usize) -> Option<JsonValue> {
    skip_ws(c, pos);
    match *c.get(*pos)? {
        '{' => parse_obj(c, pos),
        '[' => parse_arr(c, pos),
        '"' => parse_str(c, pos).map(JsonValue::Str),
        't' if matches_lit(c, *pos, "true") => {
            *pos += 4;
            Some(JsonValue::Bool(true))
        }
        'f' if matches_lit(c, *pos, "false") => {
            *pos += 5;
            Some(JsonValue::Bool(false))
        }
        'n' if matches_lit(c, *pos, "null") => {
            *pos += 4;
            Some(JsonValue::Null)
        }
        _ => parse_num(c, pos),
    }
}

fn parse_obj(c: &[char], pos: &mut usize) -> Option<JsonValue> {
    *pos += 1; // consume '{'
    let mut fields = Vec::new();
    skip_ws(c, pos);
    if *c.get(*pos)? == '}' {
        *pos += 1;
        return Some(JsonValue::Obj(fields));
    }
    loop {
        skip_ws(c, pos);
        if *c.get(*pos)? != '"' {
            return None;
        }
        let key = parse_str(c, pos)?;
        skip_ws(c, pos);
        if *c.get(*pos)? != ':' {
            return None;
        }
        *pos += 1;
        let val = parse_val(c, pos)?;
        fields.push((key, val));
        skip_ws(c, pos);
        match c.get(*pos)? {
            ',' => {
                *pos += 1;
            }
            '}' => {
                *pos += 1;
                break;
            }
            _ => return None,
        }
    }
    Some(JsonValue::Obj(fields))
}

fn parse_arr(c: &[char], pos: &mut usize) -> Option<JsonValue> {
    *pos += 1; // consume '['
    let mut items = Vec::new();
    skip_ws(c, pos);
    if *c.get(*pos)? == ']' {
        *pos += 1;
        return Some(JsonValue::Arr(items));
    }
    loop {
        items.push(parse_val(c, pos)?);
        skip_ws(c, pos);
        match c.get(*pos)? {
            ',' => {
                *pos += 1;
            }
            ']' => {
                *pos += 1;
                break;
            }
            _ => return None,
        }
    }
    Some(JsonValue::Arr(items))
}

fn parse_str(c: &[char], pos: &mut usize) -> Option<String> {
    *pos += 1; // consume opening '"'
    let mut s = String::new();
    loop {
        let ch = *c.get(*pos)?;
        *pos += 1;
        if ch == '"' {
            return Some(s);
        }
        if ch != '\\' {
            s.push(ch);
            continue;
        }
        let esc = *c.get(*pos)?;
        *pos += 1;
        match esc {
            '"' => s.push('"'),
            '\\' => s.push('\\'),
            '/' => s.push('/'),
            'n' => s.push('\n'),
            't' => s.push('\t'),
            'r' => s.push('\r'),
            'b' => s.push('\u{8}'),
            'f' => s.push('\u{c}'),
            'u' => {
                if *pos + 4 > c.len() {
                    return None;
                }
                let hex: String = c[*pos..*pos + 4].iter().collect();
                let cp = u32::from_str_radix(&hex, 16).ok()?;
                *pos += 4;
                s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
            }
            other => s.push(other),
        }
    }
}

fn parse_num(c: &[char], pos: &mut usize) -> Option<JsonValue> {
    let start = *pos;
    if c.get(*pos) == Some(&'-') {
        *pos += 1;
    }
    while matches!(c.get(*pos), Some(ch) if ch.is_ascii_digit() || matches!(ch, '.' | 'e' | 'E' | '+' | '-'))
    {
        *pos += 1;
    }
    if *pos == start {
        return None;
    }
    let s: String = c[start..*pos].iter().collect();
    s.parse::<f64>().ok().map(JsonValue::Num)
}

#[cfg(test)]
mod value_tests {
    use super::*;

    #[test]
    fn parses_nested_objects_and_arrays() {
        let json = r#"{"a":1,"b":[1,2,{"c":"x"}],"d":{"e":true,"f":null}}"#;
        let v = parse_value(json).unwrap();
        assert_eq!(v.get("a").unwrap().as_u64(), Some(1));
        assert_eq!(v.get("b").unwrap().as_arr().len(), 3);
        assert_eq!(v.get("b").unwrap().as_arr()[2].get("c").unwrap().as_str(), Some("x"));
        assert_eq!(v.get("d").unwrap().get("e").unwrap(), &JsonValue::Bool(true));
        assert_eq!(v.get("d").unwrap().get("f").unwrap(), &JsonValue::Null);
    }

    #[test]
    fn handles_escaped_strings_and_unicode() {
        let json = r#"{"msg":"line1\nline2 \"quoted\" é"}"#;
        let v = parse_value(json).unwrap();
        assert_eq!(v.get("msg").unwrap().as_str(), Some("line1\nline2 \"quoted\" é"));
    }

    #[test]
    fn malformed_input_returns_none_not_panic() {
        assert!(parse_value("{\"a\":").is_none());
        assert!(parse_value("not json at all").is_none());
        assert!(parse_value("").is_none());
    }

    #[test]
    fn missing_key_returns_none() {
        let v = parse_value(r#"{"a":1}"#).unwrap();
        assert!(v.get("missing").is_none());
        assert!(v.get("a").unwrap().get("nested").is_none());
    }
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
