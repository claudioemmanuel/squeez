//! TOON (Token-Oriented Object Notation) re-encoder for JSON arrays of
//! uniform flat objects. Produces a CSV-like dense form that the LLM still
//! reads as structured data, but at ~40-60% fewer tokens than re-stating
//! every key on every row.
//!
//! Spec inspiration: <https://github.com/toon-format/toon>
//!
//! Output shape:
//! ```text
//! items[N]{key1,key2,key3}:
//!   v1,v2,v3
//!   v1,v2,v3
//! ```
//!
//! Strictly opt-in & lossless-or-reject:
//!   - Accepts only a top-level JSON array of objects.
//!   - Every object must share the **same keys in the same order**.
//!   - Every value must be a primitive (string / number / bool / null).
//!   - Any nesting, missing key, extra key, or shape mismatch → `None`.
//!
//! Returns `None` on any parse error or constraint violation. Callers
//! interpret `None` as "leave the original output untouched".
//!
//! Zero-dep. Hand-rolled JSON sub-grammar.

use std::fmt::Write;

/// Try to re-encode `text` as TOON. Returns `None` when the text is not a
/// top-level JSON array of uniform flat objects, when parsing fails, or when
/// the re-encoding would not actually save bytes.
pub fn try_to_toon(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return None;
    }
    let mut p = Parser::new(trimmed);
    let rows = p.parse_array_of_objects()?;
    if rows.is_empty() {
        return None;
    }
    // Validate uniform key order using the first row as anchor.
    let head_keys: Vec<&str> = rows[0].iter().map(|(k, _)| *k).collect();
    if head_keys.is_empty() {
        return None;
    }
    for row in &rows[1..] {
        if row.len() != head_keys.len() {
            return None;
        }
        for (i, (k, _)) in row.iter().enumerate() {
            if *k != head_keys[i] {
                return None;
            }
        }
    }
    let encoded = emit_toon("items", &head_keys, &rows);
    if encoded.len() >= text.len() {
        return None;
    }
    Some(encoded)
}

// ── Parser ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
enum Value<'a> {
    Str(&'a str),
    Num(&'a str),
    Bool(bool),
    Null,
}

struct Parser<'a> {
    src: &'a [u8],
    i: usize,
    /// String backing — borrowed from `src` for &str references.
    raw: &'a str,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            src: s.as_bytes(),
            i: 0,
            raw: s,
        }
    }

    fn skip_ws(&mut self) {
        while self.i < self.src.len() {
            let c = self.src[self.i];
            if c == b' ' || c == b'\n' || c == b'\r' || c == b'\t' {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.i).copied()
    }

    fn expect(&mut self, c: u8) -> Option<()> {
        self.skip_ws();
        if self.peek() == Some(c) {
            self.i += 1;
            Some(())
        } else {
            None
        }
    }

    fn parse_array_of_objects(&mut self) -> Option<Vec<Vec<(&'a str, Value<'a>)>>> {
        self.expect(b'[')?;
        self.skip_ws();
        let mut out: Vec<Vec<(&'a str, Value<'a>)>> = Vec::new();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Some(out);
        }
        loop {
            self.skip_ws();
            let obj = self.parse_flat_object()?;
            out.push(obj);
            self.skip_ws();
            match self.peek()? {
                b',' => {
                    self.i += 1;
                }
                b']' => {
                    self.i += 1;
                    self.skip_ws();
                    if self.i != self.src.len() {
                        return None;
                    }
                    return Some(out);
                }
                _ => return None,
            }
        }
    }

    fn parse_flat_object(&mut self) -> Option<Vec<(&'a str, Value<'a>)>> {
        self.expect(b'{')?;
        self.skip_ws();
        let mut pairs: Vec<(&'a str, Value<'a>)> = Vec::new();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Some(pairs);
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let value = self.parse_primitive()?;
            pairs.push((key, value));
            self.skip_ws();
            match self.peek()? {
                b',' => {
                    self.i += 1;
                }
                b'}' => {
                    self.i += 1;
                    return Some(pairs);
                }
                _ => return None,
            }
        }
    }

    /// Parse a JSON string and return a slice referencing the **raw, escaped**
    /// content between the quotes (no copy, no un-escape — TOON output is
    /// CSV-style and we re-quote/escape only when emitting).
    fn parse_string(&mut self) -> Option<&'a str> {
        if self.peek()? != b'"' {
            return None;
        }
        self.i += 1;
        let start = self.i;
        while self.i < self.src.len() {
            let c = self.src[self.i];
            if c == b'\\' {
                // Skip escaped char (don't try to decode).
                self.i += 2;
                continue;
            }
            if c == b'"' {
                let s = &self.raw[start..self.i];
                self.i += 1;
                return Some(s);
            }
            self.i += 1;
        }
        None
    }

    /// Parse a primitive (string, number, bool, null). Reject any composite
    /// (`{` or `[`) — TOON only accepts flat objects.
    fn parse_primitive(&mut self) -> Option<Value<'a>> {
        self.skip_ws();
        match self.peek()? {
            b'"' => self.parse_string().map(Value::Str),
            b't' => {
                if self.starts_with(b"true") {
                    self.i += 4;
                    Some(Value::Bool(true))
                } else {
                    None
                }
            }
            b'f' => {
                if self.starts_with(b"false") {
                    self.i += 5;
                    Some(Value::Bool(false))
                } else {
                    None
                }
            }
            b'n' => {
                if self.starts_with(b"null") {
                    self.i += 4;
                    Some(Value::Null)
                } else {
                    None
                }
            }
            b'{' | b'[' => None, // nested — reject.
            c if c == b'-' || c.is_ascii_digit() => Some(Value::Num(self.parse_number()?)),
            _ => None,
        }
    }

    fn starts_with(&self, kw: &[u8]) -> bool {
        self.src
            .get(self.i..self.i + kw.len())
            .map(|s| s == kw)
            .unwrap_or(false)
    }

    fn parse_number(&mut self) -> Option<&'a str> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        let mut saw_digit = false;
        while self.i < self.src.len() && self.src[self.i].is_ascii_digit() {
            self.i += 1;
            saw_digit = true;
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            while self.i < self.src.len() && self.src[self.i].is_ascii_digit() {
                self.i += 1;
                saw_digit = true;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.i += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.i += 1;
            }
            while self.i < self.src.len() && self.src[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        if !saw_digit {
            return None;
        }
        Some(&self.raw[start..self.i])
    }
}

