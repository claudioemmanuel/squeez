//! Reversible compression store (P1 — headroom CCR-style).
//!
//! When squeez compresses a large output it stashes the verbatim original in a
//! content-addressed blob so the model can recover it on demand via the
//! `squeez_retrieve` MCP tool — instead of the dropped content being lost
//! forever. This is what lets compression be aggressive without information
//! loss: trim hard, but keep a safety net the model can pull back.
//!
//! Layout: `~/.claude/squeez/blobs/<id>` where `<id>` is the 16-hex FNV-1a
//! hash of the content (so identical outputs dedup to one blob). TTL pruning
//! keeps the directory bounded. Zero-dep — stdlib fs + the existing hasher.

use crate::context::hash::fnv1a_64;
use crate::session::squeez_dir;
use std::path::{Path, PathBuf};

/// Directory holding stashed originals.
pub fn blobs_dir() -> PathBuf {
    squeez_dir().join("blobs")
}

/// A retrieval id is exactly 16 lowercase hex chars. Validated on the way in
/// (retrieve) so a model-supplied id can never escape `blobs_dir` via `..`
/// or an absolute path.
pub fn is_valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Store `content` verbatim and return its retrieval id. Content-addressed:
/// identical content maps to the same id and reuses the blob. Returns `None`
/// on any I/O failure (best-effort — a failed stash just means no marker).
pub fn store(content: &str) -> Option<String> {
    store_in(&blobs_dir(), content)
}

/// Retrieve a previously stored blob by id. `None` if the id is malformed, the
/// blob is missing, or it was pruned.
pub fn retrieve(id: &str) -> Option<String> {
    retrieve_in(&blobs_dir(), id)
}

/// Remove blobs whose last modification is older than `ttl_secs`. Best-effort;
/// a `ttl_secs` of 0 disables pruning (keeps everything).
pub fn prune(ttl_secs: u64) {
    prune_in(&blobs_dir(), ttl_secs)
}

// ── Dir-injected cores (so tests don't race on the global SQUEEZ_DIR env) ────

fn store_in(dir: &Path, content: &str) -> Option<String> {
    let id = format!("{:016x}", fnv1a_64(content.as_bytes()));
    std::fs::create_dir_all(dir).ok()?;
    // Rewriting an existing blob is fine — it refreshes the mtime so an output
    // the model keeps re-encountering stays alive past the TTL.
    std::fs::write(dir.join(&id), content).ok()?;
    Some(id)
}

fn retrieve_in(dir: &Path, id: &str) -> Option<String> {
    if !is_valid_id(id) {
        return None;
    }
    std::fs::read_to_string(dir.join(id)).ok()
}

fn prune_in(dir: &Path, ttl_secs: u64) {
    if ttl_secs == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else { continue };
        // `elapsed()` errors only if mtime is in the future — treat as fresh.
        let age = modified.elapsed().map(|d| d.as_secs()).unwrap_or(0);
        if age > ttl_secs {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Unique temp dir per test → fully parallel-safe, no global SQUEEZ_DIR.
    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "sqret-{tag}-{}-{}",
            std::process::id(),
            tag.len()
        ));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn store_then_retrieve_round_trips() {
        let dir = tmp("rt");
        let original = "line one\nline two\nline three\n".repeat(50);
        let id = store_in(&dir, &original).expect("store should succeed");
        assert!(is_valid_id(&id), "id must be 16 hex chars: {id}");
        assert_eq!(retrieve_in(&dir, &id).as_deref(), Some(original.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identical_content_is_content_addressed() {
        let dir = tmp("ca");
        let a = store_in(&dir, "same content").unwrap();
        let b = store_in(&dir, "same content").unwrap();
        assert_eq!(a, b, "identical content must map to the same blob id");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_path_traversal_and_bad_ids() {
        let dir = tmp("sec");
        assert!(retrieve_in(&dir, "../../etc/passwd").is_none());
        assert!(retrieve_in(&dir, "not-hex-at-all!!").is_none());
        assert!(retrieve_in(&dir, "ABCDEF0123456789").is_none(), "uppercase rejected");
        assert!(retrieve_in(&dir, "deadbeef").is_none(), "wrong length rejected");
    }

    #[test]
    fn prune_keeps_fresh_blobs() {
        let dir = tmp("prune");
        let id = store_in(&dir, "prune me please, this is long enough").unwrap();
        prune_in(&dir, 0); // ttl 0 disables pruning entirely
        assert!(retrieve_in(&dir, &id).is_some());
        prune_in(&dir, 86_400); // just-written blob is younger than 1 day → survives
        assert!(retrieve_in(&dir, &id).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
