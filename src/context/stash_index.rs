//! Search over the retrieve stash (E4) — a zero-dep replacement for
//! BM25/FTS5/RRF: linear scan over at most a few dozen blobs (µs-scale),
//! scored by distinctive-term overlap plus trigram-shingle Jaccard fuzzy
//! match. Lets the model find a prior stashed output by *meaning* instead
//! of needing the exact retrieve key.
//!
//! Each blob `<id>` in the retrieve store gets a sidecar `<id>.idx` holding
//! its top distinctive terms, MinHash shingle sketch (reusing `hash.rs`),
//! and a short preview — written alongside the blob in `retrieve::store()`.

use crate::context::hash;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Common English/log/code words excluded from the distinctive-term list —
/// frequency alone would otherwise surface "the", "error", "line" on nearly
/// every blob, crowding out the terms that actually distinguish one stashed
/// output from another.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "that", "with", "this", "from", "have", "are", "was", "were", "been",
    "has", "had", "not", "but", "can", "all", "any", "its", "into", "only", "over", "such",
    "then", "them", "these", "they", "will", "would", "could", "should", "about", "after",
    "before", "between", "during", "each", "more", "most", "other", "some", "than", "very",
    "what", "when", "where", "which", "while", "who", "why", "you", "your", "our", "out", "off",
    "per", "via", "log", "info", "error", "warning", "warn", "line", "lines", "file", "files",
    "test", "tests", "true", "false", "null", "none", "import", "export", "function", "const",
    "let", "var", "return", "src", "lib", "index", "node", "modules", "package", "json", "yaml",
    "yml", "txt", "done", "running", "call", "calls", "output", "result", "results", "value",
    "values", "type", "types", "name", "names", "data",
];

/// Distinctive terms in `text`, ranked by raw frequency (stopwords and pure
/// digit/very-short/very-long tokens excluded), capped at `k`.
pub fn distinctive_terms(text: &str, k: usize) -> Vec<String> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for raw in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if raw.len() < 3 || raw.len() > 40 {
            continue;
        }
        let lower = raw.to_lowercase();
        if STOPWORDS.contains(&lower.as_str()) {
            continue;
        }
        if lower.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        *counts.entry(lower).or_insert(0) += 1;
    }
    let mut v: Vec<(String, u32)> = counts.into_iter().collect();
    // Sort by frequency desc, then alphabetically for determinism on ties.
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.into_iter().take(k).map(|(t, _)| t).collect()
}

const TOP_K_TERMS: usize = 10;

pub struct IndexEntry {
    pub terms: Vec<String>,
    pub shingles: Vec<u64>,
    pub preview: String,
    /// Byte length of the blob at store time. `0` for sidecars written before
    /// this field existed — treated as "legacy, no length to check against"
    /// rather than a real zero-byte blob (store() never persists empty content
    /// as a searchable blob in practice).
    pub byte_count: u64,
}

/// Builds the sidecar index content for a blob about to be (or already)
/// stored — top distinctive terms, shingle sketch, byte length, and a 2-line
/// preview.
pub fn build_index(content: &str) -> IndexEntry {
    let terms = distinctive_terms(content, TOP_K_TERMS);
    let shingles = hash::shingle_minhash(content);
    let preview: String = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(2)
        .map(|l| l.chars().take(100).collect::<String>())
        .collect::<Vec<_>>()
        .join(" / ");
    IndexEntry { terms, shingles, preview, byte_count: content.len() as u64 }
}

fn serialize(entry: &IndexEntry) -> String {
    format!(
        "{{\"terms\":{},\"shingles\":{},\"preview\":\"{}\",\"byte_count\":{}}}",
        crate::json_util::str_array(&entry.terms),
        crate::json_util::u64_array(&entry.shingles),
        crate::json_util::escape_str(&entry.preview),
        entry.byte_count,
    )
}

fn deserialize(s: &str) -> IndexEntry {
    IndexEntry {
        terms: crate::json_util::extract_str_array(s, "terms"),
        shingles: crate::json_util::extract_u64_array(s, "shingles"),
        preview: crate::json_util::extract_str(s, "preview").unwrap_or_default(),
        // Missing on sidecars written before this field existed — see the
        // doc comment on `IndexEntry::byte_count`.
        byte_count: crate::json_util::extract_u64(s, "byte_count").unwrap_or(0),
    }
}

fn index_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{}.idx", id))
}

/// Writes (or overwrites) the sidecar index for `id`. Best-effort — an I/O
/// failure here only means that blob is unsearchable, not unretrievable.
pub fn write_index_in(dir: &Path, id: &str, content: &str) {
    let entry = build_index(content);
    let _ = std::fs::write(index_path(dir, id), serialize(&entry));
}

/// Removes the sidecar index for `id` (called when its blob is pruned, so
/// `.idx` files don't outlive the content they describe).
pub fn remove_index_in(dir: &Path, id: &str) {
    let _ = std::fs::remove_file(index_path(dir, id));
}

