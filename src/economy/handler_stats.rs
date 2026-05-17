// Continuous handler calibration (item 2 from Hermes-inspired roadmap).
//
// Aggregates per-handler {calls, in_tokens, out_tokens} across sessions in
// `~/.claude/squeez/sessions/handler_stats.json` so `squeez_handler_stats`
// can surface under-performers (savings <10%) and over-performers (>90%).
//
// Pure JSON over parallel arrays — no extra runtime deps, matches the rest
// of squeez's persistence style.

use std::path::Path;

use crate::json_util;

const MAX_HANDLERS: usize = 64;

#[derive(Debug, Clone, Default)]
pub struct HandlerStats {
    pub names: Vec<String>,
    pub calls: Vec<u64>,
    pub in_tokens: Vec<u64>,
    pub out_tokens: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct HandlerRow {
    pub name: String,
    pub calls: u64,
    pub in_tokens: u64,
    pub out_tokens: u64,
    /// Compression ratio in basis points (0-10000 = 0-100%). 0 when in_tokens == 0.
    pub savings_bp: u32,
}

impl HandlerStats {
    pub fn load(sessions_dir: &Path) -> Self {
        let path = sessions_dir.join("handler_stats.json");
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size == 0 || size > crate::memory::MAX_FILE_BYTES {
            return Self::default();
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        let map = json_util::extract_all(&content);
        let names = json_util::map_str_array(&map, "names");
        let calls = json_util::map_u64_array(&map, "calls");
        let in_tk = json_util::map_u64_array(&map, "in_tokens");
        let out_tk = json_util::map_u64_array(&map, "out_tokens");
        let n = names.len().min(calls.len()).min(in_tk.len()).min(out_tk.len());
        Self {
            names: names.into_iter().take(n).collect(),
            calls: calls.into_iter().take(n).collect(),
            in_tokens: in_tk.into_iter().take(n).collect(),
            out_tokens: out_tk.into_iter().take(n).collect(),
        }
    }

    pub fn save(&self, sessions_dir: &Path) {
        let _ = std::fs::create_dir_all(sessions_dir);
        let path = sessions_dir.join("handler_stats.json");
        let tmp = path.with_extension("json.tmp");
        let json = format!(
            "{{\"names\":{},\"calls\":{},\"in_tokens\":{},\"out_tokens\":{}}}",
            json_util::str_array(&self.names),
            json_util::u64_array(&self.calls),
            json_util::u64_array(&self.in_tokens),
            json_util::u64_array(&self.out_tokens),
        );
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&tmp)
            {
                let _ = f.write_all(json.as_bytes());
            }
        }
        #[cfg(not(unix))]
        {
            let _ = std::fs::write(&tmp, &json);
        }
        let _ = std::fs::rename(&tmp, &path);
    }

    /// Record one bash invocation's accounting under the given handler name.
    /// Idempotent on the array shape: appends a new slot the first time `name`
    /// is seen, otherwise increments in place. Caps the table at MAX_HANDLERS.
    pub fn record(&mut self, name: &str, in_tk: u64, out_tk: u64) {
        if name.is_empty() {
            return;
        }
        if let Some(idx) = self.names.iter().position(|n| n == name) {
            self.calls[idx] = self.calls[idx].saturating_add(1);
            self.in_tokens[idx] = self.in_tokens[idx].saturating_add(in_tk);
            self.out_tokens[idx] = self.out_tokens[idx].saturating_add(out_tk);
            return;
        }
        if self.names.len() >= MAX_HANDLERS {
            return; // skip; rare in practice
        }
        self.names.push(name.to_string());
        self.calls.push(1);
        self.in_tokens.push(in_tk);
        self.out_tokens.push(out_tk);
    }

    /// Sorted view: most-called first. Caller may further filter by savings.
    pub fn rows(&self) -> Vec<HandlerRow> {
        let mut rows: Vec<HandlerRow> = (0..self.names.len())
            .map(|i| {
                let in_tk = self.in_tokens[i];
                let out_tk = self.out_tokens[i];
                let savings_bp = if in_tk > 0 {
                    let saved = in_tk.saturating_sub(out_tk);
                    ((saved.saturating_mul(10_000)) / in_tk).min(10_000) as u32
                } else {
                    0
                };
                HandlerRow {
                    name: self.names[i].clone(),
                    calls: self.calls[i],
                    in_tokens: in_tk,
                    out_tokens: out_tk,
                    savings_bp,
                }
            })
            .collect();
        rows.sort_by(|a, b| b.calls.cmp(&a.calls));
        rows
    }
}

