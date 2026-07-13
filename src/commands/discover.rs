//! `squeez discover` (E8) — ranks commands whose compression is weak
//! (dispatch falls through to GenericHandler, or achieves <15% reduction)
//! by total tokens emitted, so a user can see where a custom filter-DSL
//! rule (E8's other half) would pay off most.
//!
//! Reads `handler_stats.json`, which aggregates {calls, in_tokens,
//! out_tokens} per command first-word across sessions — the same store
//! `squeez_handler_stats` reads, so this is a filtered, re-ranked view of
//! data already being collected, not a new collection path.

use crate::economy::handler_stats::HandlerStats;
use std::path::Path;

/// Below this reduction percentage a well-dispatched (non-Generic) handler
/// is still flagged as a candidate — matches the plan's "long tail" framing:
/// weak compression is a signal independent of which handler produced it.
const REDUCTION_FLOOR_PCT: f32 = 15.0;

pub struct Candidate {
    pub cmd: String,
    pub calls: u64,
    pub tokens_emitted: u64,
    pub savings_pct: f32,
    pub reason: &'static str,
}

/// Top `top_n` filter-DSL candidates, ranked by total tokens emitted
/// (already frequency-weighted — it is a sum across all calls of that
/// command). Only commands with at least one call and nonzero input are
/// considered; a command with in_tokens == 0 has nothing to compress.
pub fn rank(sessions_dir: &Path, top_n: usize) -> Vec<Candidate> {
    let stats = HandlerStats::load(sessions_dir);
    let mut candidates: Vec<Candidate> = stats
        .rows()
        .into_iter()
        .filter(|row| row.calls > 0 && row.in_tokens > 0)
        .filter_map(|row| {
            let savings_pct = row.savings_bp as f32 / 100.0;
            let is_generic = crate::filter::handler_name(&row.name) == "generic";
            let is_low = savings_pct < REDUCTION_FLOOR_PCT;
            if !is_generic && !is_low {
                return None;
            }
            Some(Candidate {
                cmd: row.name,
                calls: row.calls,
                tokens_emitted: row.out_tokens,
                savings_pct,
                reason: if is_generic { "generic handler" } else { "low reduction" },
            })
        })
        .collect();
    candidates.sort_by(|a, b| b.tokens_emitted.cmp(&a.tokens_emitted));
    candidates.truncate(top_n);
    candidates
}

pub fn format_table(candidates: &[Candidate]) -> String {
    if candidates.is_empty() {
        return "squeez discover: no filter-DSL candidates found (all handled commands compress well or no data yet).\n".to_string();
    }
    let mut out = String::from(
        "Top filter-DSL candidates (GenericHandler or <15% reduction), by tokens emitted:\n",
    );
    for c in candidates {
        out.push_str(&format!(
            "  {:<14} {:>5} calls  {:>9} tk emitted  {:>5.1}% reduction  ({})\n",
            c.cmd, c.calls, c.tokens_emitted, c.savings_pct, c.reason
        ));
    }
    out.push_str(
        "\nAdd a [filter \"<cmd>\"] section to .squeez/filters.ini to write a custom rule for one of these.\n",
    );
    out
}

pub fn run() -> i32 {
    let sessions_dir = crate::session::sessions_dir();
    let candidates = rank(&sessions_dir, 10);
    print!("{}", format_table(&candidates));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "sqdisc-{tag}-{}-{}",
            std::process::id(),
            tag.len()
        ));
        let _ = std::fs::remove_dir_all(&p);
        let _ = std::fs::create_dir_all(&p);
        p
    }

    #[test]
    fn discover_ranks_generic_heavy() {
        let dir = tmp("generic-heavy");
        let mut stats = HandlerStats::default();
        // "foobar123" has no dispatch entry -> GenericHandler, heavy volume.
        stats.record("foobar123", 5000, 4900);
        stats.record("foobar123", 5000, 4900);
        // "git" is well-dispatched and compresses well -> not a candidate.
        stats.record("git", 2000, 200);
        stats.save(&dir);

        let candidates = rank(&dir, 10);
        assert!(candidates.iter().any(|c| c.cmd == "foobar123"));
        assert!(!candidates.iter().any(|c| c.cmd == "git"));
        assert_eq!(candidates[0].reason, "generic handler");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_ranks_low_reduction_non_generic() {
        let dir = tmp("low-reduction");
        let mut stats = HandlerStats::default();
        // "curl" dispatches to NetworkHandler (not generic) but barely compresses.
        stats.record("curl", 1000, 950); // 5% reduction
        stats.save(&dir);

        let candidates = rank(&dir, 10);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].cmd, "curl");
        assert_eq!(candidates[0].reason, "low reduction");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_excludes_well_compressed_known_handlers() {
        let dir = tmp("well-compressed");
        let mut stats = HandlerStats::default();
        stats.record("git", 5000, 500); // 90% reduction, known handler
        stats.save(&dir);

        let candidates = rank(&dir, 10);
        assert!(candidates.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_ranks_by_tokens_emitted_descending() {
        let dir = tmp("ranking");
        let mut stats = HandlerStats::default();
        stats.record("small_generic_cmd", 100, 90);
        stats.record("big_generic_cmd", 100_000, 99_000);
        stats.save(&dir);

        let candidates = rank(&dir, 10);
        assert_eq!(candidates[0].cmd, "big_generic_cmd");
        assert_eq!(candidates[1].cmd, "small_generic_cmd");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_respects_top_n_cap() {
        let dir = tmp("cap");
        let mut stats = HandlerStats::default();
        for i in 0..15 {
            stats.record(&format!("generic_cmd_{}", i), 1000, 900);
        }
        stats.save(&dir);

        let candidates = rank(&dir, 5);
        assert_eq!(candidates.len(), 5);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn format_table_handles_empty() {
        let out = format_table(&[]);
        assert!(out.contains("no filter-DSL candidates"));
    }
}