pub struct SearchHit {
    pub key: String,
    pub score: f32,
    pub preview: String,
}

/// Scans every `.idx` sidecar in `dir` and scores it against `query`:
/// distinctive-term overlap (primary signal) plus shingle-Jaccard fuzzy
/// match (catches paraphrase / reordering a pure term match would miss).
/// O(entries) — dozens of blobs at µs scale, no index structure needed.
pub fn search_in(dir: &Path, query: &str, n: usize) -> Vec<SearchHit> {
    let query_terms = distinctive_terms(query, 32);
    let query_shingles = hash::shingle_minhash(query);

    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut hits: Vec<SearchHit> = Vec::new();
    for e in entries.flatten() {
        let Ok(name) = e.file_name().into_string() else {
            continue;
        };
        let Some(id) = name.strip_suffix(".idx") else {
            continue;
        };
        if !crate::context::retrieve::is_valid_id(id) {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        let entry = deserialize(&raw);
        let term_overlap = entry.terms.iter().filter(|t| query_terms.contains(t)).count() as f32;
        let fuzzy = hash::jaccard(&query_shingles, &entry.shingles);
        let score = term_overlap + fuzzy * 3.0;
        if score > 0.0 {
            hits.push(SearchHit { key: id.to_string(), score, preview: entry.preview });
        }
    }
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(n);
    hits
}

/// Top-N distinctive terms for a stashed blob, read straight from its
/// sidecar index (falls back to empty if the blob was never indexed).
/// Used by the post-compact snapshot (E4) to hint what each retrievable
/// key is about without re-reading the whole blob.
pub fn terms_for_in(dir: &Path, id: &str, n: usize) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(index_path(dir, id)) else {
        return Vec::new();
    };
    let entry = deserialize(&raw);
    entry.terms.into_iter().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "sqidx-{tag}-{}-{}",
            std::process::id(),
            tag.len()
        ));
        let _ = std::fs::remove_dir_all(&p);
        let _ = std::fs::create_dir_all(&p);
        p
    }

    #[test]
    fn stopwords_and_noise_excluded() {
        let terms = distinctive_terms("the error was in the file and the line was 12345", 10);
        assert!(!terms.contains(&"the".to_string()));
        assert!(!terms.contains(&"error".to_string()));
        assert!(!terms.contains(&"12345".to_string()));
    }

    #[test]
    fn ranks_by_frequency() {
        let terms = distinctive_terms(
            "redundancy redundancy redundancy fuzzy match match preservation",
            3,
        );
        assert_eq!(terms[0], "redundancy");
        assert_eq!(terms[1], "match");
    }

    #[test]
    fn index_round_trips() {
        let entry = build_index("panicked at src/context/redundancy.rs:88:9\nassertion failed\n");
        let raw = serialize(&entry);
        let back = deserialize(&raw);
        assert_eq!(back.terms, entry.terms);
        assert_eq!(back.shingles, entry.shingles);
        assert_eq!(back.preview, entry.preview);
    }

    #[test]
    fn search_finds_term_overlap() {
        let dir = tmp("search-term");
        write_index_in(&dir, "aaaaaaaaaaaaaaaa", "panicked at src/context/redundancy.rs:88:9 assertion failed left 0.72 right 0.85");
        write_index_in(&dir, "bbbbbbbbbbbbbbbb", "npm install completed added 42 packages in 3.2s");

        let hits = search_in(&dir, "redundancy assertion failure", 5);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].key, "aaaaaaaaaaaaaaaa");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_ranks_more_relevant_higher() {
        let dir = tmp("search-rank");
        write_index_in(&dir, "1111111111111111", "cargo test failures context redundancy fuzzy match preservation threshold");
        write_index_in(&dir, "2222222222222222", "docker build successfully tagged image registry");

        let hits = search_in(&dir, "redundancy fuzzy match preservation", 5);
        assert_eq!(hits[0].key, "1111111111111111");
        assert!(hits.len() == 1 || hits[0].score > hits[1].score);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_returns_empty_for_unrelated_query() {
        let dir = tmp("search-empty");
        write_index_in(&dir, "3333333333333333", "npm install completed added packages");
        let hits = search_in(&dir, "zzz qqq xxx nonsense", 5);
        assert!(hits.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_index_deletes_sidecar() {
        let dir = tmp("remove");
        write_index_in(&dir, "4444444444444444", "some content with distinctive terms here");
        assert!(index_path(&dir, "4444444444444444").exists());
        remove_index_in(&dir, "4444444444444444");
        assert!(!index_path(&dir, "4444444444444444").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn terms_for_reads_sidecar() {
        let dir = tmp("terms-for");
        write_index_in(&dir, "5555555555555555", "redundancy redundancy fuzzy match preservation threshold context");
        let terms = terms_for_in(&dir, "5555555555555555", 3);
        assert!(!terms.is_empty());
        assert!(terms.contains(&"redundancy".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
