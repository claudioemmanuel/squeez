//! End-to-end screenshot diet through the system resizer. Skipped when the
//! machine has neither `sips` (macOS) nor ImageMagick, as on minimal CI.
use squeez::commands::image_diet::{b64_encode, dimensions, rewrite};
use squeez::config::Config;
use std::process::Command;

fn make_png(path: &std::path::Path) -> bool {
    Command::new("magick")
        .args(["-size", "1920x1080", "gradient:blue-red"])
        .arg(path)
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn oversized_mcp_screenshot_is_downscaled_and_original_kept() {
    let dir = std::env::temp_dir().join(format!("squeez-imgdiet-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("big.png");
    if !make_png(&png) {
        eprintln!("skipped: ImageMagick not available to build the fixture");
        return;
    }
    let data = b64_encode(&std::fs::read(&png).unwrap());
    let raw = format!(
        r#"{{"tool_name":"mcp__x__screenshot","tool_response":[{{"type":"text","text":"ok"}},{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{data}"}}}}]}}"#
    );
    let store = dir.join("images");
    let out = rewrite(&raw, "mcp__x__screenshot", &Config::default(), &store).expect("rewritten");
    assert!(out.contains("1920×1080 → 1024×576"), "{out}");
    let v = squeez::json_util::parse_value(&out).unwrap();
    let blocks = v.get("hookSpecificOutput").unwrap().get("updatedToolOutput").unwrap();
    let squeez::json_util::JsonValue::Arr(blocks) = blocks else { panic!() };
    assert_eq!(blocks.len(), 3, "text, smaller image, note");
    let small = blocks[1].get("source").unwrap().get("data").unwrap().as_str().unwrap();
    let bytes = squeez::commands::image_diet::b64_decode(small).unwrap();
    assert_eq!(dimensions(&bytes), Some((1024, 576)));
    assert_eq!(std::fs::read_dir(&store).unwrap().count(), 1, "full-size original kept");

    // Disabled by config: untouched.
    let cfg = Config { image_max_edge: 0, ..Config::default() };
    assert!(rewrite(&raw, "mcp__x__screenshot", &cfg, &store).is_none());
}