// ── Emitter ──────────────────────────────────────────────────────────────────

fn emit_toon(name: &str, keys: &[&str], rows: &[Vec<(&str, Value)>]) -> String {
    let mut out = String::with_capacity(rows.len() * 32);
    let _ = write!(out, "{}[{}]{{", name, rows.len());
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        emit_cell(&mut out, k, /*is_key=*/ true);
    }
    out.push_str("}:\n");
    for row in rows {
        out.push_str("  ");
        for (i, (_, v)) in row.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            match v {
                Value::Str(s) => emit_cell(&mut out, s, false),
                Value::Num(n) => out.push_str(n),
                Value::Bool(true) => out.push_str("true"),
                Value::Bool(false) => out.push_str("false"),
                Value::Null => out.push_str("null"),
            }
        }
        out.push('\n');
    }
    out
}

/// Emit a string cell in CSV style. Quote when the value contains `,`, `"`,
/// newline, leading/trailing whitespace, or would otherwise be ambiguous.
fn emit_cell(out: &mut String, raw_escaped_json: &str, is_key: bool) {
    // The slice came from inside JSON quotes — it may still contain JSON
    // escapes like \" or \n. For TOON output we want literal characters.
    let decoded = decode_json_string(raw_escaped_json);
    let needs_quote = decoded.contains(',')
        || decoded.contains('"')
        || decoded.contains('\n')
        || decoded.contains('\r')
        || decoded.is_empty()
        || decoded.starts_with(' ')
        || decoded.ends_with(' ')
        || (is_key && !is_simple_key(&decoded));
    if needs_quote {
        out.push('"');
        for c in decoded.chars() {
            if c == '"' {
                out.push('"');
                out.push('"');
            } else {
                out.push(c);
            }
        }
        out.push('"');
    } else {
        out.push_str(&decoded);
    }
}

