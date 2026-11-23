//! `squeez export` — content-free savings metrics for a team's own backend.
//!
//! Everything squeez measures stays on each developer's machine, so whoever
//! pays for the team's tokens sees nothing. `squeez export` writes the
//! aggregate numbers — sessions, tokens avoided, and per-program calls,
//! tokens in/out and retrieves — as JSON or OTLP/JSON metrics, to stdout, a
//! file, or an OTLP/HTTP collector (`--endpoint http://…/v1/metrics`).
//! No command text, file path, output or prompt is ever read for it: the
//! sources are `summaries.jsonl` totals and `handler_stats.json`, whose only
//! string is the program name (`--no-handlers` drops those too). Plain HTTP
//! only — point it at a local collector that forwards over TLS.

use std::io::{Read, Write};

use crate::economy::handler_stats::HandlerStats;
use crate::json_util::JsonValue;

pub struct Snapshot {
    pub version: String,
    pub days: u64,
    pub now: i64,
    pub sessions: u64,
    pub tokens_avoided: u64,
    /// (program, calls, in_tokens, out_tokens, retrieves)
    pub handlers: Vec<(String, u64, u64, u64, u64)>,
}

pub fn snapshot(summaries: &str, stats: &HandlerStats, now: i64, days: u64, handlers: bool) -> Snapshot {
    let since = now - days as i64 * 86_400;
    let (mut sessions, mut tokens) = (0, 0);
    for l in summaries.lines().filter(|l| !l.trim().is_empty()) {
        if crate::json_util::extract_u64(l, "ts").unwrap_or(0) as i64 >= since {
            sessions += 1;
            tokens += crate::json_util::extract_u64(l, "tokens_saved").unwrap_or(0);
        }
    }
    let rows = if handlers {
        stats
            .rows()
            .into_iter()
            // A program run by absolute path would carry the user's home
            // directory: export its base name only.
            .map(|r| {
                let base = r.name.rsplit(['/', '\\']).next().unwrap_or(&r.name).to_string();
                (base, r.calls, r.in_tokens, r.out_tokens, r.retrieves)
            })
            .collect()
    } else {
        Vec::new()
    };
    Snapshot {
        version: env!("CARGO_PKG_VERSION").to_string(),
        days,
        now,
        sessions,
        tokens_avoided: tokens,
        handlers: rows,
    }
}

fn s(v: &str) -> JsonValue {
    JsonValue::Str(v.to_string())
}
fn n(v: u64) -> JsonValue {
    JsonValue::Num(v as f64)
}
fn obj(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

pub fn to_json(snap: &Snapshot) -> String {
    let handlers = snap
        .handlers
        .iter()
        .map(|(name, c, i, o, r)| {
            obj(vec![("program", s(name)), ("calls", n(*c)), ("in_tokens", n(*i)), ("out_tokens", n(*o)), ("retrieves", n(*r))])
        })
        .collect();
    crate::json_util::to_compact(&obj(vec![
        ("squeez_version", s(&snap.version)),
        ("window_days", n(snap.days)),
        ("generated_at", n(snap.now as u64)),
        ("sessions", n(snap.sessions)),
        ("tokens_avoided", n(snap.tokens_avoided)),
        ("handlers", JsonValue::Arr(handlers)),
    ]))
}

fn attr(key: &str, value: &str) -> JsonValue {
    obj(vec![("key", s(key)), ("value", obj(vec![("stringValue", s(value))]))])
}

fn gauge(name: &str, unit: &str, points: Vec<(u64, Vec<JsonValue>)>, t: &str) -> JsonValue {
    let dps = points
        .into_iter()
        .map(|(v, attrs)| obj(vec![("asInt", s(&v.to_string())), ("timeUnixNano", s(t)), ("attributes", JsonValue::Arr(attrs))]))
        .collect();
    obj(vec![("name", s(name)), ("unit", s(unit)), ("gauge", obj(vec![("dataPoints", JsonValue::Arr(dps))]))])
}

/// OTLP/JSON `ExportMetricsServiceRequest` (gauges).
pub fn to_otlp(snap: &Snapshot) -> String {
    let t = format!("{}000000000", snap.now);
    let per = |f: fn(&(String, u64, u64, u64, u64)) -> u64| -> Vec<(u64, Vec<JsonValue>)> {
        snap.handlers.iter().map(|h| (f(h), vec![attr("program", &h.0)])).collect()
    };
    let mut metrics = vec![
        gauge("squeez.sessions", "{session}", vec![(snap.sessions, vec![])], &t),
        gauge("squeez.tokens_avoided", "{token}", vec![(snap.tokens_avoided, vec![])], &t),
    ];
    if !snap.handlers.is_empty() {
        metrics.push(gauge("squeez.handler.calls", "{call}", per(|h| h.1), &t));
        metrics.push(gauge("squeez.handler.tokens_in", "{token}", per(|h| h.2), &t));
        metrics.push(gauge("squeez.handler.tokens_out", "{token}", per(|h| h.3), &t));
        metrics.push(gauge("squeez.handler.retrieves", "{retrieve}", per(|h| h.4), &t));
    }
    let resource = obj(vec![(
        "attributes",
        JsonValue::Arr(vec![attr("service.name", "squeez"), attr("service.version", &snap.version)]),
    )]);
    let scope = obj(vec![
        ("scope", obj(vec![("name", s("squeez"))])),
        ("metrics", JsonValue::Arr(metrics)),
    ]);
    crate::json_util::to_compact(&obj(vec![(
        "resourceMetrics",
        JsonValue::Arr(vec![obj(vec![("resource", resource), ("scopeMetrics", JsonValue::Arr(vec![scope]))])]),
    )]))
}

/// POST `body` to a plain-HTTP `url`; returns the status code.
pub fn post(url: &str, body: &str) -> Result<u16, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or("only http:// endpoints are supported — run a local OTLP collector for TLS")?;
    let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let addr = if host.contains(':') { host.to_string() } else { format!("{host}:80") };
    let mut sock = std::net::TcpStream::connect(&addr).map_err(|e| format!("connect {addr}: {e}"))?;
    let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    sock.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut resp = String::new();
    let _ = sock.read_to_string(&mut resp);
    resp.split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| "no HTTP status in response".to_string())
}

