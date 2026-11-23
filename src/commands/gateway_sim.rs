//! `squeez gateway-sim` — what an API-level gateway would add over hooks.
//!
//! squeez reaches tool output through one adapter per host, and a host change
//! can silence it. The alternative is a gateway between the host and the
//! model API that sees every tool result whatever the host. Before building
//! one (credentials, provider terms and the prompt cache are real risks),
//! this command measures what it would buy, read-only: it replays Claude Code
//! transcripts, takes every tool result as the model saw it, and reports per
//! tool how much the hooks already compressed and how much more squeez's own
//! pipeline would remove at the gateway (text through `filter::compress`,
//! images through the screenshot-diet size rule). Nothing is written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::json_util::{self, JsonValue};

/// Text results shorter than this are left alone, as `wrap` leaves them.
const MIN_LINES: usize = 20;

#[derive(Default, Debug, Clone, PartialEq)]
pub struct ToolRow {
    pub results: u64,
    pub tokens_seen: u64,
    /// Results that already carry a squeez marker (hooks compressed them).
    pub hooked: u64,
    /// Further tokens a gateway running squeez's pipeline would remove.
    pub gateway_savings: u64,
}

fn text_tokens(s: &str) -> u64 {
    crate::tokens::estimate(s) as u64
}

/// Image tokens as seen, and what the diet would save.
fn image_cost(block: &JsonValue, max_edge: u32) -> (u64, u64) {
    let data = block.get("source").and_then(|s| s.get("data")).and_then(JsonValue::as_str).unwrap_or("");
    let dims = crate::commands::image_diet::b64_decode(data)
        .and_then(|b| crate::commands::image_diet::dimensions(&b));
    let Some((w, h)) = dims else { return (0, 0) };
    let seen = crate::commands::image_diet::image_tokens(w, h);
    let long = w.max(h);
    if max_edge == 0 || long <= max_edge {
        return (seen, 0);
    }
    let scale = max_edge as f64 / long as f64;
    let after = crate::commands::image_diet::image_tokens((w as f64 * scale) as u32, (h as f64 * scale) as u32);
    (seen, seen.saturating_sub(after))
}

