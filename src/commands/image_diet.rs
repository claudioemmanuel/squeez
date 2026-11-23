//! Screenshot diet: downscale oversized images in MCP tool results.
//!
//! An image costs about `width × height / 750` input tokens, whatever its
//! byte size or JPEG quality, so the lever is resolution. Screenshots from
//! browser tools on a large display arrive at full size; most of the time the
//! model needs the layout and the text, not every pixel. Images whose long
//! edge exceeds `image_max_edge` are resized with the platform's own tool —
//! `sips` on macOS, ImageMagick (`magick`/`convert`) elsewhere — so squeez
//! keeps zero crate dependencies. No tool, an unknown format, or any failure:
//! the original passes through untouched (fail open).
//!
//! Only MCP results are rewritten: Claude Code passes MCP output through
//! without schema validation, so the same content-block array with a smaller
//! image is accepted. Built-in tools (Read of an image) have a fixed output
//! schema and are left alone. The full-size original is saved under
//! `<squeez dir>/images/`, and a text block names the path so the model can
//! Read it when it needs the detail.

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::json_util::{self, JsonValue};

/// Approximate image tokens (Anthropic vision pricing rule of thumb).
pub fn image_tokens(w: u32, h: u32) -> u64 {
    (w as u64 * h as u64) / 750
}

// ── base64 ──────────────────────────────────────────────────────────────────

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' => continue,
            _ => return None,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xff) as u8);
        }
    }
    Some(out)
}

// ── dimensions ──────────────────────────────────────────────────────────────

/// `(width, height)` of a PNG or JPEG, read from its header.
pub fn dimensions(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() >= 24 && b.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes(b[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(b[20..24].try_into().ok()?);
        return Some((w, h));
    }
    if b.len() >= 4 && b[0] == 0xFF && b[1] == 0xD8 {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = b[i + 1];
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            // SOF0..SOF15 except DHT (C4), JPG (C8), DAC (CC).
            if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

// ── resize ──────────────────────────────────────────────────────────────────

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| d.join(tool).is_file() || d.join(format!("{tool}.exe")).is_file())
    })
}

/// Resize `input` so its long edge is `edge`, writing `output`. Returns false
/// when no resizer is available or it failed.
fn resize(input: &Path, output: &Path, edge: u32) -> bool {
    let e = edge.to_string();
    let run = |cmd: &mut std::process::Command| {
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
            && output.is_file()
    };
    if cfg!(target_os = "macos") && Path::new("/usr/bin/sips").is_file() {
        // JPEG quality 80: fewer pixels should not mean more bytes.
        return run(crate::spawn::helper("/usr/bin/sips")
            .args(["-Z", &e, "-s", "formatOptions", "80"])
            .arg(input)
            .arg("--out")
            .arg(output));
    }
    let geometry = format!("{e}x{e}>");
    for tool in ["magick", "convert"] {
        if on_path(tool) {
            return run(crate::spawn::helper(tool).arg(input).args(["-resize", &geometry]).arg(output));
        }
    }
    false
}

/// What happened to one image.
#[derive(Debug, PartialEq)]
pub struct Shrunk {
    pub from: (u32, u32),
    pub to: (u32, u32),
    pub original: PathBuf,
}

/// Downscale one base64 image if its long edge exceeds `max_edge`.
/// Returns the new base64 data and what changed, or `None` to keep it.
pub fn shrink(data: &str, media_type: &str, max_edge: u32, store: &Path) -> Option<(String, Shrunk)> {
    let bytes = b64_decode(data)?;
    let (w, h) = dimensions(&bytes)?;
    if max_edge == 0 || w.max(h) <= max_edge {
        return None;
    }
    let ext = match media_type {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        _ => return None,
    };
    std::fs::create_dir_all(store).ok()?;
    let fp = crate::context::hash::fnv1a_64(data.as_bytes());
    let original = store.join(format!("{fp:016x}.{ext}"));
    if !original.is_file() {
        std::fs::write(&original, &bytes).ok()?;
    }
    let tmp = store.join(format!("{fp:016x}.{}.small.{ext}", std::process::id()));
    let ok = resize(&original, &tmp, max_edge);
    let small = ok.then(|| std::fs::read(&tmp).ok()).flatten();
    let _ = std::fs::remove_file(&tmp);
    let small = small?;
    let to = dimensions(&small)?;
    if to.0.max(to.1) >= w.max(h) {
        return None;
    }
    Some((b64_encode(&small), Shrunk { from: (w, h), to, original }))
}