/// Format the handler stats as a human-readable table. Used by both the MCP
/// tool and the SessionStart banner (so output stays consistent).
pub fn format_table(stats: &HandlerStats) -> String {
    let rows = stats.rows();
    if rows.is_empty() {
        return "(no handler stats recorded yet — run a few commands first)".to_string();
    }
    let mut out = String::from("squeez handler stats (cumulative, cross-session)\n");
    out.push_str("name             calls    in_tok    out_tok   saved\n");
    for r in &rows {
        out.push_str(&format!(
            "{:<16} {:>5}  {:>8}  {:>8}   {:>4}%\n",
            truncate(&r.name, 16),
            r.calls,
            r.in_tokens,
            r.out_tokens,
            r.savings_bp / 100,
        ));
    }
    // Highlight under/over-performers (calls ≥ 5 to avoid noise).
    let under: Vec<&HandlerRow> = rows.iter().filter(|r| r.calls >= 5 && r.savings_bp < 1_000).collect();
    let over: Vec<&HandlerRow> = rows.iter().filter(|r| r.calls >= 5 && r.savings_bp >= 9_000).collect();
    if !under.is_empty() {
        out.push_str("\nunder-performers (savings <10%, ≥5 calls): ");
        out.push_str(&under.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", "));
        out.push('\n');
    }
    if !over.is_empty() {
        out.push_str("over-performers (savings ≥90%, ≥5 calls): ");
        out.push_str(&over.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", "));
        out.push('\n');
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "squeez_hs_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ))
    }

    #[test]
    fn record_inserts_new_handler() {
        let mut s = HandlerStats::default();
        s.record("git", 1000, 100);
        assert_eq!(s.names, vec!["git"]);
        assert_eq!(s.calls, vec![1]);
        assert_eq!(s.in_tokens, vec![1000]);
        assert_eq!(s.out_tokens, vec![100]);
    }

    #[test]
    fn record_accumulates_existing_handler() {
        let mut s = HandlerStats::default();
        s.record("cargo", 500, 50);
        s.record("cargo", 700, 70);
        assert_eq!(s.calls, vec![2]);
        assert_eq!(s.in_tokens, vec![1200]);
        assert_eq!(s.out_tokens, vec![120]);
    }

    #[test]
    fn rows_sorted_by_calls_desc() {
        let mut s = HandlerStats::default();
        s.record("a", 1, 1);
        s.record("b", 1, 1);
        s.record("b", 1, 1);
        s.record("c", 1, 1);
        s.record("c", 1, 1);
        s.record("c", 1, 1);
        let rows = s.rows();
        assert_eq!(rows[0].name, "c");
        assert_eq!(rows[1].name, "b");
        assert_eq!(rows[2].name, "a");
    }

    #[test]
    fn savings_bp_computed_correctly() {
        let mut s = HandlerStats::default();
        s.record("nine", 1000, 100); // 90% saved → 9000 bp
        s.record("zero", 1000, 1000); // 0% saved → 0 bp
        s.record("full", 100, 0); // 100% saved → 10_000 bp
        let rows = s.rows();
        let by_name = |n: &str| rows.iter().find(|r| r.name == n).unwrap().savings_bp;
        assert_eq!(by_name("nine"), 9_000);
        assert_eq!(by_name("zero"), 0);
        assert_eq!(by_name("full"), 10_000);
    }

    #[test]
    fn round_trip_persistence() {
        let dir = tmp();
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = HandlerStats::default();
        s.record("git", 1000, 200);
        s.record("git", 500, 100);
        s.record("cargo", 5000, 800);
        s.save(&dir);

        let loaded = HandlerStats::load(&dir);
        assert_eq!(loaded.names, s.names);
        assert_eq!(loaded.calls, s.calls);
        assert_eq!(loaded.in_tokens, s.in_tokens);
        assert_eq!(loaded.out_tokens, s.out_tokens);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_is_empty() {
        let dir = tmp();
        std::fs::create_dir_all(&dir).unwrap();
        let loaded = HandlerStats::load(&dir);
        assert!(loaded.names.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cap_at_max_handlers() {
        let mut s = HandlerStats::default();
        for i in 0..(MAX_HANDLERS + 10) {
            s.record(&format!("h{}", i), 100, 10);
        }
        assert_eq!(s.names.len(), MAX_HANDLERS);
    }

    #[test]
    fn format_table_calls_out_under_and_over_performers() {
        let mut s = HandlerStats::default();
        // 5 calls of `bad` at 0% savings.
        for _ in 0..5 {
            s.record("bad", 100, 100);
        }
        // 5 calls of `good` at 95% savings.
        for _ in 0..5 {
            s.record("good", 1000, 50);
        }
        // Below-threshold (4 calls) should not be flagged.
        for _ in 0..4 {
            s.record("rare", 1000, 50);
        }
        let table = format_table(&s);
        assert!(table.contains("under-performers"));
        assert!(table.contains("bad"));
        assert!(table.contains("over-performers"));
        assert!(table.contains("good"));
        // `rare` has 4 calls (<5) so it's not flagged.
        assert!(!table.contains("under-performers") || !table.split('\n').any(|l| l.starts_with("under-performers") && l.contains("rare")));
    }

    #[test]
    fn empty_stats_format_message() {
        let s = HandlerStats::default();
        assert!(format_table(&s).contains("no handler stats"));
    }
}