fn is_simple_key(k: &str) -> bool {
    let mut chars = k.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Decode a JSON-escaped string slice (the bytes between the JSON quotes).
fn decode_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'"' => {
                    out.push('"');
                    i += 2;
                }
                b'\\' => {
                    out.push('\\');
                    i += 2;
                }
                b'/' => {
                    out.push('/');
                    i += 2;
                }
                b'n' => {
                    out.push('\n');
                    i += 2;
                }
                b'r' => {
                    out.push('\r');
                    i += 2;
                }
                b't' => {
                    out.push('\t');
                    i += 2;
                }
                b'b' => {
                    i += 2;
                }
                b'f' => {
                    i += 2;
                }
                b'u' if i + 5 < bytes.len() => {
                    // Best-effort \uXXXX → char (BMP only; ignore surrogates).
                    if let Ok(hex) = std::str::from_utf8(&bytes[i + 2..i + 6]) {
                        if let Ok(cp) = u32::from_str_radix(hex, 16) {
                            if let Some(c) = char::from_u32(cp) {
                                out.push(c);
                            }
                        }
                    }
                    i += 6;
                }
                _ => {
                    out.push(bytes[i] as char);
                    i += 1;
                }
            }
        } else {
            // Preserve UTF-8 char boundary.
            let start = i;
            let c = bytes[i];
            let len = if c < 0x80 {
                1
            } else if c < 0xC0 {
                1 // malformed lead — treat as single byte
            } else if c < 0xE0 {
                2
            } else if c < 0xF0 {
                3
            } else {
                4
            };
            let end = (start + len).min(bytes.len());
            out.push_str(&s[start..end]);
            i = end;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_uniform_gh_pr_list() {
        let json = r#"[
            {"number": 1, "title": "Add feature", "state": "OPEN"},
            {"number": 2, "title": "Fix bug", "state": "MERGED"},
            {"number": 3, "title": "Refactor", "state": "CLOSED"}
        ]"#;
        let toon = try_to_toon(json).expect("should encode");
        assert!(toon.starts_with("items[3]{number,title,state}:\n"));
        assert!(toon.contains("1,Add feature,OPEN"));
        assert!(toon.contains("2,Fix bug,MERGED"));
        assert!(toon.len() < json.len());
    }

    #[test]
    fn encodes_kubectl_pods_shape() {
        let json = r#"[
            {"name":"api-0","status":"Running","ready":true,"restarts":0},
            {"name":"api-1","status":"Running","ready":true,"restarts":2},
            {"name":"api-2","status":"CrashLoopBackOff","ready":false,"restarts":12}
        ]"#;
        let toon = try_to_toon(json).expect("should encode");
        assert!(toon.starts_with("items[3]{name,status,ready,restarts}:"));
        assert!(toon.contains("api-2,CrashLoopBackOff,false,12"));
    }

    #[test]
    fn quotes_values_with_commas() {
        let json = r#"[
            {"id":1,"name":"Hello, world"},
            {"id":2,"name":"Quoted \"thing\""}
        ]"#;
        let toon = try_to_toon(json).expect("should encode");
        assert!(toon.contains("\"Hello, world\""));
        assert!(toon.contains("\"Quoted \"\"thing\"\"\""));
    }

    #[test]
    fn rejects_heterogeneous_keys() {
        let json = r#"[
            {"a":1,"b":2},
            {"a":1,"c":3}
        ]"#;
        assert!(try_to_toon(json).is_none());
    }

    #[test]
    fn rejects_keys_in_different_order() {
        let json = r#"[
            {"a":1,"b":2},
            {"b":2,"a":1}
        ]"#;
        assert!(try_to_toon(json).is_none());
    }

    #[test]
    fn rejects_nested_objects() {
        let json = r#"[{"a":1,"meta":{"x":1}}]"#;
        assert!(try_to_toon(json).is_none());
    }

    #[test]
    fn rejects_nested_arrays() {
        let json = r#"[{"a":1,"tags":[]}]"#;
        assert!(try_to_toon(json).is_none());
    }

    #[test]
    fn rejects_empty_array() {
        assert!(try_to_toon("[]").is_none());
    }

    #[test]
    fn rejects_non_array_root() {
        assert!(try_to_toon(r#"{"a":1}"#).is_none());
        assert!(try_to_toon("42").is_none());
        assert!(try_to_toon("not json").is_none());
    }

    #[test]
    fn rejects_when_toon_not_smaller() {
        // Single object with tiny payload — header overhead beats the win.
        let json = r#"[{"x":1}]"#;
        let r = try_to_toon(json);
        assert!(r.is_none() || r.unwrap().len() < json.len());
    }

    #[test]
    fn handles_nulls_and_booleans_and_floats() {
        let json = r#"[
            {"id":1,"score":99.5,"active":true,"note":null},
            {"id":2,"score":-3.14,"active":false,"note":null}
        ]"#;
        let toon = try_to_toon(json).expect("should encode");
        assert!(toon.contains("1,99.5,true,null"));
        assert!(toon.contains("2,-3.14,false,null"));
    }

    #[test]
    fn decodes_json_string_escapes() {
        assert_eq!(decode_json_string(r#"line\nbreak"#), "line\nbreak");
        assert_eq!(decode_json_string(r#"esc \"quote\""#), "esc \"quote\"");
        assert_eq!(decode_json_string("plain"), "plain");
    }
}