/// `squeez export [--format json|otlp] [--days N] [--out FILE] [--endpoint URL] [--no-handlers]`
pub fn run(args: &[String]) -> i32 {
    let (mut otlp, mut days, mut out, mut endpoint, mut handlers) = (false, 30u64, None, None, true);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--format" => otlp = it.next().map(String::as_str) == Some("otlp"),
            "--days" => days = it.next().and_then(|v| v.parse().ok()).unwrap_or(days),
            "--out" => out = it.next().cloned(),
            "--endpoint" => endpoint = it.next().cloned(),
            "--no-handlers" => handlers = false,
            _ => {
                eprintln!("usage: squeez export [--format json|otlp] [--days N] [--out FILE] [--endpoint http://host:port/v1/metrics] [--no-handlers]");
                return 2;
            }
        }
    }
    let summaries =
        std::fs::read_to_string(crate::session::memory_dir().join("summaries.jsonl")).unwrap_or_default();
    let stats = HandlerStats::load(&crate::session::sessions_dir());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let snap = snapshot(&summaries, &stats, now, days, handlers);
    // An endpoint always speaks OTLP.
    let body = if otlp || endpoint.is_some() { to_otlp(&snap) } else { to_json(&snap) };
    if let Some(url) = endpoint {
        return match post(&url, &body) {
            Ok(code) if (200..300).contains(&code) => {
                println!("squeez export: sent to {url} (HTTP {code})");
                0
            }
            Ok(code) => {
                eprintln!("squeez export: {url} answered HTTP {code}");
                1
            }
            Err(e) => {
                eprintln!("squeez export: {e}");
                1
            }
        };
    }
    match out {
        Some(path) => match std::fs::write(&path, &body) {
            Ok(()) => {
                println!("squeez export: wrote {path}");
                0
            }
            Err(e) => {
                eprintln!("squeez export: {path}: {e}");
                1
            }
        },
        None => {
            println!("{body}");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Snapshot {
        let now = 100 * 86_400;
        let sums = format!(
            "{{\"ts\":{},\"tokens_saved\":1000,\"completed\":[\"git push secret-branch\"],\"cwd\":\"/home/alice/acme\"}}\n\
             {{\"ts\":{},\"tokens_saved\":5}}\n",
            now - 86_400,
            now - 90 * 86_400
        );
        let mut st = HandlerStats::default();
        st.record("cargo", 900, 100);
        st.record("/home/alice/bin/deploy.sh", 10, 10);
        snapshot(&sums, &st, now, 30, true)
    }

    #[test]
    fn json_and_otlp_carry_numbers_never_content() {
        let snap = fixture();
        assert_eq!((snap.sessions, snap.tokens_avoided), (1, 1000));
        for body in [to_json(&snap), to_otlp(&snap)] {
            assert!(crate::json_util::parse_value(&body).is_some(), "valid JSON: {body}");
            assert!(!body.contains("secret-branch") && !body.contains("alice"), "content leaked: {body}");
            assert!(body.contains("cargo"));
        }
        let otlp = to_otlp(&snap);
        for m in ["squeez.tokens_avoided", "squeez.handler.retrieves", "\"asInt\":\"1000\"", "service.name"] {
            assert!(otlp.contains(m), "{m} missing: {otlp}");
        }
        assert!(to_json(&snap).contains("\"program\":\"deploy.sh\""));
        let bare = snapshot("", &HandlerStats::default(), 0, 30, false);
        assert!(!to_otlp(&bare).contains("squeez.handler"));
    }

    #[test]
    fn posts_to_a_plain_http_collector() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let n = c.read(&mut buf).unwrap();
            let _ = c.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        let code = post(&format!("http://127.0.0.1:{port}/v1/metrics"), &to_otlp(&fixture())).unwrap();
        assert_eq!(code, 200);
        let req = server.join().unwrap();
        assert!(req.starts_with("POST /v1/metrics HTTP/1.1") && req.contains("application/json"), "{req}");
        assert!(post("https://x/v1/metrics", "{}").unwrap_err().contains("only http://"));
    }
}
