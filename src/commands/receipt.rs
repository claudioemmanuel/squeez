//! `squeez receipt` — what squeez saved, in money and context windows.
//!
//! Users saw a percentage per call and never the total. The receipt sums the
//! per-session `tokens_saved` that `finalize` already writes to
//! `memory/summaries.jsonl` (plus the live session from `sessions/current.json`
//! when it has no summary yet) and prices it. Every figure is labelled for
//! what it is: token counts are squeez's chars/4 estimate, and the dollar
//! figure is a floor — it prices each avoided token once, at the model's base
//! input rate, ignoring the cache re-reads of the same context on later turns.

use crate::json_util;

/// Input price in USD per million tokens, Anthropic first-party rates
/// (claude-api skill table, cached 2026-11-03). Longest prefix wins.
const PRICES: &[(&str, f64)] = &[
    ("claude-fable-5-2", 8.0),
    ("claude-mythos-5-2", 8.0),
    ("claude-fable-5-1", 10.0),
    ("claude-mythos-5-1", 10.0),
    ("claude-fable-5", 10.0),
    ("claude-opus-5-5", 4.0),
    ("claude-opus-5", 5.0),
    ("claude-opus-4", 5.0),
    ("claude-sonnet-5-5", 2.0),
    ("claude-sonnet-5", 2.0),
    ("claude-sonnet-4-6", 3.0),
    ("claude-haiku-5-5", 0.10),
    ("claude-haiku-4-5", 1.0),
];

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
const DEFAULT_WINDOW: u64 = 200_000;

/// USD per million input tokens for `model`, or `None` when unknown.
pub fn input_price(model: &str) -> Option<f64> {
    let m = model.trim().to_ascii_lowercase();
    let m = m.split('[').next().unwrap_or(&m); // "claude-sonnet-5[1m]"
    PRICES
        .iter()
        .filter(|(p, _)| m.starts_with(p))
        .max_by_key(|(p, _)| p.len())
        .map(|(_, usd)| *usd)
}

#[derive(Debug, PartialEq)]
pub struct Receipt {
    pub days: u64,
    pub sessions: u64,
    pub minutes: u64,
    pub tokens_saved: u64,
    pub model: String,
    pub price: f64,
    pub window: u64,
}

impl Receipt {
    pub fn usd(&self) -> f64 {
        self.tokens_saved as f64 * self.price / 1_000_000.0
    }
    pub fn windows(&self) -> f64 {
        self.tokens_saved as f64 / self.window.max(1) as f64
    }
}

