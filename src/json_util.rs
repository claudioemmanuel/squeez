/// Extract a string value from a flat JSON object: {"key":"value",...}
pub fn extract_str(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\":\"", key);
    let start = json.find(&pat)? + pat.len();
    let end = json[start..].find('"')?;
    Some(json[start..start + end].to_string())
}

/// Extract a u64 value from a flat JSON object: {"key":123,...}
pub fn extract_u64(json: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{}\":", key);
    let start = json.find(&pat)? + pat.len();
    let s = json[start..].trim_start();
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    s[..end].parse().ok()
}

/// Extract a bool value from a flat JSON object: {"key":true,...}
pub fn extract_bool(json: &str, key: &str) -> Option<bool> {
    let pat = format!("\"{}\":", key);
    let start = json.find(&pat)? + pat.len();
    let s = json[start..].trim_start();
    if s.starts_with("true") { Some(true) }
    else if s.starts_with("false") { Some(false) }
    else { None }
}

/// Extract a string array from a flat JSON object: {"key":["a","b"],...}
/// Values must not contain commas or brackets.
pub fn extract_str_array(json: &str, key: &str) -> Vec<String> {
    let pat = format!("\"{}\":[", key);
    let start = match json.find(&pat) { Some(i) => i + pat.len(), None => return Vec::new() };
    let end = match json[start..].find(']') { Some(i) => start + i, None => return Vec::new() };
    let arr = &json[start..end];
    if arr.trim().is_empty() { return Vec::new(); }
    arr.split(',')
        .filter_map(|s| {
            let s = s.trim().trim_matches('"');
            if s.is_empty() { None } else { Some(s.to_string()) }
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
    let inner: Vec<String> = items.iter()
        .map(|s| format!("\"{}\"", escape_str(s)))
        .collect();
    format!("[{}]", inner.join(","))
}