/// Full PostToolUse rewrite for an MCP result, or `None` to leave it alone.
/// Returns the `hookSpecificOutput` JSON.
pub fn rewrite(raw: &str, tool: &str, cfg: &Config, store: &Path) -> Option<String> {
    if !tool.starts_with("mcp__") || cfg.image_max_edge == 0 || !raw.contains("\"type\":\"image\"") {
        return None;
    }
    let d = json_util::parse_value(raw)?;
    let resp = d.get("tool_response").or_else(|| d.get("tool_result"))?;
    let JsonValue::Arr(blocks) = resp else {
        return None;
    };
    let mut out = Vec::with_capacity(blocks.len() + 1);
    let mut notes = Vec::new();
    for b in blocks {
        let src = b.get("source").filter(|_| b.get_str("type") == "image");
        let shrunk = src.and_then(|s| {
            let data = s.get("data")?.as_str()?;
            shrink(data, s.get_str("media_type"), cfg.image_max_edge, store)
        });
        match (src, shrunk) {
            (Some(s), Some((data, info))) => {
                let mut s2 = s.clone();
                s2.set("data", JsonValue::Str(data));
                let mut b2 = b.clone();
                b2.set("source", s2);
                out.push(b2);
                notes.push(format!(
                    "[squeez: image downscaled {}×{} → {}×{} (~{} → ~{} tokens); full size: {}]",
                    info.from.0,
                    info.from.1,
                    info.to.0,
                    info.to.1,
                    image_tokens(info.from.0, info.from.1),
                    image_tokens(info.to.0, info.to.1),
                    info.original.display()
                ));
            }
            _ => out.push(b.clone()),
        }
    }
    if notes.is_empty() {
        return None;
    }
    for n in notes {
        out.push(JsonValue::Obj(vec![
            ("type".into(), JsonValue::Str("text".into())),
            ("text".into(), JsonValue::Str(n)),
        ]));
    }
    let wrapped = JsonValue::Obj(vec![(
        "hookSpecificOutput".into(),
        JsonValue::Obj(vec![
            ("hookEventName".into(), JsonValue::Str("PostToolUse".into())),
            ("updatedToolOutput".into(), JsonValue::Arr(out)),
        ]),
    )]);
    Some(json_util::to_compact(&wrapped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for s in ["", "a", "ab", "abc", "abcd", "hello world!!"] {
            let e = b64_encode(s.as_bytes());
            assert_eq!(b64_decode(&e).unwrap(), s.as_bytes(), "{s} -> {e}");
        }
        assert_eq!(b64_encode(b"Man"), "TWFu");
        assert_eq!(b64_encode(b"Ma"), "TWE=");
        assert!(b64_decode("not*b64").is_none());
    }

    #[test]
    fn reads_png_and_jpeg_dimensions() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(&1080u32.to_be_bytes());
        assert_eq!(dimensions(&png), Some((1920, 1080)));
        // SOI, APP0 (len 4), SOF0 with h=440 w=660.
        let jpg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0, 0, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0xB8, 0x02,
            0x94, 0, 0, 0,
        ];
        assert_eq!(dimensions(&jpg), Some((660, 440)));
        assert_eq!(dimensions(b"GIF89a"), None);
        assert_eq!(image_tokens(660, 440), 387);
    }

    #[test]
    fn small_images_and_non_mcp_tools_pass_through() {
        let cfg = Config::default();
        let raw = r#"{"tool_name":"Read","tool_response":[{"type":"image","source":{"data":"x"}}]}"#;
        assert_eq!(rewrite(raw, "Read", &cfg, Path::new("/nonexistent")), None);
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&800u32.to_be_bytes());
        png.extend_from_slice(&600u32.to_be_bytes());
        assert_eq!(shrink(&b64_encode(&png), "image/png", 1024, Path::new("/nonexistent")), None);
    }
}