fn result_text(content: &JsonValue) -> String {
    match content {
        JsonValue::Str(s) => s.clone(),
        JsonValue::Arr(items) => items
            .iter()
            .filter(|b| b.get_str("type") == "text")
            .filter_map(|b| b.get("text").and_then(JsonValue::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Fold one transcript's tool results into `rows`.
pub fn analyse(transcript: &str, cfg: &Config, rows: &mut BTreeMap<String, ToolRow>) {
    // tool_use id -> (tool name, command for Bash)
    let mut uses: std::collections::HashMap<String, (String, String)> = Default::default();
    for line in transcript.lines() {
        let Some(rec) = json_util::parse_value(line) else { continue };
        let Some(JsonValue::Arr(blocks)) = rec.get("message").and_then(|m| m.get("content")) else {
            continue;
        };
        for b in blocks {
            match b.get_str("type") {
                "tool_use" => {
                    let cmd = b.get("input").map(|i| i.get_str("command").to_string()).unwrap_or_default();
                    uses.insert(b.get_str("id").to_string(), (b.get_str("name").to_string(), cmd));
                }
                "tool_result" => {
                    let (tool, cmd) = uses
                        .get(b.get_str("tool_use_id"))
                        .cloned()
                        .unwrap_or_else(|| ("unknown".into(), String::new()));
                    let key = if tool.starts_with("mcp__") { "mcp".to_string() } else { tool.clone() };
                    let row = rows.entry(key).or_default();
                    row.results += 1;
                    let content = b.get("content").cloned().unwrap_or(JsonValue::Null);
                    if let JsonValue::Arr(items) = &content {
                        for img in items.iter().filter(|x| x.get_str("type") == "image") {
                            let (seen, save) = image_cost(img, cfg.image_max_edge);
                            row.tokens_seen += seen;
                            row.gateway_savings += save;
                        }
                    }
                    let text = result_text(&content);
                    row.tokens_seen += text_tokens(&text);
                    if text.contains("[squeez:") || text.starts_with("# squeez") {
                        row.hooked += 1;
                        continue;
                    }
                    let lines: Vec<String> = text.lines().map(String::from).collect();
                    if lines.len() < MIN_LINES || tool == "Read" || tool.starts_with("mcp__") {
                        // squeez never summarises source files or MCP payloads.
                        continue;
                    }
                    let hint = if cmd.is_empty() { tool.as_str() } else { cmd.as_str() };
                    let out = crate::filter::compress(hint, lines, cfg).join("\n");
                    row.gateway_savings += text_tokens(&text).saturating_sub(text_tokens(&out));
                }
                _ => {}
            }
        }
    }
}

pub fn render(rows: &BTreeMap<String, ToolRow>, files: usize) -> String {
    let mut out = format!(
        "squeez gateway-sim — {files} transcript(s), read-only\n\
         tool              results  tokens seen  hooked  gateway would save\n"
    );
    let mut total = ToolRow::default();
    let mut sorted: Vec<(&String, &ToolRow)> = rows.iter().collect();
    sorted.sort_by(|a, b| b.1.tokens_seen.cmp(&a.1.tokens_seen));
    for (tool, r) in sorted {
        out.push_str(&format!(
            "{:<16} {:>8} {:>12} {:>6}% {:>19}\n",
            tool.chars().take(16).collect::<String>(),
            r.results,
            r.tokens_seen,
            r.hooked * 100 / r.results.max(1),
            r.gateway_savings
        ));
        total.results += r.results;
        total.tokens_seen += r.tokens_seen;
        total.hooked += r.hooked;
        total.gateway_savings += r.gateway_savings;
    }
    out.push_str(&format!(
        "{:<16} {:>8} {:>12} {:>6}% {:>19}\n\n\
         A gateway would remove ~{}% more of the tool-result tokens the model saw \
         (chars/4 for text, w×h/750 for images). Hooked = results already carrying a squeez marker.\n",
        "TOTAL",
        total.results,
        total.tokens_seen,
        total.hooked * 100 / total.results.max(1),
        total.gateway_savings,
        total.gateway_savings * 100 / total.tokens_seen.max(1)
    ));
    out
}

/// Transcripts of this project modified within `days`.
fn default_transcripts(days: u64) -> Vec<PathBuf> {
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    let slug: String = cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let dir = Path::new(&crate::session::home_dir()).join(".claude/projects").join(slug);
    let max_age = std::time::Duration::from_secs(days * 86_400);
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("jsonl"))
                .filter(|p| {
                    std::fs::metadata(p)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age <= max_age)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `squeez gateway-sim [--days N] [transcript.jsonl ...]`
pub fn run(args: &[String]) -> i32 {
    let mut days = 7;
    let mut files: Vec<PathBuf> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--days" => days = it.next().and_then(|v| v.parse().ok()).unwrap_or(days),
            "-h" | "--help" => {
                println!("squeez gateway-sim [--days N] [transcript.jsonl ...]");
                return 0;
            }
            p => files.push(PathBuf::from(p)),
        }
    }
    if files.is_empty() {
        files = default_transcripts(days);
    }
    if files.is_empty() {
        eprintln!("squeez gateway-sim: no Claude Code transcripts for this directory in the last {days} day(s)");
        return 1;
    }
    let cfg = Config::load();
    let mut rows = BTreeMap::new();
    for f in &files {
        if let Ok(t) = std::fs::read_to_string(f) {
            analyse(&t, &cfg, &mut rows);
        }
    }
    print!("{}", render(&rows, files.len()));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(blocks: &str) -> String {
        format!(r#"{{"type":"x","message":{{"role":"user","content":[{blocks}]}}}}"#)
    }

    #[test]
    fn counts_hooked_results_and_gateway_savings() {
        let noisy: String = (0..60).map(|i| format!("Compiling crate_{i} v0.1.{i}\\n")).collect();
        let t = [
            rec(r#"{"type":"tool_use","id":"a","name":"Bash","input":{"command":"cargo build"}}"#),
            rec(&format!(r#"{{"type":"tool_result","tool_use_id":"a","content":"{noisy}error: boom"}}"#)),
            rec(r#"{"type":"tool_use","id":"b","name":"Bash","input":{"command":"ls"}}"#),
            rec(r##"{"type":"tool_result","tool_use_id":"b","content":"# squeez [ls] 10→5 tokens\na"}"##),
            rec(r#"{"type":"tool_use","id":"c","name":"mcp__x__shot","input":{}}"#),
            rec(r#"{"type":"tool_result","tool_use_id":"c","content":[{"type":"text","text":"ok"}]}"#),
        ]
        .join("\n");
        let mut rows = BTreeMap::new();
        analyse(&t, &Config::default(), &mut rows);
        let bash = &rows["Bash"];
        assert_eq!((bash.results, bash.hooked), (2, 1));
        assert!(bash.gateway_savings > 0, "noisy cargo output compresses: {bash:?}");
        assert_eq!(rows["mcp"].results, 1);
        let out = render(&rows, 1);
        assert!(out.contains("TOTAL") && out.contains("read-only"), "{out}");
    }

    #[test]
    fn image_cost_uses_pixels_and_the_diet_rule() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&2048u32.to_be_bytes());
        png.extend_from_slice(&1024u32.to_be_bytes());
        let data = crate::commands::image_diet::b64_encode(&png);
        let block = json_util::parse_value(&format!(r#"{{"type":"image","source":{{"data":"{data}"}}}}"#)).unwrap();
        let (seen, save) = image_cost(&block, 1024);
        assert_eq!(seen, 2048 * 1024 / 750);
        assert_eq!(save, seen - 1024 * 512 / 750);
        assert_eq!(image_cost(&block, 0).1, 0);
    }
}
