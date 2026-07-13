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
/// on any I/O failure, or when the content looks sensitive (Epic E7 guard —
/// see `store_guarded`) — a failed/refused stash just means no marker.
pub fn store(content: &str) -> Option<String> {
    store_guarded(content).ok()
}

/// Like `store`, but distinguishes *why* nothing was persisted. Content is
/// checked against `sensitive::is_sensitive` before it ever reaches disk;
/// if it looks like a credential, the blob is never written and `Err` carries
/// the matched class name (e.g. `"dotenv-bulk-secrets"`). On success `Ok(id)`
/// carries the retrieval id, same as `store`.
///
/// `store()` collapses this into `Option<String>` because its call site
/// (wrap.rs) already treats `None` as "no marker" regardless of cause. This
/// function exists so a future caller — e.g. an orchestrator warning that
/// wants to tell the model "a secret was found and not stashed" — can get the
/// reason without changing that call site's contract today.
pub fn store_guarded(content: &str) -> Result<String, &'static str> {
    store_guarded_in(&blobs_dir(), content)
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

/// Searches the stash for blobs whose content best matches `query` (E4) --
/// see `stash_index::search_in` for the scoring. A model can use this to
/// find a prior stashed output by meaning instead of needing the exact key.
pub fn search(query: &str, n: usize) -> Vec<crate::context::stash_index::SearchHit> {
    crate::context::stash_index::search_in(&blobs_dir(), query, n)
}

/// Top distinctive terms for a stashed blob's sidecar index, if it has one.
pub fn terms_for(id: &str, n: usize) -> Vec<String> {
    crate::context::stash_index::terms_for_in(&blobs_dir(), id, n)
}

/// The ids of the most recently stored blobs (newest first), up to `n`. Used
/// by the post-compact summary to point the model at outputs it can re-expand.
pub fn recent_ids(n: usize) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(blobs_dir()) else {
        return Vec::new();
    };
    let mut ids: Vec<(std::time::SystemTime, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !is_valid_id(&name) {
                return None;
            }
            let mtime = e.metadata().ok()?.modified().ok()?;
            Some((mtime, name))
        })
        .collect();
    ids.sort_by(|a, b| b.0.cmp(&a.0));
    ids.into_iter().take(n).map(|(_, id)| id).collect()
}

// ── Dir-injected cores (so tests don't race on the global SQUEEZ_DIR env) ────

fn store_guarded_in(dir: &Path, content: &str) -> Result<String, &'static str> {
    if let Some(class) = crate::context::sensitive::is_sensitive(content) {
        return Err(class);
    }
    store_in(dir, content).ok_or("io-error")
}

fn store_in(dir: &Path, content: &str) -> Option<String> {
    let id = format!("{:016x}", fnv1a_64(content.as_bytes()));
    std::fs::create_dir_all(dir).ok()?;
    // Rewriting an existing blob is fine — it refreshes the mtime so an output
    // the model keeps re-encountering stays alive past the TTL.
    std::fs::write(dir.join(&id), content).ok()?;
    // Sidecar index (E4) — best-effort; a failure here only means this blob
    // is unsearchable, not unretrievable, so it's never let block the store.
    crate::context::stash_index::write_index_in(dir, &id, content);
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
            if let Some(name) = entry.file_name().to_str() {
                if is_valid_id(name) {
                    crate::context::stash_index::remove_index_in(dir, name);
                }
            }
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
    fn sensitive_dotenv_body_is_not_stashed() {
        // Epic E7: a `cat .env`-shaped body must never hit disk, even though
        // it's long enough to otherwise qualify for the blob store.
        let dir = tmp("sensitive-dotenv");
        let body = "DB_PASSWORD=hunter2\nSTRIPE_SECRET=sk_live_abcdef\n\
                     API_TOKEN=abcdefghijklmnop\nDEBUG=true\n"
            .repeat(10);
        let result = store_guarded_in(&dir, &body);
        assert_eq!(result, Err("dotenv-bulk-secrets"));
        // No blob file should exist in the (possibly not even created) dir.
        let entries = std::fs::read_dir(&dir).map(|e| e.count()).unwrap_or(0);
        assert_eq!(entries, 0, "sensitive content must not be written to disk");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sensitive_private_key_is_not_stashed() {
        let dir = tmp("sensitive-pem");
        let body = format!(
            "-----BEGIN RSA PRIVATE KEY-----\n{}\n-----END RSA PRIVATE KEY-----\n",
            "MIIEowIBAAKCAQEA".repeat(20)
        );
        assert_eq!(store_guarded_in(&dir, &body), Err("private-key-block"));
        assert_eq!(store(&body), None, "public store() must also refuse sensitive content");
        let entries = std::fs::read_dir(&dir).map(|e| e.count()).unwrap_or(0);
        assert_eq!(entries, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_content_still_round_trips_through_store_guarded() {
        let dir = tmp("clean-guarded");
        let original = "ordinary build log line\n".repeat(50);
        let id = store_guarded_in(&dir, &original).expect("clean content should store");
        assert_eq!(retrieve_in(&dir, &id).as_deref(), Some(original.as_str()));
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