/// Aggregate `summaries` (JSONL) and the live `current` session over the last
/// `days` days ending at `now`.
pub fn build(
    summaries: &str,
    current: Option<&str>,
    now: i64,
    days: u64,
    model: &str,
    window: u64,
) -> Option<Receipt> {
    let price = input_price(model)?;
    let since = now - (days as i64) * 86_400;
    let mut r = Receipt {
        days,
        sessions: 0,
        minutes: 0,
        tokens_saved: 0,
        model: model.to_string(),
        price,
        window: if window == 0 { DEFAULT_WINDOW } else { window },
    };
    let mut seen_ids: Vec<String> = Vec::new();
    for line in summaries.lines().filter(|l| !l.trim().is_empty()) {
        let ts = json_util::extract_u64(line, "ts").unwrap_or(0) as i64;
        if ts < since {
            continue;
        }
        r.sessions += 1;
        r.tokens_saved += json_util::extract_u64(line, "tokens_saved").unwrap_or(0);
        r.minutes += json_util::extract_u64(line, "duration_min").unwrap_or(0);
        if let Some(id) = json_util::extract_str(line, "host_session_id").filter(|s| !s.is_empty()) {
            seen_ids.push(id);
        }
    }
    if let Some(cur) = current {
        let start = json_util::extract_u64(cur, "start_ts").unwrap_or(0) as i64;
        let id = json_util::extract_str(cur, "host_session_id").unwrap_or_default();
        let summarised = !id.is_empty() && seen_ids.contains(&id);
        if start >= since && !summarised {
            r.sessions += 1;
            r.tokens_saved += json_util::extract_u64(cur, "tokens_saved").unwrap_or(0);
            r.minutes += ((now - start).max(0) / 60) as u64;
        }
    }
    Some(r)
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn render_text(r: &Receipt) -> String {
    format!(
        "squeez receipt — last {days} day(s)\n\
         \x20 sessions          {sessions} ({minutes} min)\n\
         \x20 tokens avoided    {tokens}  (squeez chars/4 estimate)\n\
         \x20 cost avoided      ≥ ${usd:.2}  at {model} input ${price:.2}/MTok (floor: cache re-reads not counted)\n\
         \x20 context avoided   {win:.1} × {window}K-token windows\n",
        days = r.days,
        sessions = r.sessions,
        minutes = r.minutes,
        tokens = thousands(r.tokens_saved),
        usd = r.usd(),
        model = r.model,
        price = r.price,
        win = r.windows(),
        window = r.window / 1000,
    )
}

pub fn render_md(r: &Receipt) -> String {
    format!(
        "### squeez receipt — last {} day(s)\n\n\
         | | |\n|---|---|\n\
         | Sessions | {} ({} min) |\n\
         | Tokens avoided | {} (chars/4 estimate) |\n\
         | Cost avoided | ≥ ${:.2} at `{}` input ${:.2}/MTok |\n\
         | Context avoided | {:.1} × {}K-token windows |\n\n\
         <sub>Measured by [squeez](https://github.com/claudioemmanuel/squeez). \
         Dollar figure is a floor: each avoided token priced once at the base input rate.</sub>\n",
        r.days,
        r.sessions,
        r.minutes,
        thousands(r.tokens_saved),
        r.usd(),
        r.model,
        r.price,
        r.windows(),
        r.window / 1000
    )
}

/// `squeez receipt [--days N] [--model ID] [--format text|md]`
pub fn run(args: &[String]) -> i32 {
    let mut days = 7u64;
    let ctx = crate::context::cache::SessionContext::load_current_or_latest(&crate::session::sessions_dir());
    // The model the session actually ran on, when the transcript showed one.
    let detected = Some(ctx.last_model.clone()).filter(|m| input_price(m).is_some());
    let mut model = detected.unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let mut md = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--days" => days = it.next().and_then(|v| v.parse().ok()).unwrap_or(days),
            "--model" => model = it.next().cloned().unwrap_or(model),
            "--format" => md = it.next().map(String::as_str) == Some("md"),
            "-h" | "--help" => {
                println!("squeez receipt [--days N] [--model ID] [--format text|md]");
                return 0;
            }
            _ => {}
        }
    }
    let summaries =
        std::fs::read_to_string(crate::session::memory_dir().join("summaries.jsonl")).unwrap_or_default();
    let sessions = crate::session::sessions_dir();
    let current = std::fs::read_to_string(sessions.join("current.json")).ok();
    let window = ctx.real_ctx_window;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let Some(r) = build(&summaries, current.as_deref(), now, days, &model, window) else {
        let known: Vec<&str> = PRICES.iter().map(|(p, _)| *p).collect();
        eprintln!("squeez receipt: no price for '{model}'. Known: {}", known.join(", "));
        return 1;
    };
    print!("{}", if md { render_md(&r) } else { render_text(&r) });
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    #[test]
    fn prices_match_longest_prefix() {
        assert_eq!(input_price("claude-opus-5-5"), Some(4.0));
        assert_eq!(input_price("claude-opus-5"), Some(5.0));
        assert_eq!(input_price("claude-opus-4-8"), Some(5.0));
        assert_eq!(input_price("claude-sonnet-5[1m]"), Some(2.0));
        assert_eq!(input_price("claude-fable-5-2"), Some(8.0));
        assert_eq!(input_price("gpt-5"), None);
    }

    #[test]
    fn sums_window_and_skips_old_and_summarised_current() {
        let now = 100 * DAY;
        let s = format!(
            "{{\"ts\":{},\"tokens_saved\":500000,\"duration_min\":30,\"host_session_id\":\"a\"}}\n\
             {{\"ts\":{},\"tokens_saved\":999999,\"duration_min\":5}}\n\
             {{\"ts\":{},\"tokens_saved\":250000,\"duration_min\":10,\"host_session_id\":\"b\"}}\n",
            now - DAY,
            now - 30 * DAY, // outside 7 days
            now - 2 * DAY
        );
        // Live session already summarised as "b": not counted twice.
        let cur = format!("{{\"tokens_saved\":7,\"start_ts\":{},\"host_session_id\":\"b\"}}", now - 600);
        let r = build(&s, Some(&cur), now, 7, "claude-opus-5-5", 0).unwrap();
        assert_eq!(r.sessions, 2);
        assert_eq!(r.tokens_saved, 750_000);
        assert_eq!(r.minutes, 40);
        assert!((r.usd() - 3.0).abs() < 1e-9, "750K × $4/MTok = $3");
        assert!((r.windows() - 3.75).abs() < 1e-9, "default 200K window");

        // An unsummarised live session is added.
        let live = format!("{{\"tokens_saved\":1000,\"start_ts\":{},\"host_session_id\":\"c\"}}", now - 600);
        let r = build(&s, Some(&live), now, 7, "claude-opus-5-5", 1_000_000).unwrap();
        assert_eq!((r.sessions, r.tokens_saved, r.minutes), (3, 751_000, 50));
        assert!((r.windows() - 0.751).abs() < 1e-9);
    }

    #[test]
    fn unknown_model_and_rendering() {
        assert!(build("", None, 0, 7, "nope", 0).is_none());
        let r = build("", None, 0, 7, "claude-sonnet-5-5", 0).unwrap();
        let t = render_text(&r);
        assert!(t.contains("≥ $0.00") && t.contains("chars/4 estimate"), "{t}");
        assert!(render_md(&r).contains("| Cost avoided |"));
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
